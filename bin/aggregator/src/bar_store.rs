// BLACKER - microstructure
// Copyright (C) 2026 Juan José Caballero Rey
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License version 3 of the License.

use crate::model::{ArchivedBar, Bar, SCHEMA_VERSION};
use anyhow::{Context, Result, anyhow, bail, ensure};
use memmap2::{Mmap, MmapOptions};
use rkyv::{AlignedVec, Deserialize as RkyvDeserialize};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    sync::RwLock,
};

const STORE_FORMAT_VERSION: u32 = 1;
const INDEX_FILE: &str = "index.json";
const SEGMENT_PREFIX: &str = "segment-";
const SEGMENT_SUFFIX: &str = ".bars";
const RECORD_MAGIC: &[u8; 8] = b"BARSTR01";
const RECORD_HEADER_SIZE: usize = 32;
const RECORD_ALIGNMENT: u64 = 16;
const CHECKPOINT_EVERY_RECORDS: u64 = 128;
const CHECKPOINT_EVERY_BYTES: u64 = 64 * 1024 * 1024;
pub const MAX_BAR_SIZE_BYTES: usize = 64 * 1024 * 1024;

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub struct BarKey {
    /// Key ordering is series first, then time, to keep each series contiguous.
    pub source: String,
    pub symbol: String,
    pub timeframe: String,
    pub timestamp: u64,
}

impl BarKey {
    pub fn new(timestamp: u64, source: String, symbol: String, timeframe: String) -> Self {
        Self {
            source,
            symbol,
            timeframe,
            timestamp,
        }
    }

    pub fn from_bar(bar: &Bar) -> Self {
        Self::new(
            bar.timestamp,
            bar.source.clone(),
            bar.symbol.clone(),
            bar.timeframe.clone(),
        )
    }

    fn series_cmp(&self, source: &str, symbol: &str, timeframe: &str) -> std::cmp::Ordering {
        (
            self.source.as_str(),
            self.symbol.as_str(),
            self.timeframe.as_str(),
        )
            .cmp(&(source, symbol, timeframe))
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct IndexEntry {
    key: BarKey,
    segment: u64,
    offset: u64,
    payload_len: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct PersistedIndex {
    version: u32,
    active_segment: u64,
    /// Physical file length at the checkpoint. Any bytes after this position
    /// are an append tail that must be scanned during recovery.
    high_water: BTreeMap<u64, u64>,
    entries: Vec<IndexEntry>,
    checksum: u32,
}

struct StoreState {
    entries: Vec<IndexEntry>,
    high_water: BTreeMap<u64, u64>,
    active_segment: u64,
    uncheckpointed_records: u64,
    uncheckpointed_bytes: u64,
}

/// Append-only, segmented storage for finalized Bars.
///
/// Readers hold the state read-lock while mappings are alive. Writers need the
/// write-lock, so a mapped segment cannot be changed by this process until its
/// readers have finished. Deploy one writer per store directory.
pub struct BarStore {
    directory: PathBuf,
    max_segment_bytes: u64,
    read_only: bool,
    state: RwLock<StoreState>,
}

impl BarStore {
    pub fn open(directory: impl Into<PathBuf>, max_segment_bytes: u64) -> Result<Self> {
        Self::open_inner(directory.into(), max_segment_bytes, false)
    }

    /// Opens a store for concurrent serving without creating files,
    /// truncating incomplete tails, or updating its checkpoint.
    pub fn open_read_only(directory: impl Into<PathBuf>, max_segment_bytes: u64) -> Result<Self> {
        Self::open_inner(directory.into(), max_segment_bytes, true)
    }

    fn open_inner(directory: PathBuf, max_segment_bytes: u64, read_only: bool) -> Result<Self> {
        ensure!(
            max_segment_bytes > RECORD_HEADER_SIZE as u64,
            "Bar store segment size must exceed its record header"
        );
        if !read_only {
            fs::create_dir_all(&directory)
                .with_context(|| format!("Failed to create Bar store {}", directory.display()))?;
        }

        let segments = if directory.is_dir() {
            list_segments(&directory)?
        } else if read_only {
            Vec::new()
        } else {
            bail!(
                "Bar store directory {} is not available",
                directory.display()
            );
        };
        let mut checkpoint = match load_checkpoint(&directory, &segments) {
            Ok(checkpoint) => checkpoint,
            Err(error) => {
                tracing::warn!(
                    error = %format!("{error:#}"),
                    "Bar store index is invalid; rebuilding it from data segments"
                );
                None
            }
        };

        let mut entries_by_key = BTreeMap::new();
        let mut high_water = BTreeMap::new();
        let mut active_segment = 0;
        let mut scan_from = BTreeMap::new();

        if let Some(index) = checkpoint.take() {
            for entry in index.entries {
                entries_by_key.insert(entry.key.clone(), entry);
            }
            high_water = index.high_water;
            scan_from.clone_from(&high_water);
            active_segment = index.active_segment;
        }

        for (segment, path) in &segments {
            let start = scan_from.get(segment).copied().unwrap_or(0);
            let valid_end = if read_only {
                scan_segment_read_only(path, *segment, start, &mut entries_by_key)?
            } else {
                scan_segment(path, *segment, start, &mut entries_by_key)?
            };
            high_water.insert(*segment, valid_end);
            active_segment = active_segment.max(*segment);
        }

        if segments.is_empty() && !read_only {
            let path = segment_path(&directory, 0);
            OpenOptions::new()
                .create(true)
                .append(true)
                .open(&path)
                .with_context(|| format!("Failed to create Bar segment {}", path.display()))?;
            high_water.insert(0, 0);
            active_segment = 0;
        }

        let entries: Vec<_> = entries_by_key.into_values().collect();
        let store = Self {
            directory,
            max_segment_bytes,
            read_only,
            state: RwLock::new(StoreState {
                entries,
                high_water,
                active_segment,
                uncheckpointed_records: 0,
                uncheckpointed_bytes: 0,
            }),
        };
        if !read_only {
            store.persist_index()?;
        }
        Ok(store)
    }

    /// Refreshes an already-open read-only view after the aggregator appends
    /// records. It only indexes fully written, checksum-valid records.
    pub fn refresh(&self) -> Result<()> {
        ensure!(
            self.read_only,
            "refresh is only available on a read-only Bar store"
        );
        let segments = if self.directory.is_dir() {
            list_segments(&self.directory)?
        } else {
            Vec::new()
        };
        let mut state = self
            .state
            .write()
            .map_err(|_| anyhow!("Bar store index lock is poisoned"))?;
        for (segment, path) in segments {
            let start = state.high_water.get(&segment).copied().unwrap_or(0);
            let mut discovered = BTreeMap::new();
            let valid_end = scan_segment_read_only(&path, segment, start, &mut discovered)?;
            for (key, entry) in discovered {
                match find_entry(&state.entries, &key) {
                    Ok(position) => state.entries[position] = entry,
                    Err(position) => state.entries.insert(position, entry),
                }
            }
            state.high_water.insert(segment, valid_end);
            state.active_segment = state.active_segment.max(segment);
        }
        Ok(())
    }

    pub fn len(&self) -> usize {
        self.state
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .entries
            .len()
    }

    pub fn contains_bar(&self, bar: &Bar) -> Result<bool> {
        self.contains(&BarKey::from_bar(bar))
    }

    pub fn contains(&self, key: &BarKey) -> Result<bool> {
        let state = self
            .state
            .read()
            .map_err(|_| anyhow!("Bar store index lock is poisoned"))?;
        Ok(find_entry(&state.entries, key).is_ok())
    }

    /// Durably checkpoints the in-memory index. Data records are synced on
    /// every write; an uncheckpointed tail is replayed after an unclean exit.
    pub fn flush(&self) -> Result<()> {
        ensure!(!self.read_only, "cannot checkpoint a read-only Bar store");
        let mut state = self
            .state
            .write()
            .map_err(|_| anyhow!("Bar store index lock is poisoned"))?;
        self.persist_index_locked(&state)?;
        state.uncheckpointed_records = 0;
        state.uncheckpointed_bytes = 0;
        Ok(())
    }

    /// Inserts a new Bar and rejects an existing composite key.
    pub fn write(&self, bar: &Bar) -> Result<()> {
        self.append_bar(bar, false)
    }

    /// Appends a new version, switching the active index entry for this key.
    /// Previously written bytes are never overwritten.
    pub fn upsert(&self, bar: &Bar) -> Result<()> {
        self.append_bar(bar, true)
    }

    pub fn read(&self, key: &BarKey) -> Result<Option<Bar>> {
        let state = self
            .state
            .read()
            .map_err(|_| anyhow!("Bar store index lock is poisoned"))?;
        let Ok(position) = find_entry(&state.entries, key) else {
            return Ok(None);
        };
        let entry = &state.entries[position];
        Ok(Some(self.read_entry(entry, state.active_segment)?))
    }

    /// Reads the newest `n` Bars in a series, ordered by descending timestamp.
    pub fn read_last_n(
        &self,
        source: &str,
        symbol: &str,
        timeframe: &str,
        n: usize,
    ) -> Result<Vec<Bar>> {
        if n == 0 {
            return Ok(Vec::new());
        }
        let state = self
            .state
            .read()
            .map_err(|_| anyhow!("Bar store index lock is poisoned"))?;
        let start = state
            .entries
            .partition_point(|entry| entry.key.series_cmp(source, symbol, timeframe).is_lt());
        let end = state
            .entries
            .partition_point(|entry| !entry.key.series_cmp(source, symbol, timeframe).is_gt());
        let selected: Vec<_> = state.entries[start..end]
            .iter()
            .rev()
            .take(n)
            .cloned()
            .collect();

        let mut mappings = BTreeMap::<u64, Mmap>::new();
        let mut active_file = None;
        let mut bars = Vec::with_capacity(selected.len());
        for entry in &selected {
            if self.read_only && entry.segment == state.active_segment {
                if active_file.is_none() {
                    let path = segment_path(&self.directory, entry.segment);
                    active_file = Some(File::open(&path).with_context(|| {
                        format!("Failed to open Bar segment {}", path.display())
                    })?);
                }
                let payload = self.read_aligned_payload_from(
                    active_file
                        .as_mut()
                        .context("Active Bar segment is not open")?,
                    entry,
                )?;
                let archived = checked_archive(payload.as_ref())?;
                validate_archive_key(archived, &entry.key)?;
                bars.push(deserialize_bar(archived)?);
                continue;
            }
            if !mappings.contains_key(&entry.segment) {
                mappings.insert(entry.segment, self.map_segment(entry.segment)?);
            }
            let mapping = mappings
                .get(&entry.segment)
                .context("Mapped Bar segment disappeared")?;
            let archived = archived_for_entry(mapping, entry)?;
            validate_archive_key(archived, &entry.key)?;
            bars.push(deserialize_bar(archived)?);
        }
        Ok(bars)
    }

    /// Gives a validated zero-copy archived view for the duration of `f`.
    ///
    /// The closure runs while the index lock and its backing mmap/buffer are
    /// held, preventing this process from invalidating the archived reference.
    pub fn with_archived<R>(
        &self,
        key: &BarKey,
        f: impl FnOnce(&ArchivedBar) -> Result<R>,
    ) -> Result<Option<R>> {
        let state = self
            .state
            .read()
            .map_err(|_| anyhow!("Bar store index lock is poisoned"))?;
        let Ok(position) = find_entry(&state.entries, key) else {
            return Ok(None);
        };
        let entry = &state.entries[position];
        if self.read_only && entry.segment == state.active_segment {
            let payload = self.read_aligned_payload(entry)?;
            let archived = checked_archive(payload.as_ref())?;
            validate_archive_key(archived, &entry.key)?;
            return f(archived).map(Some);
        }
        let mapping = self.map_segment(entry.segment)?;
        let archived = archived_for_entry(&mapping, entry)?;
        validate_archive_key(archived, &entry.key)?;
        f(archived).map(Some)
    }

    fn read_entry(&self, entry: &IndexEntry, active_segment: u64) -> Result<Bar> {
        if self.read_only && entry.segment == active_segment {
            let payload = self.read_aligned_payload(entry)?;
            let archived = checked_archive(payload.as_ref())?;
            validate_archive_key(archived, &entry.key)?;
            return deserialize_bar(archived);
        }
        let mapping = self.map_segment(entry.segment)?;
        let archived = archived_for_entry(&mapping, entry)?;
        validate_archive_key(archived, &entry.key)?;
        deserialize_bar(archived)
    }

    fn read_aligned_payload(&self, entry: &IndexEntry) -> Result<AlignedVec> {
        let path = segment_path(&self.directory, entry.segment);
        let mut file = File::open(&path)
            .with_context(|| format!("Failed to open Bar segment {}", path.display()))?;
        self.read_aligned_payload_from(&mut file, entry)
    }

    fn read_aligned_payload_from(&self, file: &mut File, entry: &IndexEntry) -> Result<AlignedVec> {
        file.seek(SeekFrom::Start(entry.offset))?;
        let mut header = [0; RECORD_HEADER_SIZE];
        file.read_exact(&mut header)
            .context("Bar record header is truncated")?;
        let (payload_len, checksum) = parse_record_header(&header)?;
        ensure!(
            payload_len == entry.payload_len,
            "Bar record length does not match the index"
        );
        let mut payload = AlignedVec::with_capacity(usize::try_from(payload_len)?);
        payload.resize(usize::try_from(payload_len)?, 0);
        file.read_exact(&mut payload)
            .context("Bar record payload is truncated")?;
        ensure!(
            crc32fast::hash(payload.as_ref()) == checksum,
            "Bar record checksum mismatch"
        );
        Ok(payload)
    }

    fn append_bar(&self, bar: &Bar, replace: bool) -> Result<()> {
        ensure!(!self.read_only, "cannot write to a read-only Bar store");
        validate_bar(bar)?;
        let serialized = rkyv::to_bytes::<_, 4096>(bar)
            .map_err(|error| anyhow!("Failed to serialize Bar with rkyv: {error}"))?;
        validate_serialized_size(serialized.len())?;
        checked_archive(serialized.as_ref()).context("Serialized Bar failed rkyv validation")?;

        let key = BarKey::from_bar(bar);
        let payload_len = u64::try_from(serialized.len())?;
        let checksum = crc32fast::hash(serialized.as_ref());
        let mut state = self
            .state
            .write()
            .map_err(|_| anyhow!("Bar store index lock is poisoned"))?;
        let existing = find_entry(&state.entries, &key).ok();
        ensure!(
            replace || existing.is_none(),
            "Bar already exists for key {:?}",
            key
        );

        let mut segment = state.active_segment;
        let mut path = segment_path(&self.directory, segment);
        let mut file = open_segment_for_append(&path)?;
        let mut old_len = file.metadata()?.len();
        let mut offset = align_up(old_len)?;
        let record_len = (RECORD_HEADER_SIZE as u64)
            .checked_add(payload_len)
            .context("Bar record length overflows u64")?;
        if old_len > 0
            && offset
                .checked_add(record_len)
                .context("Bar segment length overflows u64")?
                > self.max_segment_bytes
        {
            segment = segment
                .checked_add(1)
                .context("Bar segment identifier overflows u64")?;
            path = segment_path(&self.directory, segment);
            file = open_segment_for_append(&path)?;
            old_len = file.metadata()?.len();
            offset = align_up(old_len)?;
            // Keep the new segment active even if writing this first record
            // fails. A read-only server may already have observed its file.
            state.active_segment = segment;
            state.high_water.entry(segment).or_insert(old_len);
        }

        let frame = make_record_header(payload_len, checksum);
        let write_result = (|| -> Result<()> {
            file.seek(SeekFrom::Start(old_len))?;
            if offset > old_len {
                let padding_len = usize::try_from(offset - old_len)?;
                file.write_all(&vec![0; padding_len])?;
            }
            file.write_all(&frame)?;
            file.write_all(serialized.as_ref())?;
            file.sync_all()
                .context("Failed to durably sync appended Bar record")?;
            Ok(())
        })();
        if let Err(error) = write_result {
            let _ = file.set_len(old_len);
            let _ = file.sync_all();
            return Err(error)
                .with_context(|| format!("Failed to append Bar to {}", path.display()));
        }

        let entry = IndexEntry {
            key: key.clone(),
            segment,
            offset,
            payload_len,
        };
        match find_entry(&state.entries, &key) {
            Ok(position) => state.entries[position] = entry,
            Err(position) => state.entries.insert(position, entry),
        }
        state.active_segment = segment;
        state.high_water.insert(segment, file.metadata()?.len());
        state.uncheckpointed_records = state.uncheckpointed_records.saturating_add(1);
        state.uncheckpointed_bytes = state.uncheckpointed_bytes.saturating_add(record_len);

        // Checkpoint in bounded batches. Each record is already synced, and
        // startup replays bytes beyond the last checkpoint.
        if state.uncheckpointed_records >= CHECKPOINT_EVERY_RECORDS
            || state.uncheckpointed_bytes >= CHECKPOINT_EVERY_BYTES
        {
            match self.persist_index_locked(&state) {
                Ok(()) => {
                    state.uncheckpointed_records = 0;
                    state.uncheckpointed_bytes = 0;
                }
                Err(error) => {
                    tracing::warn!(
                        error = %format!("{error:#}"),
                        "Bar was stored durably, but its index checkpoint failed"
                    );
                }
            }
        }
        Ok(())
    }

    fn map_segment(&self, segment: u64) -> Result<Mmap> {
        let path = segment_path(&self.directory, segment);
        let file = File::open(&path)
            .with_context(|| format!("Failed to open Bar segment {}", path.display()))?;
        ensure!(
            file.metadata()?.len() > 0,
            "Bar segment {} is empty",
            path.display()
        );
        // SAFETY: writer handles hold the store's RwLock read guard for the
        // complete mapping lifetime, while local appends need its write guard.
        // Read-only handles mmap only sealed segments; their active segment is
        // read through File I/O, and the append-only writer never changes a
        // sealed segment. Store directories must have one active writer.
        unsafe { MmapOptions::new().map(&file) }
            .with_context(|| format!("Failed to mmap Bar segment {}", path.display()))
    }

    fn persist_index(&self) -> Result<()> {
        let state = self
            .state
            .read()
            .map_err(|_| anyhow!("Bar store index lock is poisoned"))?;
        self.persist_index_locked(&state)
    }

    fn persist_index_locked(&self, state: &StoreState) -> Result<()> {
        let index = PersistedIndex {
            version: STORE_FORMAT_VERSION,
            active_segment: state.active_segment,
            high_water: state.high_water.clone(),
            entries: state.entries.clone(),
            checksum: checkpoint_checksum(
                STORE_FORMAT_VERSION,
                state.active_segment,
                &state.high_water,
                &state.entries,
            )?,
        };
        let bytes = serde_json::to_vec(&index).context("Failed to encode Bar store index")?;
        let temporary = self.directory.join(format!("{INDEX_FILE}.tmp"));
        let destination = self.directory.join(INDEX_FILE);
        let mut file = OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(&temporary)
            .with_context(|| format!("Failed to create {}", temporary.display()))?;
        file.write_all(&bytes)
            .context("Failed to write Bar store index")?;
        file.sync_all()
            .context("Failed to durably sync Bar store index")?;
        fs::rename(&temporary, &destination)
            .context("Failed to atomically replace Bar store index")?;
        sync_directory(&self.directory)?;
        Ok(())
    }
}

fn validate_bar(bar: &Bar) -> Result<()> {
    ensure!(
        bar.schema_version == SCHEMA_VERSION,
        "Unsupported Bar schema version {}; expected {}",
        bar.schema_version,
        SCHEMA_VERSION
    );
    ensure!(!bar.source.is_empty(), "Bar source cannot be empty");
    ensure!(!bar.symbol.is_empty(), "Bar symbol cannot be empty");
    ensure!(!bar.timeframe.is_empty(), "Bar timeframe cannot be empty");
    Ok(())
}

fn validate_serialized_size(size: usize) -> Result<()> {
    ensure!(
        size <= MAX_BAR_SIZE_BYTES,
        "Serialized Bar is {size} bytes; maximum is {MAX_BAR_SIZE_BYTES} bytes"
    );
    Ok(())
}

fn find_entry(entries: &[IndexEntry], key: &BarKey) -> std::result::Result<usize, usize> {
    entries.binary_search_by(|entry| entry.key.cmp(key))
}

fn checked_archive(bytes: &[u8]) -> Result<&ArchivedBar> {
    rkyv::check_archived_root::<Bar>(bytes)
        .map_err(|error| anyhow!("Invalid archived Bar bytes: {error}"))
}

fn deserialize_bar(archived: &ArchivedBar) -> Result<Bar> {
    archived
        .deserialize(&mut rkyv::Infallible)
        .map_err(|never| match never {})
}

fn validate_archive_key(archived: &ArchivedBar, key: &BarKey) -> Result<()> {
    ensure!(
        archived.timestamp == key.timestamp
            && archived.source.as_str() == key.source
            && archived.symbol.as_str() == key.symbol
            && archived.timeframe.as_str() == key.timeframe,
        "Bar record key does not match its index entry"
    );
    ensure!(
        archived.schema_version == SCHEMA_VERSION,
        "Unsupported archived Bar schema version {}",
        archived.schema_version
    );
    Ok(())
}

fn archived_for_entry<'a>(mapping: &'a Mmap, entry: &IndexEntry) -> Result<&'a ArchivedBar> {
    let frame_offset = usize::try_from(entry.offset)?;
    let payload_len = usize::try_from(entry.payload_len)?;
    let payload_offset = frame_offset
        .checked_add(RECORD_HEADER_SIZE)
        .context("Bar record payload offset overflows")?;
    let payload_end = payload_offset
        .checked_add(payload_len)
        .context("Bar record end offset overflows")?;
    ensure!(
        payload_end <= mapping.len(),
        "Bar record extends beyond mapped segment"
    );
    let header_end = frame_offset
        .checked_add(RECORD_HEADER_SIZE)
        .context("Bar record header offset overflows")?;
    let header = mapping
        .get(frame_offset..header_end)
        .context("Bar record header is truncated")?;
    let (stored_len, expected_checksum) = parse_record_header(header)?;
    ensure!(
        stored_len == entry.payload_len,
        "Bar record length does not match the index"
    );
    let payload = mapping
        .get(payload_offset..payload_end)
        .context("Bar record payload is truncated")?;
    ensure!(
        crc32fast::hash(payload) == expected_checksum,
        "Bar record checksum mismatch"
    );
    checked_archive(payload)
}

fn make_record_header(payload_len: u64, checksum: u32) -> [u8; RECORD_HEADER_SIZE] {
    let mut header = [0; RECORD_HEADER_SIZE];
    header[..8].copy_from_slice(RECORD_MAGIC);
    header[8..12].copy_from_slice(&STORE_FORMAT_VERSION.to_le_bytes());
    header[12..20].copy_from_slice(&payload_len.to_le_bytes());
    header[20..24].copy_from_slice(&checksum.to_le_bytes());
    header
}

fn parse_record_header(header: &[u8]) -> Result<(u64, u32)> {
    ensure!(
        header.len() == RECORD_HEADER_SIZE,
        "Invalid Bar record header size"
    );
    ensure!(&header[..8] == RECORD_MAGIC, "Invalid Bar record magic");
    let version = u32::from_le_bytes(header[8..12].try_into()?);
    ensure!(
        version == STORE_FORMAT_VERSION,
        "Unsupported Bar record format version {version}"
    );
    let payload_len = u64::from_le_bytes(header[12..20].try_into()?);
    ensure!(
        payload_len > 0 && payload_len <= MAX_BAR_SIZE_BYTES as u64,
        "Invalid Bar payload length {payload_len}"
    );
    let checksum = u32::from_le_bytes(header[20..24].try_into()?);
    Ok((payload_len, checksum))
}

fn load_checkpoint(
    directory: &Path,
    segments: &[(u64, PathBuf)],
) -> Result<Option<PersistedIndex>> {
    let path = directory.join(INDEX_FILE);
    if !path.exists() {
        return Ok(None);
    }
    let bytes = fs::read(&path).with_context(|| format!("Failed to read {}", path.display()))?;
    let index: PersistedIndex =
        serde_json::from_slice(&bytes).context("Failed to decode Bar store index")?;
    ensure!(
        index.version == STORE_FORMAT_VERSION,
        "Unsupported Bar store index version {}",
        index.version
    );
    ensure!(
        index.checksum
            == checkpoint_checksum(
                index.version,
                index.active_segment,
                &index.high_water,
                &index.entries,
            )?,
        "Bar store index checksum mismatch"
    );
    let segment_lengths: BTreeMap<_, _> = segments
        .iter()
        .map(|(id, path)| {
            Ok((
                *id,
                fs::metadata(path)
                    .with_context(|| format!("Failed to stat {}", path.display()))?
                    .len(),
            ))
        })
        .collect::<Result<_>>()?;
    for (segment, high_water) in &index.high_water {
        let physical_len = segment_lengths
            .get(segment)
            .with_context(|| format!("Index references missing segment {segment}"))?;
        ensure!(
            high_water <= physical_len,
            "Bar store segment {segment} is shorter than its checkpoint"
        );
    }
    let mut previous_key: Option<&BarKey> = None;
    for entry in &index.entries {
        if let Some(previous) = previous_key {
            ensure!(
                previous < &entry.key,
                "Bar store index entries are not strictly sorted"
            );
        }
        previous_key = Some(&entry.key);
        ensure!(
            entry.offset % RECORD_ALIGNMENT == 0,
            "Bar store index contains an unaligned record offset"
        );
        ensure!(
            entry.payload_len > 0 && entry.payload_len <= MAX_BAR_SIZE_BYTES as u64,
            "Bar store index contains an invalid record length"
        );
        let checkpoint_len = index.high_water.get(&entry.segment).with_context(|| {
            format!(
                "Bar store index entry references segment {} without a checkpoint",
                entry.segment
            )
        })?;
        let end = entry
            .offset
            .checked_add(RECORD_HEADER_SIZE as u64)
            .and_then(|offset| offset.checked_add(entry.payload_len))
            .context("Bar store index record range overflows")?;
        ensure!(
            end <= *checkpoint_len,
            "Bar store index entry extends past its checkpoint"
        );
    }
    ensure!(
        segment_lengths.contains_key(&index.active_segment) || segments.is_empty(),
        "Bar store active segment is missing"
    );
    Ok(Some(index))
}

fn checkpoint_checksum(
    version: u32,
    active_segment: u64,
    high_water: &BTreeMap<u64, u64>,
    entries: &[IndexEntry],
) -> Result<u32> {
    let bytes = serde_json::to_vec(&(version, active_segment, high_water, entries))
        .context("Failed to encode Bar store index checksum input")?;
    Ok(crc32fast::hash(&bytes))
}

fn scan_segment(
    path: &Path,
    segment: u64,
    start: u64,
    entries: &mut BTreeMap<BarKey, IndexEntry>,
) -> Result<u64> {
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
        .with_context(|| format!("Failed to open Bar segment {}", path.display()))?;
    let physical_len = file.metadata()?.len();
    ensure!(
        start <= physical_len,
        "Bar segment {} is shorter than its recovery checkpoint",
        path.display()
    );
    if physical_len == 0 {
        return Ok(0);
    }

    let mut truncate_at = None;
    {
        // Startup recovery is single-threaded and this mapping is dropped
        // before any truncation is performed.
        // SAFETY: no readers or writers exist while a store is being opened.
        let mapping = unsafe { MmapOptions::new().map(&file) }
            .with_context(|| format!("Failed to mmap Bar segment {}", path.display()))?;
        let mut offset = align_up(start)?;
        if offset > start {
            let padding_start = usize::try_from(start)?;
            let padding_end = usize::try_from(offset)?;
            if padding_end <= mapping.len() {
                ensure!(
                    mapping[padding_start..padding_end]
                        .iter()
                        .all(|byte| *byte == 0),
                    "Invalid padding at the end of the Bar store checkpoint"
                );
            }
        }

        while offset < physical_len {
            let frame_offset = usize::try_from(offset)?;
            let remaining = mapping.len().saturating_sub(frame_offset);
            if remaining == 0 {
                break;
            }
            if remaining < RECORD_HEADER_SIZE {
                truncate_at = Some(offset);
                break;
            }
            let header_end = frame_offset + RECORD_HEADER_SIZE;
            let header = &mapping[frame_offset..header_end];
            if &header[..8] != RECORD_MAGIC {
                if mapping[frame_offset..].iter().all(|byte| *byte == 0) {
                    truncate_at = Some(offset);
                    break;
                }
                bail!("Corrupt Bar record magic in segment {segment} at offset {offset}");
            }
            let (payload_len, checksum) = parse_record_header(header)
                .with_context(|| format!("Invalid Bar record in segment {segment} at {offset}"))?;
            let payload_start = header_end;
            let payload_end = payload_start
                .checked_add(usize::try_from(payload_len)?)
                .context("Bar record range overflows address space")?;
            if payload_end > mapping.len() {
                truncate_at = Some(offset);
                break;
            }
            let payload = &mapping[payload_start..payload_end];
            let frame_end = u64::try_from(payload_end)?;
            if crc32fast::hash(payload) != checksum {
                if frame_end == physical_len {
                    truncate_at = Some(offset);
                    break;
                }
                bail!("Bar record checksum mismatch in segment {segment} at offset {offset}");
            }
            let archived = match checked_archive(payload) {
                Ok(archived) => archived,
                Err(error) if frame_end == physical_len => {
                    tracing::warn!(
                        segment,
                        offset,
                        error = %format!("{error:#}"),
                        "Discarding incomplete final Bar record"
                    );
                    truncate_at = Some(offset);
                    break;
                }
                Err(error) => {
                    return Err(error).with_context(|| {
                        format!("Corrupt Bar record in segment {segment} at offset {offset}")
                    });
                }
            };
            let bar = deserialize_bar(archived)?;
            validate_bar(&bar)
                .with_context(|| format!("Invalid Bar in segment {segment} at offset {offset}"))?;
            let key = BarKey::from_bar(&bar);
            entries.insert(
                key.clone(),
                IndexEntry {
                    key,
                    segment,
                    offset,
                    payload_len,
                },
            );
            offset = align_up(frame_end)?;
        }
    }

    if let Some(offset) = truncate_at {
        file.set_len(offset).with_context(|| {
            format!("Failed to trim incomplete Bar record in {}", path.display())
        })?;
        file.sync_all()
            .context("Failed to sync recovered Bar segment")?;
    }
    Ok(file.metadata()?.len())
}

/// Read-only scanner used by the HTTP server. It never truncates an incomplete
/// tail: the valid watermark stays before that tail so the next refresh retries
/// it after the writer completes the record.
fn scan_segment_read_only(
    path: &Path,
    segment: u64,
    start: u64,
    entries: &mut BTreeMap<BarKey, IndexEntry>,
) -> Result<u64> {
    let mut file = File::open(path)
        .with_context(|| format!("Failed to open Bar segment {}", path.display()))?;
    let physical_len = file.metadata()?.len();
    ensure!(
        start <= physical_len,
        "Bar segment {} is shorter than its recovery checkpoint",
        path.display()
    );
    let mut offset = align_up(start)?;
    if offset > physical_len {
        return Ok(start);
    }
    if offset > start {
        let padding_len = usize::try_from(offset - start)?;
        let mut padding = vec![0; padding_len];
        file.seek(SeekFrom::Start(start))?;
        file.read_exact(&mut padding)
            .context("Bar segment alignment padding is truncated")?;
        ensure!(
            padding.iter().all(|byte| *byte == 0),
            "Invalid padding at the end of the Bar store checkpoint"
        );
    }

    let mut valid_end = start;
    while offset < physical_len {
        let remaining = physical_len - offset;
        if remaining < RECORD_HEADER_SIZE as u64 {
            break;
        }
        file.seek(SeekFrom::Start(offset))?;
        let mut header = [0; RECORD_HEADER_SIZE];
        if let Err(error) = file.read_exact(&mut header) {
            if error.kind() == std::io::ErrorKind::UnexpectedEof {
                break;
            }
            return Err(error).context("Failed to read Bar record header");
        }
        if &header[..8] != RECORD_MAGIC {
            if remaining == RECORD_HEADER_SIZE as u64 || header.iter().all(|byte| *byte == 0) {
                break;
            }
            bail!("Corrupt Bar record magic in segment {segment} at offset {offset}");
        }
        let (payload_len, checksum) = parse_record_header(&header)
            .with_context(|| format!("Invalid Bar record in segment {segment} at {offset}"))?;
        let payload_offset = offset
            .checked_add(RECORD_HEADER_SIZE as u64)
            .context("Bar payload offset overflows")?;
        let record_end = payload_offset
            .checked_add(payload_len)
            .context("Bar record end offset overflows")?;
        if record_end > physical_len {
            break;
        }

        let mut payload = AlignedVec::with_capacity(usize::try_from(payload_len)?);
        payload.resize(usize::try_from(payload_len)?, 0);
        file.seek(SeekFrom::Start(payload_offset))?;
        if let Err(error) = file.read_exact(&mut payload) {
            if error.kind() == std::io::ErrorKind::UnexpectedEof {
                break;
            }
            return Err(error).context("Failed to read Bar record payload");
        }
        if crc32fast::hash(payload.as_ref()) != checksum {
            if record_end == physical_len {
                break;
            }
            bail!("Bar record checksum mismatch in segment {segment} at offset {offset}");
        }
        let archived = match checked_archive(payload.as_ref()) {
            Ok(archived) => archived,
            Err(error) if record_end == physical_len => {
                tracing::debug!(
                    segment,
                    offset,
                    error = %format!("{error:#}"),
                    "Ignoring a possibly incomplete final Bar while refreshing"
                );
                break;
            }
            Err(error) => {
                return Err(error).with_context(|| {
                    format!("Corrupt Bar record in segment {segment} at offset {offset}")
                });
            }
        };
        let bar = deserialize_bar(archived)?;
        validate_bar(&bar)
            .with_context(|| format!("Invalid Bar in segment {segment} at offset {offset}"))?;
        let key = BarKey::from_bar(&bar);
        entries.insert(
            key.clone(),
            IndexEntry {
                key,
                segment,
                offset,
                payload_len,
            },
        );
        valid_end = record_end;
        offset = align_up(record_end)?;
    }
    Ok(valid_end)
}

fn list_segments(directory: &Path) -> Result<Vec<(u64, PathBuf)>> {
    let mut segments = Vec::new();
    for entry in fs::read_dir(directory)
        .with_context(|| format!("Failed to list Bar store {}", directory.display()))?
    {
        let entry = entry?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        let Some(number) = name
            .strip_prefix(SEGMENT_PREFIX)
            .and_then(|name| name.strip_suffix(SEGMENT_SUFFIX))
        else {
            continue;
        };
        let Ok(segment) = number.parse::<u64>() else {
            continue;
        };
        segments.push((segment, entry.path()));
    }
    segments.sort_by_key(|(segment, _)| *segment);
    let mut ids = BTreeSet::new();
    for (segment, _) in &segments {
        ensure!(ids.insert(*segment), "Duplicate Bar segment id {segment}");
    }
    Ok(segments)
}

fn segment_path(directory: &Path, segment: u64) -> PathBuf {
    directory.join(format!("{SEGMENT_PREFIX}{segment:020}{SEGMENT_SUFFIX}"))
}

fn open_segment_for_append(path: &Path) -> Result<File> {
    OpenOptions::new()
        .create(true)
        .read(true)
        .append(true)
        .open(path)
        .with_context(|| format!("Failed to open Bar segment {}", path.display()))
}

fn align_up(value: u64) -> Result<u64> {
    value
        .checked_add(RECORD_ALIGNMENT - 1)
        .map(|value| value / RECORD_ALIGNMENT * RECORD_ALIGNMENT)
        .context("Bar segment offset overflows u64")
}

fn sync_directory(directory: &Path) -> Result<()> {
    #[cfg(unix)]
    File::open(directory)
        .and_then(|file| file.sync_all())
        .with_context(|| format!("Failed to sync Bar store directory {}", directory.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{BarKey, BarStore, MAX_BAR_SIZE_BYTES, segment_path, validate_serialized_size};
    use crate::model::{Tick, build_bar_for_timeframe};
    use std::{
        fs,
        io::Write,
        path::PathBuf,
        sync::atomic::{AtomicU64, Ordering},
    };

    static NEXT_TEMP_DIR: AtomicU64 = AtomicU64::new(0);

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new() -> Self {
            let unique = NEXT_TEMP_DIR.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "aggregator-bar-store-{}-{unique}",
                std::process::id()
            ));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn bar(timestamp: u64, symbol: &str, price: u64) -> crate::model::Bar {
        let tick = Tick {
            time_us: i64::try_from(timestamp * 1_000).unwrap(),
            source: "test-source".to_string(),
            symbol: symbol.to_string(),
            id: format!("trade-{timestamp}-{price}"),
            price,
            qty: 1,
            is_buyer_maker: 0,
        };
        build_bar_for_timeframe(&[tick], timestamp, "1m", 60_000, 1, 1).unwrap()
    }

    fn key(timestamp: u64, symbol: &str) -> BarKey {
        BarKey::new(
            timestamp,
            "test-source".to_string(),
            symbol.to_string(),
            "1m".to_string(),
        )
    }

    #[test]
    fn write_read_and_reject_duplicate_key() {
        let directory = TestDirectory::new();
        let store = BarStore::open(&directory.0, 1024 * 1024).unwrap();
        let bar = bar(0, "BTCUSD", 100);

        store.write(&bar).unwrap();
        assert_eq!(store.read(&key(0, "BTCUSD")).unwrap().unwrap().timestamp, 0);
        assert!(store.write(&bar).is_err());
        assert_eq!(store.len(), 1);
    }

    #[test]
    fn upsert_keeps_one_active_version_and_reads_last_n_descending() {
        let directory = TestDirectory::new();
        let store = BarStore::open(&directory.0, 1024 * 1024).unwrap();
        store.write(&bar(0, "BTCUSD", 100)).unwrap();
        store.write(&bar(60_000, "BTCUSD", 101)).unwrap();
        store.write(&bar(120_000, "BTCUSD", 102)).unwrap();

        let replacement = bar(60_000, "BTCUSD", 999);
        store.upsert(&replacement).unwrap();

        let latest = store.read_last_n("test-source", "BTCUSD", "1m", 2).unwrap();
        assert_eq!(
            latest.iter().map(|bar| bar.timestamp).collect::<Vec<_>>(),
            vec![120_000, 60_000]
        );
        assert_eq!(latest[1].price.close, replacement.price.close);
        assert_eq!(store.len(), 3);
        assert!(
            store
                .read_last_n("other", "BTCUSD", "1m", 10)
                .unwrap()
                .is_empty()
        );
        assert!(
            store
                .read_last_n("test-source", "BTCUSD", "1m", 0)
                .unwrap()
                .is_empty()
        );
        drop(store);

        let reopened = BarStore::open(&directory.0, 1024 * 1024).unwrap();
        assert_eq!(
            reopened
                .read(&key(60_000, "BTCUSD"))
                .unwrap()
                .unwrap()
                .price
                .close,
            replacement.price.close
        );
    }

    #[test]
    fn mmap_archived_view_is_available_without_deserializing_the_bar() {
        let directory = TestDirectory::new();
        let store = BarStore::open(&directory.0, 1024 * 1024).unwrap();
        store.write(&bar(0, "BTCUSD", 100)).unwrap();

        let timestamp = store
            .with_archived(&key(0, "BTCUSD"), |archived| Ok(archived.timestamp))
            .unwrap();
        assert_eq!(timestamp, Some(0));
    }

    #[test]
    fn read_only_view_refreshes_concurrent_appends_without_checkpointing() {
        let directory = TestDirectory::new();
        let writer = BarStore::open(&directory.0, 1024 * 1024).unwrap();
        writer.write(&bar(0, "BTCUSD", 100)).unwrap();
        let checkpoint = fs::read(directory.0.join("index.json")).unwrap();
        let reader = BarStore::open_read_only(&directory.0, 1024 * 1024).unwrap();
        assert_eq!(reader.len(), 1);

        writer.write(&bar(60_000, "BTCUSD", 101)).unwrap();
        reader.refresh().unwrap();
        let latest = reader
            .read_last_n("test-source", "BTCUSD", "1m", 2)
            .unwrap();

        assert_eq!(
            latest.iter().map(|bar| bar.timestamp).collect::<Vec<_>>(),
            vec![60_000, 0]
        );
        assert!(reader.write(&bar(120_000, "BTCUSD", 102)).is_err());
        assert_eq!(
            checkpoint,
            fs::read(directory.0.join("index.json")).unwrap()
        );
    }

    #[test]
    fn read_only_open_does_not_create_a_missing_store_directory() {
        let directory = TestDirectory::new();
        let missing_store = directory.0.join("not-created");

        let reader = BarStore::open_read_only(&missing_store, 1024 * 1024).unwrap();

        assert_eq!(reader.len(), 0);
        assert!(!missing_store.exists());
    }

    #[test]
    fn rebuilds_corrupt_index_and_recovers_uncheckpointed_tail() {
        let directory = TestDirectory::new();
        {
            let store = BarStore::open(&directory.0, 1024 * 1024).unwrap();
            store.write(&bar(0, "BTCUSD", 100)).unwrap();
        }
        fs::write(directory.0.join("index.json"), b"not-json").unwrap();
        let segment = segment_path(&directory.0, 0);
        fs::OpenOptions::new()
            .append(true)
            .open(&segment)
            .unwrap()
            .write_all(b"BAR")
            .unwrap();

        let recovered = BarStore::open(&directory.0, 1024 * 1024).unwrap();
        assert_eq!(recovered.len(), 1);
        assert_eq!(
            recovered
                .read(&key(0, "BTCUSD"))
                .unwrap()
                .unwrap()
                .timestamp,
            0
        );
        assert_eq!(fs::metadata(segment).unwrap().len() % 16, 0);
    }

    #[test]
    fn rebuilds_a_valid_json_index_when_its_entries_are_incomplete() {
        let directory = TestDirectory::new();
        {
            let store = BarStore::open(&directory.0, 1024 * 1024).unwrap();
            store.write(&bar(0, "BTCUSD", 100)).unwrap();
        }
        let index_path = directory.0.join("index.json");
        let mut index: serde_json::Value =
            serde_json::from_slice(&fs::read(&index_path).unwrap()).unwrap();
        index["entries"] = serde_json::json!([]);
        fs::write(&index_path, serde_json::to_vec(&index).unwrap()).unwrap();

        let recovered = BarStore::open(&directory.0, 1024 * 1024).unwrap();
        assert_eq!(recovered.len(), 1);
        assert!(recovered.read(&key(0, "BTCUSD")).unwrap().is_some());
    }

    #[test]
    fn segment_rotation_preserves_old_records() {
        let directory = TestDirectory::new();
        let store = BarStore::open(&directory.0, 512).unwrap();
        for timestamp in [0, 60_000, 120_000] {
            store
                .write(&bar(timestamp, "BTCUSD", 100 + timestamp))
                .unwrap();
        }
        assert!(fs::metadata(segment_path(&directory.0, 1)).is_ok());
        assert_eq!(
            store
                .read_last_n("test-source", "BTCUSD", "1m", 3)
                .unwrap()
                .len(),
            3
        );
    }

    #[test]
    fn serialized_limit_is_sixty_four_mib() {
        assert_eq!(MAX_BAR_SIZE_BYTES, 64 * 1024 * 1024);
        assert!(validate_serialized_size(MAX_BAR_SIZE_BYTES).is_ok());
        assert!(validate_serialized_size(MAX_BAR_SIZE_BYTES + 1).is_err());
    }
}

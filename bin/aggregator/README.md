# Aggregator

Builds bars for its configured timeframe from the shared `ticks` table in
QuestDB, stores them in timeframe-isolated tables, and publishes each JSON event
to a source/symbol/timeframe-specific Pulsar topic. The code default timeframe is
`1m`; the sample `.env.example` explicitly configures `5m`. Supported timeframes
are `1m`, `5m`, `15m`, `30m`, `1h`, `4h`, and `1d`.
While a timeframe window is open, it also publishes a complete live snapshot
once per `LIVE_POLL_INTERVAL_MS` when the bar has changed. The default interval
is one second.

## Run

Start QuestDB and Pulsar, then configure and run the service:

```sh
cp bin/aggregator/.env.example bin/aggregator/.env.dev
cargo run --manifest-path bin/aggregator/Cargo.toml
```

The process reads `.env.dev` from this directory; environment variables take
precedence. The shared `ticks` table must be populated by the merger. Set
`AGGREGATOR_TIMEFRAME` independently for each worker. If unset, the code uses
`1m`; copying the sample `.env.example` to `.env.dev` configures this worker for
`5m`. The `1m` worker uses `bars_1m`, `bar_events` (the event outbox), and
`aggregator_state`; other timeframes use suffixed names, for example `bars_5m`,
`bar_events_5m`, and `aggregator_state_5m`.

## Processing and delivery

- Windows use UTC epoch boundaries for the configured timeframe and the
  half-open range `[start, end)`. A bar timestamp is the beginning of its
  timeframe window.
- The bar and event envelope include `timeframe`. Pulsar topics are named
  `persistent://public/default/bars-{source}-{symbol}-{timeframe}`; for example,
  `bars-binance-BTCUSDT-1m`.
- A bar is created for each `(source, symbol)` with at least one tick. Empty
  windows do not produce bars.
- The persisted JSON contains the schema from `logic.md` and the ordered
  `raw_ticks`, retaining every field available in the QuestDB tick row plus
  `time_us` so consumers can independently verify the checksum.
- Pulsar events wrap the bar in `data`; `event_id` is an envelope field, not a
  bar field. Consumers should deduplicate by that stable ID.
- Live events use the same complete `Bar` schema and topic as finalized events.
  The event envelope adds `is_final: false` for live snapshots and
  `is_final: true` for finalized bars. A live event ID is derived from the
  `bar_key`, trade count, and checksum; finalized events retain the
  `{bar_key}:r{revision}` ID. Live snapshots are published directly to Pulsar,
  not written to the finalized-bars table or durable outbox. They are therefore
  best effort; the next poll recomputes the latest snapshot after a restart.
  Subscribers using `InitialPosition::Latest` receive snapshots published after
  subscription, not a previously emitted live snapshot.
- The live poll re-reads the active window and emits only when the serialized
  `Bar` changes. Since each event contains all `raw_ticks`, frequent snapshots
  can generate substantial query, network, and broker traffic.
- For finalized bars, the bar and a pending event-outbox row are written to
  QuestDB before the event is sent, using separate durable QWP flushes. Each individual row must
  still fit in one QWP frame. The aggregator configures both `max_buf_size` and
  `sf_max_segment_bytes` to 128 MiB by default. The development QuestDB service
  in `infra.yaml` sets `http.recv.buffer.size` to 17 MiB, enough for QuestDB
  10.2's hard 16 MiB QWP frame ceiling after framing overhead, and
  `http.send.buffer.size` to 64 MiB for query results. Increasing the receive
  buffer cannot raise that protocol ceiling. Large JSON payloads are gzip
  compressed and Base64 encoded in QuestDB, then transparently decoded by the
  aggregator and history API; Pulsar events and API responses remain ordinary
  JSON. Direct SQL readers of `payload` must decode values prefixed with
  `gzip+base64:`. Legacy uncompressed rows remain readable. The send buffer
  must exceed the largest outbox payload; QuestDB's 2 MiB default cannot return
  a single larger event row. Configure adequate limits in external deployments
  too. Compose applies these values when the containers start, so recreate
  QuestDB after changing them. If the process stops between the two writes,
  rescanning the bucket repairs the missing outbox row. Event delivery is **at
  least once**; consumers should deduplicate using `event_id`, which is stable
  for a bar revision.
- Each timeframe has separate bars, outbox, cursor, ID registry, and
  store-and-forward paths. The `1m` worker retains the existing table and
  registry paths for compatibility; other workers use timeframe-suffixed names.
- Finalized Bars are also stored in the aggregator's append-only rkyv store.
  Records are CRC-checked, 16-byte aligned, and held in segmented files;
  `index.json` is atomically checkpointed and any uncheckpointed tail is scanned
  on startup. Replacements append a new record instead of changing mapped
  bytes. The in-memory sorted index supports `write`, `upsert`, `read`,
  `read_last_n`, and a validated zero-copy archived view. A Bar is written to
  this store before its QuestDB row; unchanged bars encountered during the
  normal reprocessing window are added if missing. QuestDB remains the source
  for tick queries, processing cursors, finalized-bar compatibility, and the
  durable Pulsar outbox.
- Bars use `(time, source, symbol)` as the UPSERT key within their timeframe
  table. If a late tick falls within the rescan window, the aggregator
  recomputes the affected bar, increments its revision, upserts it, and emits a
  new event.
- The timeframe-specific state table stores the next window to process, so
  empty windows and catch-up after downtime do not lose the processing cursor.
- Run a single active aggregator instance per timeframe/database: instances for
  distinct timeframes have separate state and persistence, but two instances
  configured for the same timeframe would still contend as writers.

QuestDB and Pulsar do not share a transaction. The outbox closes the crash gap
between bar persistence and publishing, but a crash after Pulsar accepts an
event and before QuestDB records it as published can cause a duplicate event.
Use the deterministic `event_id` for consumer-side idempotency.

## Metric conventions and source limitations

`price` and `qty` remain integers scaled by `10^8`; notional is their integer
product. `is_buyer_maker=0` contributes to buy flow and `=1` to sell flow;
other values remain in `raw_ticks` and contribute to total flow, but are
excluded from side-specific totals. Price changes and their moments/histogram
are integer price changes, not percentage returns. Inter-arrival times are
represented in microseconds, but the configured exchange sources expose event
timestamps at millisecond precision; their effective inter-arrival resolution
is therefore 1,000 microseconds, and same-millisecond trades can have zero
inter-arrival.

Ingestion accepts only positive price/quantity values that can be represented
exactly at the fixed `10^8` scale. Decimal input with more than eight fractional
places is accepted only when the extra places are zero; values that would need
rounding are logged and ignored. The merger and aggregator validate positive
price and quantity again. Existing invalid rows in QuestDB make bar generation
fail explicitly instead of silently affecting OHLC or volume.

The footprint uses observed price levels, a 70% volume value area, and a 3:1
buy/sell imbalance threshold. The value-area target uses known-side quantity;
unknown-side ticks remain in total flow but do not affect the footprint volume
target. The histogram fields have 16 equal-width bins;
each histogram also includes its minimum and maximum so its bins can be
interpreted. See `logic.md` for formulas, flag assignments, and the exact
checksum byte encoding.

`realized_variance` and `bipower_variation` currently aggregate absolute price
changes, not returns, and the latter omits the conventional `π/2` factor. The
absorption, exhaustion, and unfinished-auction fields are implementation
heuristics, not independently validated market signals. See `logic.md` before
using these fields as standardized financial measures.

In schema version 4, every signed or unsigned 64-bit and 128-bit integer is
serialized as an exact base-10 JSON string (including histogram arrays). This
avoids precision loss in JavaScript and other binary64-based consumers.
Version 4 also adds the bar timeframe, includes it in bar identity, and defines
`active_seconds` as distinct UTC seconds with ticks. Consumers should inspect
`schema_version`.
Previously persisted version-3 bars remain unchanged until their windows are
reprocessed or explicitly rebuilt, so history can contain both versions during
migration.

Some integrity inputs are not present in the current `ticks` schema: receive
timestamps, original arrival order, pre-ingestion duplicate counts, expected
sequence counts, and rejected/dropped tick counts. Their corresponding
unavailable fields are zero, and the `coverage_flags`/`quality_flags` bits
identify the missing coverage. `duplicate_count` counts repeated IDs present
in the bar (numeric IDs by value, non-numeric IDs by exact text), while
`out_of_order_count` counts numeric ID regressions in event-time order; neither
recovers original arrival order. Numeric trade IDs are used for ordering and
gap checks; non-numeric IDs are preserved verbatim in both the identity section
and `raw_ticks`.

Serialized Pulsar payload sizes depend on the number of raw ticks retained in a
bar. The development broker in `infra.yaml` allows messages up to 64 MiB via
`maxMessageSize`, and BookKeeper accepts the corresponding frame size. External
deployments must raise both limits above the largest encoded bar event. Set the
aggregator log filter to `debug` to record each encoded event's byte size.

Because the source table has no receive timestamp, `REPROCESS_MINUTES` is the
late-tick correction horizon, not a proof that arbitrarily late arrivals have
been observed. The horizon is rounded back to timeframe boundaries and scans
whole bars, so at least the latest closed bar is rescanned. For example, a
5-minute horizon on a `1d` worker still rescans the previous daily bar. Ticks
remain in QuestDB; increase this window or rebuild affected bars from the raw
table if data can arrive later than the configured horizon.

The source/symbol numeric IDs are allocated in the timeframe-specific registry
derived from `AGGREGATOR_ID_REGISTRY`. Keep each registry on persistent storage
and do not share it between active instances for the same timeframe.

## Configuration

| Variable | Default | Purpose |
| --- | --- | --- |
| `PULSAR_URL` | `pulsar://localhost:6650` | Pulsar broker |
| `PULSAR_NAMESPACE` | `persistent://public/default` | Namespace prefix for per-source/symbol/timeframe bar topics |
| `AGGREGATOR_TIMEFRAME` | `1m` | Timeframe this worker owns; the sample `.env.example` sets `5m` |
| `QUESTDB_CONNECT` | `ws::addr=localhost:9000` | QuestDB QWP connection |
| `QUESTDB_SF_DIR` | `./data/questdb-sf` | Persistent QuestDB client store-and-forward directory |
| `AGGREGATOR_BAR_STORE_DIR` | `./data/bars` | Append-only rkyv Bar segments and mmap index; isolated by timeframe |
| `AGGREGATOR_BAR_SEGMENT_BYTES` | `268435456` | Target maximum segment size; an individual Bar can be up to 64 MiB |
| `QUESTDB_MAX_BUF_SIZE_BYTES` | `134217728` | Maximum encoded QuestDB client buffer size; must be at least 32 MiB and no smaller than the segment size |
| `QUESTDB_SF_MAX_SEGMENT_BYTES` | `134217728` | QuestDB store-and-forward segment size; must be at least 32 MiB |
| `POLL_INTERVAL_SECONDS` | `60` | Aggregator polling cadence |
| `LIVE_POLL_INTERVAL_MS` | `1000` | Live bar snapshot cadence in milliseconds; must be greater than zero |
| `ALLOWED_LATENESS_SECONDS` | `5` | Delay after timeframe close before first calculation |
| `REPROCESS_MINUTES` | `5` | Late-tick horizon in minutes, rounded back to complete timeframe bars (at least one closed bar) |
| `AGGREGATOR_ID_REGISTRY` | `./data/ids.json` | Persistent source/symbol ID map |

Set the server's `PULSAR_NAMESPACE` to the same value as the aggregator when
overriding the default namespace.

// BLACKER - microstructure
// Copyright (C) 2026 Juan José Caballero Rey
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation version 3 of the License.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License
// along with this program. If not, see <https://www.gnu.org/licenses/>.

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashSet},
    fs,
    path::{Path, PathBuf},
};

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct IdRegistry {
    sources: BTreeMap<String, u8>,
    symbols: BTreeMap<String, u16>,
    #[serde(skip)]
    path: PathBuf,
}

impl IdRegistry {
    pub fn load(path: PathBuf) -> Result<Self> {
        let mut registry = if path.is_file() {
            let bytes = fs::read(&path)
                .with_context(|| format!("Failed to read ID registry {}", path.display()))?;
            serde_json::from_slice(&bytes)
                .with_context(|| format!("Invalid ID registry {}", path.display()))?
        } else {
            Self::default()
        };
        registry.path = path;
        Ok(registry)
    }

    pub fn load_for_timeframe(path: PathBuf, timeframe: &str) -> Result<Self> {
        if timeframe == "1m" {
            return Self::load(path);
        }

        let file_name = path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .context("ID registry filename is not valid UTF-8")?;
        let extension = path.extension().and_then(|extension| extension.to_str());
        let timeframe_file = match extension {
            Some(extension) => format!("{file_name}-{timeframe}.{extension}"),
            None => format!("{file_name}-{timeframe}"),
        };
        Self::load(path.with_file_name(timeframe_file))
    }

    pub fn ids_for(&mut self, source: &str, symbol: &str) -> Result<(u8, u16)> {
        let mut changed = false;
        let source_id = match self.sources.get(source) {
            Some(id) => *id,
            None => {
                let id = next_id(
                    self.sources.values().copied().map(u16::from),
                    u8::MAX.into(),
                )?;
                let id = u8::try_from(id)?;
                self.sources.insert(source.to_string(), id);
                changed = true;
                id
            }
        };
        let symbol_id = match self.symbols.get(symbol) {
            Some(id) => *id,
            None => {
                let id = next_id(self.symbols.values().copied(), u16::MAX)?;
                self.symbols.insert(symbol.to_string(), id);
                changed = true;
                id
            }
        };
        if changed {
            self.save()?;
        }
        Ok((source_id, symbol_id))
    }

    fn save(&self) -> Result<()> {
        let parent = self
            .path
            .parent()
            .context("ID registry path has no parent directory")?;
        fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create {}", parent.display()))?;
        let temp_path = self.path.with_extension("json.tmp");
        let contents = serde_json::to_vec_pretty(self).context("Failed to encode ID registry")?;
        fs::write(&temp_path, contents)
            .with_context(|| format!("Failed to write {}", temp_path.display()))?;
        fs::rename(&temp_path, &self.path).with_context(|| {
            format!(
                "Failed to atomically replace ID registry {}",
                self.path.display()
            )
        })?;
        Ok(())
    }
}

fn next_id(values: impl Iterator<Item = u16>, max: u16) -> Result<u16> {
    let used: HashSet<u16> = values.collect();
    (1..=max)
        .find(|candidate| !used.contains(candidate))
        .ok_or_else(|| anyhow::anyhow!("ID registry capacity ({max}) exhausted"))
}

#[allow(dead_code)]
fn validate_registry_path(path: &Path) -> Result<()> {
    if path.as_os_str().is_empty() {
        bail!("ID registry path cannot be empty");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::IdRegistry;

    #[test]
    fn ids_are_stable_and_unique() {
        let path = std::env::temp_dir().join(format!(
            "aggregator-ids-{}-{}.json",
            std::process::id(),
            std::thread::current().name().unwrap_or("test")
        ));
        let _ = std::fs::remove_file(&path);

        let mut registry = IdRegistry::load(path.clone()).unwrap();
        let first = registry.ids_for("binance", "BTCUSDT").unwrap();
        let second = registry.ids_for("dydx", "BTC-USD").unwrap();
        assert_ne!(first.0, second.0);
        assert_ne!(first.1, second.1);
        drop(registry);

        let mut reloaded = IdRegistry::load(path.clone()).unwrap();
        assert_eq!(reloaded.ids_for("binance", "BTCUSDT").unwrap(), first);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn timeframe_registry_paths_are_isolated_without_moving_the_1m_registry() {
        let path = std::path::PathBuf::from("/tmp/aggregator-ids.json");
        let one_minute = IdRegistry::load_for_timeframe(path.clone(), "1m").unwrap();
        let five_minutes = IdRegistry::load_for_timeframe(path, "5m").unwrap();

        assert_eq!(
            one_minute.path,
            std::path::PathBuf::from("/tmp/aggregator-ids.json")
        );
        assert_eq!(
            five_minutes.path,
            std::path::PathBuf::from("/tmp/aggregator-ids-5m.json")
        );
    }
}

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

use anyhow::{Context, Result, anyhow, bail, ensure};
use std::{
    env,
    path::{Path, PathBuf},
};

const DEFAULT_QUESTDB_MAX_BUF_SIZE_BYTES: usize = 32 * 1024 * 1024;
const DEFAULT_QUESTDB_SF_MAX_SEGMENT_BYTES: u64 = 32 * 1024 * 1024;
const DEFAULT_LIVE_POLL_INTERVAL_MS: u64 = 1_000;

#[derive(Debug, Clone)]
pub struct Config {
    pub pulsar_url: String,
    pub pulsar_namespace: String,
    pub timeframe: String,
    pub timeframe_ms: u64,
    pub questdb_connect: String,
    pub questdb_sf_dir: PathBuf,
    pub questdb_max_buf_size_bytes: usize,
    pub questdb_sf_max_segment_bytes: u64,
    pub id_registry_path: PathBuf,
    pub poll_interval_seconds: u64,
    pub live_poll_interval_ms: u64,
    pub allowed_lateness_seconds: u64,
    pub reprocess_minutes: u64,
}

impl Config {
    pub fn from_env() -> Result<Self> {
        let env_dev_path = Path::new(env!("CARGO_MANIFEST_DIR")).join(".env.dev");
        if env_dev_path.is_file() {
            dotenvy::from_path(&env_dev_path)
                .with_context(|| format!("Failed to load {}", env_dev_path.display()))?;
        }

        let timeframe = read("AGGREGATOR_TIMEFRAME", "1m");
        let timeframe_ms = timeframe_duration_ms(&timeframe).ok_or_else(|| {
            anyhow!(
                "AGGREGATOR_TIMEFRAME={} is not supported; use one of 1m, 5m, 15m, 30m, 1h, 4h, or 1d",
                timeframe
            )
        })?;
        let questdb_sf_dir = timeframe_path(
            resolve_path("QUESTDB_SF_DIR", "./data/questdb-sf"),
            &timeframe,
        );
        let config = Self {
            pulsar_url: read("PULSAR_URL", "pulsar://localhost:6650"),
            pulsar_namespace: read("PULSAR_NAMESPACE", "persistent://public/default"),
            timeframe,
            timeframe_ms,
            questdb_connect: read("QUESTDB_CONNECT", "ws::addr=localhost:9000"),
            questdb_sf_dir,
            questdb_max_buf_size_bytes: read_parse(
                "QUESTDB_MAX_BUF_SIZE_BYTES",
                DEFAULT_QUESTDB_MAX_BUF_SIZE_BYTES,
            )?,
            questdb_sf_max_segment_bytes: read_parse(
                "QUESTDB_SF_MAX_SEGMENT_BYTES",
                DEFAULT_QUESTDB_SF_MAX_SEGMENT_BYTES,
            )?,
            id_registry_path: resolve_path("AGGREGATOR_ID_REGISTRY", "./data/ids.json"),
            poll_interval_seconds: read_parse("POLL_INTERVAL_SECONDS", 60)?,
            live_poll_interval_ms: read_parse(
                "LIVE_POLL_INTERVAL_MS",
                DEFAULT_LIVE_POLL_INTERVAL_MS,
            )?,
            allowed_lateness_seconds: read_parse("ALLOWED_LATENESS_SECONDS", 5)?,
            reprocess_minutes: read_parse("REPROCESS_MINUTES", 5)?,
        };

        if config.pulsar_url.trim().is_empty() {
            bail!("PULSAR_URL cannot be empty");
        }
        if config.pulsar_namespace.trim().is_empty() {
            bail!("PULSAR_NAMESPACE cannot be empty");
        }
        if config.poll_interval_seconds == 0 {
            bail!("POLL_INTERVAL_SECONDS must be greater than zero");
        }
        if config.live_poll_interval_ms == 0 {
            bail!("LIVE_POLL_INTERVAL_MS must be greater than zero");
        }
        if config.reprocess_minutes == 0 {
            bail!("REPROCESS_MINUTES must be greater than zero");
        }
        if config.questdb_sf_dir.as_os_str().is_empty()
            || config.questdb_sf_dir.to_string_lossy().contains(';')
        {
            bail!("QUESTDB_SF_DIR must be a non-empty path without semicolons");
        }
        if config.questdb_max_buf_size_bytes < DEFAULT_QUESTDB_MAX_BUF_SIZE_BYTES {
            bail!("QUESTDB_MAX_BUF_SIZE_BYTES must be at least 33554432 (32 MiB)");
        }
        if config.questdb_sf_max_segment_bytes < DEFAULT_QUESTDB_SF_MAX_SEGMENT_BYTES {
            bail!("QUESTDB_SF_MAX_SEGMENT_BYTES must be at least 33554432 (32 MiB)");
        }
        ensure!(
            u64::try_from(config.questdb_max_buf_size_bytes)?
                >= config.questdb_sf_max_segment_bytes,
            "QUESTDB_MAX_BUF_SIZE_BYTES must be greater than or equal to \
             QUESTDB_SF_MAX_SEGMENT_BYTES"
        );
        if config.id_registry_path.as_os_str().is_empty() {
            bail!("AGGREGATOR_ID_REGISTRY cannot be empty");
        }
        if config.id_registry_path.to_string_lossy().contains(';') {
            bail!("AGGREGATOR_ID_REGISTRY cannot contain semicolons");
        }

        let questdb_connect = config.questdb_connect.trim();
        if !(questdb_connect.starts_with("ws::") || questdb_connect.starts_with("wss::"))
            || questdb_connect.contains('\n')
        {
            bail!("QUESTDB_CONNECT must be a QuestDB ws:: or wss:: connection string");
        }
        for (name, _) in questdb_connect
            .split(';')
            .filter_map(|part| part.split_once('='))
        {
            let option_name = name.trim().rsplit("::").next().unwrap_or(name.trim());
            if matches!(
                option_name.to_ascii_lowercase().as_str(),
                "sf_dir"
                    | "sf_durability"
                    | "sf_max_segment_bytes"
                    | "request_durable_ack"
                    | "sender_pool_min"
                    | "sender_pool_max"
                    | "max_buf_size"
            ) {
                bail!("QUESTDB_CONNECT cannot override aggregator-managed option {option_name}");
            }
        }

        Ok(config)
    }
}

fn timeframe_duration_ms(timeframe: &str) -> Option<u64> {
    match timeframe {
        "1m" => Some(60_000),
        "5m" => Some(5 * 60_000),
        "15m" => Some(15 * 60_000),
        "30m" => Some(30 * 60_000),
        "1h" => Some(60 * 60_000),
        "4h" => Some(4 * 60 * 60_000),
        "1d" => Some(24 * 60 * 60_000),
        _ => None,
    }
}

fn resolve_path(name: &str, default: &str) -> PathBuf {
    let path = PathBuf::from(read(name, default));
    if path.is_absolute() {
        path
    } else {
        Path::new(env!("CARGO_MANIFEST_DIR")).join(path)
    }
}

fn timeframe_path(path: PathBuf, timeframe: &str) -> PathBuf {
    if timeframe == "1m" {
        return path;
    }

    let Some(file_name) = path.file_stem().and_then(|stem| stem.to_str()) else {
        return path;
    };
    let timeframe_name = match path.extension().and_then(|extension| extension.to_str()) {
        Some(extension) => format!("{file_name}-{timeframe}.{extension}"),
        None => format!("{file_name}-{timeframe}"),
    };
    path.with_file_name(timeframe_name)
}

fn read(name: &str, default: &str) -> String {
    env::var(name)
        .map(|value| value.trim().to_string())
        .unwrap_or_else(|_| default.to_string())
}

fn read_parse<T>(name: &str, default: T) -> Result<T>
where
    T: std::str::FromStr,
    T::Err: std::fmt::Display,
{
    match env::var(name) {
        Ok(value) => value
            .trim()
            .parse()
            .map_err(|error| anyhow!("Invalid {name}: {error}")),
        Err(_) => Ok(default),
    }
}

#[cfg(test)]
mod tests {
    use super::{timeframe_duration_ms, timeframe_path};
    use std::path::PathBuf;

    #[test]
    fn keeps_the_1m_paths_and_separates_other_timeframes() {
        let registry = PathBuf::from("/data/ids.json");
        let store_and_forward = PathBuf::from("/data/questdb-sf");

        assert_eq!(
            timeframe_path(registry.clone(), "1m"),
            PathBuf::from("/data/ids.json")
        );
        assert_eq!(
            timeframe_path(registry, "5m"),
            PathBuf::from("/data/ids-5m.json")
        );
        assert_eq!(
            timeframe_path(store_and_forward, "5m"),
            PathBuf::from("/data/questdb-sf-5m")
        );
    }

    #[test]
    fn supports_the_timeframes_exposed_by_the_market_server() {
        assert_eq!(timeframe_duration_ms("1m"), Some(60_000));
        assert_eq!(timeframe_duration_ms("5m"), Some(300_000));
        assert_eq!(timeframe_duration_ms("15m"), Some(900_000));
        assert_eq!(timeframe_duration_ms("30m"), Some(1_800_000));
        assert_eq!(timeframe_duration_ms("1h"), Some(3_600_000));
        assert_eq!(timeframe_duration_ms("4h"), Some(14_400_000));
        assert_eq!(timeframe_duration_ms("1d"), Some(86_400_000));
        assert_eq!(timeframe_duration_ms("2m"), None);
    }
}

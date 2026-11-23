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

use anyhow::{Context, Result, anyhow, bail};
use std::{
    env,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone)]
pub struct Config {
    pub pulsar_url: String,
    pub pulsar_topic: String,
    pub questdb_connect: String,
    pub questdb_sf_dir: PathBuf,
    pub id_registry_path: PathBuf,
    pub poll_interval_seconds: u64,
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

        let config = Self {
            pulsar_url: read("PULSAR_URL", "pulsar://localhost:6650"),
            pulsar_topic: read("PULSAR_BARS_TOPIC", "persistent://public/default/bars-1m"),
            questdb_connect: read("QUESTDB_CONNECT", "ws::addr=localhost:9000"),
            questdb_sf_dir: resolve_path("QUESTDB_SF_DIR", "./data/questdb-sf"),
            id_registry_path: resolve_path("AGGREGATOR_ID_REGISTRY", "./data/ids.json"),
            poll_interval_seconds: read_parse("POLL_INTERVAL_SECONDS", 60)?,
            allowed_lateness_seconds: read_parse("ALLOWED_LATENESS_SECONDS", 5)?,
            reprocess_minutes: read_parse("REPROCESS_MINUTES", 5)?,
        };

        if config.pulsar_url.trim().is_empty() {
            bail!("PULSAR_URL cannot be empty");
        }
        if config.pulsar_topic.trim().is_empty() {
            bail!("PULSAR_BARS_TOPIC cannot be empty");
        }
        if config.poll_interval_seconds == 0 {
            bail!("POLL_INTERVAL_SECONDS must be greater than zero");
        }
        if config.reprocess_minutes == 0 {
            bail!("REPROCESS_MINUTES must be greater than zero");
        }
        if config.questdb_sf_dir.as_os_str().is_empty()
            || config.questdb_sf_dir.to_string_lossy().contains(';')
        {
            bail!("QUESTDB_SF_DIR must be a non-empty path without semicolons");
        }
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
                    | "request_durable_ack"
                    | "sender_pool_min"
                    | "sender_pool_max"
            ) {
                bail!("QUESTDB_CONNECT cannot override aggregator-managed option {option_name}");
            }
        }

        Ok(config)
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

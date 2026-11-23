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
    pub pulsar_subscription: String,
    pub pulsar_max_redeliver_count: usize,
    pub pulsar_dead_letter_topic: String,
    pub redis_url: String,
    pub redis_key_prefix: String,
    pub redis_ttl_seconds: u64,
    pub questdb_connect: String,
    pub questdb_sf_dir: PathBuf,
}

impl Config {
    pub fn from_env() -> Result<Self> {
        let env_dev_path = Path::new(env!("CARGO_MANIFEST_DIR")).join(".env.dev");
        if env_dev_path.is_file() {
            dotenvy::from_path(&env_dev_path)
                .with_context(|| format!("Failed to load {}", env_dev_path.display()))?;
        }

        let questdb_sf_dir = PathBuf::from(read("QUESTDB_SF_DIR", "./data/questdb-sf"));
        let questdb_sf_dir = if questdb_sf_dir.is_absolute() {
            questdb_sf_dir
        } else {
            Path::new(env!("CARGO_MANIFEST_DIR")).join(questdb_sf_dir)
        };

        let config = Self {
            pulsar_url: read("PULSAR_URL", "pulsar://localhost:6650"),
            pulsar_subscription: read("PULSAR_SUBSCRIPTION", "tick-persistence"),
            pulsar_max_redeliver_count: read_parse("PULSAR_MAX_REDELIVER_COUNT", 10)?,
            pulsar_dead_letter_topic: read(
                "PULSAR_DEAD_LETTER_TOPIC",
                "persistent://public/default/merger-dead-letter",
            ),
            redis_url: read("REDIS_URL", "redis://localhost:6379/0"),
            redis_key_prefix: read("REDIS_KEY_PREFIX", "merger:tick:v1"),
            redis_ttl_seconds: read_parse("REDIS_TTL_SECONDS", 30 * 24 * 60 * 60)?,
            questdb_connect: read("QUESTDB_CONNECT", "ws::addr=localhost:9000"),
            questdb_sf_dir,
        };

        if config.pulsar_subscription.trim().is_empty() {
            bail!("PULSAR_SUBSCRIPTION cannot be empty");
        }
        if config.pulsar_dead_letter_topic.trim().is_empty() {
            bail!("PULSAR_DEAD_LETTER_TOPIC cannot be empty");
        }
        if config.pulsar_max_redeliver_count == 0 {
            bail!("PULSAR_MAX_REDELIVER_COUNT must be greater than zero");
        }
        if config.redis_ttl_seconds == 0 {
            bail!("REDIS_TTL_SECONDS must be greater than zero");
        }
        if config.redis_key_prefix.trim().is_empty() {
            bail!("REDIS_KEY_PREFIX cannot be empty");
        }
        let questdb_connect = config.questdb_connect.trim();
        if !(questdb_connect.starts_with("ws::") || questdb_connect.starts_with("wss::"))
            || questdb_connect.contains('\n')
        {
            bail!("QUESTDB_CONNECT must be a QuestDB ws:: or wss:: connection string");
        }
        for option in questdb_connect
            .split(';')
            .filter_map(|part| part.split_once('='))
        {
            let option_name = option
                .0
                .trim()
                .rsplit("::")
                .next()
                .unwrap_or(option.0.trim());
            if matches!(
                option_name.to_ascii_lowercase().as_str(),
                "sf_dir"
                    | "sf_durability"
                    | "request_durable_ack"
                    | "sender_pool_min"
                    | "sender_pool_max"
            ) {
                bail!(
                    "QUESTDB_CONNECT cannot override merger-managed option {}",
                    option_name
                );
            }
        }
        if config.questdb_sf_dir.as_os_str().is_empty()
            || config.questdb_sf_dir.to_string_lossy().contains(';')
        {
            bail!("QUESTDB_SF_DIR must be a non-empty path without semicolons");
        }

        Ok(config)
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

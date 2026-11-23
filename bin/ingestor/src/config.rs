use anyhow::{Context, Result, bail};
use std::{env, path::Path};

#[derive(Debug, Clone)]
pub struct Config {
    pub source: String,
    pub symbols: Vec<String>,
    pub pulsar_url: String,
}

impl Config {
    pub fn from_env() -> Result<Self> {
        let env_dev_path: std::path::PathBuf = Path::new(env!("CARGO_MANIFEST_DIR")).join(".env.dev");
        if env_dev_path.is_file() {
            dotenvy::from_path(&env_dev_path)
                .with_context(|| format!("Failed to load {}", env_dev_path.display()))?;
        }

        let source: String = env::var("SOURCE")
            .context("Missing SOURCE environment variable")?
            .trim()
            .to_ascii_lowercase();

        let symbols: Vec<String> = env::var("SYMBOLS")
            .context("Missing SYMBOLS environment variable")?
            .split(',')
            .map(str::trim)
            .filter(|symbol| !symbol.is_empty())
            .map(str::to_ascii_uppercase)
            .collect();

        if symbols.is_empty() {
            bail!("SYMBOLS must contain at least one comma-separated symbol");
        }

        let pulsar_url: String =
            env::var("PULSAR_URL").unwrap_or_else(|_| "pulsar://localhost:6650".to_string());

        Ok(Self {
            source,
            symbols,
            pulsar_url,
        })
    }
}

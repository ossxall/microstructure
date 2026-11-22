use anyhow::{Context, Result, bail};
use std::env;

#[derive(Debug, Clone)]
pub struct Config {
    pub source: String,
    pub symbols: Vec<String>,
    pub pulsar_url: String,
}

impl Config {
    pub fn from_env() -> Result<Self> {
        let source = env::var("SOURCE")
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

        let pulsar_url =
            env::var("PULSAR_URL").unwrap_or_else(|_| "pulsar://localhost:6650".to_string());

        Ok(Self {
            source,
            symbols,
            pulsar_url,
        })
    }
}

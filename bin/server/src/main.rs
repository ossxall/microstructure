// BLACKER
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

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::Arc,
};

use aggregator::bar_store::BarStore;
use anyhow::{Context, Result};
use pulsar::{Pulsar, TokioExecutor};
use questdb::QuestDb;
use server::{server::start_http_server, state::AppState};

const DEFAULT_BAR_STORE_SEGMENT_BYTES: u64 = 256 * 1024 * 1024;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    let questdb_connect =
        std::env::var("QUESTDB_CONNECT").unwrap_or_else(|_| "ws::addr=localhost:9000".to_string());
    let questdb = QuestDb::connect(&questdb_connect).context("Failed to connect to QuestDB")?;

    {
        let mut reader = questdb
            .borrow_reader()
            .context("Failed to open QuestDB health-check reader")?;
        let mut cursor = reader
            .execute("SELECT 1")
            .context("Failed to query QuestDB")?;
        while cursor
            .next_batch()
            .context("Failed while reading the QuestDB health check")?
            .is_some()
        {}
    }

    let pulsar_url =
        std::env::var("PULSAR_URL").unwrap_or_else(|_| "pulsar://localhost:6650".to_string());
    let pulsar: Arc<Pulsar<TokioExecutor>> = Arc::new(
        Pulsar::builder(pulsar_url, TokioExecutor)
            .build()
            .await
            .context("Failed to connect to Pulsar")?,
    );

    let pulsar_namespace = std::env::var("PULSAR_NAMESPACE")
        .unwrap_or_else(|_| "persistent://public/default".to_string());
    anyhow::ensure!(
        !pulsar_namespace.trim().is_empty(),
        "PULSAR_NAMESPACE cannot be empty"
    );
    let bar_store_base =
        std::env::var("AGGREGATOR_BAR_STORE_DIR").unwrap_or_else(|_| "./data/bars".to_string());
    anyhow::ensure!(
        !bar_store_base.trim().is_empty(),
        "AGGREGATOR_BAR_STORE_DIR cannot be empty"
    );
    let bar_store_base = PathBuf::from(bar_store_base.trim());
    let bar_store_base = if bar_store_base.is_absolute() {
        bar_store_base
    } else {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../aggregator")
            .join(bar_store_base)
    };
    let segment_bytes = std::env::var("AGGREGATOR_BAR_SEGMENT_BYTES")
        .ok()
        .map(|value| {
            value
                .trim()
                .parse::<u64>()
                .context("AGGREGATOR_BAR_SEGMENT_BYTES must be an unsigned integer")
        })
        .transpose()?
        .unwrap_or(DEFAULT_BAR_STORE_SEGMENT_BYTES);
    let mut bar_stores = BTreeMap::new();
    for timeframe in server::market::SUPPORTED_TIMEFRAMES {
        let directory = server::market::bar_store_path(&bar_store_base, timeframe);
        let store = BarStore::open_read_only(&directory, segment_bytes)
            .with_context(|| format!("Failed to open Bar history store {}", directory.display()))?;
        tracing::info!(
            timeframe,
            directory = %directory.display(),
            bars = store.len(),
            "Opened read-only Bar history store"
        );
        bar_stores.insert(timeframe.to_string(), Arc::new(store));
    }
    let state: AppState = AppState::new(
        Arc::new(questdb),
        pulsar,
        pulsar_namespace.trim().trim_end_matches('/').to_string(),
        bar_stores,
    );

    start_http_server(state).await;

    Ok(())
}

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

use std::sync::Arc;

use anyhow::{Context, Result};
use pulsar::{Pulsar, TokioExecutor};
use questdb::QuestDb;
use server::{server::start_http_server, state::AppState};

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
    let state: AppState = AppState::new(
        Arc::new(questdb),
        pulsar,
        pulsar_namespace.trim().trim_end_matches('/').to_string(),
    );

    start_http_server(state).await;

    Ok(())
}

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

use anyhow::Result;
use async_channel;
use ingestor::{
    config::Config,
    models::Tick,
    sources::endpoint::get_source_endpoint,
    tasks::{batch_dispatcher, listen_source_ws, pulsar_writer},
};
use pulsar::{Producer, Pulsar, TokioExecutor};
use rustls::crypto::ring;
use std::collections::HashMap;
use tokio::task::JoinSet;
use tracing::{error, info};

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    ring::default_provider()
        .install_default()
        .expect("Failed to install rustls crypto provider");

    let config: Config = Config::from_env()?;
    get_source_endpoint(&config.source)?;

    let pulsar: Pulsar<TokioExecutor> = Pulsar::builder(&config.pulsar_url, TokioExecutor)
        .build()
        .await?;
    let producers: HashMap<String, Producer<TokioExecutor>> =
        pulsar_writer::create_producers(&pulsar, &config.source, &config.symbols).await?;

    info!(
        "Ingesting source {}, symbols {}, publishing to Pulsar at {}",
        config.source,
        config.symbols.join(","),
        config.pulsar_url
    );

    let (tick_tx, tick_rx) = async_channel::bounded::<Vec<Tick>>(100_000);
    let (batch_tx, batch_rx) = async_channel::bounded::<Vec<Tick>>(200);

    let mut tasks: JoinSet<Result<()>> = JoinSet::new();

    tasks.spawn(listen_source_ws::run(
        tick_tx,
        config.source,
        config.symbols.join(","),
    ));

    tasks.spawn(batch_dispatcher::run(tick_rx, batch_tx));

    tasks.spawn(pulsar_writer::run(producers, batch_rx));

    while let Some(result) = tasks.join_next().await {
        match result {
            Ok(Ok(())) => {
                error!("Critical task exited unexpectedly");
                std::process::exit(1);
            }

            Ok(Err(err)) => {
                error!("Critical task failed: {err:#}");
                std::process::exit(1);
            }

            Err(err) => {
                error!("Task panicked: {err:#}");
                std::process::exit(1);
            }
        }
    }

    Ok(())
}

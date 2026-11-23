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

use crate::models::Tick;
use anyhow::{Context, Result, anyhow};
use async_channel::Receiver;
use pulsar::{Error as PulsarError, Producer, Pulsar, SerializeMessage, TokioExecutor, producer};
use serde::Serialize;
use std::collections::HashMap;
use tracing::info;

#[derive(Serialize)]
struct TickBatchMessage {
    ticks: Vec<Tick>,
}

impl SerializeMessage for TickBatchMessage {
    fn serialize_message(input: Self) -> std::result::Result<producer::Message, PulsarError> {
        let payload =
            rmp_serde::to_vec(&input).map_err(|error| PulsarError::Custom(error.to_string()))?;

        Ok(producer::Message {
            payload,
            ..Default::default()
        })
    }
}

/// Creates one producer per symbol, using the topic naming convention shared
/// with the rest of the application: `ticks-{source}-{symbol}`.
pub async fn create_producers(
    pulsar: &Pulsar<TokioExecutor>,
    source: &str,
    symbols: &[String],
) -> Result<HashMap<String, Producer<TokioExecutor>>> {
    let mut producers = HashMap::with_capacity(symbols.len());

    for symbol in symbols {
        let topic = format!("persistent://public/default/ticks-{source}-{symbol}");
        let producer = pulsar
            .producer()
            .with_topic(topic)
            .build()
            .await
            .with_context(|| format!("Failed to create Pulsar producer for {symbol}"))?;
        producers.insert(symbol.clone(), producer);
    }

    Ok(producers)
}

/// Routes each incoming batch to the producer corresponding to each tick's
/// symbol, and waits for Pulsar's delivery acknowledgement.
pub async fn run(
    mut producers: HashMap<String, Producer<TokioExecutor>>,
    batch_rx: Receiver<Vec<Tick>>,
) -> Result<()> {
    loop {
        let batch = batch_rx
            .recv()
            .await
            .map_err(|_| anyhow!("batch channel closed"))?;

        let mut ticks_by_symbol: HashMap<String, Vec<Tick>> = HashMap::new();
        for tick in batch {
            ticks_by_symbol
                .entry(tick.symbol.clone())
                .or_default()
                .push(tick);
        }

        for (symbol, ticks) in ticks_by_symbol {
            let producer = producers
                .get_mut(&symbol)
                .with_context(|| format!("No Pulsar producer configured for symbol {symbol}"))?;

            info!("Publishing {} ticks for {symbol}", ticks.len());

            let delivery = producer
                .send_non_blocking(TickBatchMessage { ticks })
                .await
                .with_context(|| format!("Failed to publish ticks for {symbol}"))?;
            delivery.await.with_context(|| {
                format!("Failed to receive Pulsar acknowledgement for {symbol}")
            })?;
        }
    }
}

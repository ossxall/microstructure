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

mod config;
mod model;
mod persistence;
mod registry;

use anyhow::{Context, Result};
use chrono::Utc;
use config::Config;
use model::{Bar1m, Tick, build_bar};
use persistence::{BarEvent, PendingEvent};
use pulsar::{Pulsar, TokioExecutor, producer::Producer};
use questdb::QuestDb;
use registry::IdRegistry;
use std::{collections::BTreeMap, fs, sync::Arc, time::Duration};
use tokio::time::{Instant, interval_at};
use tracing::{debug, error, info};
use tracing_subscriber::EnvFilter;

const MINUTE_MS: u64 = 60_000;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();

    let config = Config::from_env()?;
    fs::create_dir_all(&config.questdb_sf_dir).with_context(|| {
        format!(
            "Failed to create QuestDB store-and-forward directory {}",
            config.questdb_sf_dir.display()
        )
    })?;
    let mut ids = IdRegistry::load(config.id_registry_path.clone())?;
    let questdb = connect_questdb(&config).await?;
    let pulsar = Pulsar::builder(&config.pulsar_url, TokioExecutor)
        .build()
        .await
        .context("Failed to connect to Pulsar")?;
    let mut producer = pulsar
        .producer()
        .with_topic(&config.pulsar_topic)
        .with_name("microstructure-aggregator-bars-1m")
        .build()
        .await
        .context("Failed to create 1-minute bar event producer")?;

    info!(
        topic = %config.pulsar_topic,
        poll_seconds = config.poll_interval_seconds,
        allowed_lateness_seconds = config.allowed_lateness_seconds,
        reprocess_minutes = config.reprocess_minutes,
        "Aggregator started"
    );

    let period = Duration::from_secs(config.poll_interval_seconds);
    let first_tick = aligned_next_poll(&config, period);
    let mut poll = interval_at(first_tick, period);
    let mut next_minute_ms = initialize_cursor(Arc::clone(&questdb)).await?;

    // Run once at startup to recover completed bars and outbox events, then
    // continue on the configured cadence.
    if let Err(error) = run_cycle(
        &config,
        Arc::clone(&questdb),
        &mut producer,
        &mut ids,
        &mut next_minute_ms,
    )
    .await
    {
        error!(
            error = %format!("{error:#}"),
            "Aggregator startup cycle failed; it will retry on the next poll"
        );
    }

    loop {
        tokio::select! {
            _ = poll.tick() => {
                if let Err(error) = run_cycle(
                    &config,
                    Arc::clone(&questdb),
                    &mut producer,
                    &mut ids,
                    &mut next_minute_ms,
                ).await {
                    error!(
                    error = %format!("{error:#}"),
                    "Aggregator cycle failed; work remains recoverable from QuestDB"
                    );
                }
            }
            signal = tokio::signal::ctrl_c() => {
                signal.context("Failed waiting for Ctrl-C")?;
                info!("Aggregator shutdown requested");
                break;
            }
        }
    }

    producer
        .close()
        .await
        .context("Failed to close Pulsar producer")?;
    Ok(())
}

async fn connect_questdb(config: &Config) -> Result<Arc<QuestDb>> {
    let connect_string = questdb_connect_string(config);
    let db = tokio::task::spawn_blocking(move || {
        let db = persistence::connect(&connect_string)?;
        persistence::ensure_schema(&db)?;
        Ok::<_, anyhow::Error>(Arc::new(db))
    })
    .await
    .context("QuestDB initialization task failed")??;
    info!(
        store_and_forward = %config.questdb_sf_dir.display(),
        "Connected to QuestDB and verified aggregator schemas"
    );
    Ok(db)
}

fn questdb_connect_string(config: &Config) -> String {
    format!(
        "{};sf_dir={};sf_durability=periodic;request_durable_ack=local;\
         sender_pool_min=1;sender_pool_max=1;max_buf_size={};sf_max_segment_bytes={};",
        config.questdb_connect.trim_end_matches(';'),
        config.questdb_sf_dir.display(),
        config.questdb_max_buf_size_bytes,
        config.questdb_sf_max_segment_bytes
    )
}

fn aligned_next_poll(config: &Config, period: Duration) -> Instant {
    let now_ms = u64::try_from(Utc::now().timestamp_millis()).unwrap_or(0);
    let minute_start = now_ms.div_euclid(MINUTE_MS) * MINUTE_MS;
    let lateness_ms = config.allowed_lateness_seconds.saturating_mul(1_000);
    let target = minute_start
        .saturating_add(MINUTE_MS)
        .saturating_add(lateness_ms);
    let delay_ms = target.saturating_sub(now_ms).max(1);
    let configured_ms = u64::try_from(period.as_millis()).unwrap_or(u64::MAX);
    Instant::now() + Duration::from_millis(delay_ms.min(configured_ms))
}

async fn initialize_cursor(db: Arc<QuestDb>) -> Result<Option<u64>> {
    let saved = {
        let db = Arc::clone(&db);
        tokio::task::spawn_blocking(move || persistence::read_cursor_ms(&db))
            .await
            .context("QuestDB cursor query task failed")??
    };
    if saved.is_some() {
        return Ok(saved);
    }
    let first_tick = {
        let db = Arc::clone(&db);
        tokio::task::spawn_blocking(move || persistence::first_tick_time(&db))
            .await
            .context("QuestDB first tick query task failed")??
    };
    first_tick
        .map(|time_us| {
            let time_ms = u64::try_from(time_us / 1_000)
                .context("First tick timestamp is before the Unix epoch")?;
            Ok(time_ms.div_euclid(MINUTE_MS) * MINUTE_MS)
        })
        .transpose()
}

async fn run_cycle(
    config: &Config,
    db: Arc<QuestDb>,
    producer: &mut Producer<TokioExecutor>,
    ids: &mut IdRegistry,
    next_minute_ms: &mut Option<u64>,
) -> Result<()> {
    dispatch_pending_events(Arc::clone(&db), producer).await?;

    if next_minute_ms.is_none() {
        *next_minute_ms = initialize_cursor(Arc::clone(&db)).await?;
    }
    let Some(next) = *next_minute_ms else {
        return Ok(());
    };
    let first_tick = {
        let db = Arc::clone(&db);
        tokio::task::spawn_blocking(move || persistence::first_tick_time(&db))
            .await
            .context("QuestDB first tick query task failed")??
    };
    let Some(first_tick_us) = first_tick else {
        return Ok(());
    };
    let earliest_tick_ms = u64::try_from(first_tick_us)
        .context("First tick timestamp is before the Unix epoch")?
        / 1_000;
    let earliest_bucket_ms = earliest_tick_ms.div_euclid(MINUTE_MS) * MINUTE_MS;

    let now_ms = u64::try_from(Utc::now().timestamp_millis())?;
    let deadline_ms = now_ms.saturating_sub(config.allowed_lateness_seconds.saturating_mul(1_000));
    let latest_closed_start =
        (deadline_ms.div_euclid(MINUTE_MS) * MINUTE_MS).checked_sub(MINUTE_MS);
    let Some(latest_closed_start) = latest_closed_start else {
        return Ok(());
    };

    let reprocess_span_ms = config
        .reprocess_minutes
        .saturating_sub(1)
        .saturating_mul(MINUTE_MS);
    let scan_start = next
        .saturating_sub(reprocess_span_ms)
        .max(earliest_bucket_ms);
    let mut bucket_ms = scan_start;
    while bucket_ms <= latest_closed_start {
        process_bucket(Arc::clone(&db), ids, bucket_ms).await?;
        dispatch_pending_events(Arc::clone(&db), producer).await?;

        if bucket_ms >= next {
            let following = bucket_ms
                .checked_add(MINUTE_MS)
                .context("Aggregator cursor overflows u64")?;
            {
                let db = Arc::clone(&db);
                tokio::task::spawn_blocking(move || persistence::persist_cursor(&db, following))
                    .await
                    .context("QuestDB cursor persistence task failed")??;
            }
            *next_minute_ms = Some(following);
        }
        bucket_ms = bucket_ms
            .checked_add(MINUTE_MS)
            .context("Minute bucket timestamp overflows u64")?;
    }
    Ok(())
}

async fn process_bucket(
    db: Arc<QuestDb>,
    ids: &mut IdRegistry,
    bucket_start_ms: u64,
) -> Result<()> {
    let start_us = i64::try_from(
        bucket_start_ms
            .checked_mul(1_000)
            .context("Minute bucket timestamp overflows")?,
    )?;
    let end_us = i64::try_from(
        bucket_start_ms
            .checked_add(MINUTE_MS)
            .context("Minute bucket end overflows")?
            .checked_mul(1_000)
            .context("Minute bucket end timestamp overflows")?,
    )?;
    let ticks = {
        let db = Arc::clone(&db);
        tokio::task::spawn_blocking(move || persistence::load_ticks(&db, start_us, end_us))
            .await
            .context("QuestDB tick query task failed")??
    };
    if ticks.is_empty() {
        return Ok(());
    }

    let mut groups = BTreeMap::<(String, String), Vec<Tick>>::new();
    for tick in ticks {
        groups
            .entry((tick.source.clone(), tick.symbol.clone()))
            .or_default()
            .push(tick);
    }
    let bucket_time_us = start_us;
    let existing = {
        let db = Arc::clone(&db);
        tokio::task::spawn_blocking(move || persistence::existing_bars(&db, bucket_time_us))
            .await
            .context("QuestDB existing-bars query task failed")??
    };

    for ((source, symbol), group) in groups {
        let (source_id, symbol_id) = ids.ids_for(&source, &symbol)?;
        let mut bar =
            build_bar(&group, bucket_start_ms, source_id, symbol_id).with_context(|| {
                format!("Failed to aggregate {source}/{symbol} at {bucket_start_ms}")
            })?;
        let prior = existing.get(&(source.clone(), symbol.clone()));
        let mut changed = true;
        if let Some(prior) = prior {
            bar.revision = prior.revision;
            let current_payload = serde_json::to_string(&bar)?;
            if current_payload == prior.payload {
                changed = false;
            } else {
                bar.revision = prior
                    .revision
                    .checked_add(1)
                    .context("Bar revision overflows u64")?;
            }
        }

        let event_id = format!("{}:r{}", bar.bar_key, bar.revision);
        let time_us = start_us;
        let event_exists = {
            let db = Arc::clone(&db);
            let event_id = event_id.clone();
            tokio::task::spawn_blocking(move || {
                persistence::outbox_event_exists(&db, time_us, &event_id)
            })
            .await
            .context("QuestDB outbox lookup task failed")??
        };
        if !changed && event_exists {
            continue;
        }

        if changed {
            persist_bar(Arc::clone(&db), &bar, &event_id).await?;
        }
        persist_event(Arc::clone(&db), &bar, &event_id).await?;
        info!(
            source = %bar.source,
            symbol = %bar.symbol,
            timestamp = bar.timestamp,
            revision = bar.revision,
            ticks = bar.identity.n_trades,
            "Persisted 1-minute bar and queued Pulsar event"
        );
    }

    Ok(())
}

async fn persist_bar(db: Arc<QuestDb>, bar: &Bar1m, event_id: &str) -> Result<()> {
    let db = Arc::clone(&db);
    let bar = bar.clone();
    let event_id = event_id.to_string();
    tokio::task::spawn_blocking(move || persistence::persist_bar(&db, &bar, &event_id))
        .await
        .context("QuestDB bar persistence task failed")??;
    Ok(())
}

async fn persist_event(db: Arc<QuestDb>, bar: &Bar1m, event_id: &str) -> Result<()> {
    let event = BarEvent {
        event_id,
        event_type: "bar.1m",
        timestamp: bar.timestamp,
        source: &bar.source,
        symbol: &bar.symbol,
        revision: bar.revision,
        data: bar,
    };
    let payload = serde_json::to_string(&event).context("Failed to encode bar event")?;
    debug!(
        event_id,
        payload_bytes = payload.len(),
        "Encoded 1-minute bar event"
    );
    let db = Arc::clone(&db);
    let bar = bar.clone();
    let event_id = event_id.to_string();
    tokio::task::spawn_blocking(move || {
        persistence::persist_pending_event(&db, &bar, &event_id, &payload)
    })
    .await
    .context("QuestDB outbox persistence task failed")??;
    Ok(())
}

async fn dispatch_pending_events(
    db: Arc<QuestDb>,
    producer: &mut Producer<TokioExecutor>,
) -> Result<()> {
    let pending = {
        let db = Arc::clone(&db);
        tokio::task::spawn_blocking(move || persistence::pending_events(&db))
            .await
            .context("QuestDB outbox query task failed")??
    };
    for event in pending {
        send_event(producer, &event).await?;
        let db = Arc::clone(&db);
        tokio::task::spawn_blocking(move || persistence::mark_event_published(&db, &event))
            .await
            .context("QuestDB outbox acknowledgement task failed")??;
    }
    Ok(())
}

async fn send_event(producer: &mut Producer<TokioExecutor>, event: &PendingEvent) -> Result<()> {
    producer
        .send_non_blocking(event.pulsar_payload().to_string())
        .await
        .context("Failed to enqueue bar event in Pulsar")?
        .await
        .context("Pulsar did not acknowledge bar event")?;
    info!(
        event_id = %event.event_id,
        source = %event.source,
        symbol = %event.symbol,
        revision = event.revision,
        "Published persisted 1-minute bar event"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::questdb_connect_string;
    use crate::config::Config;
    use std::path::PathBuf;

    #[test]
    fn questdb_connection_applies_frame_limits_and_keeps_them_above_the_reported_failure() {
        let config = Config {
            pulsar_url: "pulsar://localhost:6650".to_string(),
            pulsar_topic: "persistent://public/default/bars-1m".to_string(),
            questdb_connect: "ws::addr=localhost:9000".to_string(),
            questdb_sf_dir: PathBuf::from("/tmp/aggregator-sf"),
            questdb_max_buf_size_bytes: 32 * 1024 * 1024,
            questdb_sf_max_segment_bytes: 32 * 1024 * 1024,
            id_registry_path: PathBuf::from("/tmp/aggregator-ids.json"),
            poll_interval_seconds: 60,
            allowed_lateness_seconds: 5,
            reprocess_minutes: 5,
        };

        let connect_string = questdb_connect_string(&config);
        assert!(connect_string.contains("max_buf_size=33554432"));
        assert!(connect_string.contains("sf_max_segment_bytes=33554432"));
        assert!(
            questdb::ingress::SenderBuilder::from_conf(
                "ws::addr=localhost:9000;max_buf_size=33554432;sf_max_segment_bytes=33554432;"
            )
            .is_ok()
        );
    }
}

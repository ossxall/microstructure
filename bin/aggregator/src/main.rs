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
use model::{Bar, Tick, build_bar_for_timeframe};
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
    let mut ids =
        IdRegistry::load_for_timeframe(config.id_registry_path.clone(), &config.timeframe)?;
    let questdb = connect_questdb(&config).await?;
    let pulsar = Pulsar::builder(&config.pulsar_url, TokioExecutor)
        .build()
        .await
        .context("Failed to connect to Pulsar")?;
    let mut producers = BTreeMap::<String, Producer<TokioExecutor>>::new();

    info!(
        timeframe = %config.timeframe,
        pulsar_namespace = %config.pulsar_namespace,
        poll_seconds = config.poll_interval_seconds,
        allowed_lateness_seconds = config.allowed_lateness_seconds,
        reprocess_minutes = config.reprocess_minutes,
        "Aggregator started"
    );

    let period = Duration::from_secs(config.poll_interval_seconds);
    let first_tick = aligned_next_poll(&config, period);
    let mut poll = interval_at(first_tick, period);
    let mut next_bucket_ms =
        initialize_cursor(Arc::clone(&questdb), &config.timeframe, config.timeframe_ms).await?;

    // Run once at startup to recover completed bars and outbox events, then
    // continue on the configured cadence.
    if let Err(error) = run_cycle(
        &config,
        Arc::clone(&questdb),
        &pulsar,
        &mut producers,
        &mut ids,
        &mut next_bucket_ms,
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
                    &pulsar,
                    &mut producers,
                    &mut ids,
                    &mut next_bucket_ms,
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

    for producer in producers.values_mut() {
        producer
            .close()
            .await
            .context("Failed to close Pulsar producer")?;
    }
    Ok(())
}

async fn connect_questdb(config: &Config) -> Result<Arc<QuestDb>> {
    let connect_string = questdb_connect_string(config);
    let timeframe = config.timeframe.clone();
    let db = tokio::task::spawn_blocking(move || {
        let db = persistence::connect(&connect_string)?;
        persistence::ensure_schema(&db, &timeframe)?;
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
    let bucket_start = now_ms.div_euclid(config.timeframe_ms) * config.timeframe_ms;
    let lateness_ms = config.allowed_lateness_seconds.saturating_mul(1_000);
    let target = bucket_start
        .saturating_add(config.timeframe_ms)
        .saturating_add(lateness_ms);
    let delay_ms = target.saturating_sub(now_ms).max(1);
    let configured_ms = u64::try_from(period.as_millis()).unwrap_or(u64::MAX);
    Instant::now() + Duration::from_millis(delay_ms.min(configured_ms))
}

async fn initialize_cursor(
    db: Arc<QuestDb>,
    timeframe: &str,
    timeframe_ms: u64,
) -> Result<Option<u64>> {
    let saved = {
        let db = Arc::clone(&db);
        let timeframe = timeframe.to_string();
        tokio::task::spawn_blocking(move || persistence::read_cursor_ms(&db, &timeframe))
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
            Ok(time_ms.div_euclid(timeframe_ms) * timeframe_ms)
        })
        .transpose()
}

async fn run_cycle(
    config: &Config,
    db: Arc<QuestDb>,
    pulsar: &Pulsar<TokioExecutor>,
    producers: &mut BTreeMap<String, Producer<TokioExecutor>>,
    ids: &mut IdRegistry,
    next_bucket_ms: &mut Option<u64>,
) -> Result<()> {
    dispatch_pending_events(
        Arc::clone(&db),
        &config.timeframe,
        pulsar,
        producers,
        &config.pulsar_namespace,
    )
    .await?;

    if next_bucket_ms.is_none() {
        *next_bucket_ms =
            initialize_cursor(Arc::clone(&db), &config.timeframe, config.timeframe_ms).await?;
    }
    let Some(next) = *next_bucket_ms else {
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
    let interval_ms = config.timeframe_ms;
    let earliest_bucket_ms = earliest_tick_ms.div_euclid(interval_ms) * interval_ms;

    let now_ms = u64::try_from(Utc::now().timestamp_millis())?;
    let deadline_ms = now_ms.saturating_sub(config.allowed_lateness_seconds.saturating_mul(1_000));
    let latest_closed_start =
        (deadline_ms.div_euclid(interval_ms) * interval_ms).checked_sub(interval_ms);
    let Some(latest_closed_start) = latest_closed_start else {
        return Ok(());
    };

    let reprocess_span_ms = config.reprocess_minutes.saturating_mul(MINUTE_MS);
    // Recompute whole timeframe buckets, rounding the time horizon backward to
    // its containing bucket (at least the most recently closed bucket).
    let scan_start = (next.saturating_sub(reprocess_span_ms) / interval_ms * interval_ms)
        .max(earliest_bucket_ms);
    let mut bucket_ms = scan_start;
    while bucket_ms <= latest_closed_start {
        process_bucket(
            Arc::clone(&db),
            &config.timeframe,
            interval_ms,
            ids,
            bucket_ms,
        )
        .await?;
        dispatch_pending_events(
            Arc::clone(&db),
            &config.timeframe,
            pulsar,
            producers,
            &config.pulsar_namespace,
        )
        .await?;

        if bucket_ms >= next {
            let following = bucket_ms
                .checked_add(interval_ms)
                .context("Aggregator cursor overflows u64")?;
            {
                let db = Arc::clone(&db);
                let timeframe = config.timeframe.clone();
                tokio::task::spawn_blocking(move || {
                    persistence::persist_cursor(&db, &timeframe, following)
                })
                .await
                .context("QuestDB cursor persistence task failed")??;
            }
            *next_bucket_ms = Some(following);
        }
        bucket_ms = bucket_ms
            .checked_add(interval_ms)
            .context("Timeframe bucket timestamp overflows u64")?;
    }
    Ok(())
}

async fn process_bucket(
    db: Arc<QuestDb>,
    timeframe: &str,
    timeframe_ms: u64,
    ids: &mut IdRegistry,
    bucket_start_ms: u64,
) -> Result<()> {
    let start_us = i64::try_from(
        bucket_start_ms
            .checked_mul(1_000)
            .context("Timeframe bucket timestamp overflows")?,
    )?;
    let end_us = i64::try_from(
        bucket_start_ms
            .checked_add(timeframe_ms)
            .context("Timeframe bucket end overflows")?
            .checked_mul(1_000)
            .context("Timeframe bucket end timestamp overflows")?,
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
        let timeframe = timeframe.to_string();
        tokio::task::spawn_blocking(move || {
            persistence::existing_bars(&db, &timeframe, bucket_time_us)
        })
        .await
        .context("QuestDB existing-bars query task failed")??
    };

    for ((source, symbol), group) in groups {
        let (source_id, symbol_id) = ids.ids_for(&source, &symbol)?;
        let mut bar = build_bar_for_timeframe(
            &group,
            bucket_start_ms,
            timeframe,
            timeframe_ms,
            source_id,
            symbol_id,
        )
        .with_context(|| {
            format!("Failed to aggregate {source}/{symbol} at {bucket_start_ms} ({timeframe})")
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
            let timeframe = timeframe.to_string();
            tokio::task::spawn_blocking(move || {
                persistence::outbox_event_exists(&db, &timeframe, time_us, &event_id)
            })
            .await
            .context("QuestDB outbox lookup task failed")??
        };
        if !changed && event_exists {
            continue;
        }

        if changed {
            persist_bar(Arc::clone(&db), timeframe, &bar, &event_id).await?;
        }
        persist_event(Arc::clone(&db), timeframe, &bar, &event_id).await?;
        info!(
            source = %bar.source,
            symbol = %bar.symbol,
            timestamp = bar.timestamp,
            revision = bar.revision,
            ticks = bar.identity.n_trades,
            "Persisted timeframe bar and queued Pulsar event"
        );
    }

    Ok(())
}

async fn persist_bar(db: Arc<QuestDb>, timeframe: &str, bar: &Bar, event_id: &str) -> Result<()> {
    let db = Arc::clone(&db);
    let bar = bar.clone();
    let event_id = event_id.to_string();
    let timeframe = timeframe.to_string();
    tokio::task::spawn_blocking(move || persistence::persist_bar(&db, &timeframe, &bar, &event_id))
        .await
        .context("QuestDB bar persistence task failed")??;
    Ok(())
}

async fn persist_event(db: Arc<QuestDb>, timeframe: &str, bar: &Bar, event_id: &str) -> Result<()> {
    let event_type = format!("bar.{}", bar.timeframe);
    let event = BarEvent {
        event_id,
        event_type: &event_type,
        timestamp: bar.timestamp,
        source: &bar.source,
        symbol: &bar.symbol,
        timeframe: &bar.timeframe,
        revision: bar.revision,
        data: bar,
    };
    let payload = serde_json::to_string(&event).context("Failed to encode bar event")?;
    debug!(
        event_id,
        payload_bytes = payload.len(),
        "Encoded timeframe bar event"
    );
    let db = Arc::clone(&db);
    let bar = bar.clone();
    let event_id = event_id.to_string();
    let timeframe = timeframe.to_string();
    tokio::task::spawn_blocking(move || {
        persistence::persist_pending_event(&db, &timeframe, &bar, &event_id, &payload)
    })
    .await
    .context("QuestDB outbox persistence task failed")??;
    Ok(())
}

async fn dispatch_pending_events(
    db: Arc<QuestDb>,
    timeframe: &str,
    pulsar: &Pulsar<TokioExecutor>,
    producers: &mut BTreeMap<String, Producer<TokioExecutor>>,
    pulsar_namespace: &str,
) -> Result<()> {
    let pending = {
        let db = Arc::clone(&db);
        let timeframe = timeframe.to_string();
        tokio::task::spawn_blocking(move || persistence::pending_events(&db, &timeframe))
            .await
            .context("QuestDB outbox query task failed")??
    };
    for event in pending {
        send_event(pulsar, producers, pulsar_namespace, &event).await?;
        let db = Arc::clone(&db);
        let timeframe = timeframe.to_string();
        tokio::task::spawn_blocking(move || {
            persistence::mark_event_published(&db, &timeframe, &event)
        })
        .await
        .context("QuestDB outbox acknowledgement task failed")??;
    }
    Ok(())
}

async fn send_event(
    pulsar: &Pulsar<TokioExecutor>,
    producers: &mut BTreeMap<String, Producer<TokioExecutor>>,
    namespace: &str,
    event: &PendingEvent,
) -> Result<()> {
    let topic = bar_topic_name(namespace, &event.source, &event.symbol, &event.timeframe)?;
    if !producers.contains_key(&topic) {
        let producer = pulsar
            .producer()
            .with_topic(topic.clone())
            .with_name(format!(
                "microstructure-aggregator-{}-{}-{}",
                event.source, event.symbol, event.timeframe
            ))
            .build()
            .await
            .with_context(|| format!("Failed to create Pulsar producer for {topic}"))?;
        producers.insert(topic.clone(), producer);
    }
    let producer = producers
        .get_mut(&topic)
        .expect("the producer was inserted above");
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
        timeframe = %event.timeframe,
        topic = %topic,
        revision = event.revision,
        "Published persisted bar event"
    );
    Ok(())
}

fn ensure_topic_segment(value: &str) -> Result<()> {
    anyhow::ensure!(
        !value.is_empty()
            && value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_')),
        "Pulsar topic segment contains invalid characters: {value:?}"
    );
    Ok(())
}

fn bar_topic_name(namespace: &str, source: &str, symbol: &str, timeframe: &str) -> Result<String> {
    ensure_topic_segment(source)?;
    ensure_topic_segment(symbol)?;
    ensure_topic_segment(timeframe)?;
    anyhow::ensure!(
        !namespace.trim().is_empty(),
        "PULSAR_NAMESPACE cannot be empty"
    );
    Ok(format!(
        "{}/bars-{source}-{symbol}-{timeframe}",
        namespace.trim().trim_end_matches('/')
    ))
}

#[cfg(test)]
mod tests {
    use super::{bar_topic_name, questdb_connect_string};
    use crate::config::Config;
    use std::path::PathBuf;

    #[test]
    fn questdb_connection_applies_frame_limits_and_keeps_them_above_the_reported_failure() {
        let config = Config {
            pulsar_url: "pulsar://localhost:6650".to_string(),
            pulsar_namespace: "persistent://public/default".to_string(),
            timeframe: "1m".to_string(),
            timeframe_ms: 60_000,
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

    #[test]
    fn names_the_topic_for_a_source_symbol_and_timeframe() {
        assert_eq!(
            bar_topic_name("persistent://public/default/", "binance", "BTCUSDT", "1m").unwrap(),
            "persistent://public/default/bars-binance-BTCUSDT-1m"
        );
        assert_eq!(
            bar_topic_name("persistent://public/default/", "binance", "BTCUSDT", "5m").unwrap(),
            "persistent://public/default/bars-binance-BTCUSDT-5m"
        );
        assert!(
            bar_topic_name("persistent://public/default", "binance", "BTC/USDT", "1m").is_err()
        );
    }
}

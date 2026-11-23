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
mod dedup;
mod model;
mod persistence;

use anyhow::{Context, Result, ensure};
use config::Config;
use dedup::redis_key;
use futures_util::StreamExt;
use model::TickBatch;
use pulsar::{
    Consumer, Pulsar, SubType, TokioExecutor,
    consumer::{ConsumerOptions, DeadLetterPolicy, InitialPosition},
    message::proto::command_get_topics_of_namespace::Mode as TopicMode,
};
use questdb::QuestDb;
use redis::{AsyncCommands, aio::ConnectionManager};
use regex::Regex;
use std::{collections::HashMap, fs, sync::Arc, time::Duration};
use tracing::{error, info, warn};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();

    let config = Config::from_env()?;
    let questdb = connect_questdb(&config).await?;
    let mut redis = connect_redis(&config).await?;
    let pulsar = connect_pulsar(&config).await?;
    let mut subscribed_topics = discover_tick_topics(&pulsar).await?;
    let mut consumer = create_consumer(&pulsar, &config, &subscribed_topics).await?;
    let mut refresh_topics = tokio::time::interval(Duration::from_secs(30));
    refresh_topics.tick().await;

    info!(
        subscription = %config.pulsar_subscription,
        topic_count = subscribed_topics.len(),
        "Merger is listening for ingestor tick topics"
    );

    loop {
        tokio::select! {
            message = consumer.next() => {
                let message = match message {
                    Some(Ok(message)) => message,
                    Some(Err(error)) => {
                        warn!(%error, "Pulsar consumer error");
                        continue;
                    }
                    None => anyhow::bail!("Pulsar consumer stream ended"),
                };

                let result = process_message(
                    &message,
                    &mut redis,
                    Arc::clone(&questdb),
                    &config,
                )
                .await;

                match result {
                    Ok(tick_count) => {
                        if let Err(error) = consumer.ack(&message).await {
                            error!(%error, "Could not acknowledge persisted Pulsar message");
                        } else {
                            info!(tick_count, "Persisted tick batch");
                        }
                    }
                    Err(error) => {
                        error!(%error, "Tick batch processing failed; requesting redelivery");
                        if let Err(nack_error) = consumer.nack(&message).await {
                            error!(%nack_error, "Could not negatively acknowledge Pulsar message");
                        }
                    }
                }
            }

            _ = refresh_topics.tick() => {
                match discover_tick_topics(&pulsar).await {
                    Ok(topics) if topics != subscribed_topics => {
                        info!(
                            previous_topic_count = subscribed_topics.len(),
                            topic_count = topics.len(),
                            "Refreshing Pulsar tick-topic subscription"
                        );
                        let next_consumer = create_consumer(&pulsar, &config, &topics).await?;
                        if let Err(error) = consumer.close().await {
                            warn!(%error, "Could not close previous Pulsar consumer cleanly");
                        }
                        consumer = next_consumer;
                        subscribed_topics = topics;
                    }
                    Ok(_) => {}
                    Err(error) => {
                        warn!(%error, "Could not refresh Pulsar tick-topic list");
                    }
                }
            }
        }
    }
}

async fn connect_questdb(config: &Config) -> Result<Arc<QuestDb>> {
    fs::create_dir_all(&config.questdb_sf_dir).with_context(|| {
        format!(
            "Failed to create QuestDB store-and-forward directory {}",
            config.questdb_sf_dir.display()
        )
    })?;

    let connect_string = format!(
        "{};sf_dir={};sf_durability=periodic;request_durable_ack=local;sender_pool_min=1;sender_pool_max=1;",
        config.questdb_connect.trim_end_matches(';'),
        config.questdb_sf_dir.display()
    );

    let db = tokio::task::spawn_blocking(move || {
        let db = persistence::connect(&connect_string)?;
        persistence::ensure_schema(&db)?;
        Ok::<_, anyhow::Error>(Arc::new(db))
    })
    .await
    .context("QuestDB initialization task failed")??;

    info!(
        store_and_forward = %config.questdb_sf_dir.display(),
        "Connected to QuestDB and verified ticks schema"
    );
    Ok(db)
}

async fn connect_redis(config: &Config) -> Result<ConnectionManager> {
    let client = redis::Client::open(config.redis_url.as_str()).context("Invalid REDIS_URL")?;
    let manager = ConnectionManager::new(client)
        .await
        .context("Failed to connect to Redis")?;
    info!("Connected to Redis deduplication cache");
    Ok(manager)
}

async fn connect_pulsar(config: &Config) -> Result<Pulsar<TokioExecutor>> {
    Pulsar::builder(&config.pulsar_url, TokioExecutor)
        .build()
        .await
        .context("Failed to connect to Pulsar")
}

async fn discover_tick_topics(pulsar: &Pulsar<TokioExecutor>) -> Result<Vec<String>> {
    let mut topics = pulsar
        .get_topics_of_namespace("public/default".to_string(), TopicMode::Persistent)
        .await
        .context("Failed to list Pulsar topics")?;
    topics.retain(|topic| {
        topic
            .rsplit('/')
            .next()
            .is_some_and(|name| name.starts_with("ticks-"))
    });
    topics.sort();
    topics.dedup();
    Ok(topics)
}

async fn create_consumer(
    pulsar: &Pulsar<TokioExecutor>,
    config: &Config,
    topics: &[String],
) -> Result<Consumer<TickBatch, TokioExecutor>> {
    let topic_regex = Regex::new(r"^ticks-.*$").expect("static topic regex is valid");
    let builder = pulsar
        .consumer()
        .with_lookup_namespace("public/default")
        .with_subscription(&config.pulsar_subscription)
        .with_subscription_type(SubType::Shared)
        .with_options(ConsumerOptions::default().with_initial_position(InitialPosition::Earliest))
        .with_dead_letter_policy(DeadLetterPolicy {
            max_redeliver_count: config.pulsar_max_redeliver_count,
            dead_letter_topic: config.pulsar_dead_letter_topic.clone(),
        })
        .with_topic_refresh(Duration::from_secs(30));
    let builder = if topics.is_empty() {
        builder.with_topic_regex(topic_regex)
    } else {
        builder.with_topics(topics)
    };

    builder
        .build()
        .await
        .context("Failed to subscribe to Pulsar tick topics")
}

async fn process_message(
    message: &pulsar::consumer::message::Message<TickBatch>,
    redis: &mut ConnectionManager,
    questdb: Arc<QuestDb>,
    config: &Config,
) -> Result<usize> {
    let batch = message
        .deserialize()
        .context("Invalid MessagePack tick batch")?;
    let ticks = batch.ticks;

    for tick in &ticks {
        tick.validate()?;
    }
    if ticks.is_empty() {
        return Ok(0);
    }

    let mut identities = HashMap::with_capacity(ticks.len());
    for tick in &ticks {
        let key = redis_key(&config.redis_key_prefix, tick);
        // Match QuestDB UPSERT semantics for duplicate identities in one
        // MessagePack batch: retain the last version of the tick.
        identities.insert(key, tick.clone());
    }
    let identities: Vec<_> = identities
        .into_iter()
        .map(|(key, tick)| (tick, key))
        .collect();
    let redis_keys: Vec<String> = identities.iter().map(|(_, key)| key.clone()).collect();

    let seen: Vec<Option<String>> = redis
        .mget(&redis_keys)
        .await
        .context("Failed to query Redis tick deduplication keys")?;
    ensure!(
        seen.len() == identities.len(),
        "Redis returned an incomplete tick deduplication result"
    );
    let mut new_ticks = Vec::new();
    let mut new_keys = Vec::new();
    for ((tick, key), value) in identities.into_iter().zip(seen) {
        if value.is_none() {
            new_ticks.push(tick);
            new_keys.push(key);
        }
    }

    if new_ticks.is_empty() {
        return Ok(0);
    }

    let tick_count = new_ticks.len();
    let db = Arc::clone(&questdb);
    tokio::task::spawn_blocking(move || persistence::persist_ticks(&db, &new_ticks))
        .await
        .context("QuestDB persistence task failed")??;

    // Redis is a fast deduplication cache, not the source of truth. Set keys
    // only after QuestDB's local-durable acknowledgement; QuestDB's own
    // UPSERT keys make the write safe if this command or the Pulsar ACK fails.
    let mut pipeline = redis::pipe();
    for key in new_keys {
        pipeline
            .cmd("SET")
            .arg(key)
            .arg("stored")
            .arg("EX")
            .arg(config.redis_ttl_seconds)
            .ignore();
    }
    pipeline
        .query_async::<()>(redis)
        .await
        .context("Failed to mark persisted ticks in Redis")?;

    Ok(tick_count)
}

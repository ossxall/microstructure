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

use crate::model::{Bar, Tick, decimal_string};
use anyhow::{Context, Result, ensure};
use questdb::{
    QuestDb,
    egress::{ColumnView, Terminal},
    ingress::{AckLevel, TimestampMicros},
};
use serde::Serialize;

const CREATE_SCHEMA: &str = include_str!("../../../infra/sql/questdb_aggregator.sql");

#[derive(Debug, Clone)]
pub struct PendingEvent {
    pub time_us: i64,
    pub event_id: String,
    pub source: String,
    pub symbol: String,
    pub timeframe: String,
    pub revision: u64,
}

pub const STATE_KEY: &str = "minute-cursor";

#[derive(Debug, Serialize)]
pub struct BarEvent<'a> {
    pub event_id: &'a str,
    pub event_type: &'a str,
    #[serde(serialize_with = "decimal_string::serialize")]
    pub timestamp: u64,
    pub source: &'a str,
    pub symbol: &'a str,
    pub timeframe: &'a str,
    #[serde(serialize_with = "decimal_string::serialize")]
    pub revision: u64,
    pub is_final: bool,
    pub data: &'a Bar,
}

pub fn connect(connect_string: &str) -> Result<QuestDb> {
    QuestDb::connect(connect_string).context("Failed to connect to QuestDB")
}

pub fn ensure_schema(db: &QuestDb, timeframe: &str) -> Result<()> {
    let mut reader = db
        .borrow_reader()
        .context("Failed to open QuestDB schema connection")?;
    let schema = CREATE_SCHEMA
        .replace("{event_table}", &event_table_name(timeframe))
        .replace("{state_table}", &state_table_name(timeframe));
    for statement in schema
        .split(';')
        .map(str::trim)
        .filter(|statement| !statement.is_empty())
    {
        let mut cursor = reader
            .execute(statement)
            .with_context(|| format!("Failed to execute aggregator schema: {statement}"))?;
        while cursor
            .next_batch()
            .context("QuestDB aggregator schema creation failed")?
            .is_some()
        {}
        ensure!(
            matches!(cursor.terminal(), Some(Terminal::ExecDone { .. })),
            "QuestDB did not confirm aggregator schema statement: {statement}"
        );
    }
    Ok(())
}

pub fn load_ticks(db: &QuestDb, start_us: i64, end_us: i64) -> Result<Vec<Tick>> {
    let mut reader = db
        .borrow_reader()
        .context("Failed to open QuestDB tick reader")?;
    let mut cursor = reader
        .prepare(
            "SELECT timestamp, source, symbol, id, price, qty, is_buyer_maker \
             FROM ticks WHERE timestamp >= $1 AND timestamp < $2 ORDER BY timestamp, id",
        )
        .bind_timestamp_micros(start_us)
        .bind_timestamp_micros(end_us)
        .execute()
        .context("Failed to query ticks for timeframe bucket")?;

    let mut ticks = Vec::new();
    while let Some(batch) = cursor
        .next_batch()
        .context("Failed while reading QuestDB ticks")?
    {
        let timestamps = match batch.column(0)? {
            ColumnView::Timestamp(column) => column,
            _ => anyhow::bail!("QuestDB ticks.time is not TIMESTAMP"),
        };
        let sources = match batch.column(1)? {
            ColumnView::Symbol(column) => column,
            _ => anyhow::bail!("QuestDB ticks.source is not SYMBOL"),
        };
        let symbols = match batch.column(2)? {
            ColumnView::Symbol(column) => column,
            _ => anyhow::bail!("QuestDB ticks.symbol is not SYMBOL"),
        };
        let ids = match batch.column(3)? {
            ColumnView::Varchar(column) => column,
            _ => anyhow::bail!("QuestDB ticks.id is not VARCHAR"),
        };
        let prices = match batch.column(4)? {
            ColumnView::Long(column) => column,
            _ => anyhow::bail!("QuestDB ticks.price is not LONG"),
        };
        let quantities = match batch.column(5)? {
            ColumnView::Long(column) => column,
            _ => anyhow::bail!("QuestDB ticks.qty is not LONG"),
        };
        let buyer_maker = match batch.column(6)? {
            ColumnView::Short(column) => column,
            _ => anyhow::bail!("QuestDB ticks.is_buyer_maker is not SHORT"),
        };

        for row in 0..batch.row_count() {
            ensure!(
                !timestamps.is_null(row)
                    && !prices.is_null(row)
                    && !quantities.is_null(row)
                    && !buyer_maker.is_null(row),
                "QuestDB contains a null in a required tick field"
            );
            ticks.push(Tick {
                time_us: timestamps.value(row),
                source: sources
                    .resolve(row)
                    .context("QuestDB tick source symbol is null")?
                    .to_string(),
                symbol: symbols
                    .resolve(row)
                    .context("QuestDB tick symbol is null")?
                    .to_string(),
                id: ids
                    .value(row)
                    .context("QuestDB tick ID is null")?
                    .to_string(),
                price: u64::try_from(prices.value(row))
                    .context("QuestDB tick price is negative")?,
                qty: u64::try_from(quantities.value(row))
                    .context("QuestDB tick quantity is negative")?,
                is_buyer_maker: u8::try_from(buyer_maker.value(row))
                    .context("QuestDB is_buyer_maker is outside the u8 range")?,
            });
        }
    }
    Ok(ticks)
}

pub fn first_tick_time(db: &QuestDb) -> Result<Option<i64>> {
    read_timestamp_scalar(db, "SELECT min(timestamp) FROM ticks")
}

fn read_timestamp_scalar(db: &QuestDb, sql: &str) -> Result<Option<i64>> {
    let mut reader = db
        .borrow_reader()
        .context("Failed to open QuestDB scalar reader")?;
    let mut cursor = reader
        .execute(sql)
        .with_context(|| format!("Failed to run QuestDB query: {sql}"))?;
    let mut result = None;
    while let Some(batch) = cursor
        .next_batch()
        .context("Failed while reading QuestDB scalar result")?
    {
        if batch.row_count() == 0 {
            continue;
        }
        let values = match batch.column(0)? {
            ColumnView::Timestamp(column) => column,
            _ => anyhow::bail!("QuestDB scalar result is not a TIMESTAMP"),
        };
        if !values.is_null(0) {
            result = Some(values.value(0));
        }
    }
    Ok(result)
}

pub fn read_cursor_ms(db: &QuestDb, timeframe: &str) -> Result<Option<u64>> {
    let table = state_table_name(timeframe);
    let mut reader = db
        .borrow_reader()
        .context("Failed to open QuestDB cursor reader")?;
    let mut cursor = reader
        .execute(format!(
            "SELECT cursor_ms FROM {table} \
             WHERE state_key = 'minute-cursor' ORDER BY time DESC LIMIT 1"
        ))
        .context("Failed to query aggregator cursor")?;
    let mut result = None;
    while let Some(batch) = cursor
        .next_batch()
        .context("Failed while reading aggregator cursor")?
    {
        if batch.row_count() == 0 {
            continue;
        }
        let values = match batch.column(0)? {
            ColumnView::Long(column) => column,
            _ => anyhow::bail!("QuestDB aggregator cursor is not LONG"),
        };
        if !values.is_null(0) {
            result = Some(u64::try_from(values.value(0))?);
        }
    }
    Ok(result)
}

pub fn persist_cursor(db: &QuestDb, timeframe: &str, next_minute_ms: u64) -> Result<()> {
    let cursor_time_us = next_minute_ms
        .checked_mul(1_000)
        .context("Aggregator cursor timestamp overflows")?;
    let cursor_time_us =
        i64::try_from(cursor_time_us).context("Aggregator cursor exceeds QuestDB range")?;
    let mut sender = db
        .borrow_sender()
        .context("Failed to borrow QuestDB cursor sender")?;
    let mut buffer = sender.new_buffer();
    let table = state_table_name(timeframe);
    buffer
        .table(table.as_str())?
        .symbol("state_key", STATE_KEY)?
        .column_i64("cursor_ms", i64::try_from(next_minute_ms)?)?
        .at(TimestampMicros::new(cursor_time_us))?;
    sender
        .flush_buffer_and_wait(&mut buffer, AckLevel::LocalDurable)
        .context("QuestDB did not durably acknowledge the aggregator cursor")
}

pub fn persist_pending_event(
    db: &QuestDb,
    timeframe: &str,
    bar: &Bar,
    event_id: &str,
) -> Result<()> {
    let time_us = bar
        .timestamp
        .checked_mul(1_000)
        .context("bar timestamp overflows QuestDB TIMESTAMP")?;
    let time_us = i64::try_from(time_us).context("bar timestamp exceeds QuestDB range")?;
    let mut sender = db
        .borrow_sender()
        .context("Failed to borrow QuestDB outbox sender")?;
    let mut buffer = sender.new_buffer();
    let table = event_table_name(timeframe);
    buffer
        .table(table.as_str())?
        .column_str("event_id", event_id)?
        .symbol("source", bar.source.as_str())?
        .symbol("symbol", bar.symbol.as_str())?
        .column_i64("revision", i64::try_from(bar.revision)?)?
        // Bar contents live in BarStore; QuestDB stores only outbox metadata.
        .column_str("payload", "")?
        .column_bool("published", false)?
        .at(TimestampMicros::new(time_us))?;

    sender
        .flush_buffer_and_wait(&mut buffer, AckLevel::LocalDurable)
        .context("QuestDB did not durably acknowledge the bar outbox event")
}

pub fn outbox_event_exists(
    db: &QuestDb,
    timeframe: &str,
    time_us: i64,
    event_id: &str,
) -> Result<bool> {
    let table = event_table_name(timeframe);
    let mut reader = db
        .borrow_reader()
        .context("Failed to open QuestDB outbox lookup reader")?;
    let mut cursor = reader
        .prepare(format!(
            "SELECT event_id FROM {table} WHERE time = $1 AND event_id = $2 LIMIT 1"
        ))
        .bind_timestamp_micros(time_us)
        .bind_varchar(event_id)
        .execute()
        .context("Failed to check for an existing bar outbox event")?;
    while let Some(batch) = cursor
        .next_batch()
        .context("Failed while checking bar outbox event")?
    {
        if batch.row_count() > 0 {
            return Ok(true);
        }
    }
    Ok(false)
}

pub fn pending_events(db: &QuestDb, timeframe: &str) -> Result<Vec<PendingEvent>> {
    let table = event_table_name(timeframe);
    let mut reader = db
        .borrow_reader()
        .context("Failed to open QuestDB event outbox reader")?;
    let mut cursor = reader
        .execute(format!(
            "SELECT time, event_id, source, symbol, revision \
             FROM {table} WHERE published = false ORDER BY time, event_id"
        ))
        .context("Failed to query pending bar events")?;
    let mut events = Vec::new();
    while let Some(batch) = cursor
        .next_batch()
        .context("Failed while reading pending bar events")?
    {
        let times = match batch.column(0)? {
            ColumnView::Timestamp(column) => column,
            _ => anyhow::bail!("QuestDB outbox time is not TIMESTAMP"),
        };
        let event_ids = match batch.column(1)? {
            ColumnView::Varchar(column) => column,
            _ => anyhow::bail!("QuestDB outbox event_id is not VARCHAR"),
        };
        let sources = match batch.column(2)? {
            ColumnView::Symbol(column) => column,
            _ => anyhow::bail!("QuestDB outbox source is not SYMBOL"),
        };
        let symbols = match batch.column(3)? {
            ColumnView::Symbol(column) => column,
            _ => anyhow::bail!("QuestDB outbox symbol is not SYMBOL"),
        };
        let revisions = match batch.column(4)? {
            ColumnView::Long(column) => column,
            _ => anyhow::bail!("QuestDB outbox revision is not LONG"),
        };
        for row in 0..batch.row_count() {
            events.push(PendingEvent {
                time_us: times.value(row),
                event_id: event_ids
                    .value(row)
                    .context("Outbox event ID is null")?
                    .to_string(),
                source: sources
                    .resolve(row)
                    .context("Outbox source is null")?
                    .to_string(),
                symbol: symbols
                    .resolve(row)
                    .context("Outbox symbol is null")?
                    .to_string(),
                timeframe: timeframe.to_string(),
                revision: u64::try_from(revisions.value(row))?,
            });
        }
    }
    Ok(events)
}

pub fn mark_event_published(db: &QuestDb, timeframe: &str, event: &PendingEvent) -> Result<()> {
    let mut sender = db
        .borrow_sender()
        .context("Failed to borrow QuestDB outbox sender")?;
    let mut buffer = sender.new_buffer();
    let table = event_table_name(timeframe);
    buffer
        .table(table.as_str())?
        .column_str("event_id", event.event_id.as_str())?
        .symbol("source", event.source.as_str())?
        .symbol("symbol", event.symbol.as_str())?
        .column_i64("revision", i64::try_from(event.revision)?)?
        // Clear legacy event payloads when their durable delivery completes.
        .column_str("payload", "")?
        .column_bool("published", true)?
        .at(TimestampMicros::new(event.time_us))?;
    sender
        .flush_buffer_and_wait(&mut buffer, AckLevel::LocalDurable)
        .context("QuestDB did not durably mark bar event as published")
}

fn event_table_name(timeframe: &str) -> String {
    if timeframe == "1m" {
        "bar_events".to_string()
    } else {
        format!("bar_events_{timeframe}")
    }
}

fn state_table_name(timeframe: &str) -> String {
    if timeframe == "1m" {
        "aggregator_state".to_string()
    } else {
        format!("aggregator_state_{timeframe}")
    }
}

#[cfg(test)]
mod tests {
    use super::{CREATE_SCHEMA, event_table_name, state_table_name};

    #[test]
    fn each_timeframe_gets_separate_persistence_tables() {
        assert_eq!(event_table_name("1m"), "bar_events");
        assert_eq!(state_table_name("1m"), "aggregator_state");

        assert_eq!(event_table_name("5m"), "bar_events_5m");
        assert_eq!(state_table_name("5m"), "aggregator_state_5m");
    }

    #[test]
    fn questdb_schema_does_not_create_final_bar_tables() {
        assert!(!CREATE_SCHEMA.contains("{bars_table}"));
        assert!(!CREATE_SCHEMA.contains("CREATE TABLE IF NOT EXISTS bars_"));
    }
}

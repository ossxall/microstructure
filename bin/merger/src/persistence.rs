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

use anyhow::{Context, Result};
use questdb::{
    QuestDb,
    egress::Terminal,
    ingress::{AckLevel, TimestampMicros},
};

pub const CREATE_TICKS_TABLE: &str = include_str!("../../../infra/sql/questdb_ticks.sql");

pub fn connect(connect_string: &str) -> Result<QuestDb> {
    QuestDb::connect(connect_string).context("Failed to connect to QuestDB")
}

pub fn ensure_schema(db: &QuestDb) -> Result<()> {
    let mut reader = db
        .borrow_reader()
        .context("Failed to open QuestDB schema connection")?;
    let mut cursor = reader
        .execute(CREATE_TICKS_TABLE)
        .context("Failed to create QuestDB ticks table")?;

    while cursor
        .next_batch()
        .context("QuestDB schema creation failed")?
        .is_some()
    {}

    if !matches!(cursor.terminal(), Some(Terminal::ExecDone { .. })) {
        anyhow::bail!("QuestDB did not confirm ticks table creation");
    }

    Ok(())
}

pub fn persist_ticks(db: &QuestDb, ticks: &[crate::model::Tick]) -> Result<()> {
    if ticks.is_empty() {
        return Ok(());
    }

    let mut sender = db
        .borrow_sender()
        .context("Failed to borrow QuestDB sender")?;
    let mut buffer = sender.new_buffer();

    for tick in ticks {
        let timestamp_micros = tick.validate()?;
        buffer
            .table("ticks")
            .context("Failed to select QuestDB ticks table")?
            .symbol("source", tick.source.as_str())?
            .symbol("symbol", tick.symbol.as_str())?
            .column_str("id", tick.id.as_str())?
            .column_i64("price", i64::try_from(tick.price)?)?
            .column_i64("qty", i64::try_from(tick.qty)?)?
            .column_i16("is_buyer_maker", i16::from(tick.is_buyer_maker))?
            .at(TimestampMicros::new(timestamp_micros))?;
    }

    sender
        .flush_buffer_and_wait(&mut buffer, AckLevel::LocalDurable)
        .context("QuestDB did not durably acknowledge ticks")
}

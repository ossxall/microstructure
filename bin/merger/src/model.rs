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

use anyhow::{Context, Result, ensure};
use pulsar::{DeserializeMessage, message::Payload};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Tick {
    pub source: String,
    pub symbol: String,
    pub id: String,
    pub time: u64,
    pub price: u64,
    pub qty: u64,
    pub is_buyer_maker: u8,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct TickBatch {
    pub ticks: Vec<Tick>,
}

impl DeserializeMessage for TickBatch {
    type Output = Result<Self, rmp_serde::decode::Error>;

    fn deserialize_message(payload: &Payload) -> Self::Output {
        rmp_serde::from_slice(&payload.data)
    }
}

impl Tick {
    /// Validate values before they are sent to Redis or QuestDB.
    ///
    /// QuestDB LONG is signed and its TIMESTAMP uses a signed 64-bit count of
    /// microseconds, so reject values that cannot be represented exactly.
    pub fn validate(&self) -> Result<i64> {
        ensure!(!self.source.is_empty(), "tick source cannot be empty");
        ensure!(!self.symbol.is_empty(), "tick symbol cannot be empty");
        ensure!(!self.id.is_empty(), "tick id cannot be empty");
        ensure!(self.price > 0, "tick price must be positive");
        ensure!(self.qty > 0, "tick quantity must be positive");
        i64::try_from(self.price).context("tick price exceeds QuestDB LONG range")?;
        i64::try_from(self.qty).context("tick quantity exceeds QuestDB LONG range")?;

        let timestamp_ms =
            i64::try_from(self.time).context("tick timestamp exceeds QuestDB range")?;
        timestamp_ms
            .checked_mul(1_000)
            .context("tick timestamp overflows QuestDB microseconds")
    }
}

#[cfg(test)]
mod tests {
    use super::{Tick, TickBatch};

    #[test]
    fn decodes_ingestor_messagepack_batch() {
        let encoded = rmp_serde::to_vec(&TickBatch {
            ticks: vec![Tick {
                source: "binance".to_string(),
                symbol: "BTCUSDT".to_string(),
                id: "42".to_string(),
                time: 1_700_000_000_123,
                price: 6_500_000_000_000,
                qty: 100_000,
                is_buyer_maker: 1,
            }],
        })
        .unwrap();

        let decoded: TickBatch = rmp_serde::from_slice(&encoded).unwrap();
        assert_eq!(decoded.ticks.len(), 1);
        assert_eq!(decoded.ticks[0].source, "binance");
        assert_eq!(decoded.ticks[0].symbol, "BTCUSDT");
        assert_eq!(decoded.ticks[0].id, "42");
    }

    #[test]
    fn validates_timestamp_and_scaled_values_without_changing_them() {
        let tick = Tick {
            source: "dydx".to_string(),
            symbol: "BTC-USD".to_string(),
            id: "trade-1".to_string(),
            time: 1_700_000_000_000,
            price: 65_000_000_000_000,
            qty: 100_000_000,
            is_buyer_maker: 255,
        };

        assert_eq!(tick.validate().unwrap(), 1_700_000_000_000_000);
    }

    #[test]
    fn rejects_values_that_cannot_be_stored_exactly() {
        let tick = Tick {
            source: "binance".to_string(),
            symbol: "BTCUSDT".to_string(),
            id: "42".to_string(),
            time: 1,
            price: u64::MAX,
            qty: 1,
            is_buyer_maker: 0,
        };

        assert!(tick.validate().is_err());
    }

    #[test]
    fn rejects_zero_price_or_quantity() {
        let mut tick = Tick {
            source: "binance".to_string(),
            symbol: "BTCUSDT".to_string(),
            id: "42".to_string(),
            time: 1,
            price: 1,
            qty: 1,
            is_buyer_maker: 0,
        };

        tick.price = 0;
        assert!(tick.validate().is_err());

        tick.price = 1;
        tick.qty = 0;
        assert!(tick.validate().is_err());
    }
}

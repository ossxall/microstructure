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

use crate::models::{Tick, decimal_to_scaled_u64};
use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::Deserialize;
use tracing::warn;

#[derive(Debug, Deserialize)]
struct MessageType {
    #[serde(rename = "type")]
    msg_type: String,
}

/**
* {
       "id": "05e4dc7a0000000200000012",
       "side": "SELL",
       "size": "0.0985",
       "price": "64100",
       "type": "LIMIT",
       "createdAt": "2026-07-24T21:47:35.074Z",
       "createdAtHeight": "98884730"
   }

   {
       "id": "05e4e8f80000000200000008",
       "side": "BUY",
       "size": "0.0165",
       "price": "64113",
       "type": "LIMIT",
       "createdAt": "2026-07-24T22:25:47.898Z"
   }
*/
#[derive(Debug, Deserialize)]
struct DydxTrade {
    id: String,

    side: String,

    size: String,

    price: String,

    #[serde(rename = "createdAt")]
    created_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
struct DydxContents {
    trades: Vec<DydxTrade>,
}

#[derive(Debug, Deserialize)]
struct DydxWsMessage {
    id: String,

    contents: DydxContents,
}

#[inline]
fn side_to_u8(side: &str) -> u8 {
    match side {
        "BUY" => 0,
        "SELL" => 1,
        _ => 255,
    }
}

/// Parses a dYdX WebSocket message into a collection of `Tick`s.
///
/// Only `channel_data` messages containing trades are converted into ticks.
/// Connection, subscription, error, and unknown message types return an empty
/// vector without failing.
pub fn parse_dydx_trade(text: &str) -> Result<Vec<Tick>> {
    let msg_type: MessageType = serde_json::from_str(text)?;

    match msg_type.msg_type.as_str() {
        "channel_data" => {
            let message: DydxWsMessage = serde_json::from_str(text)?;

            let mut ticks: Vec<Tick> = Vec::with_capacity(message.contents.trades.len());

            for trade in message.contents.trades {
                let scaled_values = decimal_to_scaled_u64(&trade.price)
                    .and_then(|price| decimal_to_scaled_u64(&trade.size).map(|qty| (price, qty)));
                let (price, qty) = match scaled_values {
                    Ok(values) => values,
                    Err(error) => {
                        warn!(%error, trade_id = %trade.id, "Ignoring dYdX trade with invalid price, size, or precision");
                        continue;
                    }
                };

                let tick: Tick = Tick {
                    source: "dydx".to_string(),

                    symbol: message.id.clone(),

                    id: trade.id,

                    time: trade.created_at.timestamp_millis() as u64,

                    price,

                    qty,

                    is_buyer_maker: side_to_u8(&trade.side),
                };

                ticks.push(tick);
            }

            Ok(ticks)
        }

        "connected" => {
            println!("Connected");
            return Ok(Vec::new());
        }

        "subscribed" => {
            println!("Subscribed");
            return Ok(Vec::new());
        }

        "error" => {
            println!("Exchange error: {text}");
            return Ok(Vec::new());
        }

        other => {
            println!("Unknown message type: {other}");
            return Ok(Vec::new());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::parse_dydx_trade;

    #[test]
    fn skips_invalid_trade_in_batch_without_losing_valid_trades() {
        let message = r#"{
            "type":"channel_data",
            "id":"BTC-USD",
            "contents":{"trades":[
                {"id":"trade-1","side":"BUY","size":"0.0985","price":"64100","createdAt":"2026-07-24T21:47:35.074Z"},
                {"id":"trade-2","side":"SELL","size":"0.000000001","price":"64100","createdAt":"2026-07-24T21:47:35.075Z"}
            ]}
        }"#;

        let ticks = parse_dydx_trade(message).unwrap();
        assert_eq!(ticks.len(), 1);
        assert_eq!(ticks[0].id, "trade-1");
        assert_eq!(ticks[0].qty, 9_850_000);
    }
}

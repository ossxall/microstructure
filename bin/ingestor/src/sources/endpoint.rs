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

use crate::{
    models::Tick,
    sources::{binance::parse_binance_trade, dydx::parse_dydx_trade},
};
use anyhow::{Result, bail};
use futures_util::{SinkExt, stream::SplitSink};
use tokio::net::TcpStream;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, tungstenite::Message};

/// Returns the WebSocket endpoint for the given data source.
pub fn get_source_endpoint(source: &str) -> Result<&'static str> {
    Ok(match source {
        "dydx" => "wss://indexer.dydx.trade/v4/ws",
        "binance" => "wss://fstream.binance.com/ws",
        _ => bail!("Unsupported source: {source}"),
    })
}

/// Sends the subscription message required to initialize the WebSocket
/// connection for the specified market data source.
pub async fn prepare_source_endpoint(
    source: &str,
    symbols: &str,
    write: &mut SplitSink<WebSocketStream<MaybeTlsStream<TcpStream>>, Message>,
) -> Result<()> {
    match source {
        "dydx" => {
            for symbol in symbols
                .split(',')
                .map(str::trim)
                .filter(|s: &&str| !s.is_empty())
            {
                let subscribe: serde_json::Value = serde_json::json!({
                    "type": "subscribe",
                    "channel": "v4_trades",
                    "id": symbol
                });

                write
                    .send(Message::Text(subscribe.to_string().into()))
                    .await?;
            }
        }

        "binance" => {
            for (id, symbol) in symbols
                .split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .enumerate()
            {
                let subscribe: serde_json::Value = serde_json::json!({
                    "method": "SUBSCRIBE",
                    "params": [
                        format!("{}@trade", symbol.to_lowercase())
                    ],
                    "id": id
                });

                write
                    .send(Message::Text(subscribe.to_string().into()))
                    .await?;
            }
        }

        _ => bail!("Unsupported source: {source}"),
    }

    Ok(())
}

/// Parses a trade message from the specified source into one or more `Tick` values.
pub fn parse_source_trade(source: &str, text: &str) -> Result<Vec<Tick>> {
    match source {
        "dydx" => parse_dydx_trade(text),
        "binance" => parse_binance_trade(text),
        _ => bail!("Unsupported source: {source}"),
    }
}

use crate::models::{Tick, decimal_to_scaled_u64};
use anyhow::Result;
use serde::Deserialize;
use tracing::warn;

/**
Binance wsconection logs example:

Connected to wss://fstream.binance.com/ws
{
  "method": "SUBSCRIBE",
  "params": [
    "btcusdt@trade"
  ],
  "id": 1
}
{
  "result": null,
  "id": 1
}
{
  "e": "trade",
  "E": 1795385689969,
  "T": 1795385689969,
  "s": "BTCUSDT",
  "t": 8321796699,
  "p": "60900.70",
  "q": "0.001",
  "X": "MARKET",
  "m": true,
  "st": 1
}

}
*/

#[derive(Debug, Deserialize)]
struct BinanceTrade {
    #[serde(rename = "T")]
    trade_time: u64,

    #[serde(rename = "s")]
    symbol: String,

    #[serde(rename = "t")]
    trade_id: u64,

    #[serde(rename = "p")]
    price: String,

    #[serde(rename = "q")]
    qty: String,

    #[serde(rename = "m")]
    is_buyer_maker: bool,
}

/// Parses Binance Futures @trade messages into Tick.
///
/// Subscription ACKs and other non-trade messages return an empty vector.
pub fn parse_binance_trade(text: &str) -> Result<Vec<Tick>> {
    let value: serde_json::Value = serde_json::from_str(text)?;

    // ACK del SUBSCRIBE
    if value.get("result").is_some() {
        return Ok(Vec::new());
    }

    // Ignorar cualquier cosa que no sea un trade
    if value.get("e").and_then(|v| v.as_str()) != Some("trade") {
        return Ok(Vec::new());
    }

    let trade: BinanceTrade = serde_json::from_value(value)?;

    let price = match decimal_to_scaled_u64(&trade.price) {
        Ok(value) => value,
        Err(error) => {
            warn!(%error, "Ignoring Binance trade with invalid price or precision");
            return Ok(Vec::new());
        }
    };
    let qty = match decimal_to_scaled_u64(&trade.qty) {
        Ok(value) => value,
        Err(error) => {
            warn!(%error, "Ignoring Binance trade with invalid quantity or precision");
            return Ok(Vec::new());
        }
    };

    Ok(vec![Tick {
        source: "binance".to_string(),

        symbol: trade.symbol,

        id: trade.trade_id.to_string(),

        time: trade.trade_time,

        price,

        qty,

        is_buyer_maker: trade.is_buyer_maker as u8,
    }])
}

#[cfg(test)]
mod tests {
    use super::parse_binance_trade;

    #[test]
    fn parses_exactly_scaled_trade_and_skips_precision_loss() {
        let valid =
            r#"{"e":"trade","T":1000,"s":"BTCUSDT","t":1,"p":"60900.70","q":"0.001","m":false}"#;
        let ticks = parse_binance_trade(valid).unwrap();
        assert_eq!(ticks.len(), 1);
        assert_eq!(ticks[0].price, 6_090_070_000_000);
        assert_eq!(ticks[0].qty, 100_000);

        let too_precise =
            r#"{"e":"trade","T":1000,"s":"BTCUSDT","t":2,"p":"1.000000001","q":"1","m":false}"#;
        assert!(parse_binance_trade(too_precise).unwrap().is_empty());

        let zero_quantity =
            r#"{"e":"trade","T":1000,"s":"BTCUSDT","t":3,"p":"1","q":"0.00000000","m":false}"#;
        assert!(parse_binance_trade(zero_quantity).unwrap().is_empty());
    }
}

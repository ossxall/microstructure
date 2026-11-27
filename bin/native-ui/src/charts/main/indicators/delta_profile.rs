use std::collections::BTreeMap;

use serde_json::Value;

use crate::{charts::DeltaProfileLevel, market::BarSnapshot};

const WINDOW_MINUTES: u64 = 15;
const TICK_SIZE: i128 = 25;
const MINUTE_MS: u64 = 60_000;

const TIMEFRAME_MINUTES: &[(&str, u64)] = &[
    ("1m", 1),
    ("5m", 5),
    ("15m", 15),
    ("30m", 30),
    ("1h", 60),
    ("4h", 240),
    ("1d", 1_440),
];

#[derive(Default)]
struct VolumeAtPrice {
    buy: f64,
    sell: f64,
}

pub(super) fn calculate(bars: &[BarSnapshot], timeframe: &str) -> (Vec<DeltaProfileLevel>, f64) {
    let Some(latest_timestamp) = bars.iter().map(|bar| bar.timestamp_ms).max() else {
        return (Vec::new(), TICK_SIZE as f64);
    };
    let earliest_timestamp = window_start(latest_timestamp, timeframe);

    let mut volumes = BTreeMap::<i128, VolumeAtPrice>::new();
    let mut active_price_scale = None;
    for bar in bars
        .iter()
        .filter(|bar| bar.timestamp_ms >= earliest_timestamp)
    {
        let attributes = bar.attributes();
        let Some(header) = attributes.get("header") else {
            continue;
        };
        let Some(price_scale) = header.get("price_scale").and_then(read_u32) else {
            continue;
        };
        if price_scale > 18 || active_price_scale.is_some_and(|scale| scale != price_scale) {
            continue;
        }
        let Some(quantity_scale) = header
            .get("quantity_scale")
            .and_then(read_u32)
            .filter(|scale| *scale <= 18)
        else {
            continue;
        };
        active_price_scale.get_or_insert(price_scale);
        let Some(levels) = attributes
            .get("footprint")
            .and_then(|footprint| footprint.get("levels"))
            .and_then(Value::as_array)
        else {
            continue;
        };

        accumulate_levels(&mut volumes, levels, price_scale, quantity_scale);
    }

    let price_scale = active_price_scale.unwrap_or(0);
    let price_divisor = 10_f64.powi(price_scale as i32);
    let levels = volumes
        .into_iter()
        .filter_map(|(price_raw, volume)| {
            (volume.buy + volume.sell > 0.0).then_some(DeltaProfileLevel {
                price: price_raw as f64 / price_divisor,
                buy_volume: volume.buy,
                sell_volume: volume.sell,
            })
        })
        .collect();

    (levels, TICK_SIZE as f64)
}

fn timeframe_minutes(timeframe: &str) -> u64 {
    TIMEFRAME_MINUTES
        .iter()
        .find_map(|(name, minutes)| (*name == timeframe).then_some(*minutes))
        .unwrap_or(1)
}

fn window_start(latest_timestamp: u64, timeframe: &str) -> u64 {
    let covered_minutes = WINDOW_MINUTES.saturating_sub(timeframe_minutes(timeframe));
    latest_timestamp.saturating_sub(covered_minutes.saturating_mul(MINUTE_MS))
}

fn accumulate_levels(
    volumes: &mut BTreeMap<i128, VolumeAtPrice>,
    levels: &[Value],
    price_scale: u32,
    quantity_scale: u32,
) {
    let tick_raw = TICK_SIZE * 10_i128.pow(price_scale);
    let quantity_divisor = 10_f64.powi(quantity_scale as i32);
    for level in levels {
        let Some(price_raw) = level.get("price").and_then(read_i128) else {
            continue;
        };
        let Some(buy_raw) = level.get("buy_qty").and_then(read_u128) else {
            continue;
        };
        let Some(sell_raw) = level.get("sell_qty").and_then(read_u128) else {
            continue;
        };
        let price_key = price_raw.div_euclid(tick_raw) * tick_raw;
        let volume = volumes.entry(price_key).or_default();
        volume.buy += buy_raw as f64 / quantity_divisor;
        volume.sell += sell_raw as f64 / quantity_divisor;
    }
}

fn read_i128(value: &Value) -> Option<i128> {
    match value {
        Value::Number(number) => number
            .as_i64()
            .map(i128::from)
            .or_else(|| number.as_u64().map(i128::from)),
        Value::String(value) => value.parse().ok(),
        _ => None,
    }
}

fn read_u128(value: &Value) -> Option<u128> {
    match value {
        Value::Number(number) => number.as_u64().map(u128::from).or_else(|| {
            number
                .as_i64()
                .filter(|value| *value >= 0)
                .map(|value| value as u128)
        }),
        Value::String(value) => value.parse().ok(),
        _ => None,
    }
}

fn read_u32(value: &Value) -> Option<u32> {
    read_u128(value).and_then(|value| u32::try_from(value).ok())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{TICK_SIZE, VolumeAtPrice, accumulate_levels, timeframe_minutes, window_start};

    #[test]
    fn timeframe_coverage_matches_the_rolling_profile_window() {
        assert_eq!(timeframe_minutes("1m"), 1);
        assert_eq!(timeframe_minutes("15m"), 15);
        assert_eq!(timeframe_minutes("unknown"), 1);
        assert_eq!(TICK_SIZE, 25);
        assert_eq!(window_start(1_000_000, "1m"), 160_000);
        assert_eq!(window_start(1_000_000, "15m"), 1_000_000);
    }

    #[test]
    fn scaled_integer_parsers_accept_wire_numbers_and_strings() {
        assert_eq!(super::read_i128(&json!("-1200")), Some(-1200));
        assert_eq!(super::read_i128(&json!("1200")), Some(1200));
        assert_eq!(
            super::read_u128(&json!("9007199254740993")),
            Some(9_007_199_254_740_993)
        );
        assert_eq!(super::read_u128(&json!("-1")), None);
    }

    #[test]
    fn footprint_levels_are_merged_by_tick_and_directional_volume() {
        let mut volumes = std::collections::BTreeMap::<i128, VolumeAtPrice>::new();
        let levels = [
            json!({"price": "10010", "buy_qty": "900", "sell_qty": "100"}),
            json!({"price": "10024", "buy_qty": "100", "sell_qty": "500"}),
            json!({"price": "12501", "buy_qty": "0", "sell_qty": "300"}),
        ];

        accumulate_levels(&mut volumes, &levels, 2, 2);

        assert_eq!(volumes.len(), 2);
        assert_eq!(volumes[&10_000].buy, 10.0);
        assert_eq!(volumes[&10_000].sell, 6.0);
        assert_eq!(volumes[&12_500].buy, 0.0);
        assert_eq!(volumes[&12_500].sell, 3.0);
    }
}

use serde_json::Value;

use crate::{
    charts::{BidAskFlowCandle, BidAskFlowData},
    market::BarSnapshot,
};

pub(super) fn calculate(bars: &[BarSnapshot]) -> BidAskFlowData {
    calculate_attributes(bars.iter().map(|bar| (bar.timestamp_ms, bar.attributes())))
}

fn calculate_attributes<'a>(bars: impl IntoIterator<Item = (u64, &'a Value)>) -> BidAskFlowData {
    let mut candles = Vec::new();
    let mut previous_close = 0.0;

    for (timestamp_ms, attributes) in bars {
        let Some(candle) = build_candle(timestamp_ms, attributes, previous_close) else {
            continue;
        };
        previous_close = candle.close;
        candles.push(candle);
    }

    BidAskFlowData {
        candles: candles.into(),
    }
}

fn build_candle(
    timestamp_ms: u64,
    attributes: &Value,
    previous_close: f64,
) -> Option<BidAskFlowCandle> {
    let quantity_scale = numeric(
        attributes
            .get("header")
            .and_then(|header| header.get("quantity_scale")),
    );
    if !quantity_scale.is_finite()
        || quantity_scale.fract() != 0.0
        || !(0.0..=18.0).contains(&quantity_scale)
    {
        return None;
    }

    let ticks = attributes.get("raw_ticks")?.as_array()?;
    if ticks.is_empty() {
        return None;
    }
    let divisor = 10_f64.powi(quantity_scale as i32);
    let mut ordered = ticks
        .iter()
        .enumerate()
        .map(|(index, tick)| OrderedTick {
            tick,
            index,
            time: numeric(tick.get("time")),
            time_us: numeric(tick.get("time_us")),
        })
        .collect::<Vec<_>>();
    ordered.sort_by(|left, right| {
        finite_or_infinity(left.time)
            .total_cmp(&finite_or_infinity(right.time))
            .then_with(|| {
                finite_or_infinity(left.time_us).total_cmp(&finite_or_infinity(right.time_us))
            })
            .then_with(|| left.index.cmp(&right.index))
    });

    let mut running = if previous_close.is_finite() {
        previous_close
    } else {
        0.0
    };
    let open = running;
    let mut high = open;
    let mut low = open;
    let mut ask_qty = 0.0;
    let mut bid_qty = 0.0;
    let mut has_valid_tick = false;

    for ordered_tick in ordered {
        let tick = ordered_tick.tick;
        let qty = numeric(tick.get("qty"));
        let buyer_maker = numeric(tick.get("is_buyer_maker"));
        if !qty.is_finite() || qty < 0.0 || (buyer_maker != 0.0 && buyer_maker != 1.0) {
            continue;
        }

        has_valid_tick = true;
        let signed_qty = if buyer_maker == 0.0 { qty } else { -qty } / divisor;
        running += signed_qty;
        if signed_qty > 0.0 {
            ask_qty += signed_qty;
        } else {
            bid_qty += -signed_qty;
        }
        high = high.max(running);
        low = low.min(running);
    }

    has_valid_tick.then_some(BidAskFlowCandle {
        time: timestamp_ms as f64 / 1_000.0,
        open,
        high,
        low,
        close: running,
        ask_qty,
        bid_qty,
    })
}

struct OrderedTick<'a> {
    tick: &'a Value,
    index: usize,
    time: f64,
    time_us: f64,
}

fn numeric(value: Option<&Value>) -> f64 {
    match value {
        Some(Value::Number(value)) => value.as_f64().unwrap_or(f64::NAN),
        Some(Value::String(value)) if !value.trim().is_empty() => value.parse().unwrap_or(f64::NAN),
        _ => f64::NAN,
    }
}

fn finite_or_infinity(value: f64) -> f64 {
    if value.is_finite() {
        value
    } else {
        f64::INFINITY
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::calculate_attributes;

    #[test]
    fn cumulative_candles_scale_and_order_raw_ticks_without_reading_prices() {
        let first = json!({
            "header": {"quantity_scale": "2"},
            "raw_ticks": [
                {"time": "2", "qty": "300", "is_buyer_maker": 1},
                {"time": "1", "qty": "500", "is_buyer_maker": "0"}
            ]
        });
        let second = json!({
            "header": {"quantity_scale": 2},
            "raw_ticks": [
                {"time": 3, "qty": 100, "is_buyer_maker": 1}
            ]
        });

        let result = calculate_attributes([(60_000, &first), (120_000, &second)]);

        assert_eq!(result.candles.len(), 2);
        assert_eq!(
            result.candles[0],
            crate::charts::BidAskFlowCandle {
                time: 60.0,
                open: 0.0,
                high: 5.0,
                low: 0.0,
                close: 2.0,
                ask_qty: 5.0,
                bid_qty: 3.0,
            }
        );
        assert_eq!(result.candles[1].open, 2.0);
        assert_eq!(result.candles[1].low, 1.0);
        assert_eq!(result.candles[1].close, 1.0);
        assert_eq!(result.candles[1].bid_qty, 1.0);
    }

    #[test]
    fn invalid_bars_are_skipped_without_resetting_the_cumulative_close() {
        let first = json!({
            "header": {"quantity_scale": 0},
            "raw_ticks": [{"qty": 4, "is_buyer_maker": 0}]
        });
        let invalid = json!({
            "header": {"quantity_scale": 19},
            "raw_ticks": [{"qty": 100, "is_buyer_maker": 0}]
        });
        let last = json!({
            "header": {"quantity_scale": 0},
            "raw_ticks": [
                {"qty": -1, "is_buyer_maker": 0},
                {"qty": 2, "is_buyer_maker": 1}
            ]
        });

        let result =
            calculate_attributes([(60_000, &first), (120_000, &invalid), (180_000, &last)]);

        assert_eq!(result.candles.len(), 2);
        assert_eq!(result.candles[1].open, 4.0);
        assert_eq!(result.candles[1].close, 2.0);
    }
}

use serde_json::Value;

use crate::{
    charts::{MomentumIndicatorData, MomentumPoint},
    market::BarSnapshot,
};

use super::WARMUP_BARS;

const SIGNAL_PERIOD: f64 = 14.0;
const BASE_PERIOD: f64 = 100.0;
const ALPHA: f64 = 0.5;
const MAX_INTENSITY: f64 = 3.0;
const RATIO_PRECISION: u64 = 1_000_000_000_000_000;
const MIN_TIMESTAMP_MS: u64 = 946_684_800_000;
const MAX_TIMESTAMP_MS: u64 = 4_133_980_800_000;

#[derive(Debug)]
struct MomentumBar {
    timestamp_ms: u64,
    buy_notional: u128,
    sell_notional: u128,
    positive_change: u128,
    negative_change: u128,
    doubtful: bool,
}

pub(super) fn calculate(bars: &[BarSnapshot]) -> MomentumIndicatorData {
    if bars.is_empty() {
        return MomentumIndicatorData::default();
    }

    let result = bars
        .iter()
        .enumerate()
        .map(|(index, bar)| parse_bar(bar.attributes(), index + 1))
        .collect::<Result<Vec<_>, _>>()
        .map(|bars| calculate_points(&bars));

    match result {
        Ok(points) => MomentumIndicatorData {
            points: points.into(),
            error: None,
        },
        Err(error) => MomentumIndicatorData {
            points: Vec::new().into(),
            error: Some(error),
        },
    }
}

fn calculate_points(bars: &[MomentumBar]) -> Vec<MomentumPoint> {
    let signal_k = 2.0 / (SIGNAL_PERIOD + 1.0);
    let base_k = 2.0 / (BASE_PERIOD + 1.0);
    let mut base = None;
    let mut bullish = None;
    let mut bearish = None;

    bars.iter()
        .enumerate()
        .map(|(index, bar)| {
            let total = bar.buy_notional as f64 + bar.sell_notional as f64;
            let imbalance = signed_ratio(bar.buy_notional, bar.sell_notional);
            let efficiency = signed_ratio(bar.positive_change, bar.negative_change);
            let reference = base.filter(|value: &f64| *value > 0.0).unwrap_or(total);
            let intensity = if reference > 0.0 {
                (total / reference).sqrt().min(MAX_INTENSITY)
            } else {
                0.0
            };
            base = Some(match base {
                Some(previous) => previous + base_k * (total - previous),
                None => total,
            });

            // The ratio helper returns zero for an empty pair, matching the
            // TypeScript implementation's zero-denominator behavior.
            let up = intensity * (ALPHA * imbalance.max(0.0) + (1.0 - ALPHA) * efficiency.max(0.0));
            let down = intensity
                * (ALPHA * (-imbalance).max(0.0) + (1.0 - ALPHA) * (-efficiency).max(0.0));
            bullish = Some(match bullish {
                Some(previous) => previous + signal_k * (up - previous),
                None => up,
            });
            bearish = Some(match bearish {
                Some(previous) => previous + signal_k * (down - previous),
                None => down,
            });

            let visible = index >= WARMUP_BARS;
            MomentumPoint {
                time: bar.timestamp_ms as f64 / 1_000.0,
                bullish: visible.then_some(bullish.unwrap_or_default()),
                bearish: visible.then_some(bearish.unwrap_or_default()),
                doubtful: bar.doubtful,
            }
        })
        .collect()
}

/// Calculate a signed ratio after subtracting the exact unsigned integers.
/// This avoids overflow for wire values near u128::MAX.
fn signed_ratio(positive: u128, negative: u128) -> f64 {
    let denominator = Wide::sum(positive, negative);
    if denominator == Wide::default() {
        return 0.0;
    }
    let (magnitude, sign) = if positive >= negative {
        (positive - negative, 1.0)
    } else {
        (negative - positive, -1.0)
    };
    let numerator = Wide::from(magnitude);
    if numerator == denominator {
        return sign;
    }

    // The source implementation truncates BigInt ratios to 15 decimal places.
    // Long division keeps that behavior without requiring a wider integer type.
    let mut remainder = numerator;
    let mut scaled = 0_u64;
    for _ in 0..15 {
        remainder = remainder.multiply_by_ten();
        let mut digit = 0_u64;
        while remainder >= denominator {
            remainder = remainder.subtract(denominator);
            digit += 1;
        }
        scaled = scaled * 10 + digit;
    }
    sign * (scaled as f64 / RATIO_PRECISION as f64)
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
struct Wide {
    high: u64,
    low: u128,
}

impl Wide {
    fn from(value: u128) -> Self {
        Self {
            high: 0,
            low: value,
        }
    }

    fn sum(left: u128, right: u128) -> Self {
        let (low, carry) = left.overflowing_add(right);
        Self {
            high: u64::from(carry),
            low,
        }
    }

    fn multiply_by_ten(self) -> Self {
        let low_low = self.low as u64;
        let low_high = (self.low >> 64) as u64;
        let product_low = u128::from(low_low) * 10;
        let product_high = u128::from(low_high) * 10 + (product_low >> 64);
        Self {
            high: self.high * 10 + (product_high >> 64) as u64,
            low: (u128::from(product_high as u64) << 64) | u128::from(product_low as u64),
        }
    }

    fn subtract(self, other: Self) -> Self {
        let (low, borrow) = self.low.overflowing_sub(other.low);
        Self {
            high: self.high - other.high - u64::from(borrow),
            low,
        }
    }
}

fn parse_bar(value: &Value, line: usize) -> Result<MomentumBar, String> {
    for field in ["source", "symbol", "timeframe", "bar_key"] {
        required_string(value.get(field), field, line)?;
    }

    let revision = value.get("revision_raw").or_else(|| value.get("revision"));
    required_u64(revision, "revision", line)?;
    let timestamp_ms = required_u64(value.get("timestamp"), "timestamp", line)?;
    if !(MIN_TIMESTAMP_MS..MAX_TIMESTAMP_MS).contains(&timestamp_ms) {
        return Err(format!(
            "Fecha fuera del rango 2000–2100 en timestamp, línea {line}; comprueba la unidad (ms)."
        ));
    }

    let price = required_object(value.get("price"), "price", line)?;
    required_i64(price.get("close"), "price.close", line)?;
    let volume_flow = required_object(value.get("volume_flow"), "volume_flow", line)?;
    let price_changes = required_object(value.get("price_changes"), "price_changes", line)?;
    let integrity = value.get("integrity").and_then(Value::as_object);

    Ok(MomentumBar {
        timestamp_ms,
        buy_notional: required_u128(
            volume_flow.get("buy_notional"),
            "volume_flow.buy_notional",
            line,
        )?,
        sell_notional: required_u128(
            volume_flow.get("sell_notional"),
            "volume_flow.sell_notional",
            line,
        )?,
        positive_change: required_u128(
            price_changes.get("positive_change_magnitude"),
            "price_changes.positive_change_magnitude",
            line,
        )?,
        negative_change: required_signed_magnitude(
            price_changes.get("negative_change_magnitude"),
            "price_changes.negative_change_magnitude",
            line,
        )?,
        doubtful: optional_u64(
            integrity.and_then(|fields| fields.get("sequence_gaps")),
            "integrity.sequence_gaps",
            line,
        )? > 0
            || optional_u64(
                integrity.and_then(|fields| fields.get("dropped_trade_count")),
                "integrity.dropped_trade_count",
                line,
            )? > 0
            || optional_u64(
                integrity.and_then(|fields| fields.get("invalid_trade_count")),
                "integrity.invalid_trade_count",
                line,
            )? > 0,
    })
}

fn required_string(value: Option<&Value>, field: &str, line: usize) -> Result<(), String> {
    if value
        .and_then(Value::as_str)
        .is_some_and(|value| !value.trim().is_empty())
    {
        Ok(())
    } else {
        Err(format!(
            "Campo obligatorio ausente o inválido: {field}, línea {line}"
        ))
    }
}

fn required_object<'a>(
    value: Option<&'a Value>,
    field: &str,
    line: usize,
) -> Result<&'a serde_json::Map<String, Value>, String> {
    value
        .and_then(Value::as_object)
        .ok_or_else(|| format!("Campo obligatorio ausente: {field}, línea {line}"))
}

fn required_u64(value: Option<&Value>, field: &str, line: usize) -> Result<u64, String> {
    read_u128(value)
        .and_then(|value| u64::try_from(value).ok())
        .ok_or_else(|| {
            format!("Campo obligatorio ausente o entero inválido: {field}, línea {line}")
        })
}

fn required_u128(value: Option<&Value>, field: &str, line: usize) -> Result<u128, String> {
    read_u128(value).ok_or_else(|| {
        format!("Campo obligatorio ausente o entero inválido: {field}, línea {line}")
    })
}

fn required_i64(value: Option<&Value>, field: &str, line: usize) -> Result<i64, String> {
    value
        .and_then(|value| match value {
            Value::Number(number) => number
                .as_i64()
                .or_else(|| number.as_u64().and_then(|value| i64::try_from(value).ok())),
            Value::String(value) => value.parse().ok(),
            _ => None,
        })
        .ok_or_else(|| {
            format!("Campo obligatorio ausente o entero inválido: {field}, línea {line}")
        })
}

fn required_signed_magnitude(
    value: Option<&Value>,
    field: &str,
    line: usize,
) -> Result<u128, String> {
    let parsed = value.and_then(|value| match value {
        Value::Number(number) => {
            if let Some(value) = number.as_i64() {
                Some(u128::from(value.unsigned_abs()))
            } else {
                number.as_u64().map(u128::from)
            }
        }
        Value::String(value) => value
            .strip_prefix('-')
            .map_or_else(|| value.parse().ok(), |magnitude| magnitude.parse().ok()),
        _ => None,
    });
    parsed.ok_or_else(|| {
        format!("Campo obligatorio ausente o entero inválido: {field}, línea {line}")
    })
}

fn optional_u64(value: Option<&Value>, field: &str, line: usize) -> Result<u64, String> {
    match value {
        None | Some(Value::Null) => Ok(0),
        Some(value) => required_u64(Some(value), field, line),
    }
}

fn read_u128(value: Option<&Value>) -> Option<u128> {
    match value? {
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

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::{MomentumBar, calculate_points, parse_bar};

    fn bar(index: u64, buy: &str, sell: &str, up: &str, down: &str) -> Value {
        json!({
            "source": "binance",
            "symbol": "BTCUSDT",
            "timeframe": "5m",
            "bar_key": format!("binance/BTCUSDT/5m/{index}"),
            "revision": "1",
            "timestamp": (1_600_000_000_000_u64 + index * 300_000).to_string(),
            "price": {"close": "123456789012345678"},
            "volume_flow": {
                "buy_notional": buy,
                "sell_notional": sell
            },
            "price_changes": {
                "positive_change_magnitude": up,
                "negative_change_magnitude": down
            },
            "integrity": {}
        })
    }

    fn values(count: usize, flow: (&str, &str, &str, &str)) -> Vec<MomentumBar> {
        (0..count)
            .map(|index| {
                let value = bar(index as u64, flow.0, flow.1, flow.2, flow.3);
                parse_bar(&value, index + 1).unwrap()
            })
            .collect()
    }

    fn close_to(actual: f64, expected: f64) {
        assert!((actual - expected).abs() < 1e-6, "{actual} ≉ {expected}");
    }

    #[test]
    fn uses_the_typescript_default_warmup_and_signal_smoothing() {
        let points = calculate_points(&values(60, ("600", "400", "60", "40")));

        assert!(
            points[..30]
                .iter()
                .all(|point| point.bullish.is_none() && point.bearish.is_none())
        );
        close_to(points[30].bullish.unwrap(), 0.2);
        close_to(points[59].bullish.unwrap(), 0.2);
        close_to(points[59].bearish.unwrap(), 0.0);
    }

    #[test]
    fn scales_the_notional_intensity_and_caps_the_spike() {
        let mut bars = values(60, ("600", "400", "60", "40"));
        bars.push(parse_bar(&bar(60, "4000", "0", "100", "0"), 61).unwrap());
        bars.push(parse_bar(&bar(61, "4000", "0", "100", "0"), 62).unwrap());
        let points = calculate_points(&bars);

        close_to(points[60].bullish.unwrap(), 0.44);
        close_to(points[61].bullish.unwrap(), 0.640416);
    }

    #[test]
    fn handles_large_integer_wire_values_and_integrity_flags() {
        let mut value = bar(
            0,
            "1000000000000000000000000000000",
            "3000000000000000000000000000000",
            "0",
            "-0",
        );
        value["integrity"] = json!({"dropped_trade_count": "2"});
        let input = parse_bar(&value, 1).unwrap();
        let mut bars = values(
            30,
            (
                "1000000000000000000000000000000",
                "3000000000000000000000000000000",
                "0",
                "0",
            ),
        );
        bars.push(input);
        let point = calculate_points(&bars)[30];

        close_to(point.bullish.unwrap(), 0.0);
        close_to(point.bearish.unwrap(), 0.25);
        assert!(point.doubtful);
    }

    #[test]
    fn signed_ratios_do_not_overflow_at_the_u128_limit() {
        let max = u128::MAX;

        assert_eq!(super::signed_ratio(max, 0), 1.0);
        assert_eq!(super::signed_ratio(0, max), -1.0);
        assert_eq!(super::signed_ratio(max, max), 0.0);
        assert_eq!(super::signed_ratio(3, 1), 0.5);
    }

    #[test]
    fn rejects_required_microstructure_fields() {
        let mut value = bar(0, "1", "1", "1", "1");
        value["price_changes"] = Value::Null;

        assert!(
            parse_bar(&value, 1)
                .unwrap_err()
                .contains("price_changes, línea 1")
        );
    }
}

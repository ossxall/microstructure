use std::collections::BTreeMap;

use serde_json::Value;

use crate::market::BarSnapshot;

const SCHEMA_VERSION: u64 = 4;
const PRICE_SCALE: u32 = 8;
const QUANTITY_SCALE: u32 = 8;

#[derive(Clone, Debug, Default)]
pub struct FootprintIndicatorData {
    pub target_timeframe_ms: u64,
    pub candle: Option<FootprintCandle>,
    pub error: Option<String>,
}

#[derive(Clone, Debug)]
pub struct FootprintCandle {
    pub timestamp_ms: u64,
    pub end_timestamp_ms: u64,
    pub price_scale: u32,
    pub quantity_scale: u32,
    pub open: i64,
    pub high: i64,
    pub low: i64,
    pub close: i64,
    pub total_qty: u128,
    pub total_notional: u128,
    pub buy_qty: u128,
    pub sell_qty: u128,
    pub buy_notional: u128,
    pub sell_notional: u128,
    pub signed_qty: i128,
    pub signed_notional: i128,
    pub buy_count: u64,
    pub sell_count: u64,
    pub n_trades: u64,
    pub max_trade_qty: u128,
    pub max_trade_notional: u128,
    pub vwap: u128,
    pub coverage_flags: u32,
    pub coverage: FootprintCoverage,
    pub footprint: FootprintMetrics,
    pub levels: Vec<FootprintLevel>,
}

#[derive(Clone, Debug, Default)]
pub struct FootprintCoverage {
    pub closed: bool,
    pub n_bars: usize,
    /// Elapsed source intervals without a finalized Bar.
    pub missing: Vec<u64>,
    /// Source intervals that have not elapsed yet in the current target window.
    pub pending: Vec<u64>,
    pub continuity: FootprintContinuity,
    pub revisions: Vec<(String, u64)>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FootprintContinuity {
    Verified,
    Broken,
    #[default]
    Unverifiable,
}

impl FootprintContinuity {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Verified => "VERIFICADA",
            Self::Broken => "ROTA",
            Self::Unverifiable => "NO VERIFICABLE",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FootprintLevel {
    pub price: i64,
    pub buy_qty: u128,
    pub sell_qty: u128,
    pub buy_notional: u128,
    pub sell_notional: u128,
    pub buy_count: u32,
    pub sell_count: u32,
    pub delta: i128,
    pub total_qty: u128,
    pub total_count: u32,
    pub flags: u16,
}

#[derive(Clone, Debug, Default)]
pub struct FootprintMetrics {
    pub level_count: u32,
    pub poc_price: i64,
    pub poc_volume: u128,
    pub poc_delta: i128,
    pub value_area_low: i64,
    pub value_area_high: i64,
    pub max_positive_delta: i128,
    pub max_negative_delta: i128,
    pub imbalance_count: u32,
    pub stacked_buy_imbalance: u32,
    pub stacked_sell_imbalance: u32,
    pub unfinished_auction_high: bool,
    pub unfinished_auction_low: bool,
}

#[derive(Clone, Debug)]
struct ParsedBar<'a> {
    timestamp_ms: u64,
    revision: u64,
    revision_conflict: bool,
    bar_key: String,
    attributes: &'a Value,
    first_trade_id: u64,
    last_trade_id: u64,
    trade_ids_numeric: bool,
    sequence_contiguous: bool,
    identity_coverage_flags: u32,
    open: i64,
    high: i64,
    low: i64,
    close: i64,
    total_qty: u128,
    total_notional: u128,
    buy_qty: u128,
    sell_qty: u128,
    buy_notional: u128,
    sell_notional: u128,
    signed_qty: i128,
    signed_notional: i128,
    buy_count: u64,
    sell_count: u64,
    n_trades: u64,
    max_trade_qty: u128,
    max_trade_notional: u128,
    levels: Vec<FootprintLevel>,
}

pub(super) fn calculate(
    bars: &[BarSnapshot],
    source: &str,
    symbol: &str,
    source_timeframe: &str,
    target_timeframe_ms: u64,
    now_ms: u64,
) -> Result<Option<FootprintCandle>, String> {
    let source_timeframe_ms = super::timeframe_millis(source_timeframe)
        .ok_or_else(|| format!("Timeframe fuente no soportado: {source_timeframe}"))?;
    if target_timeframe_ms < source_timeframe_ms
        || !target_timeframe_ms.is_multiple_of(source_timeframe_ms)
    {
        return Err(format!(
            "El timeframe footprint debe ser múltiplo de {source_timeframe}"
        ));
    }

    let mut latest_by_key = BTreeMap::<String, ParsedBar<'_>>::new();
    let mut seen_revisions = BTreeMap::<(String, u64), &Value>::new();
    for snapshot in bars {
        if !snapshot.is_final {
            continue;
        }
        let attributes = snapshot.attributes();
        if attributes.get("source").and_then(Value::as_str) != Some(source)
            || attributes.get("symbol").and_then(Value::as_str) != Some(symbol)
            || attributes.get("timeframe").and_then(Value::as_str) != Some(source_timeframe)
        {
            continue;
        }
        if read_u64(attributes.get("schema_version")) != Some(SCHEMA_VERSION) {
            continue;
        }
        let parsed = parse_bar(snapshot, attributes)?;
        if parsed.timestamp_ms % source_timeframe_ms != 0 {
            return Err(format!(
                "Timestamp {} de Bar {} no alineado a {source_timeframe}",
                parsed.timestamp_ms, parsed.bar_key
            ));
        }

        let revision_key = (parsed.bar_key.clone(), parsed.revision);
        if let Some(previous_content) = seen_revisions.get(&revision_key) {
            if *previous_content != parsed.attributes {
                return Err(format!(
                    "La Bar {} tiene contenido distinto con revision {}",
                    parsed.bar_key, parsed.revision
                ));
            }
        } else {
            seen_revisions.insert(revision_key, parsed.attributes);
        }

        match latest_by_key.get(&parsed.bar_key) {
            Some(current) if current.revision > parsed.revision => {}
            Some(current) if current.revision == parsed.revision => {
                if current.attributes != parsed.attributes {
                    return Err(format!(
                        "La Bar {} tiene contenido distinto con revision {}",
                        parsed.bar_key, parsed.revision
                    ));
                }
            }
            Some(current) if current.timestamp_ms != parsed.timestamp_ms => {
                return Err(format!(
                    "La Bar {} cambió de timestamp entre revisiones",
                    parsed.bar_key
                ));
            }
            _ => {
                latest_by_key.insert(parsed.bar_key.clone(), parsed);
            }
        }
    }

    let mut finalized = latest_by_key.into_values().collect::<Vec<_>>();
    finalized.sort_by_key(|bar| bar.timestamp_ms);
    let Some(latest_timestamp) = finalized.last().map(|bar| bar.timestamp_ms) else {
        return Ok(None);
    };

    // Bar.timestamp is the UTC start of its source interval. Use the latest
    // finalized source Bar so the chart keeps showing the latest available
    // target candle even before the first source Bar of a new interval closes.
    let start_ms = latest_timestamp / target_timeframe_ms * target_timeframe_ms;
    let end_ms = start_ms
        .checked_add(target_timeframe_ms)
        .ok_or_else(|| "El final de la ventana footprint desborda u64".to_string())?;
    let window = finalized
        .iter()
        .filter(|bar| start_ms <= bar.timestamp_ms && bar.timestamp_ms < end_ms)
        .collect::<Vec<_>>();
    if window.is_empty() {
        return Ok(None);
    }

    if let Some(bar) = window.iter().find(|bar| bar.revision_conflict) {
        return Err(format!(
            "Revisiones con contenido distinto para Bar {}",
            bar.bar_key
        ));
    }

    for bar in &window {
        if bar.timestamp_ms % source_timeframe_ms != 0 {
            return Err(format!(
                "Timestamp fuente no alineado: {}",
                bar.timestamp_ms
            ));
        }
    }

    let levels = merge_levels(&window)?;
    if levels.is_empty() {
        return Err("La ventana no contiene niveles footprint".to_string());
    }
    let mut candle = aggregate_candle(&window, start_ms, end_ms, levels)?;
    candle.coverage = evaluate_coverage(
        &finalized,
        &window,
        start_ms,
        end_ms,
        source_timeframe_ms,
        now_ms,
    )?;
    Ok(Some(candle))
}

fn parse_bar<'a>(
    snapshot: &'a BarSnapshot,
    attributes: &'a Value,
) -> Result<ParsedBar<'a>, String> {
    let bar_key = required_str(attributes.get("bar_key"), "bar_key")?.to_owned();
    let revision = required_u64(attributes.get("revision"), "revision")?;
    if revision != snapshot.revision {
        return Err(format!(
            "La revisión del snapshot no coincide con Bar {bar_key}"
        ));
    }
    let timestamp_ms = required_u64(attributes.get("timestamp"), "timestamp")?;
    if timestamp_ms != snapshot.timestamp_ms {
        return Err(format!(
            "El timestamp del snapshot no coincide con Bar {bar_key}"
        ));
    }

    let header = required_object(attributes.get("header"), "header")?;
    let header_schema = required_u64(header.get("schema_version"), "header.schema_version")?;
    if header_schema != SCHEMA_VERSION {
        return Err(format!("Header de Bar {bar_key} no usa schema v4"));
    }
    let price_scale = required_u64(header.get("price_scale"), "header.price_scale")?;
    let quantity_scale = required_u64(header.get("quantity_scale"), "header.quantity_scale")?;
    if price_scale != u64::from(PRICE_SCALE) || quantity_scale != u64::from(QUANTITY_SCALE) {
        return Err(format!(
            "Escalas no soportadas en Bar {bar_key}: price={price_scale}, quantity={quantity_scale}"
        ));
    }

    let identity = required_object(attributes.get("identity"), "identity")?;
    let price = required_object(attributes.get("price"), "price")?;
    let volume_flow = required_object(attributes.get("volume_flow"), "volume_flow")?;
    let footprint = required_object(attributes.get("footprint"), "footprint")?;
    let raw_levels = footprint
        .get("levels")
        .and_then(Value::as_array)
        .ok_or_else(|| "Campo obligatorio ausente o inválido: footprint.levels".to_string())?;
    let levels = raw_levels
        .iter()
        .enumerate()
        .map(|(index, level)| parse_level(level, index))
        .collect::<Result<Vec<_>, _>>()?;

    Ok(ParsedBar {
        timestamp_ms,
        revision,
        revision_conflict: snapshot.revision_conflict,
        bar_key,
        attributes,
        first_trade_id: required_u64(identity.get("first_trade_id"), "identity.first_trade_id")?,
        last_trade_id: required_u64(identity.get("last_trade_id"), "identity.last_trade_id")?,
        trade_ids_numeric: required_u64(
            identity.get("trade_ids_numeric"),
            "identity.trade_ids_numeric",
        )? == 1,
        sequence_contiguous: required_u64(
            identity.get("sequence_contiguous"),
            "identity.sequence_contiguous",
        )? == 1,
        identity_coverage_flags: required_u32(
            identity.get("coverage_flags"),
            "identity.coverage_flags",
        )?,
        open: required_i64(price.get("open"), "price.open")?,
        high: required_i64(price.get("high"), "price.high")?,
        low: required_i64(price.get("low"), "price.low")?,
        close: required_i64(price.get("close"), "price.close")?,
        total_qty: required_u128(volume_flow.get("total_qty"), "volume_flow.total_qty")?,
        total_notional: required_u128(
            volume_flow.get("total_notional"),
            "volume_flow.total_notional",
        )?,
        buy_qty: required_u128(volume_flow.get("buy_qty"), "volume_flow.buy_qty")?,
        sell_qty: required_u128(volume_flow.get("sell_qty"), "volume_flow.sell_qty")?,
        buy_notional: required_u128(volume_flow.get("buy_notional"), "volume_flow.buy_notional")?,
        sell_notional: required_u128(
            volume_flow.get("sell_notional"),
            "volume_flow.sell_notional",
        )?,
        signed_qty: required_i128(volume_flow.get("signed_qty"), "volume_flow.signed_qty")?,
        signed_notional: required_i128(
            volume_flow.get("signed_notional"),
            "volume_flow.signed_notional",
        )?,
        buy_count: required_u64(volume_flow.get("buy_count"), "volume_flow.buy_count")?,
        sell_count: required_u64(volume_flow.get("sell_count"), "volume_flow.sell_count")?,
        n_trades: required_u64(identity.get("n_trades"), "identity.n_trades")?,
        max_trade_qty: required_u128(
            volume_flow.get("max_trade_qty"),
            "volume_flow.max_trade_qty",
        )?,
        max_trade_notional: required_u128(
            volume_flow.get("max_trade_notional"),
            "volume_flow.max_trade_notional",
        )?,
        levels,
    })
}

fn parse_level(value: &Value, index: usize) -> Result<FootprintLevel, String> {
    let object = required_object(Some(value), "footprint.level")?;
    let field = |name: &str| format!("footprint.levels[{index}].{name}");
    Ok(FootprintLevel {
        price: required_i64(object.get("price"), &field("price"))?,
        buy_qty: required_u128(object.get("buy_qty"), &field("buy_qty"))?,
        sell_qty: required_u128(object.get("sell_qty"), &field("sell_qty"))?,
        buy_notional: required_u128(object.get("buy_notional"), &field("buy_notional"))?,
        sell_notional: required_u128(object.get("sell_notional"), &field("sell_notional"))?,
        buy_count: required_u32(object.get("buy_count"), &field("buy_count"))?,
        sell_count: required_u32(object.get("sell_count"), &field("sell_count"))?,
        delta: required_i128(object.get("delta"), &field("delta"))?,
        total_qty: required_u128(object.get("total_qty"), &field("total_qty"))?,
        total_count: required_u32(object.get("total_count"), &field("total_count"))?,
        flags: required_u16(object.get("flags"), &field("flags"))?,
    })
}

fn merge_levels(window: &[&ParsedBar<'_>]) -> Result<Vec<FootprintLevel>, String> {
    let mut merged = BTreeMap::<i64, FootprintLevel>::new();
    for bar in window {
        for level in &bar.levels {
            let target = merged.entry(level.price).or_insert(FootprintLevel {
                price: level.price,
                ..Default::default()
            });
            target.buy_qty = checked_add(target.buy_qty, level.buy_qty, "footprint buy_qty")?;
            target.sell_qty = checked_add(target.sell_qty, level.sell_qty, "footprint sell_qty")?;
            target.buy_notional = checked_add(
                target.buy_notional,
                level.buy_notional,
                "footprint buy_notional",
            )?;
            target.sell_notional = checked_add(
                target.sell_notional,
                level.sell_notional,
                "footprint sell_notional",
            )?;
            target.buy_count = target
                .buy_count
                .checked_add(level.buy_count)
                .ok_or_else(|| "Desbordamiento u32 en footprint buy_count".to_string())?;
            target.sell_count = target
                .sell_count
                .checked_add(level.sell_count)
                .ok_or_else(|| "Desbordamiento u32 en footprint sell_count".to_string())?;
        }
    }

    for level in merged.values_mut() {
        level.total_qty = checked_add(level.buy_qty, level.sell_qty, "footprint total_qty")?;
        level.total_count = level
            .buy_count
            .checked_add(level.sell_count)
            .ok_or_else(|| "Desbordamiento u32 en footprint total_count".to_string())?;
        level.delta = i128::try_from(level.buy_qty)
            .map_err(|_| "footprint buy_qty excede i128".to_string())?
            .checked_sub(
                i128::try_from(level.sell_qty)
                    .map_err(|_| "footprint sell_qty excede i128".to_string())?,
            )
            .ok_or_else(|| "Desbordamiento i128 en footprint delta".to_string())?;

        let buy_imbalance =
            level.buy_qty > 0 && (level.sell_qty == 0 || level.buy_qty / level.sell_qty >= 3);
        let sell_imbalance =
            level.sell_qty > 0 && (level.buy_qty == 0 || level.sell_qty / level.buy_qty >= 3);
        level.flags = u16::from(buy_imbalance) | (u16::from(sell_imbalance) << 1);
    }

    Ok(merged.into_values().collect())
}

fn aggregate_candle(
    window: &[&ParsedBar<'_>],
    start_ms: u64,
    end_ms: u64,
    levels: Vec<FootprintLevel>,
) -> Result<FootprintCandle, String> {
    let first = window
        .first()
        .ok_or_else(|| "No hay Bars fuente en la ventana".to_string())?;
    let last = window
        .last()
        .ok_or_else(|| "No hay Bars fuente en la ventana".to_string())?;

    let mut result = FootprintCandle {
        timestamp_ms: start_ms,
        end_timestamp_ms: end_ms,
        price_scale: PRICE_SCALE,
        quantity_scale: QUANTITY_SCALE,
        open: first.open,
        high: i64::MIN,
        low: i64::MAX,
        close: last.close,
        total_qty: 0,
        total_notional: 0,
        buy_qty: 0,
        sell_qty: 0,
        buy_notional: 0,
        sell_notional: 0,
        signed_qty: 0,
        signed_notional: 0,
        buy_count: 0,
        sell_count: 0,
        n_trades: 0,
        max_trade_qty: 0,
        max_trade_notional: 0,
        vwap: 0,
        coverage_flags: 0,
        coverage: FootprintCoverage::default(),
        footprint: FootprintMetrics::default(),
        levels,
    };

    for bar in window {
        result.high = result.high.max(bar.high);
        result.low = result.low.min(bar.low);
        result.total_qty = checked_add(result.total_qty, bar.total_qty, "volume_flow.total_qty")?;
        result.total_notional = checked_add(
            result.total_notional,
            bar.total_notional,
            "volume_flow.total_notional",
        )?;
        result.buy_qty = checked_add(result.buy_qty, bar.buy_qty, "volume_flow.buy_qty")?;
        result.sell_qty = checked_add(result.sell_qty, bar.sell_qty, "volume_flow.sell_qty")?;
        result.buy_notional = checked_add(
            result.buy_notional,
            bar.buy_notional,
            "volume_flow.buy_notional",
        )?;
        result.sell_notional = checked_add(
            result.sell_notional,
            bar.sell_notional,
            "volume_flow.sell_notional",
        )?;
        result.signed_qty = result
            .signed_qty
            .checked_add(bar.signed_qty)
            .ok_or_else(|| "Desbordamiento i128 en volume_flow.signed_qty".to_string())?;
        result.signed_notional = result
            .signed_notional
            .checked_add(bar.signed_notional)
            .ok_or_else(|| "Desbordamiento i128 en volume_flow.signed_notional".to_string())?;
        result.buy_count = result
            .buy_count
            .checked_add(bar.buy_count)
            .ok_or_else(|| "Desbordamiento u64 en volume_flow.buy_count".to_string())?;
        result.sell_count = result
            .sell_count
            .checked_add(bar.sell_count)
            .ok_or_else(|| "Desbordamiento u64 en volume_flow.sell_count".to_string())?;
        result.n_trades = result
            .n_trades
            .checked_add(bar.n_trades)
            .ok_or_else(|| "Desbordamiento u64 en identity.n_trades".to_string())?;
        result.max_trade_qty = result.max_trade_qty.max(bar.max_trade_qty);
        result.max_trade_notional = result.max_trade_notional.max(bar.max_trade_notional);
        result.coverage_flags |= bar.identity_coverage_flags;
    }

    result.vwap = result
        .total_notional
        .checked_div(result.total_qty)
        .unwrap_or(0);

    let level_buy_qty = result.levels.iter().try_fold(0_u128, |sum, level| {
        checked_add(sum, level.buy_qty, "suma footprint buy_qty")
    })?;
    let level_sell_qty = result.levels.iter().try_fold(0_u128, |sum, level| {
        checked_add(sum, level.sell_qty, "suma footprint sell_qty")
    })?;
    let level_buy_count = result.levels.iter().try_fold(0_u64, |sum, level| {
        sum.checked_add(u64::from(level.buy_count))
            .ok_or_else(|| "Desbordamiento u64 en suma footprint buy_count".to_string())
    })?;
    let level_sell_count = result.levels.iter().try_fold(0_u64, |sum, level| {
        sum.checked_add(u64::from(level.sell_count))
            .ok_or_else(|| "Desbordamiento u64 en suma footprint sell_count".to_string())
    })?;
    if (
        level_buy_qty,
        level_sell_qty,
        level_buy_count,
        level_sell_count,
    ) != (
        result.buy_qty,
        result.sell_qty,
        result.buy_count,
        result.sell_count,
    ) {
        return Err("Los niveles footprint no coinciden con volume_flow".to_string());
    }
    if result.levels.first().map(|level| level.price) != Some(result.low)
        || result.levels.last().map(|level| level.price) != Some(result.high)
    {
        return Err("Los extremos footprint no coinciden con OHLC".to_string());
    }

    result.footprint = derive_metrics(&result.levels)?;
    Ok(result)
}

fn derive_metrics(levels: &[FootprintLevel]) -> Result<FootprintMetrics, String> {
    let Some((poc_index, poc)) = levels.iter().enumerate().max_by(|(_, left), (_, right)| {
        left.total_qty
            .cmp(&right.total_qty)
            .then_with(|| right.price.cmp(&left.price))
    }) else {
        return Err("No se puede derivar footprint sin niveles".to_string());
    };

    let known_side_volume = levels.iter().try_fold(0_u128, |sum, level| {
        let level_known = checked_add(level.buy_qty, level.sell_qty, "volumen conocido")?;
        checked_add(sum, level_known, "volumen conocido")
    })?;
    let target = known_side_volume
        .checked_mul(70)
        .ok_or_else(|| "Desbordamiento al calcular el área de valor".to_string())?
        .div_ceil(100);
    let mut accumulated = poc.total_qty;
    let mut low_index = poc_index;
    let mut high_index = poc_index;
    while accumulated < target {
        let below = low_index
            .checked_sub(1)
            .map(|index| levels[index].total_qty);
        let above = (high_index + 1 < levels.len()).then(|| levels[high_index + 1].total_qty);
        let take_above = match (below, above) {
            (Some(lower), Some(upper)) => upper >= lower,
            (None, Some(_)) => true,
            (Some(_), None) => false,
            (None, None) => break,
        };
        let next_index = if take_above {
            high_index += 1;
            high_index
        } else {
            low_index -= 1;
            low_index
        };
        accumulated = checked_add(
            accumulated,
            levels[next_index].total_qty,
            "volumen del área de valor",
        )?;
    }

    let mut metrics = FootprintMetrics {
        level_count: u32::try_from(levels.len())
            .map_err(|_| "Demasiados niveles footprint para u32".to_string())?,
        poc_price: poc.price,
        poc_volume: poc.total_qty,
        poc_delta: poc.delta,
        value_area_low: levels[low_index].price,
        value_area_high: levels[high_index].price,
        unfinished_auction_high: levels.last().is_some_and(|level| level.sell_qty == 0),
        unfinished_auction_low: levels.first().is_some_and(|level| level.buy_qty == 0),
        ..Default::default()
    };

    let mut buy_run = 0_u32;
    let mut sell_run = 0_u32;
    for level in levels {
        metrics.max_positive_delta = metrics.max_positive_delta.max(level.delta);
        metrics.max_negative_delta = metrics.max_negative_delta.min(level.delta);
        if level.flags != 0 {
            metrics.imbalance_count = metrics
                .imbalance_count
                .checked_add(1)
                .ok_or_else(|| "Desbordamiento u32 en imbalance_count".to_string())?;
        }
        buy_run = if level.flags & 1 != 0 {
            buy_run
                .checked_add(1)
                .ok_or_else(|| "Desbordamiento u32 en stacked_buy_imbalance".to_string())?
        } else {
            0
        };
        sell_run = if level.flags & 2 != 0 {
            sell_run
                .checked_add(1)
                .ok_or_else(|| "Desbordamiento u32 en stacked_sell_imbalance".to_string())?
        } else {
            0
        };
        metrics.stacked_buy_imbalance = metrics.stacked_buy_imbalance.max(buy_run);
        metrics.stacked_sell_imbalance = metrics.stacked_sell_imbalance.max(sell_run);
    }
    Ok(metrics)
}

fn evaluate_coverage(
    all: &[ParsedBar<'_>],
    window: &[&ParsedBar<'_>],
    start_ms: u64,
    end_ms: u64,
    source_timeframe_ms: u64,
    now_ms: u64,
) -> Result<FootprintCoverage, String> {
    let closed = now_ms >= end_ms;
    let mut missing = Vec::new();
    let mut pending = Vec::new();
    let present = window
        .iter()
        .map(|bar| bar.timestamp_ms)
        .collect::<std::collections::HashSet<_>>();
    let expected_count = (end_ms - start_ms) / source_timeframe_ms;
    for index in 0..expected_count {
        let timestamp = start_ms
            .checked_add(index.saturating_mul(source_timeframe_ms))
            .ok_or_else(|| "Desbordamiento al enumerar cobertura".to_string())?;
        let source_end = timestamp
            .checked_add(source_timeframe_ms)
            .ok_or_else(|| "Desbordamiento en el intervalo fuente".to_string())?;
        if source_end <= now_ms {
            if !present.contains(&timestamp) {
                missing.push(timestamp);
            }
        } else {
            pending.push(timestamp);
        }
    }

    let previous = all
        .iter()
        .rev()
        .find(|bar| bar.timestamp_ms < start_ms && !bar.revision_conflict);
    let next = all
        .iter()
        .find(|bar| bar.timestamp_ms >= end_ms && !bar.revision_conflict);
    let continuity = evaluate_continuity(
        window,
        previous,
        next,
        start_ms,
        end_ms,
        source_timeframe_ms,
    );
    let revisions = window
        .iter()
        .map(|bar| (bar.bar_key.clone(), bar.revision))
        .collect();
    Ok(FootprintCoverage {
        closed,
        n_bars: window.len(),
        missing,
        pending,
        continuity,
        revisions,
    })
}

fn evaluate_continuity(
    window: &[&ParsedBar<'_>],
    previous: Option<&ParsedBar<'_>>,
    next: Option<&ParsedBar<'_>>,
    start_ms: u64,
    end_ms: u64,
    source_timeframe_ms: u64,
) -> FootprintContinuity {
    let mut chain = Vec::with_capacity(window.len() + 2);
    if let Some(previous) = previous {
        chain.push(previous);
    }
    chain.extend(window.iter().copied());
    if let Some(next) = next {
        chain.push(next);
    }
    if chain.iter().any(|bar| !bar.trade_ids_numeric) {
        return FootprintContinuity::Unverifiable;
    }
    if window.iter().any(|bar| !bar.sequence_contiguous) {
        return FootprintContinuity::Broken;
    }

    let mut uncertain_gap = false;
    for pair in chain.windows(2) {
        let left = pair[0];
        let right = pair[1];
        let expected = left.last_trade_id.checked_add(1);
        if expected == Some(right.first_trade_id) {
            continue;
        }

        let both_inside = left.timestamp_ms >= start_ms
            && left.timestamp_ms < end_ms
            && right.timestamp_ms < end_ms;
        let adjacent =
            left.timestamp_ms.checked_add(source_timeframe_ms) == Some(right.timestamp_ms);
        if both_inside || adjacent {
            return FootprintContinuity::Broken;
        }
        uncertain_gap = true;
    }

    if previous.is_none() || next.is_none() || uncertain_gap {
        FootprintContinuity::Unverifiable
    } else {
        FootprintContinuity::Verified
    }
}

fn checked_add(left: u128, right: u128, field: &str) -> Result<u128, String> {
    left.checked_add(right)
        .ok_or_else(|| format!("Desbordamiento u128 en {field}"))
}

fn required_object<'a>(
    value: Option<&'a Value>,
    field: &str,
) -> Result<&'a serde_json::Map<String, Value>, String> {
    value
        .and_then(Value::as_object)
        .ok_or_else(|| format!("Campo obligatorio ausente o inválido: {field}"))
}

fn required_str<'a>(value: Option<&'a Value>, field: &str) -> Result<&'a str, String> {
    value
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| format!("Campo obligatorio ausente o inválido: {field}"))
}

fn required_u64(value: Option<&Value>, field: &str) -> Result<u64, String> {
    read_u128(value)
        .and_then(|value| u64::try_from(value).ok())
        .ok_or_else(|| format!("Campo obligatorio ausente o inválido: {field}"))
}

fn required_u32(value: Option<&Value>, field: &str) -> Result<u32, String> {
    read_u128(value)
        .and_then(|value| u32::try_from(value).ok())
        .ok_or_else(|| format!("Campo obligatorio ausente o inválido: {field}"))
}

fn required_u16(value: Option<&Value>, field: &str) -> Result<u16, String> {
    read_u128(value)
        .and_then(|value| u16::try_from(value).ok())
        .ok_or_else(|| format!("Campo obligatorio ausente o inválido: {field}"))
}

fn required_u128(value: Option<&Value>, field: &str) -> Result<u128, String> {
    read_u128(value).ok_or_else(|| format!("Campo obligatorio ausente o inválido: {field}"))
}

fn required_i64(value: Option<&Value>, field: &str) -> Result<i64, String> {
    read_i128(value)
        .and_then(|value| i64::try_from(value).ok())
        .ok_or_else(|| format!("Campo obligatorio ausente o inválido: {field}"))
}

fn required_i128(value: Option<&Value>, field: &str) -> Result<i128, String> {
    read_i128(value).ok_or_else(|| format!("Campo obligatorio ausente o inválido: {field}"))
}

fn read_u64(value: Option<&Value>) -> Option<u64> {
    read_u128(value).and_then(|value| u64::try_from(value).ok())
}

fn read_u128(value: Option<&Value>) -> Option<u128> {
    match value? {
        Value::Number(number) => number.as_u64().map(u128::from).or_else(|| {
            number
                .as_i64()
                .filter(|value| *value >= 0)
                .map(|v| v as u128)
        }),
        Value::String(value) => value.parse().ok(),
        _ => None,
    }
}

fn read_i128(value: Option<&Value>) -> Option<i128> {
    match value? {
        Value::Number(number) => number
            .as_i64()
            .map(i128::from)
            .or_else(|| number.as_u64().map(i128::from)),
        Value::String(value) => value.parse().ok(),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use crate::market::{BarSnapshot, test_bar_snapshot};

    use super::{FootprintContinuity, calculate};

    fn bar_value(timestamp: u64, revision: u64, open: i64, close: i64, level: i64) -> Value {
        json!({
            "schema_version": 4,
            "timestamp": timestamp.to_string(),
            "source": "binance",
            "symbol": "BTCUSDT",
            "timeframe": "5m",
            "revision": revision.to_string(),
            "bar_key": format!("binance/BTCUSDT/5m/{timestamp}"),
            "header": {
                "schema_version": 4,
                "price_scale": 8,
                "quantity_scale": 8
            },
            "identity": {
                "first_trade_id": timestamp.to_string(),
                "last_trade_id": (timestamp + 1).to_string(),
                "trade_ids_numeric": 1,
                "sequence_contiguous": 1,
                "coverage_flags": 0,
                "n_trades": "2"
            },
            "price": {
                "open": open.to_string(),
                "high": open.max(close).to_string(),
                "low": open.min(close).to_string(),
                "close": close.to_string()
            },
            "volume_flow": {
                "total_qty": "300000000",
                "total_notional": "30000000000000000",
                "buy_qty": "200000000",
                "sell_qty": "100000000",
                "buy_notional": "20000000000000000",
                "sell_notional": "10000000000000000",
                "signed_qty": "100000000",
                "signed_notional": "10000000000000000",
                "buy_count": "1",
                "sell_count": "1",
                "max_trade_qty": "200000000",
                "max_trade_notional": "20000000000000000",
                "vwap": "100000000"
            },
            "footprint": {
                "levels": [{
                    "price": level.to_string(),
                    "buy_qty": "200000000",
                    "sell_qty": "100000000",
                    "buy_notional": "20000000000000000",
                    "sell_notional": "10000000000000000",
                    "buy_count": 1,
                    "sell_count": 1,
                    "delta": "100000000",
                    "total_qty": "300000000",
                    "total_count": 2,
                    "flags": 0
                }]
            }
        })
    }

    fn bar(timestamp: u64, revision: u64, open: i64, close: i64, level: i64) -> BarSnapshot {
        test_bar_snapshot(
            bar_value(timestamp, revision, open, close, level),
            revision,
            true,
            false,
        )
    }

    fn calculate_at(bars: &[BarSnapshot], now_ms: u64) -> super::FootprintCandle {
        calculate(bars, "binance", "BTCUSDT", "5m", 3_600_000, now_ms)
            .unwrap()
            .unwrap()
    }

    #[test]
    fn resamples_source_bars_into_one_utc_aligned_footprint() {
        let mut second_value = bar_value(300_000, 1, 101_000_000, 101_000_000, 101_000_000);
        second_value["identity"]["coverage_flags"] = json!(16);
        let bars = vec![
            bar(0, 1, 100_000_000, 100_000_000, 100_000_000),
            test_bar_snapshot(second_value, 1, true, false),
        ];
        let candle = calculate_at(&bars, 600_000);

        assert_eq!(candle.timestamp_ms, 0);
        assert_eq!(candle.end_timestamp_ms, 3_600_000);
        assert_eq!(
            (candle.open, candle.high, candle.low, candle.close),
            (100_000_000, 101_000_000, 100_000_000, 101_000_000)
        );
        assert_eq!(candle.total_qty, 600_000_000);
        assert_eq!(candle.levels.len(), 2);
        assert_eq!(candle.levels[0].price, 100_000_000);
        assert_eq!(candle.levels[1].price, 101_000_000);
        assert_eq!(candle.footprint.poc_price, 100_000_000);
        assert_eq!(candle.footprint.value_area_low, 100_000_000);
        assert_eq!(candle.footprint.value_area_high, 101_000_000);
        assert_eq!(candle.coverage.n_bars, 2);
        assert_eq!(candle.coverage_flags, 16);
        assert!(!candle.coverage.closed);
        assert_eq!(candle.coverage.missing.len(), 0);
        assert_eq!(candle.coverage.pending.len(), 10);
    }

    #[test]
    fn ignores_unfinalized_bars_and_selects_latest_available_interval() {
        let bars = vec![
            bar(0, 1, 100_000_000, 100_000_000, 100_000_000),
            test_bar_snapshot(
                bar_value(300_000, 2, 101_000_000, 102_000_000, 101_000_000),
                2,
                false,
                false,
            ),
        ];
        let candle = calculate_at(&bars, 600_000);

        assert_eq!(candle.coverage.n_bars, 1);
        assert_eq!(candle.close, 100_000_000);
    }

    #[test]
    fn recalculates_derived_profile_metrics_after_merging_levels() {
        let mut first = bar_value(0, 1, 100, 100, 100);
        let mut second = bar_value(300_000, 1, 100, 100, 100);
        for payload in [&mut first, &mut second] {
            payload["identity"]["n_trades"] = json!("3");
            payload["volume_flow"]["total_qty"] = json!("600000000");
            payload["volume_flow"]["total_notional"] = json!("60000000000000000");
            payload["volume_flow"]["buy_qty"] = json!("600000000");
            payload["volume_flow"]["sell_qty"] = json!("0");
            payload["volume_flow"]["buy_notional"] = json!("60000000000000000");
            payload["volume_flow"]["sell_notional"] = json!("0");
            payload["volume_flow"]["signed_qty"] = json!("600000000");
            payload["volume_flow"]["signed_notional"] = json!("60000000000000000");
            payload["volume_flow"]["buy_count"] = json!("3");
            payload["volume_flow"]["sell_count"] = json!("0");
            payload["footprint"]["levels"][0]["buy_qty"] = json!("600000000");
            payload["footprint"]["levels"][0]["sell_qty"] = json!("0");
            payload["footprint"]["levels"][0]["buy_notional"] = json!("60000000000000000");
            payload["footprint"]["levels"][0]["sell_notional"] = json!("0");
            payload["footprint"]["levels"][0]["buy_count"] = json!(3);
            payload["footprint"]["levels"][0]["sell_count"] = json!(0);
            payload["footprint"]["levels"][0]["total_qty"] = json!("600000000");
            payload["footprint"]["levels"][0]["total_count"] = json!(3);
        }
        let first = test_bar_snapshot(first, 1, true, false);
        let second = test_bar_snapshot(second, 1, true, false);
        let candle = calculate_at(&[first, second], 600_000);

        assert_eq!(candle.footprint.poc_price, 100);
        assert_eq!(candle.footprint.value_area_low, 100);
        assert_eq!(candle.footprint.value_area_high, 100);
        assert_eq!(candle.footprint.stacked_buy_imbalance, 1);
        assert!(candle.footprint.unfinished_auction_high);
        assert!(!candle.footprint.unfinished_auction_low);
    }

    #[test]
    fn ignores_revision_conflicts_outside_the_current_footprint_window() {
        let conflicted_past = test_bar_snapshot(bar_value(0, 1, 100, 101, 100), 1, true, true);
        let current = bar(3_600_000, 1, 101, 101, 101);

        let candle = calculate(
            &[conflicted_past, current],
            "binance",
            "BTCUSDT",
            "5m",
            3_600_000,
            3_600_000,
        )
        .unwrap()
        .unwrap();

        assert_eq!(candle.timestamp_ms, 3_600_000);
        assert_eq!(
            candle.coverage.continuity,
            FootprintContinuity::Unverifiable
        );
    }

    #[test]
    fn rejects_revision_conflicts_inside_the_current_footprint_window() {
        let conflicted = test_bar_snapshot(bar_value(0, 1, 100, 101, 100), 1, true, true);

        let error = calculate(
            &[conflicted],
            "binance",
            "BTCUSDT",
            "5m",
            3_600_000,
            600_000,
        )
        .unwrap_err();

        assert!(error.contains("Revisiones con contenido distinto"));
    }

    #[test]
    fn rejects_equal_revisions_with_different_content() {
        let first = bar(0, 1, 100, 101, 100);
        let mut duplicate_value = bar_value(0, 1, 100, 102, 100);
        duplicate_value["price"]["high"] = json!("102");
        let duplicate = test_bar_snapshot(duplicate_value, 1, true, false);

        let result = calculate(
            &[first, duplicate],
            "binance",
            "BTCUSDT",
            "5m",
            3_600_000,
            600_000,
        );
        assert!(result.unwrap_err().contains("contenido distinto"));
    }

    #[test]
    fn revision_selection_is_order_independent_and_ignores_old_revisions() {
        let old = bar(0, 1, 100, 100, 100);
        let latest = bar(0, 2, 100, 100, 100);

        let forward = calculate_at(&[old.clone(), latest.clone()], 300_000);
        let reversed = calculate_at(&[latest, old], 300_000);

        assert_eq!(forward.timestamp_ms, reversed.timestamp_ms);
        assert_eq!(forward.close, reversed.close);
        assert_eq!(forward.levels, reversed.levels);
        assert_eq!(forward.coverage.revisions[0].1, 2);
        assert_eq!(reversed.coverage.revisions[0].1, 2);
    }

    #[test]
    fn marks_timeframe_incompatibility_as_an_error() {
        let result = calculate(&[], "binance", "BTCUSDT", "5m", 420_000, 0);
        assert!(result.unwrap_err().contains("múltiplo"));
    }

    #[test]
    fn continuity_is_not_verifiable_without_bounding_bars() {
        let bars = vec![bar(0, 1, 100, 100, 100)];
        let candle = calculate_at(&bars, 300_000);

        assert_eq!(
            candle.coverage.continuity,
            FootprintContinuity::Unverifiable
        );
    }
}

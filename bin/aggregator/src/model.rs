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
use num_bigint::{BigInt, BigUint};
use serde::Serialize;
use std::collections::{BTreeMap, HashSet};

pub const SCHEMA_VERSION: u16 = 3;
pub const HISTOGRAM_BUCKETS: usize = 16;
pub const PRICE_SCALE: u8 = 8;
pub const QUANTITY_SCALE: u8 = 8;
pub const TIME_UNIT_MILLISECONDS: u8 = 1;

pub(crate) mod decimal_string {
    use serde::{Serialize, Serializer};
    use std::fmt::Display;

    pub(crate) fn serialize<T: Display, S: Serializer>(
        value: &T,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&value.to_string())
    }

    pub(crate) fn serialize_vec<T: Display, S: Serializer>(
        values: &[T],
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        values
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .serialize(serializer)
    }
}

pub const COVERAGE_RECEIVE_TIME_UNAVAILABLE: u32 = 1 << 0;
pub const COVERAGE_NON_NUMERIC_TRADE_ID: u32 = 1 << 1;
pub const COVERAGE_DUPLICATES_UNAVAILABLE: u32 = 1 << 2;
pub const COVERAGE_ARRIVAL_ORDER_UNAVAILABLE: u32 = 1 << 3;
pub const COVERAGE_UNKNOWN_SIDE_VALUE: u32 = 1 << 4;
pub const COVERAGE_EXPECTED_COUNT_UNAVAILABLE: u32 = 1 << 5;
pub const COVERAGE_INVALID_DROP_COUNTS_UNAVAILABLE: u32 = 1 << 6;

#[derive(Debug, Clone)]
pub struct Tick {
    pub time_us: i64,
    pub source: String,
    pub symbol: String,
    pub id: String,
    pub price: u64,
    pub qty: u64,
    pub is_buyer_maker: u8,
}

impl Tick {
    pub fn time_ms(&self) -> Result<u64> {
        let time_us = u64::try_from(self.time_us).context("tick timestamp is negative")?;
        Ok(time_us / 1_000)
    }

    fn validate(&self) -> Result<()> {
        ensure!(!self.source.is_empty(), "tick source cannot be empty");
        ensure!(!self.symbol.is_empty(), "tick symbol cannot be empty");
        ensure!(!self.id.is_empty(), "tick id cannot be empty");
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Bar1m {
    pub schema_version: u16,
    #[serde(serialize_with = "decimal_string::serialize")]
    pub timestamp: u64,
    pub source: String,
    pub symbol: String,
    #[serde(serialize_with = "decimal_string::serialize")]
    pub revision: u64,
    pub bar_key: String,
    pub header: Header,
    pub identity: Identity,
    pub price: Price,
    pub volume_flow: VolumeFlow,
    pub footprint: Footprint,
    pub price_changes: PriceChanges,
    pub trade_size_statistics: TradeSizeStatistics,
    pub timing: Timing,
    pub distributions: Distributions,
    pub microstructure: Microstructure,
    pub integrity: Integrity,
    /// The canonical input trades make this bar independently auditable and
    /// preserve every field available in the source `ticks` table.
    pub raw_ticks: Vec<RawTick>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Header {
    pub schema_version: u16,
    #[serde(serialize_with = "decimal_string::serialize")]
    pub timestamp: u64,
    pub symbol_id: u16,
    pub source_id: u8,
    pub flags: u32,
    pub aggregation_level: u8,
    pub price_scale: u8,
    pub quantity_scale: u8,
    pub time_unit: u8,
}

#[derive(Debug, Clone, Serialize)]
pub struct Identity {
    #[serde(serialize_with = "decimal_string::serialize")]
    pub first_trade_id: u64,
    #[serde(serialize_with = "decimal_string::serialize")]
    pub last_trade_id: u64,
    pub first_trade_id_raw: String,
    pub last_trade_id_raw: String,
    pub trade_ids_numeric: u8,
    #[serde(serialize_with = "decimal_string::serialize")]
    pub n_trades: u64,
    #[serde(serialize_with = "decimal_string::serialize")]
    pub first_event_ts: u64,
    #[serde(serialize_with = "decimal_string::serialize")]
    pub last_event_ts: u64,
    #[serde(serialize_with = "decimal_string::serialize")]
    pub first_receive_ts: u64,
    #[serde(serialize_with = "decimal_string::serialize")]
    pub last_receive_ts: u64,
    pub coverage_flags: u32,
    pub sequence_contiguous: u8,
}

#[derive(Debug, Clone, Serialize)]
pub struct Price {
    #[serde(serialize_with = "decimal_string::serialize")]
    pub open: i64,
    #[serde(serialize_with = "decimal_string::serialize")]
    pub high: i64,
    #[serde(serialize_with = "decimal_string::serialize")]
    pub low: i64,
    #[serde(serialize_with = "decimal_string::serialize")]
    pub close: i64,
    #[serde(serialize_with = "decimal_string::serialize")]
    pub upticks: u64,
    #[serde(serialize_with = "decimal_string::serialize")]
    pub downticks: u64,
    #[serde(serialize_with = "decimal_string::serialize")]
    pub zero_ticks: u64,
    #[serde(serialize_with = "decimal_string::serialize")]
    pub direction_changes: u64,
    #[serde(serialize_with = "decimal_string::serialize")]
    pub first_price_change: i64,
    #[serde(serialize_with = "decimal_string::serialize")]
    pub last_price_change: i64,
    #[serde(serialize_with = "decimal_string::serialize")]
    pub max_price_change: i64,
    #[serde(serialize_with = "decimal_string::serialize")]
    pub min_price_change: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct VolumeFlow {
    #[serde(serialize_with = "decimal_string::serialize")]
    pub total_qty: u128,
    #[serde(serialize_with = "decimal_string::serialize")]
    pub total_notional: u128,
    #[serde(serialize_with = "decimal_string::serialize")]
    pub buy_qty: u128,
    #[serde(serialize_with = "decimal_string::serialize")]
    pub sell_qty: u128,
    #[serde(serialize_with = "decimal_string::serialize")]
    pub buy_notional: u128,
    #[serde(serialize_with = "decimal_string::serialize")]
    pub sell_notional: u128,
    #[serde(serialize_with = "decimal_string::serialize")]
    pub signed_qty: i128,
    #[serde(serialize_with = "decimal_string::serialize")]
    pub signed_notional: i128,
    #[serde(serialize_with = "decimal_string::serialize")]
    pub buy_count: u64,
    #[serde(serialize_with = "decimal_string::serialize")]
    pub sell_count: u64,
    #[serde(serialize_with = "decimal_string::serialize")]
    pub max_trade_qty: u128,
    #[serde(serialize_with = "decimal_string::serialize")]
    pub max_trade_notional: u128,
    #[serde(serialize_with = "decimal_string::serialize")]
    pub vwap: u128,
}

#[derive(Debug, Clone, Serialize)]
pub struct Footprint {
    pub level_count: u32,
    #[serde(serialize_with = "decimal_string::serialize")]
    pub poc_price: i64,
    #[serde(serialize_with = "decimal_string::serialize")]
    pub poc_volume: u128,
    #[serde(serialize_with = "decimal_string::serialize")]
    pub poc_delta: i128,
    #[serde(serialize_with = "decimal_string::serialize")]
    pub value_area_high: i64,
    #[serde(serialize_with = "decimal_string::serialize")]
    pub value_area_low: i64,
    #[serde(serialize_with = "decimal_string::serialize")]
    pub max_positive_delta: i128,
    #[serde(serialize_with = "decimal_string::serialize")]
    pub max_negative_delta: i128,
    pub imbalance_count: u32,
    pub stacked_buy_imbalance: u32,
    pub stacked_sell_imbalance: u32,
    pub unfinished_auction_high: u8,
    pub unfinished_auction_low: u8,
    pub levels: Vec<FootprintLevel>,
}

#[derive(Debug, Clone, Serialize)]
pub struct FootprintLevel {
    #[serde(serialize_with = "decimal_string::serialize")]
    pub price: i64,
    #[serde(serialize_with = "decimal_string::serialize")]
    pub buy_qty: u128,
    #[serde(serialize_with = "decimal_string::serialize")]
    pub sell_qty: u128,
    #[serde(serialize_with = "decimal_string::serialize")]
    pub buy_notional: u128,
    #[serde(serialize_with = "decimal_string::serialize")]
    pub sell_notional: u128,
    pub buy_count: u32,
    pub sell_count: u32,
    #[serde(serialize_with = "decimal_string::serialize")]
    pub delta: i128,
    #[serde(serialize_with = "decimal_string::serialize")]
    pub total_qty: u128,
    pub total_count: u32,
    pub flags: u16,
}

#[derive(Debug, Clone, Serialize)]
pub struct PriceChanges {
    #[serde(serialize_with = "decimal_string::serialize")]
    pub change_count: u64,
    #[serde(serialize_with = "decimal_string::serialize")]
    pub first_change: i64,
    #[serde(serialize_with = "decimal_string::serialize")]
    pub last_change: i64,
    #[serde(serialize_with = "decimal_string::serialize")]
    pub change_sum: i128,
    pub change_sum_sq: String,
    pub change_sum_cu: String,
    pub change_sum_qu: String,
    pub realized_variance: String,
    pub bipower_variation: String,
    #[serde(serialize_with = "decimal_string::serialize")]
    pub positive_change_magnitude: u128,
    #[serde(serialize_with = "decimal_string::serialize")]
    pub negative_change_magnitude: u128,
    #[serde(serialize_with = "decimal_string::serialize")]
    pub max_change: i64,
    #[serde(serialize_with = "decimal_string::serialize")]
    pub min_change: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct TradeSizeStatistics {
    #[serde(serialize_with = "decimal_string::serialize")]
    pub size_sum: u128,
    pub size_sum_sq: String,
    pub size_sum_cu: String,
    pub size_sum_qu: String,
    #[serde(serialize_with = "decimal_string::serialize")]
    pub max_size: u128,
    #[serde(serialize_with = "decimal_string::serialize")]
    pub min_size: u128,
    #[serde(serialize_with = "decimal_string::serialize")]
    pub mean_size: u128,
}

#[derive(Debug, Clone, Serialize)]
pub struct Timing {
    #[serde(serialize_with = "decimal_string::serialize")]
    pub first_interarrival: u64,
    #[serde(serialize_with = "decimal_string::serialize")]
    pub last_interarrival: u64,
    #[serde(serialize_with = "decimal_string::serialize")]
    pub min_interarrival: u64,
    #[serde(serialize_with = "decimal_string::serialize")]
    pub max_interarrival: u64,
    #[serde(serialize_with = "decimal_string::serialize")]
    pub sum_interarrival: u128,
    #[serde(serialize_with = "decimal_string::serialize")]
    pub sum_interarrival_sq: u128,
    #[serde(serialize_with = "decimal_string::serialize")]
    pub interarrival_count: u64,
    pub active_seconds: u8,
    pub activity_profile: Vec<u32>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Distributions {
    #[serde(serialize_with = "decimal_string::serialize_vec")]
    pub trade_size_hist: Vec<u64>,
    #[serde(serialize_with = "decimal_string::serialize")]
    pub trade_size_hist_min: i128,
    #[serde(serialize_with = "decimal_string::serialize")]
    pub trade_size_hist_max: i128,
    #[serde(serialize_with = "decimal_string::serialize_vec")]
    pub price_change_hist: Vec<u64>,
    #[serde(serialize_with = "decimal_string::serialize")]
    pub price_change_hist_min: i128,
    #[serde(serialize_with = "decimal_string::serialize")]
    pub price_change_hist_max: i128,
    #[serde(serialize_with = "decimal_string::serialize_vec")]
    pub interarrival_hist: Vec<u64>,
    #[serde(serialize_with = "decimal_string::serialize")]
    pub interarrival_hist_min: i128,
    #[serde(serialize_with = "decimal_string::serialize")]
    pub interarrival_hist_max: i128,
    #[serde(serialize_with = "decimal_string::serialize_vec")]
    pub price_volume_hist: Vec<u64>,
    #[serde(serialize_with = "decimal_string::serialize")]
    pub price_volume_hist_min: i128,
    #[serde(serialize_with = "decimal_string::serialize")]
    pub price_volume_hist_max: i128,
}

#[derive(Debug, Clone, Serialize)]
pub struct Microstructure {
    #[serde(serialize_with = "decimal_string::serialize")]
    pub trade_imbalance: i128,
    #[serde(serialize_with = "decimal_string::serialize")]
    pub volume_imbalance: i128,
    #[serde(serialize_with = "decimal_string::serialize")]
    pub notional_imbalance: i128,
    #[serde(serialize_with = "decimal_string::serialize")]
    pub buy_trade_ratio: u64,
    #[serde(serialize_with = "decimal_string::serialize")]
    pub sell_trade_ratio: u64,
    #[serde(serialize_with = "decimal_string::serialize")]
    pub trade_intensity: u64,
    #[serde(serialize_with = "decimal_string::serialize")]
    pub average_trade_size: u128,
    #[serde(serialize_with = "decimal_string::serialize")]
    pub price_efficiency: i64,
    #[serde(serialize_with = "decimal_string::serialize")]
    pub absorption_buy: u128,
    #[serde(serialize_with = "decimal_string::serialize")]
    pub absorption_sell: u128,
    #[serde(serialize_with = "decimal_string::serialize")]
    pub exhaustion_buy: u128,
    #[serde(serialize_with = "decimal_string::serialize")]
    pub exhaustion_sell: u128,
}

#[derive(Debug, Clone, Serialize)]
pub struct Integrity {
    #[serde(serialize_with = "decimal_string::serialize")]
    pub sequence_gaps: u64,
    #[serde(serialize_with = "decimal_string::serialize")]
    pub duplicate_count: u64,
    #[serde(serialize_with = "decimal_string::serialize")]
    pub out_of_order_count: u64,
    #[serde(serialize_with = "decimal_string::serialize")]
    pub invalid_trade_count: u64,
    #[serde(serialize_with = "decimal_string::serialize")]
    pub dropped_trade_count: u64,
    #[serde(serialize_with = "decimal_string::serialize")]
    pub expected_trade_count: u64,
    #[serde(serialize_with = "decimal_string::serialize")]
    pub received_trade_count: u64,
    pub quality_flags: u32,
    #[serde(serialize_with = "decimal_string::serialize")]
    pub checksum: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct RawTick {
    #[serde(serialize_with = "decimal_string::serialize")]
    pub time: u64,
    #[serde(serialize_with = "decimal_string::serialize")]
    pub time_us: i64,
    pub source: String,
    pub symbol: String,
    pub id: String,
    #[serde(serialize_with = "decimal_string::serialize")]
    pub price: u64,
    #[serde(serialize_with = "decimal_string::serialize")]
    pub qty: u64,
    pub is_buyer_maker: u8,
}

#[derive(Debug, Default)]
struct LevelAccumulator {
    buy_qty: u128,
    sell_qty: u128,
    buy_notional: u128,
    sell_notional: u128,
    buy_count: u32,
    sell_count: u32,
}

pub fn build_bar(
    ticks: &[Tick],
    bucket_start_ms: u64,
    source_id: u8,
    symbol_id: u16,
) -> Result<Bar1m> {
    ensure!(!ticks.is_empty(), "cannot build a bar without ticks");
    let bucket_end_ms = bucket_start_ms
        .checked_add(60_000)
        .context("bar timestamp overflows")?;

    let mut ordered = ticks.to_vec();
    ordered.sort_by(|a, b| {
        a.time_us
            .cmp(&b.time_us)
            .then_with(|| compare_trade_ids(&a.id, &b.id))
    });

    let expected_source = ticks[0].source.as_str();
    let expected_symbol = ticks[0].symbol.as_str();
    for tick in &ordered {
        tick.validate()?;
        ensure!(
            tick.source == expected_source,
            "all ticks in a bar must have the same source"
        );
        ensure!(
            tick.symbol == expected_symbol,
            "all ticks in a bar must have the same symbol"
        );
        let event_ms = tick.time_ms()?;
        ensure!(
            event_ms >= bucket_start_ms && event_ms < bucket_end_ms,
            "tick falls outside its minute bucket"
        );
    }

    let n_trades = u64::try_from(ordered.len()).context("bar trade count exceeds u64")?;
    let first = ordered.first().expect("non-empty ticks checked above");
    let last = ordered.last().expect("non-empty ticks checked above");
    let first_event_ts = first.time_ms()?;
    let last_event_ts = last.time_ms()?;

    let all_trade_ids_numeric = ordered.iter().all(|tick| tick.id.parse::<u64>().is_ok());
    let mut coverage_flags = COVERAGE_RECEIVE_TIME_UNAVAILABLE
        | COVERAGE_DUPLICATES_UNAVAILABLE
        | COVERAGE_ARRIVAL_ORDER_UNAVAILABLE
        | COVERAGE_EXPECTED_COUNT_UNAVAILABLE
        | COVERAGE_INVALID_DROP_COUNTS_UNAVAILABLE;
    if !all_trade_ids_numeric {
        coverage_flags |= COVERAGE_NON_NUMERIC_TRADE_ID;
    }
    if ordered.iter().any(|tick| tick.is_buyer_maker > 1) {
        coverage_flags |= COVERAGE_UNKNOWN_SIDE_VALUE;
    }

    let mut total_qty = 0_u128;
    let mut total_notional = 0_u128;
    let mut buy_qty = 0_u128;
    let mut sell_qty = 0_u128;
    let mut buy_notional = 0_u128;
    let mut sell_notional = 0_u128;
    let mut buy_count = 0_u64;
    let mut sell_count = 0_u64;
    let mut max_trade_qty = 0_u128;
    let mut max_trade_notional = 0_u128;
    let mut levels = BTreeMap::<i64, LevelAccumulator>::new();
    let mut activity_profile = vec![0_u32; 60];
    let mut prices = Vec::with_capacity(ordered.len());
    let mut sizes = Vec::with_capacity(ordered.len());
    let mut event_times_us = Vec::with_capacity(ordered.len());
    let mut raw_ticks = Vec::with_capacity(ordered.len());
    let mut sequence_gaps = 0_u64;
    let mut duplicate_count = 0_u64;
    let mut out_of_order_count = 0_u64;
    let mut seen_trade_ids = HashSet::new();
    let mut sequence_contiguous = all_trade_ids_numeric;
    let mut previous_id: Option<u64> = None;
    let mut checksum = 0xcbf29ce484222325_u64;

    for tick in &ordered {
        if !seen_trade_ids.insert(tick.id.as_str()) {
            duplicate_count = duplicate_count
                .checked_add(1)
                .context("duplicate trade count overflows u64")?;
            sequence_contiguous = false;
        }

        let price = i64::try_from(tick.price).context("tick price exceeds i64")?;
        let qty = u128::from(tick.qty);
        let notional = u128::from(tick.price)
            .checked_mul(qty)
            .context("trade notional overflows u128")?;
        total_qty = total_qty
            .checked_add(qty)
            .context("total quantity overflows u128")?;
        total_notional = total_notional
            .checked_add(notional)
            .context("total notional overflows u128")?;
        max_trade_qty = max_trade_qty.max(qty);
        max_trade_notional = max_trade_notional.max(notional);

        let level = levels.entry(price).or_default();
        match tick.is_buyer_maker {
            0 => {
                buy_qty = buy_qty
                    .checked_add(qty)
                    .context("buy quantity overflows u128")?;
                buy_notional = buy_notional
                    .checked_add(notional)
                    .context("buy notional overflows u128")?;
                buy_count = buy_count
                    .checked_add(1)
                    .context("buy count overflows u64")?;
                level.buy_qty = level
                    .buy_qty
                    .checked_add(qty)
                    .context("footprint buy quantity overflows u128")?;
                level.buy_notional = level
                    .buy_notional
                    .checked_add(notional)
                    .context("footprint buy notional overflows u128")?;
                level.buy_count = level
                    .buy_count
                    .checked_add(1)
                    .context("footprint buy count overflows u32")?;
            }
            1 => {
                sell_qty = sell_qty
                    .checked_add(qty)
                    .context("sell quantity overflows u128")?;
                sell_notional = sell_notional
                    .checked_add(notional)
                    .context("sell notional overflows u128")?;
                sell_count = sell_count
                    .checked_add(1)
                    .context("sell count overflows u64")?;
                level.sell_qty = level
                    .sell_qty
                    .checked_add(qty)
                    .context("footprint sell quantity overflows u128")?;
                level.sell_notional = level
                    .sell_notional
                    .checked_add(notional)
                    .context("footprint sell notional overflows u128")?;
                level.sell_count = level
                    .sell_count
                    .checked_add(1)
                    .context("footprint sell count overflows u32")?;
            }
            _ => {}
        }

        let seconds_into_bar = (tick.time_ms()? - bucket_start_ms) / 1_000;
        let second = usize::try_from(seconds_into_bar.min(59))?;
        activity_profile[second] = activity_profile[second]
            .checked_add(1)
            .context("activity profile counter overflows u32")?;

        prices.push(price);
        sizes.push(i128::from(tick.qty));
        event_times_us.push(tick.time_us);
        append_checksum(&mut checksum, tick);
        raw_ticks.push(RawTick {
            time: tick.time_ms()?,
            time_us: tick.time_us,
            source: tick.source.clone(),
            symbol: tick.symbol.clone(),
            id: tick.id.clone(),
            price: tick.price,
            qty: tick.qty,
            is_buyer_maker: tick.is_buyer_maker,
        });

        match tick.id.parse::<u64>() {
            Ok(id) => {
                if let Some(previous) = previous_id {
                    if id == previous {
                        sequence_contiguous = false;
                    } else if id > previous {
                        let missing = id - previous - 1;
                        sequence_gaps = sequence_gaps
                            .checked_add(missing)
                            .context("sequence gap count overflows u64")?;
                        if missing != 0 {
                            sequence_contiguous = false;
                        }
                    } else {
                        out_of_order_count = out_of_order_count
                            .checked_add(1)
                            .context("out-of-order trade count overflows u64")?;
                        sequence_contiguous = false;
                    }
                }
                previous_id = Some(id);
            }
            Err(_) => sequence_contiguous = false,
        }
        if tick.id.parse::<u64>().is_err() {
            previous_id = None;
        }
    }

    let open = *prices.first().expect("non-empty prices");
    let close = *prices.last().expect("non-empty prices");
    let high = *prices.iter().max().expect("non-empty prices");
    let low = *prices.iter().min().expect("non-empty prices");
    let mut price_changes = Vec::<i64>::with_capacity(prices.len().saturating_sub(1));
    let mut upticks = 0_u64;
    let mut downticks = 0_u64;
    let mut zero_ticks = 0_u64;
    let mut direction_changes = 0_u64;
    let mut previous_direction = 0_i8;
    for pair in prices.windows(2) {
        let change = pair[1]
            .checked_sub(pair[0])
            .context("price change overflows i64")?;
        price_changes.push(change);
        let direction = change.signum() as i8;
        match direction {
            1 => {
                upticks = upticks
                    .checked_add(1)
                    .context("uptick counter overflows u64")?
            }
            -1 => {
                downticks = downticks
                    .checked_add(1)
                    .context("downtick counter overflows u64")?
            }
            _ => {
                zero_ticks = zero_ticks
                    .checked_add(1)
                    .context("zero-tick counter overflows u64")?
            }
        }
        if direction != 0 && previous_direction != 0 && direction != previous_direction {
            direction_changes = direction_changes
                .checked_add(1)
                .context("direction-change counter overflows u64")?;
        }
        if direction != 0 {
            previous_direction = direction;
        }
    }

    let mut change_sum = 0_i128;
    let mut change_sum_sq = BigUint::default();
    let mut change_sum_cu = BigInt::default();
    let mut change_sum_qu = BigUint::default();
    let mut positive_change_magnitude = 0_u128;
    let mut negative_change_magnitude = 0_u128;
    let mut bipower_variation = BigUint::default();
    for (index, change) in price_changes.iter().enumerate() {
        let delta = i128::from(*change);
        let abs_delta = delta.unsigned_abs();
        let abs_delta_big = BigUint::from(abs_delta);
        let square = &abs_delta_big * &abs_delta_big;
        let cube_value = BigInt::from(delta);
        let cube = &cube_value * &cube_value * &cube_value;
        let fourth = &square * &square;
        change_sum = change_sum
            .checked_add(delta)
            .context("sum of price changes overflows i128")?;
        change_sum_sq += &square;
        change_sum_cu += cube;
        change_sum_qu += fourth;
        if delta > 0 {
            positive_change_magnitude = positive_change_magnitude
                .checked_add(abs_delta)
                .context("positive change magnitude overflows u128")?;
        } else if delta < 0 {
            negative_change_magnitude = negative_change_magnitude
                .checked_add(abs_delta)
                .context("negative change magnitude overflows u128")?;
        }
        if index > 0 {
            let previous_abs = i128::from(price_changes[index - 1]).unsigned_abs();
            bipower_variation += BigUint::from(previous_abs) * &abs_delta_big;
        }
    }

    let mut size_sum = 0_u128;
    let mut size_sum_sq = BigUint::default();
    let mut size_sum_cu = BigUint::default();
    let mut size_sum_qu = BigUint::default();
    for size in &sizes {
        let value = u128::try_from(*size)?;
        let value_big = BigUint::from(value);
        let square = &value_big * &value_big;
        size_sum = size_sum
            .checked_add(value)
            .context("size sum overflows u128")?;
        size_sum_sq += &square;
        size_sum_cu += &square * &value_big;
        size_sum_qu += &square * &square;
    }

    let mut interarrivals = Vec::<u64>::with_capacity(event_times_us.len().saturating_sub(1));
    for pair in event_times_us.windows(2) {
        let delta = pair[1]
            .checked_sub(pair[0])
            .context("event timestamps are not monotonic")?;
        interarrivals.push(u64::try_from(delta).context("interarrival exceeds u64")?);
    }
    let mut interarrival_sum = 0_u128;
    let mut interarrival_sum_sq = 0_u128;
    for value in &interarrivals {
        let value = u128::from(*value);
        interarrival_sum = interarrival_sum
            .checked_add(value)
            .context("interarrival sum overflows u128")?;
        interarrival_sum_sq = interarrival_sum_sq
            .checked_add(
                value
                    .checked_mul(value)
                    .context("squared interarrival overflows u128")?,
            )
            .context("squared interarrival sum overflows u128")?;
    }

    let footprint_levels: Vec<FootprintLevel> = levels
        .iter()
        .map(|(price, level)| -> Result<FootprintLevel> {
            let total_qty = level
                .buy_qty
                .checked_add(level.sell_qty)
                .context("footprint level quantity overflows u128")?;
            let delta = i128::try_from(level.buy_qty)
                .context("footprint buy quantity exceeds i128")?
                - i128::try_from(level.sell_qty).context("footprint sell quantity exceeds i128")?;
            let total_count = level
                .buy_count
                .checked_add(level.sell_count)
                .context("footprint level count overflows u32")?;
            let buy_imbalance =
                level.buy_qty > 0 && (level.sell_qty == 0 || level.buy_qty / level.sell_qty >= 3);
            let sell_imbalance =
                level.sell_qty > 0 && (level.buy_qty == 0 || level.sell_qty / level.buy_qty >= 3);
            Ok(FootprintLevel {
                price: *price,
                buy_qty: level.buy_qty,
                sell_qty: level.sell_qty,
                buy_notional: level.buy_notional,
                sell_notional: level.sell_notional,
                buy_count: level.buy_count,
                sell_count: level.sell_count,
                delta,
                total_qty,
                total_count,
                flags: u16::from(buy_imbalance) | (u16::from(sell_imbalance) << 1),
            })
        })
        .collect::<Result<_>>()?;
    let known_side_qty = buy_qty
        .checked_add(sell_qty)
        .context("known-side quantity overflows u128")?;
    let footprint = build_footprint(&footprint_levels, known_side_qty)?;

    let active_seconds = u8::try_from(activity_profile.iter().filter(|n| **n > 0).count())?;
    let change_values: Vec<i128> = price_changes.iter().map(|v| i128::from(*v)).collect();
    let interarrival_values: Vec<i128> = interarrivals.iter().map(|v| i128::from(*v)).collect();
    let price_values: Vec<i128> = prices.iter().map(|v| i128::from(*v)).collect();
    let (trade_size_hist, trade_size_hist_min, trade_size_hist_max) =
        histogram(&sizes, HISTOGRAM_BUCKETS)?;
    let (price_change_hist, price_change_hist_min, price_change_hist_max) =
        histogram(&change_values, HISTOGRAM_BUCKETS)?;
    let (interarrival_hist, interarrival_hist_min, interarrival_hist_max) =
        histogram(&interarrival_values, HISTOGRAM_BUCKETS)?;
    let (price_volume_hist, price_volume_hist_min, price_volume_hist_max) =
        weighted_histogram(&price_values, &ordered, HISTOGRAM_BUCKETS)?;

    let known_side_count = buy_count
        .checked_add(sell_count)
        .context("known side count overflows u64")?;
    let buy_trade_ratio = ratio_fixed(buy_count, known_side_count)?;
    let sell_trade_ratio = ratio_fixed(sell_count, known_side_count)?;
    let trade_imbalance = i128::from(buy_count) - i128::from(sell_count);
    let volume_imbalance = i128::try_from(buy_qty).context("buy quantity exceeds i128")?
        - i128::try_from(sell_qty).context("sell quantity exceeds i128")?;
    let notional_imbalance = i128::try_from(buy_notional).context("buy notional exceeds i128")?
        - i128::try_from(sell_notional).context("sell notional exceeds i128")?;
    let active_seconds_u64 = u64::from(active_seconds);
    let trade_intensity = n_trades
        .checked_mul(100_000_000)
        .context("trade intensity overflows u64")?
        .checked_div(active_seconds_u64)
        .unwrap_or(0);
    let net_change = i128::from(close) - i128::from(open);
    let total_abs_changes = price_changes.iter().try_fold(0_u128, |sum, change| {
        sum.checked_add(i128::from(*change).unsigned_abs())
            .context("absolute price changes overflow u128")
    })?;
    let price_efficiency = if total_abs_changes == 0 {
        0
    } else {
        i64::try_from(
            net_change
                .checked_mul(100_000_000)
                .context("price efficiency numerator overflows i128")?
                / i128::try_from(total_abs_changes)?,
        )
        .context("price efficiency exceeds i64")?
    };
    let (absorption_buy, absorption_sell, exhaustion_buy, exhaustion_sell) =
        calculate_absorption_exhaustion(open, close, &footprint_levels)?;

    let first_change = price_changes.first().copied().unwrap_or(0);
    let last_change = price_changes.last().copied().unwrap_or(0);
    let min_change = price_changes.iter().copied().min().unwrap_or(0);
    let max_change = price_changes.iter().copied().max().unwrap_or(0);
    let first_interarrival = interarrivals.first().copied().unwrap_or(0);
    let last_interarrival = interarrivals.last().copied().unwrap_or(0);
    let min_interarrival = interarrivals.iter().copied().min().unwrap_or(0);
    let max_interarrival = interarrivals.iter().copied().max().unwrap_or(0);

    let bar_key = format!("{}/{}/{}", first.source, first.symbol, bucket_start_ms);
    let header_flags = u32::from(coverage_flags != 0) | (1 << 1);
    Ok(Bar1m {
        schema_version: SCHEMA_VERSION,
        timestamp: bucket_start_ms,
        source: first.source.clone(),
        symbol: first.symbol.clone(),
        revision: 1,
        bar_key,
        header: Header {
            schema_version: SCHEMA_VERSION,
            timestamp: bucket_start_ms,
            symbol_id,
            source_id,
            flags: header_flags,
            aggregation_level: 1,
            price_scale: PRICE_SCALE,
            quantity_scale: QUANTITY_SCALE,
            time_unit: TIME_UNIT_MILLISECONDS,
        },
        identity: Identity {
            first_trade_id: id_as_u64(&first.id),
            last_trade_id: id_as_u64(&last.id),
            first_trade_id_raw: first.id.clone(),
            last_trade_id_raw: last.id.clone(),
            trade_ids_numeric: u8::from(all_trade_ids_numeric),
            n_trades,
            first_event_ts,
            last_event_ts,
            first_receive_ts: 0,
            last_receive_ts: 0,
            coverage_flags,
            sequence_contiguous: u8::from(sequence_contiguous),
        },
        price: Price {
            open,
            high,
            low,
            close,
            upticks,
            downticks,
            zero_ticks,
            direction_changes,
            first_price_change: first_change,
            last_price_change: last_change,
            max_price_change: max_change,
            min_price_change: min_change,
        },
        volume_flow: VolumeFlow {
            total_qty,
            total_notional,
            buy_qty,
            sell_qty,
            buy_notional,
            sell_notional,
            signed_qty: volume_imbalance,
            signed_notional: notional_imbalance,
            buy_count,
            sell_count,
            max_trade_qty,
            max_trade_notional,
            vwap: total_notional.checked_div(total_qty).unwrap_or(0),
        },
        footprint,
        price_changes: PriceChanges {
            change_count: u64::try_from(price_changes.len())?,
            first_change,
            last_change,
            change_sum,
            change_sum_sq: change_sum_sq.to_str_radix(10),
            change_sum_cu: change_sum_cu.to_str_radix(10),
            change_sum_qu: change_sum_qu.to_str_radix(10),
            realized_variance: change_sum_sq.to_str_radix(10),
            bipower_variation: bipower_variation.to_str_radix(10),
            positive_change_magnitude,
            negative_change_magnitude,
            max_change,
            min_change,
        },
        trade_size_statistics: TradeSizeStatistics {
            size_sum,
            size_sum_sq: size_sum_sq.to_str_radix(10),
            size_sum_cu: size_sum_cu.to_str_radix(10),
            size_sum_qu: size_sum_qu.to_str_radix(10),
            max_size: max_trade_qty,
            min_size: u128::from(ordered.iter().map(|tick| tick.qty).min().unwrap_or(0)),
            mean_size: size_sum / u128::from(n_trades),
        },
        timing: Timing {
            first_interarrival,
            last_interarrival,
            min_interarrival,
            max_interarrival,
            sum_interarrival: interarrival_sum,
            sum_interarrival_sq: interarrival_sum_sq,
            interarrival_count: u64::try_from(interarrivals.len())?,
            active_seconds,
            activity_profile,
        },
        distributions: Distributions {
            trade_size_hist,
            trade_size_hist_min,
            trade_size_hist_max,
            price_change_hist,
            price_change_hist_min,
            price_change_hist_max,
            interarrival_hist,
            interarrival_hist_min,
            interarrival_hist_max,
            price_volume_hist,
            price_volume_hist_min,
            price_volume_hist_max,
        },
        microstructure: Microstructure {
            trade_imbalance,
            volume_imbalance,
            notional_imbalance,
            buy_trade_ratio,
            sell_trade_ratio,
            trade_intensity,
            average_trade_size: size_sum / u128::from(n_trades),
            price_efficiency,
            absorption_buy,
            absorption_sell,
            exhaustion_buy,
            exhaustion_sell,
        },
        integrity: Integrity {
            sequence_gaps,
            duplicate_count,
            out_of_order_count,
            invalid_trade_count: 0,
            dropped_trade_count: 0,
            expected_trade_count: 0,
            received_trade_count: n_trades,
            quality_flags: coverage_flags,
            checksum,
        },
        raw_ticks,
    })
}

fn build_footprint(levels: &[FootprintLevel], total_qty: u128) -> Result<Footprint> {
    ensure!(
        !levels.is_empty(),
        "cannot build a footprint without price levels"
    );
    let poc_index = levels
        .iter()
        .enumerate()
        .max_by(|(_, a), (_, b)| {
            a.total_qty
                .cmp(&b.total_qty)
                .then_with(|| b.price.cmp(&a.price))
        })
        .map(|(index, _)| index)
        .expect("non-empty levels checked above");
    let poc = &levels[poc_index];

    let target_volume = total_qty
        .checked_mul(70)
        .context("value-area target overflows u128")?
        .div_ceil(100);
    let mut accumulated = poc.total_qty;
    let mut low_index = poc_index;
    let mut high_index = poc_index;
    while accumulated < target_volume && (low_index > 0 || high_index + 1 < levels.len()) {
        let lower_volume = low_index
            .checked_sub(1)
            .map(|index| levels[index].total_qty);
        let higher_volume =
            (high_index + 1 < levels.len()).then(|| levels[high_index + 1].total_qty);
        let take_higher = match (lower_volume, higher_volume) {
            (Some(lower), Some(higher)) => higher >= lower,
            (None, Some(_)) => true,
            (Some(_), None) => false,
            (None, None) => break,
        };
        let index = if take_higher {
            high_index += 1;
            high_index
        } else {
            low_index -= 1;
            low_index
        };
        accumulated = accumulated
            .checked_add(levels[index].total_qty)
            .context("value-area volume overflows u128")?;
    }

    let mut imbalance_count = 0_u32;
    let mut stacked_buy_imbalance = 0_u32;
    let mut stacked_sell_imbalance = 0_u32;
    let mut current_buy_run = 0_u32;
    let mut current_sell_run = 0_u32;
    let mut max_positive_delta = 0_i128;
    let mut max_negative_delta = 0_i128;
    for level in levels {
        max_positive_delta = max_positive_delta.max(level.delta);
        max_negative_delta = max_negative_delta.min(level.delta);
        let buy_imbalance = level.flags & 1 != 0;
        let sell_imbalance = level.flags & 2 != 0;
        if buy_imbalance || sell_imbalance {
            imbalance_count = imbalance_count
                .checked_add(1)
                .context("footprint imbalance count overflows u32")?;
        }
        current_buy_run = if buy_imbalance {
            current_buy_run
                .checked_add(1)
                .context("stacked buy imbalance count overflows u32")?
        } else {
            0
        };
        current_sell_run = if sell_imbalance {
            current_sell_run
                .checked_add(1)
                .context("stacked sell imbalance count overflows u32")?
        } else {
            0
        };
        stacked_buy_imbalance = stacked_buy_imbalance.max(current_buy_run);
        stacked_sell_imbalance = stacked_sell_imbalance.max(current_sell_run);
    }

    Ok(Footprint {
        level_count: u32::try_from(levels.len()).context("too many footprint levels")?,
        poc_price: poc.price,
        poc_volume: poc.total_qty,
        poc_delta: poc.delta,
        value_area_high: levels[high_index].price,
        value_area_low: levels[low_index].price,
        max_positive_delta,
        max_negative_delta,
        imbalance_count,
        stacked_buy_imbalance,
        stacked_sell_imbalance,
        unfinished_auction_high: u8::from(levels.last().is_some_and(|level| level.sell_qty == 0)),
        unfinished_auction_low: u8::from(levels.first().is_some_and(|level| level.buy_qty == 0)),
        levels: levels.to_vec(),
    })
}

fn calculate_absorption_exhaustion(
    open: i64,
    close: i64,
    levels: &[FootprintLevel],
) -> Result<(u128, u128, u128, u128)> {
    let buy_qty = levels.iter().try_fold(0_u128, |sum, level| {
        sum.checked_add(level.buy_qty)
            .context("absorption buy quantity overflows u128")
    })?;
    let sell_qty = levels.iter().try_fold(0_u128, |sum, level| {
        sum.checked_add(level.sell_qty)
            .context("absorption sell quantity overflows u128")
    })?;
    let absorption_buy = if close <= open { buy_qty } else { 0 };
    let absorption_sell = if close >= open { sell_qty } else { 0 };
    let exhaustion_buy = if close <= open {
        levels.last().map(|level| level.buy_qty).unwrap_or(0)
    } else {
        0
    };
    let exhaustion_sell = if close >= open {
        levels.first().map(|level| level.sell_qty).unwrap_or(0)
    } else {
        0
    };
    Ok((
        absorption_buy,
        absorption_sell,
        exhaustion_buy,
        exhaustion_sell,
    ))
}

fn histogram(values: &[i128], buckets: usize) -> Result<(Vec<u64>, i128, i128)> {
    let min = values.iter().copied().min().unwrap_or(0);
    let max = values.iter().copied().max().unwrap_or(0);
    let mut counts = vec![0_u64; buckets];
    if values.is_empty() {
        return Ok((counts, min, max));
    }
    for value in values {
        let index = bucket_index(*value, min, max, buckets)?;
        counts[index] = counts[index]
            .checked_add(1)
            .context("histogram counter overflows u64")?;
    }
    Ok((counts, min, max))
}

fn weighted_histogram(
    values: &[i128],
    ticks: &[Tick],
    buckets: usize,
) -> Result<(Vec<u64>, i128, i128)> {
    let min = values.iter().copied().min().unwrap_or(0);
    let max = values.iter().copied().max().unwrap_or(0);
    let mut weights = vec![0_u64; buckets];
    for (value, tick) in values.iter().zip(ticks) {
        let index = bucket_index(*value, min, max, buckets)?;
        let weight = tick.qty;
        weights[index] = weights[index]
            .checked_add(weight)
            .context("price-volume histogram weight overflows u64")?;
    }
    Ok((weights, min, max))
}

fn bucket_index(value: i128, min: i128, max: i128, buckets: usize) -> Result<usize> {
    if max <= min {
        return Ok(0);
    }
    let offset = u128::try_from(value - min)?;
    let range = u128::try_from(max - min)?;
    let index = offset
        .checked_mul(u128::try_from(buckets)?)
        .context("histogram bucket calculation overflows u128")?
        / range;
    Ok(usize::try_from(index)?.min(buckets - 1))
}

fn ratio_fixed(numerator: u64, denominator: u64) -> Result<u64> {
    if denominator == 0 {
        return Ok(0);
    }
    numerator
        .checked_mul(100_000_000)
        .context("fixed-point ratio overflows u64")
        .map(|value| value / denominator)
}

fn compare_trade_ids(left: &str, right: &str) -> std::cmp::Ordering {
    match (left.parse::<u64>(), right.parse::<u64>()) {
        (Ok(left_value), Ok(right_value)) => {
            left_value.cmp(&right_value).then_with(|| left.cmp(right))
        }
        (Ok(_), Err(_)) => std::cmp::Ordering::Less,
        (Err(_), Ok(_)) => std::cmp::Ordering::Greater,
        (Err(_), Err(_)) => left.cmp(right),
    }
}

fn id_as_u64(value: &str) -> u64 {
    value
        .parse::<u64>()
        .unwrap_or_else(|_| fnv64(value.as_bytes()))
}

fn append_checksum(checksum: &mut u64, tick: &Tick) {
    fn append(checksum: &mut u64, bytes: &[u8]) {
        for byte in bytes {
            *checksum ^= u64::from(*byte);
            *checksum = checksum.wrapping_mul(0x100000001b3);
        }
    }
    for value in [&tick.source, &tick.symbol, &tick.id] {
        append(checksum, &(value.len() as u64).to_be_bytes());
        append(checksum, value.as_bytes());
    }
    append(checksum, &tick.time_us.to_be_bytes());
    append(checksum, &tick.price.to_be_bytes());
    append(checksum, &tick.qty.to_be_bytes());
    append(checksum, &[tick.is_buyer_maker]);
}

fn fnv64(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf29ce484222325_u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::{
        COVERAGE_UNKNOWN_SIDE_VALUE, FootprintLevel, Tick, build_bar,
        calculate_absorption_exhaustion, compare_trade_ids,
    };

    fn tick(time_ms: u64, id: &str, price: u64, qty: u64, is_buyer_maker: u8) -> Tick {
        Tick {
            time_us: i64::try_from(time_ms * 1_000).unwrap(),
            source: "binance".to_string(),
            symbol: "BTCUSDT".to_string(),
            id: id.to_string(),
            price,
            qty,
            is_buyer_maker,
        }
    }

    fn tick_us(time_us: i64, id: &str, price: u64, qty: u64, is_buyer_maker: u8) -> Tick {
        Tick {
            time_us,
            source: "binance".to_string(),
            symbol: "BTCUSDT".to_string(),
            id: id.to_string(),
            price,
            qty,
            is_buyer_maker,
        }
    }

    #[test]
    fn builds_complete_bar_and_keeps_raw_ticks() {
        let bar = build_bar(
            &[
                tick(30_104, "8322178460", 6_117_280_000_000, 8_100_000, 1),
                tick(30_161, "8322178461", 6_117_290_000_000, 100_000, 0),
                tick(30_161, "8322178462", 6_117_290_000_000, 5_200_000, 0),
            ],
            0,
            1,
            1,
        )
        .unwrap();

        assert_eq!(bar.price.open, 6_117_280_000_000);
        assert_eq!(bar.price.close, 6_117_290_000_000);
        assert_eq!(bar.price.high, bar.price.close);
        assert_eq!(bar.price.low, bar.price.open);
        assert_eq!(bar.identity.n_trades, 3);
        assert_eq!(bar.volume_flow.total_qty, 13_400_000);
        assert_eq!(bar.volume_flow.buy_qty, 5_300_000);
        assert_eq!(bar.volume_flow.sell_qty, 8_100_000);
        assert_eq!(bar.raw_ticks.len(), 3);
        assert_eq!(bar.raw_ticks[2].id, "8322178462");
        assert_eq!(bar.identity.sequence_contiguous, 1);
    }

    #[test]
    fn handles_non_numeric_ids_without_losing_the_original_id() {
        let bar = build_bar(&[tick(1_000, "trade:one", 100, 1, 0)], 0, 2, 3).unwrap();
        assert_eq!(bar.identity.trade_ids_numeric, 0);
        assert_eq!(bar.identity.first_trade_id_raw, "trade:one");
        assert_eq!(bar.raw_ticks[0].id, "trade:one");
        assert_ne!(bar.identity.coverage_flags, 0);
    }

    #[test]
    fn accepts_plus_prefixed_numeric_ids_and_preserves_the_raw_value() {
        let bar = build_bar(&[tick(1_000, "+5", 100, 1, 0)], 0, 1, 1).unwrap();

        assert_eq!(bar.identity.trade_ids_numeric, 1);
        assert_eq!(bar.identity.first_trade_id, 5);
        assert_eq!(bar.identity.first_trade_id_raw, "+5");
        assert_eq!(compare_trade_ids("+5", "5"), std::cmp::Ordering::Less);
    }

    #[test]
    fn sorts_equal_timestamp_numeric_ids_numerically() {
        let bar = build_bar(
            &[tick(1_000, "10", 110, 1, 0), tick(1_000, "2", 100, 1, 0)],
            0,
            1,
            1,
        )
        .unwrap();
        assert_eq!(bar.price.open, 100);
        assert_eq!(bar.price.close, 110);
    }

    #[test]
    fn uses_a_transitive_total_order_for_mixed_ids() {
        let bar = build_bar(
            &[
                tick(1_000, "1a", 12, 1, 0),
                tick(1_000, "10", 11, 1, 0),
                tick(1_000, "9", 10, 1, 0),
                tick(1_000, "02", 9, 1, 0),
                tick(1_000, "2", 8, 1, 0),
            ],
            0,
            1,
            1,
        )
        .unwrap();

        let ids: Vec<&str> = bar.raw_ticks.iter().map(|tick| tick.id.as_str()).collect();
        assert_eq!(ids, ["02", "2", "9", "10", "1a"]);
        assert_eq!(compare_trade_ids("9", "10"), std::cmp::Ordering::Less);
        assert_eq!(compare_trade_ids("10", "1a"), std::cmp::Ordering::Less);
        assert_eq!(compare_trade_ids("1a", "9"), std::cmp::Ordering::Greater);
    }

    #[test]
    fn preserves_fourth_moments_beyond_u128_range() {
        let bar = build_bar(
            &[
                tick(1_000, "1", 1, 5_000_000_000, 0),
                tick(2_000, "2", 5_000_000_001, 1, 1),
            ],
            0,
            1,
            1,
        )
        .unwrap();

        assert_eq!(bar.schema_version, 3);
        assert_eq!(bar.price_changes.change_sum_qu.len(), 39);
        assert!(bar.price_changes.change_sum_qu.starts_with("625"));
        assert_eq!(bar.trade_size_statistics.size_sum_qu.len(), 39);
        assert!(bar.trade_size_statistics.size_sum_qu.starts_with("625"));
    }

    #[test]
    fn rejects_ticks_before_the_unix_epoch_and_keeps_submillisecond_time() {
        assert_eq!(
            tick_us(60_000_999, "1", 100, 1, 0).time_ms().unwrap(),
            60_000
        );
        assert!(tick_us(-1, "1", 100, 1, 0).time_ms().is_err());
        assert!(build_bar(&[tick_us(-1, "1", 100, 1, 0)], 0, 1, 1).is_err());

        let bar = build_bar(&[tick_us(60_000_999, "1", 100, 1, 0)], 60_000, 1, 1).unwrap();
        assert_eq!(bar.raw_ticks[0].time, 60_000);
        assert_eq!(bar.raw_ticks[0].time_us, 60_000_999);
    }

    #[test]
    fn rejects_mixed_sources_or_symbols() {
        let mut different_source = tick(1_000, "2", 101, 1, 0);
        different_source.source = "kraken".to_string();
        assert!(
            build_bar(&[tick(1_000, "1", 100, 1, 0), different_source], 0, 1, 1)
                .unwrap_err()
                .to_string()
                .contains("same source")
        );

        let mut different_symbol = tick(1_000, "2", 101, 1, 0);
        different_symbol.symbol = "ETHUSDT".to_string();
        assert!(
            build_bar(&[tick(1_000, "1", 100, 1, 0), different_symbol], 0, 1, 1)
                .unwrap_err()
                .to_string()
                .contains("same symbol")
        );
    }

    #[test]
    fn unknown_side_volume_does_not_change_the_known_side_value_area_target() {
        let bar = build_bar(
            &[tick(1_000, "1", 100, 100, 0), tick(2_000, "2", 90, 900, 2)],
            0,
            1,
            1,
        )
        .unwrap();

        assert_eq!(bar.volume_flow.total_qty, 1_000);
        assert_eq!(bar.volume_flow.buy_qty, 100);
        assert_eq!(bar.footprint.level_count, 2);
        assert_eq!(bar.footprint.levels[0].price, 90);
        assert_eq!(bar.footprint.levels[0].total_qty, 0);
        assert_eq!(bar.footprint.value_area_low, 100);
        assert_eq!(bar.footprint.value_area_high, 100);
        assert_eq!(bar.volume_flow.sell_qty, 0);
        assert_eq!(bar.raw_ticks[1].is_buyer_maker, 2);
        assert_ne!(bar.identity.coverage_flags & COVERAGE_UNKNOWN_SIDE_VALUE, 0);
    }

    #[test]
    fn counts_duplicate_ids_and_numeric_sequence_regressions() {
        let duplicate = build_bar(
            &[tick(1_000, "1", 100, 1, 0), tick(2_000, "1", 101, 1, 0)],
            0,
            1,
            1,
        )
        .unwrap();
        assert_eq!(duplicate.integrity.duplicate_count, 1);
        assert_eq!(duplicate.integrity.out_of_order_count, 0);
        assert_eq!(duplicate.identity.sequence_contiguous, 0);

        let regression = build_bar(
            &[tick(1_000, "3", 100, 1, 0), tick(2_000, "2", 101, 1, 0)],
            0,
            1,
            1,
        )
        .unwrap();
        assert_eq!(regression.integrity.duplicate_count, 0);
        assert_eq!(regression.integrity.out_of_order_count, 1);
        assert_eq!(regression.identity.sequence_contiguous, 0);

        let gap = build_bar(
            &[tick(1_000, "1", 100, 1, 0), tick(2_000, "3", 101, 1, 0)],
            0,
            1,
            1,
        )
        .unwrap();
        assert_eq!(gap.integrity.sequence_gaps, 1);
        assert_eq!(gap.identity.sequence_contiguous, 0);
    }

    #[test]
    fn handles_a_single_tick_without_changes_or_interarrivals() {
        let bar = build_bar(&[tick(1_000, "1", 100, 7, 0)], 0, 1, 1).unwrap();

        assert_eq!(bar.identity.n_trades, 1);
        assert_eq!(bar.price_changes.change_count, 0);
        assert_eq!(bar.price.zero_ticks, 0);
        assert_eq!(bar.timing.interarrival_count, 0);
        assert_eq!(bar.footprint.level_count, 1);
        assert_eq!(bar.volume_flow.total_qty, 7);
    }

    #[test]
    fn value_area_ties_prefer_the_higher_level_and_poc_ties_the_lower() {
        let poc_tie = build_bar(
            &[
                tick(1_000, "1", 99, 2, 0),
                tick(2_000, "2", 100, 4, 0),
                tick(3_000, "3", 101, 4, 0),
            ],
            0,
            1,
            1,
        )
        .unwrap();
        assert_eq!(poc_tie.footprint.poc_price, 100);

        let value_area_tie = build_bar(
            &[
                tick(1_000, "1", 99, 3, 0),
                tick(2_000, "2", 100, 5, 0),
                tick(3_000, "3", 101, 3, 0),
            ],
            0,
            1,
            1,
        )
        .unwrap();
        assert_eq!(value_area_tie.footprint.value_area_low, 100);
        assert_eq!(value_area_tie.footprint.value_area_high, 101);
    }

    #[test]
    fn counts_stacked_imbalance_runs_and_breaks_them_at_balanced_levels() {
        let bar = build_bar(
            &[
                tick(1_000, "1", 100, 3, 0),
                tick(2_000, "2", 101, 3, 0),
                tick(3_000, "3", 101, 1, 1),
                tick(4_000, "4", 102, 3, 0),
                tick(5_000, "5", 103, 1, 0),
                tick(6_000, "6", 103, 1, 1),
                tick(7_000, "7", 104, 3, 1),
                tick(8_000, "8", 105, 3, 1),
            ],
            0,
            1,
            1,
        )
        .unwrap();

        assert_eq!(bar.footprint.stacked_buy_imbalance, 3);
        assert_eq!(bar.footprint.stacked_sell_imbalance, 2);
    }

    #[test]
    fn fails_explicitly_when_price_volume_histogram_weight_overflows() {
        let error = build_bar(
            &[
                tick(1_000, "1", 1, u64::MAX, 0),
                tick(2_000, "2", 1, u64::MAX, 0),
            ],
            0,
            1,
            1,
        )
        .unwrap_err();

        assert!(
            error
                .to_string()
                .contains("price-volume histogram weight overflows")
        );
    }

    #[test]
    fn absorption_sums_fail_instead_of_wrapping() {
        let level = |price, buy_qty| FootprintLevel {
            price,
            buy_qty,
            sell_qty: 0,
            buy_notional: 0,
            sell_notional: 0,
            buy_count: 0,
            sell_count: 0,
            delta: 0,
            total_qty: 0,
            total_count: 0,
            flags: 0,
        };
        let levels = [level(100, u128::MAX), level(101, 1)];

        assert!(calculate_absorption_exhaustion(100, 99, &levels).is_err());
    }

    #[test]
    fn serializes_wide_integers_as_decimal_strings_and_checksum_is_recomputable() {
        let bar = build_bar(
            &[
                tick(1_000, "1", 5_000_000_000, 5_000_000_000, 0),
                tick(2_000, "2", 5_000_000_001, 1, 1),
            ],
            0,
            1,
            1,
        )
        .unwrap();
        let json = serde_json::to_value(&bar).unwrap();

        assert_eq!(json["timestamp"].as_str(), Some("0"));
        assert_eq!(json["price"]["open"].as_str(), Some("5000000000"));
        assert!(json["volume_flow"]["total_notional"].is_string());
        assert!(json["integrity"]["checksum"].is_string());
        assert!(json["raw_ticks"][0]["time_us"].is_string());
        assert!(json["distributions"]["trade_size_hist"][0].is_string());
        let event = crate::persistence::BarEvent {
            event_id: "binance/BTCUSDT/0:r1",
            event_type: "bar.1m",
            timestamp: bar.timestamp,
            source: &bar.source,
            symbol: &bar.symbol,
            revision: bar.revision,
            data: &bar,
        };
        let event_json = serde_json::to_value(event).unwrap();
        assert_eq!(event_json["timestamp"].as_str(), Some("0"));
        assert_eq!(event_json["revision"].as_str(), Some("1"));

        let mut checksum = 0xcbf29ce484222325_u64;
        let mut append = |bytes: &[u8]| {
            for byte in bytes {
                checksum ^= u64::from(*byte);
                checksum = checksum.wrapping_mul(0x100000001b3);
            }
        };
        for raw_tick in json["raw_ticks"].as_array().unwrap() {
            for field in ["source", "symbol", "id"] {
                let value = raw_tick[field].as_str().unwrap();
                append(&(u64::try_from(value.len()).unwrap()).to_be_bytes());
                append(value.as_bytes());
            }
            append(
                &raw_tick["time_us"]
                    .as_str()
                    .unwrap()
                    .parse::<i64>()
                    .unwrap()
                    .to_be_bytes(),
            );
            append(
                &raw_tick["price"]
                    .as_str()
                    .unwrap()
                    .parse::<u64>()
                    .unwrap()
                    .to_be_bytes(),
            );
            append(
                &raw_tick["qty"]
                    .as_str()
                    .unwrap()
                    .parse::<u64>()
                    .unwrap()
                    .to_be_bytes(),
            );
            append(&[u8::try_from(raw_tick["is_buyer_maker"].as_u64().unwrap()).unwrap()]);
        }
        let expected_checksum = checksum.to_string();
        assert_eq!(
            json["integrity"]["checksum"].as_str(),
            Some(expected_checksum.as_str())
        );
    }
}

use crate::{charts::MainIndicatorData, market::BarSnapshot};

mod delta_profile;
mod ema;

pub(super) use ema::{EMA_55_PERIOD, EMA_200_PERIOD};

pub(super) fn calculate(bars: &[BarSnapshot], timeframe: &str) -> MainIndicatorData {
    let samples = bars
        .iter()
        .map(|bar| (bar.candle.timestamp_ms as f64 / 1_000.0, bar.candle.close))
        .collect::<Vec<_>>();
    let (delta_profile, delta_profile_tick_size) = delta_profile::calculate(bars, timeframe);

    MainIndicatorData {
        ema_55: ema::points(&samples, EMA_55_PERIOD).into(),
        ema_200: ema::points(&samples, EMA_200_PERIOD).into(),
        delta_profile: delta_profile.into(),
        delta_profile_tick_size,
    }
}

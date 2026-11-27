use crate::{charts::MainIndicatorData, market::BarSnapshot};

mod ema;

pub(super) use ema::{EMA_55_PERIOD, EMA_200_PERIOD, points};

pub(super) fn calculate(bars: &[BarSnapshot]) -> MainIndicatorData {
    let samples = bars
        .iter()
        .map(|bar| (bar.candle.timestamp_ms as f64 / 1_000.0, bar.candle.close))
        .collect::<Vec<_>>();

    MainIndicatorData {
        ema_55: points(&samples, EMA_55_PERIOD).into(),
        ema_200: points(&samples, EMA_200_PERIOD).into(),
    }
}

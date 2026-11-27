mod indicators;
mod render;

use crate::market::{BarFields, ChartDataRequest};

const INITIAL_VISIBLE_BARS: usize = 200;
const EMA_WARMUP_MULTIPLIER: u64 = 5;

pub(super) const TITLE: &str = "MAIN · OHLCV";
pub(super) const MENU_LABEL: &str = "Main";

pub(super) use render::show;

pub(super) fn data_request(history_limit: u64) -> ChartDataRequest {
    ChartDataRequest {
        history_limit: history_limit.max(indicators::EMA_200_PERIOD as u64 * EMA_WARMUP_MULTIPLIER),
        live: true,
        fields: BarFields {
            price: true,
            volume_flow: true,
            ..Default::default()
        },
    }
}

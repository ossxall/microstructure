mod indicators;
mod render;

use crate::{
    charts::MomentumIndicatorData,
    market::{BarFields, ChartDataRequest, ChartSnapshot},
};

const WARMUP_BARS: usize = 30;

pub(super) const TITLE: &str = "MOMENTUM ALCISTA/BAJISTA";
pub(super) const MENU_LABEL: &str = "Momentum";

pub(super) use render::show;

pub(super) fn calculate_indicators(snapshot: &ChartSnapshot) -> MomentumIndicatorData {
    indicators::calculate(&snapshot.full_bars)
}

pub(super) fn data_request(history_limit: u64) -> ChartDataRequest {
    ChartDataRequest {
        history_limit: history_limit.max((WARMUP_BARS + 1) as u64),
        live: true,
        fields: BarFields {
            complete_bar: true,
            ..Default::default()
        },
    }
}

mod indicators;
mod render;

use crate::{
    charts::BidAskFlowData,
    market::{BarFields, ChartDataRequest, ChartSnapshot},
};

pub(super) const TITLE: &str = "FLUJO BID/ASK · CUMULATIVO";
pub(super) const MENU_LABEL: &str = "Flujo Bid/Ask";

pub(super) use render::show;

pub(super) fn calculate_indicators(snapshot: &ChartSnapshot) -> BidAskFlowData {
    indicators::calculate(&snapshot.full_bars)
}

pub(super) fn data_request(history_limit: u64) -> ChartDataRequest {
    ChartDataRequest {
        history_limit: history_limit.max(1),
        live: true,
        fields: BarFields {
            raw_ticks: true,
            ..Default::default()
        },
    }
}

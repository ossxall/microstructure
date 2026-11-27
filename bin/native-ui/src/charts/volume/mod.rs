mod render;

use crate::market::{BarFields, ChartDataRequest};

pub(super) const TITLE: &str = "VOLUMEN · total_qty";
pub(super) const MENU_LABEL: &str = "Volumen";

pub(super) use render::show;

pub(super) fn data_request(history_limit: u64) -> ChartDataRequest {
    ChartDataRequest {
        history_limit,
        live: true,
        fields: BarFields {
            volume_flow: true,
            ..Default::default()
        },
    }
}

mod render;

use crate::market::{BarFields, ChartDataRequest};

pub(super) const TITLE: &str = "PRECIO · OHLC";
pub(super) const MENU_LABEL: &str = "Precio";

pub(super) use render::show;

pub(super) fn data_request(history_limit: u64) -> ChartDataRequest {
    ChartDataRequest {
        history_limit,
        live: true,
        fields: BarFields {
            price: true,
            ..Default::default()
        },
    }
}

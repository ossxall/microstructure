mod shared;

pub mod main;

use eframe::egui;

use crate::{
    market::{ChartDataRequest, ChartSnapshot},
    theme::PANEL,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChartKind {
    Main,
}

impl ChartKind {
    /// The chart catalog is shared by every feed tab.
    pub const ALL: [Self; 1] = [Self::Main];

    pub const fn title(self) -> &'static str {
        match self {
            Self::Main => main::TITLE,
        }
    }

    pub const fn menu_label(self) -> &'static str {
        match self {
            Self::Main => main::MENU_LABEL,
        }
    }

    pub fn data_request(self, history_limit: u64) -> ChartDataRequest {
        match self {
            Self::Main => main::data_request(history_limit),
        }
    }

    fn render(
        self,
        ui: &mut egui::Ui,
        chart_id: u64,
        feed_id: u64,
        timeframe: &str,
        snapshot: &ChartSnapshot,
    ) {
        match self {
            Self::Main => main::show(ui, chart_id, feed_id, timeframe, snapshot),
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Chart {
    pub id: u64,
    pub kind: ChartKind,
}

impl Chart {
    pub const fn new(id: u64, kind: ChartKind) -> Self {
        Self { id, kind }
    }

    pub fn data_request(self, history_limit: u64) -> ChartDataRequest {
        self.kind.data_request(history_limit)
    }
}

/// Renders a movable, resizable chart window bounded by the tab's chart area.
pub fn show_window(
    ctx: &egui::Context,
    chart: Chart,
    feed_id: u64,
    timeframe: &str,
    snapshot: &ChartSnapshot,
    layout_revision: u64,
    chart_area: egui::Rect,
    default_pos: egui::Pos2,
    default_size: egui::Vec2,
    min_size: egui::Vec2,
    can_close: bool,
) -> bool {
    let mut open = true;
    let title = egui::RichText::new(chart.kind.title()).small();
    let mut window = egui::Window::new(title)
        .id(egui::Id::unique((
            "chart-window",
            chart.id,
            layout_revision,
        )))
        .default_pos(default_pos)
        .default_size(default_size)
        .min_size(min_size)
        .resizable(true)
        .movable(true)
        .collapsible(false)
        .constrain_to(chart_area)
        .frame(
            egui::Frame::new()
                .fill(PANEL)
                .corner_radius(egui::CornerRadius::same(5))
                .inner_margin(egui::Margin::same(6)),
        );
    if can_close {
        window = window.open(&mut open);
    }
    window.show(ctx, |ui| {
        chart
            .kind
            .render(ui, chart.id, feed_id, timeframe, snapshot);
    });
    !open
}

#[cfg(test)]
mod tests {
    use super::ChartKind;
    use crate::market::BarFields;

    #[test]
    fn main_chart_requests_price_and_volume_for_ohlcv() {
        let main = ChartKind::Main.data_request(125);

        assert_eq!(main.history_limit, 1_000);
        assert!(main.live);
        assert_eq!(
            main.fields,
            BarFields {
                price: true,
                volume_flow: true,
                ..Default::default()
            }
        );
    }
}

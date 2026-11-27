mod shared;

pub mod price;
pub mod volume;

use eframe::egui;

use crate::{
    market::{ChartDataRequest, ChartSnapshot},
    theme::PANEL,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChartKind {
    Price,
    Volume,
}

impl ChartKind {
    /// The chart catalog is shared by every feed tab.
    pub const ALL: [Self; 2] = [Self::Price, Self::Volume];

    pub const fn title(self) -> &'static str {
        match self {
            Self::Price => price::TITLE,
            Self::Volume => volume::TITLE,
        }
    }

    pub const fn menu_label(self) -> &'static str {
        match self {
            Self::Price => price::MENU_LABEL,
            Self::Volume => volume::MENU_LABEL,
        }
    }

    pub fn data_request(self, history_limit: u64) -> ChartDataRequest {
        match self {
            Self::Price => price::data_request(history_limit),
            Self::Volume => volume::data_request(history_limit),
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
            Self::Price => price::show(ui, chart_id, feed_id, timeframe, snapshot),
            Self::Volume => volume::show(ui, chart_id, feed_id, timeframe, snapshot),
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
    chart_area: egui::Rect,
    default_pos: egui::Pos2,
    default_size: egui::Vec2,
    min_size: egui::Vec2,
    can_close: bool,
) -> bool {
    let mut open = true;
    let title = egui::RichText::new(chart.kind.title()).small();
    let mut window = egui::Window::new(title)
        .id(egui::Id::unique(("chart-window", chart.id)))
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
    fn chart_data_requirements_are_declared_by_each_chart_module() {
        let price = ChartKind::Price.data_request(125);
        let volume = ChartKind::Volume.data_request(125);

        assert_eq!(price.history_limit, 125);
        assert!(price.live);
        assert_eq!(
            price.fields,
            BarFields {
                price: true,
                ..Default::default()
            }
        );
        assert_eq!(volume.history_limit, 125);
        assert!(volume.live);
        assert_eq!(
            volume.fields,
            BarFields {
                volume_flow: true,
                ..Default::default()
            }
        );
    }
}

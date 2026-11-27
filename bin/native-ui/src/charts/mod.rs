mod shared;

pub mod price;
pub mod volume;

use eframe::egui::{self, RichText};

use crate::{
    market::{ChartDataRequest, ChartSnapshot},
    theme::{MUTED, PANEL},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChartKind {
    Price,
    Volume,
}

impl ChartKind {
    /// The chart layout is shared by every feed tab.
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

/// Renders a chart card and returns `true` when its close button is clicked.
pub fn show(
    ui: &mut egui::Ui,
    chart: Chart,
    feed_id: u64,
    timeframe: &str,
    snapshot: &ChartSnapshot,
    can_close: bool,
) -> bool {
    let mut close = false;
    egui::Frame::new()
        .fill(PANEL)
        .corner_radius(egui::CornerRadius::same(5))
        .inner_margin(egui::Margin::same(6))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(chart.kind.title())
                        .color(MUTED)
                        .small()
                        .strong(),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .add_enabled(can_close, egui::Button::new("×").small())
                        .on_hover_text("Cerrar Chart en todas las pestañas")
                        .clicked()
                    {
                        close = true;
                    }
                });
            });
            chart
                .kind
                .render(ui, chart.id, feed_id, timeframe, snapshot);
        });
    ui.add_space(8.0);
    close
}

pub fn default_layout() -> Vec<Chart> {
    ChartKind::ALL
        .into_iter()
        .enumerate()
        .map(|(index, kind)| Chart::new(index as u64 + 1, kind))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{ChartKind, default_layout};
    use crate::market::BarFields;

    #[test]
    fn default_layout_has_one_chart_for_each_catalog_entry() {
        let charts = default_layout();
        let kinds = charts.iter().map(|chart| chart.kind).collect::<Vec<_>>();

        assert_eq!(kinds, ChartKind::ALL);
        assert_eq!(
            charts.iter().map(|chart| chart.id).collect::<Vec<_>>(),
            [1, 2]
        );
    }

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

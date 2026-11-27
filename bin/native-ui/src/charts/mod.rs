mod shared;

pub mod price;
pub mod volume;

use eframe::egui::{self, RichText};

use crate::{
    market::{ChartDataRequest, ChartSnapshot},
    theme::{LIME, MUTED, PANEL},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChartDrop {
    pub dragged_chart_id: u64,
    pub target_chart_id: u64,
    pub after: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ChartResponse {
    pub close: bool,
    pub drop: Option<ChartDrop>,
}

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

/// Renders a chart card and reports close or drag-and-drop actions.
pub fn show(
    ui: &mut egui::Ui,
    chart: Chart,
    feed_id: u64,
    timeframe: &str,
    snapshot: &ChartSnapshot,
    can_close: bool,
) -> ChartResponse {
    let mut close = false;
    let frame = egui::Frame::new()
        .fill(PANEL)
        .corner_radius(egui::CornerRadius::same(5))
        .inner_margin(egui::Margin::same(6));
    let (drop_zone, dropped_payload) = ui.dnd_drop_zone::<u64, _>(frame, |ui| {
        ui.horizontal(|ui| {
            let drag_handle = ui.dnd_drag_source(
                egui::Id::unique(("chart-drag-handle", chart.id)),
                chart.id,
                |ui| {
                    ui.label(RichText::new("⠿").color(MUTED).strong());
                },
            );
            let _ = drag_handle
                .response
                .on_hover_text("Arrastrar para reordenar Charts en esta pestaña");
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

    let pointer_y = ui.input(|input| input.pointer.interact_pos().map(|pos| pos.y));
    let hovered_payload = drop_zone.response.dnd_hover_payload::<u64>();
    if let (Some(dragged_chart_id), Some(pointer_y)) = (&hovered_payload, pointer_y)
        && **dragged_chart_id != chart.id
    {
        let rect = drop_zone.response.rect;
        let after = pointer_y >= rect.center().y;
        let marker_y = if after { rect.bottom() } else { rect.top() };
        ui.painter()
            .hline(rect.x_range(), marker_y, egui::Stroke::new(2.0, LIME));
    }

    let drop = dropped_payload.and_then(|dragged_chart_id| {
        if *dragged_chart_id == chart.id {
            return None;
        }
        Some(ChartDrop {
            dragged_chart_id: *dragged_chart_id,
            target_chart_id: chart.id,
            after: pointer_y
                .is_some_and(|pointer_y| pointer_y >= drop_zone.response.rect.center().y),
        })
    });

    ui.add_space(8.0);
    ChartResponse { close, drop }
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

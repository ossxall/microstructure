mod shared;

pub mod price;
pub mod volume;

use eframe::egui::{self, RichText};

use crate::{
    market::{ChartDataRequest, ChartSnapshot},
    theme::{LIME, MUTED, PANEL},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChartDropPosition {
    Above,
    Below,
    Left,
    Right,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChartDrop {
    pub dragged_chart_id: u64,
    pub target_chart_id: u64,
    pub position: ChartDropPosition,
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
                .on_hover_text("Arrastrar para reordenar o poner Charts lado a lado");
            ui.label(
                RichText::new(chart.kind.title())
                    .color(MUTED)
                    .small()
                    .strong(),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .add_enabled(can_close, egui::Button::new("×").small())
                    .on_hover_text("Cerrar Chart en esta pestaña")
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

    let pointer_pos = ui.input(|input| input.pointer.interact_pos());
    let hovered_payload = drop_zone.response.dnd_hover_payload::<u64>();
    let hovered_position = pointer_pos.map(|pos| drop_position(drop_zone.response.rect, pos));
    if let (Some(dragged_chart_id), Some(position)) = (&hovered_payload, hovered_position)
        && **dragged_chart_id != chart.id
    {
        let rect = drop_zone.response.rect;
        let stroke = egui::Stroke::new(2.0, LIME);
        match position {
            ChartDropPosition::Above => ui.painter().hline(rect.x_range(), rect.top(), stroke),
            ChartDropPosition::Below => ui.painter().hline(rect.x_range(), rect.bottom(), stroke),
            ChartDropPosition::Left => ui.painter().vline(rect.left(), rect.y_range(), stroke),
            ChartDropPosition::Right => ui.painter().vline(rect.right(), rect.y_range(), stroke),
        };
    }

    let drop = dropped_payload.and_then(|dragged_chart_id| {
        if *dragged_chart_id == chart.id {
            return None;
        }
        Some(ChartDrop {
            dragged_chart_id: *dragged_chart_id,
            target_chart_id: chart.id,
            position: pointer_pos
                .map(|pos| drop_position(drop_zone.response.rect, pos))
                .unwrap_or(ChartDropPosition::Below),
        })
    });

    ui.add_space(8.0);
    ChartResponse { close, drop }
}

fn drop_position(rect: egui::Rect, pointer: egui::Pos2) -> ChartDropPosition {
    let horizontal_offset = (pointer.x - rect.center().x) / (rect.width() * 0.5).max(f32::EPSILON);
    let vertical_offset = (pointer.y - rect.center().y) / (rect.height() * 0.5).max(f32::EPSILON);

    if horizontal_offset.abs() > vertical_offset.abs() {
        if horizontal_offset < 0.0 {
            ChartDropPosition::Left
        } else {
            ChartDropPosition::Right
        }
    } else if vertical_offset < 0.0 {
        ChartDropPosition::Above
    } else {
        ChartDropPosition::Below
    }
}

#[cfg(test)]
mod tests {
    use super::{ChartDropPosition, ChartKind, drop_position};
    use crate::market::BarFields;
    use eframe::egui::{Pos2, Rect, pos2};

    #[test]
    fn drop_position_uses_the_nearest_edge_of_the_chart() {
        let rect = Rect::from_min_max(Pos2::ZERO, pos2(100.0, 200.0));

        assert_eq!(
            drop_position(rect, pos2(5.0, 100.0)),
            ChartDropPosition::Left
        );
        assert_eq!(
            drop_position(rect, pos2(95.0, 100.0)),
            ChartDropPosition::Right
        );
        assert_eq!(
            drop_position(rect, pos2(50.0, 5.0)),
            ChartDropPosition::Above
        );
        assert_eq!(
            drop_position(rect, pos2(50.0, 195.0)),
            ChartDropPosition::Below
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

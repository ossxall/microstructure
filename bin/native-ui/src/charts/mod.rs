mod indicator_worker;
pub mod momentum;
mod shared;

pub mod main;

use std::{collections::HashMap, sync::Arc};

use eframe::egui;

use crate::{
    app_log::AppLogger,
    market::{ChartDataRequest, ChartSnapshot},
    theme::PANEL,
};

pub use indicator_worker::{IndicatorWorkerConfig, spawn_indicator_worker};

#[derive(Clone, Debug)]
pub struct MainIndicatorData {
    pub ema_55: Arc<[[f64; 2]]>,
    pub ema_200: Arc<[[f64; 2]]>,
    pub delta_profile: Arc<[DeltaProfileLevel]>,
    pub delta_profile_tick_size: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MomentumPoint {
    pub time: f64,
    pub bullish: Option<f64>,
    pub bearish: Option<f64>,
    pub doubtful: bool,
}

#[derive(Clone, Debug, Default)]
pub struct MomentumIndicatorData {
    pub points: Arc<[MomentumPoint]>,
    pub error: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DeltaProfileLevel {
    pub price: f64,
    pub buy_volume: f64,
    pub sell_volume: f64,
}

impl Default for MainIndicatorData {
    fn default() -> Self {
        Self {
            ema_55: Vec::new().into(),
            ema_200: Vec::new().into(),
            delta_profile: Vec::new().into(),
            delta_profile_tick_size: 0.0,
        }
    }
}

#[derive(Clone, Debug)]
pub enum ChartIndicatorData {
    Main(MainIndicatorData),
    Momentum(MomentumIndicatorData),
}

#[derive(Clone, Debug, Default)]
pub struct IndicatorSnapshot {
    pub source_generation: u64,
    pub charts: HashMap<ChartKind, ChartIndicatorData>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ChartKind {
    Main,
    Momentum,
}

impl ChartKind {
    /// The chart catalog is shared by every feed tab.
    pub const ALL: [Self; 2] = [Self::Main, Self::Momentum];

    pub const fn title(self) -> &'static str {
        match self {
            Self::Main => main::TITLE,
            Self::Momentum => momentum::TITLE,
        }
    }

    pub const fn menu_label(self) -> &'static str {
        match self {
            Self::Main => main::MENU_LABEL,
            Self::Momentum => momentum::MENU_LABEL,
        }
    }

    pub fn data_request(self, history_limit: u64) -> ChartDataRequest {
        match self {
            Self::Main => main::data_request(history_limit),
            Self::Momentum => momentum::data_request(history_limit),
        }
    }

    fn calculate_indicators(self, snapshot: &ChartSnapshot, timeframe: &str) -> ChartIndicatorData {
        match self {
            Self::Main => ChartIndicatorData::Main(main::calculate_indicators(snapshot, timeframe)),
            Self::Momentum => {
                ChartIndicatorData::Momentum(momentum::calculate_indicators(snapshot))
            }
        }
    }

    fn render(
        self,
        ui: &mut egui::Ui,
        chart_id: u64,
        feed_id: u64,
        feed: &str,
        timeframe: &str,
        snapshot: &ChartSnapshot,
        indicators: &IndicatorSnapshot,
        logger: &AppLogger,
    ) {
        match self {
            Self::Main => main::show(
                ui, chart_id, feed_id, feed, timeframe, snapshot, indicators, logger,
            ),
            Self::Momentum => {
                momentum::show(ui, chart_id, feed_id, feed, timeframe, snapshot, indicators)
            }
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
    feed: &str,
    timeframe: &str,
    snapshot: &ChartSnapshot,
    indicators: &IndicatorSnapshot,
    logger: &AppLogger,
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
        chart.kind.render(
            ui, chart.id, feed_id, feed, timeframe, snapshot, indicators, logger,
        );
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
                footprint: true,
                ..Default::default()
            }
        );
    }

    #[test]
    fn momentum_chart_requests_complete_bars_for_microstructure_fields() {
        let request = ChartKind::Momentum.data_request(10);

        assert_eq!(request.history_limit, 31);
        assert!(request.live);
        assert!(request.fields.complete_bar);
    }
}

use eframe::egui::{self, Color32, Vec2b};
use egui_plot::{
    HLine, HPlacement, Legend, Line, LineStyle, MarkerShape, Plot, PlotPoints, Points,
};

use crate::{
    charts::shared::{self, show_waiting},
    charts::{ChartIndicatorData, ChartKind, IndicatorSnapshot},
    market::ChartSnapshot,
    theme::{GRID, LIME, TEXT},
};

use super::WARMUP_BARS;

const BEARISH: Color32 = Color32::from_rgb(255, 0, 0);
const DOUBTFUL: Color32 = Color32::from_rgb(255, 165, 0);
const INITIAL_VISIBLE_BARS: usize = 200;

pub fn show(
    ui: &mut egui::Ui,
    chart_id: u64,
    feed_id: u64,
    _feed: &str,
    timeframe: &str,
    snapshot: &ChartSnapshot,
    indicator_snapshot: &IndicatorSnapshot,
) {
    let bars = &snapshot.full_bars;
    if bars.is_empty() {
        show_waiting(ui);
        return;
    }

    if indicator_snapshot.source_generation != snapshot.generation {
        show_waiting(ui);
        return;
    }
    let Some(ChartIndicatorData::Momentum(indicators)) =
        indicator_snapshot.charts.get(&ChartKind::Momentum)
    else {
        show_waiting(ui);
        return;
    };

    if let Some(error) = &indicators.error {
        ui.colored_label(DOUBTFUL, format!("No se pudo calcular Momentum: {error}"));
        return;
    }

    let visible_points = indicators
        .points
        .iter()
        .filter(|point| point.bullish.is_some() && point.bearish.is_some())
        .count();
    let doubtful_count = indicators
        .points
        .iter()
        .filter(|point| point.doubtful)
        .count();
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("ALCISTA").color(LIME).small());
        ui.label(egui::RichText::new("BAJISTA").color(BEARISH).small());
        ui.label(
            egui::RichText::new(format!(
                "{visible_points} puntos · {doubtful_count} barras dudosas"
            ))
            .color(TEXT)
            .small(),
        );
    });

    if visible_points == 0 {
        ui.label(
            egui::RichText::new(format!(
                "Calentando Momentum: {} / {} barras",
                bars.len().min(WARMUP_BARS),
                WARMUP_BARS + 1
            ))
            .color(DOUBTFUL)
            .small(),
        );
    }

    let first_visible_bar = bars.len().saturating_sub(INITIAL_VISIBLE_BARS);
    let (x_min, x_max) = shared::x_bounds(
        &bars[first_visible_bar..],
        shared::timeframe_seconds(timeframe),
    );
    let bullish = indicators
        .points
        .iter()
        .filter_map(|point| point.bullish.map(|value| [point.time, value]))
        .collect::<Vec<_>>();
    let bearish = indicators
        .points
        .iter()
        .filter_map(|point| point.bearish.map(|value| [point.time, value]))
        .collect::<Vec<_>>();
    let doubtful = indicators
        .points
        .iter()
        .filter(|point| point.doubtful)
        .map(|point| [point.time, 0.0])
        .collect::<Vec<_>>();

    Plot::new(("momentum", feed_id, chart_id))
        .height(ui.available_height().max(120.0))
        .default_x_bounds(x_min, x_max)
        .default_y_bounds(0.0, 1.0)
        .auto_bounds(Vec2b::new(false, true))
        .link_axis(
            egui::Id::NULL.with(("ohlcv-time-axis", feed_id)),
            Vec2b::new(true, false),
        )
        .set_margin_fraction(egui::Vec2::new(0.05, 0.12))
        .grid_color(GRID)
        .y_axis_position(HPlacement::Right)
        .show_axes(Vec2b::new(true, true))
        .show_x(true)
        .allow_zoom(Vec2b::new(true, true))
        .allow_drag(Vec2b::new(true, true))
        .allow_scroll(Vec2b::new(true, true))
        .allow_boxed_zoom(false)
        .legend(Legend::default())
        .show(ui, |plot_ui| {
            plot_ui.hline(
                HLine::new("Base 0", 0.0)
                    .color(GRID)
                    .style(LineStyle::dotted_dense())
                    .allow_hover(false),
            );
            if !bullish.is_empty() {
                plot_ui.line(
                    Line::new("Momentum alcista", PlotPoints::from(bullish))
                        .color(LIME)
                        .width(2.0)
                        .allow_hover(false),
                );
                plot_ui.line(
                    Line::new("Momentum bajista", PlotPoints::from(bearish))
                        .color(BEARISH)
                        .width(2.0)
                        .style(LineStyle::dashed_dense())
                        .allow_hover(false),
                );
            }
            if !doubtful.is_empty() {
                plot_ui.points(
                    Points::new("Integridad dudosa", PlotPoints::from(doubtful))
                        .shape(MarkerShape::Circle)
                        .color(DOUBTFUL)
                        .radius(2.5),
                );
            }
        });
}

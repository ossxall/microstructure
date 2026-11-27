use eframe::egui::{self, Stroke};
use egui_plot::{Bar, BarChart, Plot};

use crate::{
    charts::shared::{self, show_waiting},
    market::ChartSnapshot,
    theme::{GRID, LIME_GREEN},
};

pub fn show(
    ui: &mut egui::Ui,
    chart_id: u64,
    feed_id: u64,
    timeframe: &str,
    snapshot: &ChartSnapshot,
) {
    let bars = &snapshot.full_bars;
    if bars.is_empty() {
        show_waiting(ui);
        return;
    }

    let max_volume = bars.iter().map(|bar| bar.candle.volume).fold(0.0, f64::max);
    let interval = shared::timeframe_seconds(timeframe);
    let (x_min, x_max) = shared::x_bounds(bars, interval);
    let plotted_bars = bars
        .iter()
        .map(|bar| {
            Bar::new(bar.timestamp_ms as f64 / 1_000.0, bar.candle.volume)
                .width(interval * 0.64)
                .fill(LIME_GREEN)
                .stroke(Stroke::new(1.0, LIME_GREEN))
        })
        .collect();

    Plot::new(("ohlcv-volume", feed_id, chart_id))
        .height(ui.available_height().max(120.0))
        .default_x_bounds(x_min, x_max)
        .default_y_bounds(0.0, (max_volume * 1.25).max(1.0))
        .link_axis(
            egui::Id::NULL.with(("ohlcv-time-axis", feed_id)),
            egui::Vec2b::new(true, false),
        )
        .grid_color(GRID)
        .show_axes(egui::Vec2b::new(true, false))
        .show_y(false)
        .allow_boxed_zoom(false)
        .show(ui, |plot_ui| {
            plot_ui.bar_chart(BarChart::new("Volumen", plotted_bars).allow_hover(false));
        });
}

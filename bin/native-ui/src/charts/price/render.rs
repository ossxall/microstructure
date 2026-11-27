use eframe::egui::{self, Color32, Stroke};
use egui_plot::{Line, Plot, PlotPoints, Polygon};

use crate::{
    charts::shared::{self, show_waiting},
    market::ChartSnapshot,
    theme::{GRID, LIME},
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

    let min_price = bars
        .iter()
        .map(|bar| bar.candle.low)
        .fold(f64::INFINITY, f64::min);
    let max_price = bars
        .iter()
        .map(|bar| bar.candle.high)
        .fold(f64::NEG_INFINITY, f64::max);
    let price_padding = ((max_price - min_price) * 0.08).max(max_price.abs() * 0.00001);
    let interval = shared::timeframe_seconds(timeframe);
    let (x_min, x_max) = shared::x_bounds(bars, interval);
    let x_link = egui::Id::NULL.with(("ohlcv-time-axis", feed_id));

    Plot::new(("ohlcv-price", feed_id, chart_id))
        .height(ui.available_height().max(120.0))
        .default_x_bounds(x_min, x_max)
        .default_y_bounds(min_price - price_padding, max_price + price_padding)
        .link_axis(x_link, egui::Vec2b::new(true, false))
        .grid_color(GRID)
        .show_axes(egui::Vec2b::new(false, true))
        .show_x(false)
        .allow_boxed_zoom(false)
        .show(ui, |plot_ui| {
            for (index, bar) in bars.iter().enumerate() {
                let candle = bar.candle;
                let x = candle.timestamp_ms as f64 / 1_000.0;
                let body_color = if candle.is_bullish() {
                    Color32::BLACK
                } else {
                    Color32::WHITE
                };
                let body_low = candle.open.min(candle.close);
                let body_high = candle.open.max(candle.close);
                let body_height = (body_high - body_low).max(candle.open.abs() * 0.000002);
                let body_low = (body_low + body_high - body_height) * 0.5;
                let body_high = body_low + body_height;
                let half_width = interval * 0.32;

                plot_ui.line(
                    Line::new(
                        format!("wick-{chart_id}-{index}"),
                        PlotPoints::from(vec![[x, candle.low], [x, candle.high]]),
                    )
                    .color(LIME)
                    .width(1.2)
                    .allow_hover(false),
                );

                plot_ui.polygon(
                    Polygon::new(
                        format!("candle-{chart_id}-{index}"),
                        vec![
                            [x - half_width, body_low],
                            [x + half_width, body_low],
                            [x + half_width, body_high],
                            [x - half_width, body_high],
                        ],
                    )
                    .fill_color(body_color)
                    .stroke(Stroke::new(1.0, LIME))
                    .allow_hover(false),
                );
            }
        });
}

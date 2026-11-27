use eframe::egui::{self, Color32, Stroke};
use egui_plot::{Line, Plot, PlotPoints, Polygon};

use crate::{
    app_log::AppLogger,
    charts::shared::{self, show_waiting},
    market::ChartSnapshot,
    theme::{EMA_55, EMA_200, GRID, LIME, VOLUME},
};

use super::{INITIAL_VISIBLE_BARS, indicators};

pub fn show(
    ui: &mut egui::Ui,
    chart_id: u64,
    feed_id: u64,
    feed: &str,
    timeframe: &str,
    snapshot: &ChartSnapshot,
    logger: &AppLogger,
) {
    let bars = &snapshot.full_bars;
    if bars.is_empty() {
        show_waiting(ui);
        return;
    }

    let interval = shared::timeframe_seconds(timeframe);
    let fit_to_content = ui
        .horizontal(|ui| {
            ui.label(egui::RichText::new("VOL").color(VOLUME).small().strong());
            ui.label(egui::RichText::new("EMA 55").color(EMA_55).small());
            ui.label(egui::RichText::new("EMA 200").color(EMA_200).small());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.button("Ajustar vista")
                    .on_hover_text("Mostrar todo el historial cargado y ajustar la escala")
                    .clicked()
            })
            .inner
        })
        .inner;
    if fit_to_content {
        logger.info(
            Some(feed.to_owned()),
            "Vista del gráfico ajustada al historial cargado",
        );
    }

    let first_visible_bar = bars.len().saturating_sub(INITIAL_VISIBLE_BARS);
    let initial_visible_bars = &bars[first_visible_bar..];
    let (x_min, x_max) = shared::x_bounds(initial_visible_bars, interval);
    let (all_x_min, all_x_max) = shared::x_bounds(bars, interval);
    let x_link = egui::Id::NULL.with(("ohlcv-time-axis", feed_id));
    let half_bar_width = interval * 0.24;

    let (x_min, x_max) = if fit_to_content {
        (all_x_min, all_x_max)
    } else {
        (x_min, x_max)
    };
    let mut plot = Plot::new(("ohlcv-main", feed_id, chart_id))
        .height(ui.available_height().max(120.0))
        .default_x_bounds(x_min, x_max)
        .auto_bounds(egui::Vec2b::new(false, true))
        .link_axis(x_link, egui::Vec2b::new(true, false))
        .set_margin_fraction(egui::Vec2::new(0.05, 0.12))
        .grid_color(GRID)
        .show_axes(egui::Vec2b::new(true, true))
        .show_x(true)
        .allow_zoom(egui::Vec2b::new(true, true))
        .allow_drag(egui::Vec2b::new(true, true))
        .allow_scroll(egui::Vec2b::new(true, true))
        .allow_axis_zoom_drag(egui::Vec2b::new(true, true))
        .allow_boxed_zoom(false);
    if fit_to_content {
        plot = plot.reset();
    }
    plot.show(ui, |plot_ui| {
        let bounds = plot_ui.plot_bounds();
        let x_bounds = bounds.range_x();
        let visible_bars = bars
            .iter()
            .filter(|bar| {
                let x = bar.candle.timestamp_ms as f64 / 1_000.0;
                x >= *x_bounds.start() && x <= *x_bounds.end()
            })
            .collect::<Vec<_>>();
        let visible_min_price = visible_bars
            .iter()
            .map(|bar| bar.candle.low)
            .fold(f64::INFINITY, f64::min);
        let visible_max_price = visible_bars
            .iter()
            .map(|bar| bar.candle.high)
            .fold(f64::NEG_INFINITY, f64::max);
        let visible_price_range = visible_max_price - visible_min_price;
        let max_volume = visible_bars
            .iter()
            .map(|bar| bar.candle.volume)
            .fold(0.0, f64::max);

        // Keep the volume histogram in the lower part of the same price
        // plot. Its height is normalized to the visible volume range, so
        // it never introduces a second axis or pane.
        if max_volume > 0.0 && visible_price_range.is_finite() && visible_price_range > 0.0 {
            let volume_height = visible_price_range * 0.18;
            let max_bar_height = volume_height * 0.92;
            for (index, bar) in visible_bars.iter().enumerate() {
                let x = bar.candle.timestamp_ms as f64 / 1_000.0;
                let height = (bar.candle.volume / max_volume) * max_bar_height;
                if height <= 0.0 {
                    continue;
                }
                plot_ui.polygon(
                    Polygon::new(
                        format!("volume-{chart_id}-{index}"),
                        vec![
                            [x - half_bar_width, visible_min_price],
                            [x + half_bar_width, visible_min_price],
                            [x + half_bar_width, visible_min_price + height],
                            [x - half_bar_width, visible_min_price + height],
                        ],
                    )
                    .fill_color(VOLUME)
                    .stroke(Stroke::NONE)
                    .allow_hover(false),
                );
            }
        }

        for (index, bar) in visible_bars.iter().enumerate() {
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
                        [x - half_bar_width, body_low],
                        [x + half_bar_width, body_low],
                        [x + half_bar_width, body_high],
                        [x - half_bar_width, body_high],
                    ],
                )
                .fill_color(body_color)
                .stroke(Stroke::new(1.0, LIME))
                .allow_hover(false),
            );
        }

        let ema_values = bars
            .iter()
            .map(|bar| (bar.candle.timestamp_ms as f64 / 1_000.0, bar.candle.close))
            .collect::<Vec<_>>();
        for (period, color) in [
            (indicators::EMA_55_PERIOD, EMA_55),
            (indicators::EMA_200_PERIOD, EMA_200),
        ] {
            let points = indicators::points(&ema_values, period)
                .into_iter()
                .filter(|point| point[0] >= *x_bounds.start() && point[0] <= *x_bounds.end())
                .collect::<Vec<_>>();
            if points.is_empty() {
                continue;
            }
            plot_ui.line(
                Line::new(format!("EMA {period}"), PlotPoints::from(points))
                    .color(color)
                    .width(1.8)
                    .allow_hover(false),
            );
        }
    });
}

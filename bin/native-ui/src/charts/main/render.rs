use eframe::egui::{self, Color32, Stroke};
use egui_plot::{Axis, HPlacement, Line, Plot, PlotMemory, PlotPoints, Polygon};

use crate::{
    app_log::AppLogger,
    charts::shared::{self, show_waiting},
    charts::{ChartIndicatorData, ChartKind, IndicatorSnapshot},
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
    indicator_snapshot: &IndicatorSnapshot,
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
    let inverted_axis_zoom = if fit_to_content {
        None
    } else {
        reverse_axis_zoom_drag(ui, feed_id, chart_id)
    };

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
        .y_axis_position(HPlacement::Right)
        .show_axes(egui::Vec2b::new(true, true))
        .show_x(true)
        .allow_zoom(egui::Vec2b::new(true, true))
        .allow_drag(egui::Vec2b::new(true, true))
        .allow_scroll(egui::Vec2b::new(true, true))
        .allow_axis_zoom_drag(false)
        .allow_boxed_zoom(false);
    if fit_to_content {
        plot = plot.reset();
    }
    plot.show(ui, |plot_ui| {
        if let Some((axis, bounds)) = inverted_axis_zoom {
            match axis {
                Axis::X => plot_ui.set_plot_bounds_x(bounds.range_x()),
                Axis::Y => plot_ui.set_plot_bounds_y(bounds.range_y()),
            }
        }

        let bounds = plot_ui.plot_bounds();
        let x_bounds = bounds.range_x();
        let visible_bar_range = shared::visible_range(bars, &x_bounds, |bar| {
            bar.candle.timestamp_ms as f64 / 1_000.0
        });
        let visible_bars = &bars[visible_bar_range];
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

        if indicator_snapshot.source_generation == snapshot.generation
            && let Some(ChartIndicatorData::Main(main_indicators)) =
                indicator_snapshot.charts.get(&ChartKind::Main)
        {
            for (period, color, source_points) in [
                (
                    indicators::EMA_55_PERIOD,
                    EMA_55,
                    main_indicators.ema_55.as_ref(),
                ),
                (
                    indicators::EMA_200_PERIOD,
                    EMA_200,
                    main_indicators.ema_200.as_ref(),
                ),
            ] {
                let visible_point_range =
                    shared::visible_range(source_points, &x_bounds, |point| point[0]);
                let points = &source_points[visible_point_range];
                if points.is_empty() {
                    continue;
                }
                plot_ui.line(
                    Line::new(format!("EMA {period}"), PlotPoints::from(points.to_vec()))
                        .color(color)
                        .width(1.8)
                        .allow_hover(false),
                );
            }
        }
    });
}

fn inverted_axis_zoom_factor(delta: f32) -> f32 {
    1.0 + (-0.02 * delta).clamp(-1.0, 1.0)
}

/// `egui_plot` doesn't expose an option to reverse axis-drag zoom, so handle
/// the two scale hit areas with the same behavior and an inverted delta.
fn reverse_axis_zoom_drag(
    ui: &mut egui::Ui,
    feed_id: u64,
    chart_id: u64,
) -> Option<(Axis, egui_plot::PlotBounds)> {
    let plot_id = ui.make_persistent_id(egui::IdSalt::new(("ohlcv-main", feed_id, chart_id)));
    let previous_transform = PlotMemory::load(ui.ctx(), plot_id)?.transform();
    let plot_rect = *previous_transform.frame();
    let available_rect = ui.available_rect_before_wrap();
    let chart_rect = egui::Rect::from_min_size(
        available_rect.min,
        egui::vec2(available_rect.width(), ui.available_height().max(120.0)),
    );
    let x_axis_rect = egui::Rect::from_min_max(
        egui::pos2(plot_rect.left(), plot_rect.bottom()),
        egui::pos2(plot_rect.right(), chart_rect.bottom()),
    );
    let y_axis_rect = egui::Rect::from_min_max(
        egui::pos2(plot_rect.right(), chart_rect.top()),
        egui::pos2(chart_rect.right(), plot_rect.bottom()),
    );
    let x_axis_response = ui
        .interact(
            x_axis_rect,
            ui.make_persistent_id(("ohlcv-time-axis-zoom", feed_id, chart_id)),
            egui::Sense::drag(),
        )
        .on_hover_cursor(egui::CursorIcon::ResizeHorizontal);
    let y_axis_response = ui
        .interact(
            y_axis_rect,
            ui.make_persistent_id(("ohlcv-price-axis-zoom", feed_id, chart_id)),
            egui::Sense::drag(),
        )
        .on_hover_cursor(egui::CursorIcon::ResizeVertical);
    let press_origin = ui.input(|input| input.pointer.press_origin())?;

    let (axis, delta, center) = if x_axis_response.dragged_by(egui::PointerButton::Primary) {
        (
            Axis::X,
            x_axis_response.drag_delta().x,
            egui::pos2(press_origin.x, plot_rect.center().y),
        )
    } else if y_axis_response.dragged_by(egui::PointerButton::Primary) {
        (
            Axis::Y,
            y_axis_response.drag_delta().y,
            egui::pos2(plot_rect.center().x, press_origin.y),
        )
    } else {
        return None;
    };
    if delta == 0.0 {
        return None;
    }

    let zoom_factor = inverted_axis_zoom_factor(delta);
    let zoom = match axis {
        Axis::X => egui::vec2(zoom_factor, 1.0),
        Axis::Y => egui::vec2(1.0, zoom_factor),
    };
    let mut transform = previous_transform;
    transform.zoom(zoom, center);
    Some((axis, *transform.bounds()))
}

#[cfg(test)]
mod tests {
    use super::inverted_axis_zoom_factor;

    #[test]
    fn axis_drag_zoom_direction_is_inverted() {
        assert!(inverted_axis_zoom_factor(10.0) < 1.0);
        assert!(inverted_axis_zoom_factor(-10.0) > 1.0);
    }
}

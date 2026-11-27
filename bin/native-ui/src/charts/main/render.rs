use eframe::egui::{self, Color32, Stroke};
use egui_plot::{Axis, HPlacement, Line, Plot, PlotMemory, PlotPoint, PlotPoints, Polygon, Text};

use crate::{
    app_log::AppLogger,
    charts::shared::{self, show_waiting},
    charts::{ChartIndicatorData, ChartKind, IndicatorSnapshot},
    market::ChartSnapshot,
    theme::{
        EMA_55, EMA_200, GRID, LIME, PROFILE_BEARISH, PROFILE_BULLISH, PROFILE_NEUTRAL, VOLUME,
    },
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
            ui.label(
                egui::RichText::new("DELTA PROFILE · 15m")
                    .color(PROFILE_BULLISH)
                    .small(),
            );
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

        // Keep the last completed indicator snapshot visible while the worker
        // calculates the newer market generation. Hiding these overlays during
        // that interval makes the chart flicker on every live update.
        if let Some(ChartIndicatorData::Main(main_indicators)) =
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

            draw_delta_profile(
                plot_ui,
                main_indicators.delta_profile.as_ref(),
                main_indicators.delta_profile_tick_size,
                &x_bounds,
                *bounds.range_y().start(),
                *bounds.range_y().end(),
                chart_id,
            );
        }
    });
}

fn draw_delta_profile(
    plot_ui: &mut egui_plot::PlotUi<'_>,
    levels: &[crate::charts::DeltaProfileLevel],
    tick_size: f64,
    x_bounds: &std::ops::RangeInclusive<f64>,
    price_min: f64,
    price_max: f64,
    chart_id: u64,
) {
    if levels.is_empty() || !tick_size.is_finite() || tick_size <= 0.0 {
        return;
    }

    let x_min = *x_bounds.start();
    let x_max = *x_bounds.end();
    let x_range = x_max - x_min;
    if !x_range.is_finite() || x_range <= 0.0 {
        return;
    }

    let visible_levels = levels
        .iter()
        .filter(|level| {
            level.price >= price_min - tick_size && level.price <= price_max + tick_size
        })
        .collect::<Vec<_>>();
    let max_volume = visible_levels
        .iter()
        .map(|level| level.buy_volume + level.sell_volume)
        .fold(0.0, f64::max);
    if visible_levels.is_empty() || !max_volume.is_finite() || max_volume <= 0.0 {
        return;
    }

    // Keep the profile fixed to the chart's right edge, using a narrow strip
    // of the visible time range so it stays aligned while zooming and panning.
    let profile_width = x_range * 0.2;
    let right = x_max - x_range * 0.015;
    let half_height = tick_size * 0.39;

    for (index, level) in visible_levels.into_iter().enumerate() {
        let total_volume = level.buy_volume + level.sell_volume;
        if !total_volume.is_finite() || total_volume <= 0.0 {
            continue;
        }
        let width = profile_width * (total_volume / max_volume).clamp(0.0, 1.0);
        let color = if level.buy_volume > level.sell_volume {
            PROFILE_BULLISH
        } else if level.sell_volume > level.buy_volume {
            PROFILE_BEARISH
        } else {
            PROFILE_NEUTRAL
        };
        let color = egui::Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), 190);
        let price_low = level.price - half_height;
        let price_high = level.price + half_height;
        plot_ui.polygon(
            Polygon::new(
                format!("delta-profile-{chart_id}-{index}"),
                vec![
                    [right - width, price_low],
                    [right, price_low],
                    [right, price_high],
                    [right - width, price_high],
                ],
            )
            .fill_color(color)
            .stroke(Stroke::NONE)
            .allow_hover(false),
        );
        plot_ui.text(
            Text::new(
                format!("delta-profile-value-{chart_id}-{index}"),
                PlotPoint::new(right - width / 2.0, level.price),
                format_delta(level.buy_volume - level.sell_volume),
            )
            .color(Color32::WHITE)
            .allow_hover(false),
        );
    }
}

fn format_delta(delta: f64) -> String {
    if !delta.is_finite() {
        return "—".to_owned();
    }
    if delta == 0.0 {
        return "0.000".to_owned();
    }

    let rounded = format!("{:.3}", delta.abs());
    let (integer, fraction) = rounded.split_once('.').unwrap_or((&rounded, "000"));
    let mut grouped_integer = String::with_capacity(integer.len() + integer.len() / 3);
    for (index, digit) in integer.chars().rev().enumerate() {
        if index > 0 && index % 3 == 0 {
            grouped_integer.push(',');
        }
        grouped_integer.push(digit);
    }
    let grouped_integer = grouped_integer.chars().rev().collect::<String>();
    let sign = if delta > 0.0 { "+" } else { "−" };
    format!("{sign}{grouped_integer}.{fraction}")
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
    use super::{format_delta, inverted_axis_zoom_factor};

    #[test]
    fn axis_drag_zoom_direction_is_inverted() {
        assert!(inverted_axis_zoom_factor(10.0) < 1.0);
        assert!(inverted_axis_zoom_factor(-10.0) > 1.0);
    }

    #[test]
    fn delta_labels_keep_the_sign_and_show_trailing_zeroes() {
        assert_eq!(format_delta(12.5), "+12.500");
        assert_eq!(format_delta(-3.125), "−3.125");
        assert_eq!(format_delta(0.0), "0.000");
    }

    #[test]
    fn delta_labels_group_thousands_and_show_three_fraction_digits() {
        assert_eq!(format_delta(1_234_567.8914), "+1,234,567.891");
        assert_eq!(format_delta(-1_000.0), "−1,000.000");
        assert_eq!(format_delta(0.0004), "+0.000");
        assert_eq!(format_delta(f64::INFINITY), "—");
    }
}

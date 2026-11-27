use eframe::egui::{self, Color32, Stroke};
use egui_plot::{HLine, HPlacement, Line, LineStyle, Plot, PlotPoints, Polygon};

use crate::{
    charts::shared::{self, show_waiting},
    charts::{BidAskFlowCandle, ChartIndicatorData, ChartKind, IndicatorSnapshot},
    market::ChartSnapshot,
    theme::{GRID, LIME},
};

const RED: Color32 = Color32::from_rgb(255, 0, 0);
const INITIAL_VISIBLE_BARS: usize = 200;
const BAR_WIDTH_RATIO: f64 = 0.62;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum CandleStyle {
    #[default]
    Candles,
    Ohlc,
}

pub fn show(
    ui: &mut egui::Ui,
    chart_id: u64,
    feed_id: u64,
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
    let Some(ChartIndicatorData::BidAskFlow(data)) =
        indicator_snapshot.charts.get(&ChartKind::BidAskFlow)
    else {
        show_waiting(ui);
        return;
    };

    let style_id =
        ui.make_persistent_id(egui::IdSalt::new(("bid-ask-flow-style", feed_id, chart_id)));
    let mut style = ui
        .ctx()
        .data(|data| data.get_temp::<CandleStyle>(style_id))
        .unwrap_or_default();
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("ASK").color(LIME).small());
        ui.label(egui::RichText::new("BID").color(RED).small());
        ui.separator();
        if ui
            .selectable_label(style == CandleStyle::Candles, "Velas")
            .clicked()
        {
            style = CandleStyle::Candles;
        }
        if ui
            .selectable_label(style == CandleStyle::Ohlc, "OHLC")
            .clicked()
        {
            style = CandleStyle::Ohlc;
        }
        ui.separator();
        if let Some(candle) = data.candles.last() {
            let color = candle_color(candle);
            for (label, value, value_color) in [
                ("O", candle.open, color),
                ("H", candle.high, color),
                ("L", candle.low, color),
                ("C", candle.close, color),
                ("Ask", candle.ask_qty, LIME),
                ("Bid", candle.bid_qty, RED),
                ("Δ", candle.ask_qty - candle.bid_qty, color),
            ] {
                ui.label(egui::RichText::new(label).color(GRID).small());
                ui.label(
                    egui::RichText::new(format_quantity(value))
                        .color(value_color)
                        .monospace()
                        .small(),
                );
            }
        } else {
            ui.label(egui::RichText::new("Sin ticks válidos").color(GRID).small());
        }
    });
    ui.ctx().data_mut(|data| data.insert_temp(style_id, style));

    let first_visible_bar = bars.len().saturating_sub(INITIAL_VISIBLE_BARS);
    let (x_min, x_max) = shared::x_bounds(
        &bars[first_visible_bar..],
        shared::timeframe_seconds(timeframe),
    );
    let half_bar_width = shared::timeframe_seconds(timeframe) * BAR_WIDTH_RATIO * 0.5;

    Plot::new(("bid-ask-flow", feed_id, chart_id))
        .height(ui.available_height().max(120.0))
        .default_x_bounds(x_min, x_max)
        .auto_bounds(egui::Vec2b::new(false, true))
        .link_axis(
            egui::Id::NULL.with(("ohlcv-time-axis", feed_id)),
            egui::Vec2b::new(true, false),
        )
        .set_margin_fraction(egui::Vec2::new(0.05, 0.12))
        .grid_color(GRID)
        .y_axis_position(HPlacement::Right)
        .show_axes(egui::Vec2b::new(true, true))
        .show_x(true)
        .allow_zoom(egui::Vec2b::new(true, true))
        .allow_drag(egui::Vec2b::new(true, true))
        .allow_scroll(egui::Vec2b::new(true, true))
        .allow_boxed_zoom(false)
        .show(ui, |plot_ui| {
            plot_ui.hline(
                HLine::new("Cero", 0.0)
                    .color(GRID)
                    .style(LineStyle::dashed_dense())
                    .allow_hover(false),
            );

            let x_bounds = plot_ui.plot_bounds().range_x();
            let visible = shared::visible_range(&data.candles, &x_bounds, |candle| candle.time);
            for (index, candle) in data.candles[visible].iter().enumerate() {
                draw_candle(plot_ui, chart_id, index, candle, half_bar_width, style);
            }
        });
}

fn draw_candle(
    plot_ui: &mut egui_plot::PlotUi<'_>,
    chart_id: u64,
    index: usize,
    candle: &BidAskFlowCandle,
    half_bar_width: f64,
    style: CandleStyle,
) {
    let color = candle_color(candle);
    plot_ui.line(
        Line::new(
            format!("bid-ask-flow-wick-{chart_id}-{index}"),
            PlotPoints::from(vec![[candle.time, candle.high], [candle.time, candle.low]]),
        )
        .color(color)
        .width(1.5)
        .allow_hover(false),
    );

    if style == CandleStyle::Ohlc {
        plot_ui.line(
            Line::new(
                format!("bid-ask-flow-open-{chart_id}-{index}"),
                PlotPoints::from(vec![
                    [candle.time - half_bar_width, candle.open],
                    [candle.time, candle.open],
                ]),
            )
            .color(color)
            .width(1.5)
            .allow_hover(false),
        );
        plot_ui.line(
            Line::new(
                format!("bid-ask-flow-close-{chart_id}-{index}"),
                PlotPoints::from(vec![
                    [candle.time, candle.close],
                    [candle.time + half_bar_width, candle.close],
                ]),
            )
            .color(color)
            .width(1.5)
            .allow_hover(false),
        );
        return;
    }

    if candle.close == candle.open {
        plot_ui.line(
            Line::new(
                format!("bid-ask-flow-doji-{chart_id}-{index}"),
                PlotPoints::from(vec![
                    [candle.time - half_bar_width, candle.open],
                    [candle.time + half_bar_width, candle.open],
                ]),
            )
            .color(color)
            .width(2.0)
            .allow_hover(false),
        );
        return;
    }

    let body_low = candle.open.min(candle.close);
    let body_high = candle.open.max(candle.close);
    plot_ui.polygon(
        Polygon::new(
            format!("bid-ask-flow-candle-{chart_id}-{index}"),
            vec![
                [candle.time - half_bar_width, body_low],
                [candle.time + half_bar_width, body_low],
                [candle.time + half_bar_width, body_high],
                [candle.time - half_bar_width, body_high],
            ],
        )
        .fill_color(color)
        .stroke(Stroke::new(1.0, color))
        .allow_hover(false),
    );
}

fn candle_color(candle: &BidAskFlowCandle) -> Color32 {
    if candle.close >= candle.open {
        LIME
    } else {
        RED
    }
}

fn format_quantity(value: f64) -> String {
    if !value.is_finite() {
        return "—".to_owned();
    }

    let mut raw = (0..=12)
        .find_map(|precision| {
            let candidate = format!("{value:.precision$}");
            (candidate.parse::<f64>().ok() == Some(value)).then_some(candidate)
        })
        .unwrap_or_else(|| format!("{value:.12}"));
    if raw.contains('.') {
        while raw.ends_with('0') {
            raw.pop();
        }
        if raw.ends_with('.') {
            raw.pop();
        }
    }
    let (sign, unsigned) = raw
        .strip_prefix('-')
        .map_or(("", raw.as_str()), |value| ("−", value));
    let (integer, fraction) = unsigned.split_once('.').unwrap_or((unsigned, ""));
    let mut grouped = String::with_capacity(integer.len() + integer.len() / 3);
    for (index, digit) in integer.chars().rev().enumerate() {
        if index > 0 && index % 3 == 0 {
            grouped.push(',');
        }
        grouped.push(digit);
    }
    let grouped = grouped.chars().rev().collect::<String>();
    if fraction.is_empty() {
        format!("{sign}{grouped}")
    } else {
        format!("{sign}{grouped}.{fraction}")
    }
}

#[cfg(test)]
mod tests {
    use super::format_quantity;

    #[test]
    fn quantity_format_keeps_precision_and_groups_large_values() {
        assert_eq!(format_quantity(1_234_567.8914), "1,234,567.8914");
        assert_eq!(format_quantity(12.0), "12");
        assert_eq!(format_quantity(f64::INFINITY), "—");
    }
}

use eframe::egui::{self, Color32, Stroke};
use egui_plot::{HLine, HPlacement, Line, Plot, PlotPoint, PlotPoints, Polygon, Text};
use std::collections::BTreeMap;

use crate::{
    app_log::AppLogger,
    charts::IndicatorSnapshot,
    market::ChartSnapshot,
    theme::{GRID, LIME, PROFILE_BEARISH, PROFILE_BULLISH, TEXT},
};

use super::{FootprintCandle, FootprintLevel, timeframe_label, timeframe_options};

const GOLD: Color32 = Color32::from_rgb(255, 215, 0);
const CANDLE_BODY_ALPHA: u8 = 96;
const DEFAULT_LEVEL_GROUP: usize = 4;
const LEVEL_GROUP_OPTIONS: [usize; 5] = [1, 2, 4, 8, 16];

#[derive(Clone, Copy, Debug)]
struct DisplayLevel {
    price_low: i128,
    price_high: i128,
    buy_qty: u128,
    sell_qty: u128,
    is_poc: bool,
}

pub(crate) fn show(
    ui: &mut egui::Ui,
    chart_id: u64,
    feed_id: u64,
    feed: &str,
    source_timeframe: &str,
    target_timeframe_ms: u64,
    snapshot: &ChartSnapshot,
    indicator_snapshot: &IndicatorSnapshot,
    logger: &AppLogger,
    timeframe_change: &mut Option<u64>,
) {
    let mut requested_timeframe = target_timeframe_ms;
    let mut custom_minutes = target_timeframe_ms.div_ceil(60_000);
    let grouping_id = ui.make_persistent_id(egui::IdSalt::new((
        "footprint-level-group",
        feed_id,
        chart_id,
    )));
    let mut level_group = ui
        .ctx()
        .data(|data| data.get_temp::<usize>(grouping_id))
        .unwrap_or(DEFAULT_LEVEL_GROUP);
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("SELL").color(PROFILE_BEARISH).small());
        ui.label(egui::RichText::new("BUY").color(PROFILE_BULLISH).small());
        ui.separator();
        ui.menu_button(format!("Niveles ×{level_group}"), |ui| {
            ui.label(
                egui::RichText::new("Suma niveles de precio contiguos")
                    .color(GRID)
                    .small(),
            );
            for group in LEVEL_GROUP_OPTIONS {
                if ui
                    .selectable_label(level_group == group, format!("×{group}"))
                    .clicked()
                    && level_group != group
                {
                    level_group = group;
                    logger.info(
                        Some(feed.to_owned()),
                        format!("Footprint #{chart_id}: agrupación de niveles ×{group}"),
                    );
                }
            }
        });
        ui.separator();
        ui.menu_button(
            format!("{} ▾", timeframe_label(target_timeframe_ms)),
            |ui| {
                ui.label(
                    egui::RichText::new(format!("Fuente: {source_timeframe}"))
                        .color(GRID)
                        .small(),
                );
                ui.separator();
                for option in timeframe_options(source_timeframe) {
                    if ui
                        .selectable_label(requested_timeframe == option.millis, option.label)
                        .clicked()
                    {
                        requested_timeframe = option.millis;
                    }
                }
                ui.separator();
                ui.horizontal(|ui| {
                    ui.label("Personalizado");
                    let changed = ui
                        .add(
                            egui::DragValue::new(&mut custom_minutes)
                                .range(1..=10_080)
                                .suffix("m"),
                        )
                        .changed();
                    if changed
                        && let Some(target_ms) = custom_minutes.checked_mul(60_000)
                        && super::is_valid_timeframe(source_timeframe, target_ms)
                    {
                        requested_timeframe = target_ms;
                    }
                });
            },
        );
        ui.ctx()
            .data_mut(|data| data.insert_temp(grouping_id, level_group));
        if requested_timeframe != target_timeframe_ms {
            *timeframe_change = Some(requested_timeframe);
        }
    });

    let Some(data) = indicator_snapshot.footprints.get(&chart_id) else {
        show_waiting(ui);
        return;
    };
    if data.target_timeframe_ms != target_timeframe_ms {
        show_waiting(ui);
        return;
    }
    if let Some(error) = &data.error {
        ui.colored_label(Color32::LIGHT_RED, error);
        return;
    }
    let Some(candle) = &data.candle else {
        show_waiting(ui);
        return;
    };

    let is_calculating = indicator_snapshot.source_generation < snapshot.generation;
    show_summary(ui, candle, unix_time_ms(), is_calculating);
    draw_footprint(ui, feed_id, chart_id, candle, level_group);
}

fn show_waiting(ui: &mut egui::Ui) {
    ui.centered_and_justified(|ui| {
        ui.label(
            egui::RichText::new("Esperando Bars finales de la tab…")
                .color(GRID)
                .small(),
        );
    });
}

fn show_summary(ui: &mut egui::Ui, candle: &FootprintCandle, now_ms: u64, is_calculating: bool) {
    let is_closed = candle.coverage.closed || now_ms >= candle.end_timestamp_ms;
    ui.horizontal_wrapped(|ui| {
        for (label, value) in [
            ("O", format_price(candle.open, candle.price_scale)),
            ("H", format_price(candle.high, candle.price_scale)),
            ("L", format_price(candle.low, candle.price_scale)),
            ("C", format_price(candle.close, candle.price_scale)),
            (
                "VOL",
                format_quantity(candle.total_qty, candle.quantity_scale),
            ),
            ("VWAP", format_scaled_u128(candle.vwap, candle.price_scale)),
            (
                "Δ",
                format_signed_quantity(candle.signed_qty, candle.quantity_scale),
            ),
            (
                "POC",
                format_price(candle.footprint.poc_price, candle.price_scale),
            ),
            (
                "POC VOL",
                format_quantity(candle.footprint.poc_volume, candle.quantity_scale),
            ),
            (
                "POC Δ",
                format_signed_quantity(candle.footprint.poc_delta, candle.quantity_scale),
            ),
            (
                "ΔMAX+",
                format_signed_quantity(candle.footprint.max_positive_delta, candle.quantity_scale),
            ),
            (
                "ΔMAX−",
                format_signed_quantity(candle.footprint.max_negative_delta, candle.quantity_scale),
            ),
            ("NIVELES", candle.footprint.level_count.to_string()),
            (
                "IMB",
                format!(
                    "{} · {}↑/{}↓",
                    candle.footprint.imbalance_count,
                    candle.footprint.stacked_buy_imbalance,
                    candle.footprint.stacked_sell_imbalance
                ),
            ),
            (
                "UA",
                format!(
                    "{}{}",
                    if candle.footprint.unfinished_auction_high {
                        "H"
                    } else {
                        ""
                    },
                    if candle.footprint.unfinished_auction_low {
                        "L"
                    } else {
                        ""
                    }
                ),
            ),
        ] {
            ui.label(egui::RichText::new(label).color(GRID).small());
            let color = match label {
                "Δ" => delta_color(candle.signed_qty),
                "POC Δ" => delta_color(candle.footprint.poc_delta),
                "ΔMAX+" => PROFILE_BULLISH,
                "ΔMAX−" => PROFILE_BEARISH,
                _ => TEXT,
            };
            ui.label(
                egui::RichText::new(value)
                    .color(color)
                    .monospace()
                    .small()
                    .strong(),
            );
        }
        ui.label(egui::RichText::new("VA").color(GRID).small());
        ui.label(
            egui::RichText::new(format!(
                "{}–{}",
                format_price(candle.footprint.value_area_low, candle.price_scale),
                format_price(candle.footprint.value_area_high, candle.price_scale)
            ))
            .color(GOLD)
            .monospace()
            .small(),
        );
        ui.label(
            egui::RichText::new(if is_closed {
                "CERRADA"
            } else {
                "EN FORMACIÓN"
            })
            .color(GRID)
            .small(),
        );
        let coverage_response = ui.label(
            egui::RichText::new(format!(
                "{} Bars · {} faltantes · {} pendientes · continuidad {}",
                candle.coverage.n_bars,
                candle.coverage.missing.len(),
                candle.coverage.pending.len(),
                candle.coverage.continuity.label()
            ))
            .color(GRID)
            .small(),
        );
        let revisions = candle
            .coverage
            .revisions
            .iter()
            .map(|(bar_key, revision)| format!("{bar_key}: r{revision}"))
            .collect::<Vec<_>>()
            .join("\n");
        coverage_response.on_hover_text(format!(
            "coverage_flags OR: {}\nRevisiones:\n{}\n{}",
            candle.coverage_flags,
            revisions,
            if is_calculating {
                "Actualizando con el último snapshot"
            } else {
                "Cálculo actualizado"
            }
        ));
    });
}

fn delta_color(value: i128) -> Color32 {
    if value > 0 {
        PROFILE_BULLISH
    } else if value < 0 {
        PROFILE_BEARISH
    } else {
        TEXT
    }
}

fn aggregate_display_levels(
    levels: &[FootprintLevel],
    tick_size_raw: u64,
    group_size: usize,
    poc_price: i64,
) -> Option<Vec<DisplayLevel>> {
    let group_size = group_size.max(1);
    let bucket_width = i128::from(tick_size_raw.max(1)).checked_mul(group_size as i128)?;
    let mut grouped = BTreeMap::<i128, DisplayLevel>::new();

    for level in levels {
        let bucket = i128::from(level.price).div_euclid(bucket_width);
        let price_low = bucket.checked_mul(bucket_width)?;
        let price_high = price_low.checked_add(bucket_width)?;
        let target = grouped.entry(bucket).or_insert(DisplayLevel {
            price_low,
            price_high,
            buy_qty: 0,
            sell_qty: 0,
            is_poc: false,
        });
        target.buy_qty = target.buy_qty.checked_add(level.buy_qty)?;
        target.sell_qty = target.sell_qty.checked_add(level.sell_qty)?;
        target.is_poc |= level.price == poc_price;
    }

    Some(grouped.into_values().collect())
}

fn draw_footprint(
    ui: &mut egui::Ui,
    feed_id: u64,
    chart_id: u64,
    candle: &FootprintCandle,
    level_group: usize,
) {
    if candle.levels.is_empty() {
        return;
    }
    let price_divisor = 10_f64.powi(candle.price_scale.min(18) as i32);
    let time_start = candle.timestamp_ms as f64 / 1_000.0;
    let time_end = candle.end_timestamp_ms as f64 / 1_000.0;
    let duration = time_end - time_start;
    if !duration.is_finite() || duration <= 0.0 {
        return;
    }
    let min_tick_raw = candle
        .levels
        .windows(2)
        .map(|levels| {
            levels[1]
                .price
                .saturating_sub(levels[0].price)
                .unsigned_abs()
        })
        .filter(|difference| *difference > 0)
        .min()
        .unwrap_or(1)
        .max(1);
    let Some(grouped_levels) = aggregate_display_levels(
        &candle.levels,
        min_tick_raw,
        level_group,
        candle.footprint.poc_price,
    ) else {
        return;
    };
    if grouped_levels.is_empty() {
        return;
    }
    let grouped_tick_raw = i128::from(min_tick_raw).saturating_mul(level_group.max(1) as i128);
    let grouped_tick_size = grouped_tick_raw as f64 / price_divisor;
    let center = (time_start + time_end) * 0.5;
    let max_cell_width = duration * 0.38;
    let max_quantity = grouped_levels
        .iter()
        .map(|level| level.buy_qty.max(level.sell_qty))
        .max()
        .unwrap_or(0) as f64;

    let row_half_height = (grouped_tick_size * 0.5).max(1.0 / price_divisor);
    let low = candle.low as f64 / price_divisor;
    let high = candle.high as f64 / price_divisor;
    let y_min = low.min(high) - row_half_height * 2.0;
    let y_max = low.max(high) + row_half_height * 2.0;
    let body_low = candle.open.min(candle.close) as f64 / price_divisor;
    let body_high = (candle.open.max(candle.close) as f64 / price_divisor)
        .max(body_low + row_half_height * 0.25);
    let body_width = duration * 0.06;
    let fit_memory_id =
        ui.make_persistent_id(egui::IdSalt::new(("footprint-auto-fit", feed_id, chart_id)));
    let fit_signature = format!(
        "{}:{}:{}:{:?}",
        candle.timestamp_ms, candle.end_timestamp_ms, level_group, candle.coverage.revisions
    );
    let previous_fit = ui.ctx().data(|data| data.get_temp::<String>(fit_memory_id));
    let should_auto_fit = previous_fit.as_deref() != Some(fit_signature.as_str());
    if should_auto_fit {
        ui.ctx()
            .data_mut(|data| data.insert_temp(fit_memory_id, fit_signature));
    }

    let mut plot = Plot::new(("footprint", feed_id, chart_id))
        .height(ui.available_height().max(140.0))
        .default_x_bounds(time_start, time_end)
        .default_y_bounds(y_min, y_max)
        .auto_bounds(egui::Vec2b::new(false, false))
        .grid_color(GRID)
        .y_axis_position(HPlacement::Right)
        .show_axes(egui::Vec2b::new(true, true))
        .show_x(true)
        .allow_zoom(false)
        .allow_drag(false)
        .allow_scroll(false)
        .allow_axis_zoom_drag(false)
        .allow_boxed_zoom(false);
    if should_auto_fit {
        plot = plot.reset();
    }
    plot.show(ui, |plot_ui| {
        plot_ui.hline(
            HLine::new(
                "VAH",
                candle.footprint.value_area_high as f64 / price_divisor,
            )
            .color(GOLD)
            .width(1.0)
            .allow_hover(false),
        );
        plot_ui.hline(
            HLine::new(
                "VAL",
                candle.footprint.value_area_low as f64 / price_divisor,
            )
            .color(GOLD)
            .width(1.0)
            .allow_hover(false),
        );
        plot_ui.line(
            Line::new(
                format!("footprint-wick-{chart_id}"),
                PlotPoints::from(vec![
                    [center, low],
                    [center, high.max(low + grouped_tick_size * 0.1)],
                ]),
            )
            .color(candle_color(candle.open, candle.close))
            .width(1.4)
            .allow_hover(false),
        );

        let body_color = candle_color(candle.open, candle.close);
        let body_fill = candle_body_fill_color(candle.open, candle.close);
        plot_ui.polygon(
            Polygon::new(
                format!("footprint-body-{chart_id}"),
                vec![
                    [center - body_width * 0.5, body_low],
                    [center + body_width * 0.5, body_low],
                    [center + body_width * 0.5, body_high],
                    [center - body_width * 0.5, body_high],
                ],
            )
            .fill_color(body_fill)
            .stroke(Stroke::new(1.0, body_color))
            .allow_hover(false),
        );

        let y_bounds = plot_ui.plot_bounds().range_y();
        let first_visible = grouped_levels
            .partition_point(|level| level.price_high as f64 / price_divisor < *y_bounds.start());
        let last_visible = grouped_levels
            .partition_point(|level| level.price_low as f64 / price_divisor <= *y_bounds.end());
        for (index, level) in grouped_levels[first_visible..last_visible]
            .iter()
            .enumerate()
            .map(|(index, level)| (first_visible + index, level))
        {
            let row_low = level.price_low as f64 / price_divisor;
            let row_high = level.price_high as f64 / price_divisor;
            let price = (row_low + row_high) * 0.5;
            if level.is_poc {
                plot_ui.polygon(
                    Polygon::new(
                        format!("footprint-poc-{chart_id}-{index}"),
                        vec![
                            [center - max_cell_width, row_low],
                            [center + max_cell_width, row_low],
                            [center + max_cell_width, row_high],
                            [center - max_cell_width, row_high],
                        ],
                    )
                    .fill_color(Color32::from_rgba_unmultiplied(255, 215, 0, 38))
                    .stroke(Stroke::NONE)
                    .allow_hover(false),
                );
            }

            if max_quantity > 0.0 {
                let sell_width =
                    max_cell_width * (level.sell_qty as f64 / max_quantity).clamp(0.0, 1.0);
                let buy_width =
                    max_cell_width * (level.buy_qty as f64 / max_quantity).clamp(0.0, 1.0);
                if sell_width > 0.0 {
                    plot_ui.polygon(
                        Polygon::new(
                            format!("footprint-sell-{chart_id}-{index}"),
                            vec![
                                [center - sell_width, row_low],
                                [center, row_low],
                                [center, row_high],
                                [center - sell_width, row_high],
                            ],
                        )
                        .fill_color(PROFILE_BEARISH)
                        .stroke(Stroke::NONE)
                        .allow_hover(false),
                    );
                }
                if buy_width > 0.0 {
                    plot_ui.polygon(
                        Polygon::new(
                            format!("footprint-buy-{chart_id}-{index}"),
                            vec![
                                [center, row_low],
                                [center + buy_width, row_low],
                                [center + buy_width, row_high],
                                [center, row_high],
                            ],
                        )
                        .fill_color(PROFILE_BULLISH)
                        .stroke(Stroke::NONE)
                        .allow_hover(false),
                    );
                }
            }

            let sell_text_x = center - max_cell_width * 0.55;
            let buy_text_x = center + max_cell_width * 0.55;
            let sell_color = if level.is_poc { GOLD } else { PROFILE_BEARISH };
            let buy_color = if level.is_poc { GOLD } else { PROFILE_BULLISH };
            plot_ui.text(
                Text::new(
                    format!("footprint-sell-value-{chart_id}-{index}"),
                    PlotPoint::new(sell_text_x, price),
                    format_quantity(level.sell_qty, candle.quantity_scale),
                )
                .color(sell_color)
                .allow_hover(false),
            );
            plot_ui.text(
                Text::new(
                    format!("footprint-buy-value-{chart_id}-{index}"),
                    PlotPoint::new(buy_text_x, price),
                    format_quantity(level.buy_qty, candle.quantity_scale),
                )
                .color(buy_color)
                .allow_hover(false),
            );
        }
    });
}

fn candle_color(open: i64, close: i64) -> Color32 {
    if close >= open { LIME } else { PROFILE_BEARISH }
}

fn candle_body_fill_color(open: i64, close: i64) -> Color32 {
    let color = candle_color(open, close);
    Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), CANDLE_BODY_ALPHA)
}

fn format_price(value: i64, scale: u32) -> String {
    format_scaled_i128(i128::from(value), scale)
}

fn format_signed_quantity(value: i128, scale: u32) -> String {
    let sign = if value > 0 {
        "+"
    } else if value < 0 {
        "−"
    } else {
        ""
    };
    format!("{sign}{}", format_scaled_u128(value.unsigned_abs(), scale))
}

fn format_quantity(value: u128, scale: u32) -> String {
    format_scaled_u128(value, scale)
}

fn format_scaled_i128(value: i128, scale: u32) -> String {
    let sign = if value < 0 { "−" } else { "" };
    format!("{sign}{}", format_scaled_u128(value.unsigned_abs(), scale))
}

fn format_scaled_u128(value: u128, scale: u32) -> String {
    let divisor = 10_u128.checked_pow(scale).unwrap_or(u128::MAX);
    let integer = value / divisor;
    let fraction = value % divisor;
    if fraction == 0 || scale == 0 {
        return integer.to_string();
    }
    let mut fraction_text = format!("{fraction:0width$}", width = scale as usize);
    while fraction_text.ends_with('0') {
        fraction_text.pop();
    }
    format!("{integer}.{fraction_text}")
}

fn unix_time_ms() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};

    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u64::MAX as u128) as u64
}

#[cfg(test)]
mod tests {
    use super::{
        aggregate_display_levels, candle_body_fill_color, candle_color, delta_color, format_price,
        format_quantity, format_signed_quantity,
    };
    use crate::charts::footprint::FootprintLevel;
    use crate::theme::{LIME, PROFILE_BEARISH, PROFILE_BULLISH, TEXT};
    use eframe::egui::Color32;

    #[test]
    fn formats_schema_scaled_values_without_losing_precision() {
        assert_eq!(format_price(123_456_789, 8), "1.23456789");
        assert_eq!(
            format_quantity(9_007_199_254_740_993, 8),
            "90071992.54740993"
        );
        assert_eq!(format_signed_quantity(-125_000_000, 8), "−1.25");
        assert_eq!(format_quantity(100_000_000, 8), "1");
    }

    #[test]
    fn groups_adjacent_price_levels_and_accumulates_both_sides() {
        let levels = vec![
            FootprintLevel {
                price: 100,
                buy_qty: 1,
                sell_qty: 2,
                ..Default::default()
            },
            FootprintLevel {
                price: 101,
                buy_qty: 3,
                sell_qty: 4,
                ..Default::default()
            },
            FootprintLevel {
                price: 102,
                buy_qty: 5,
                sell_qty: 6,
                ..Default::default()
            },
            FootprintLevel {
                price: 104,
                buy_qty: 7,
                sell_qty: 8,
                ..Default::default()
            },
        ];
        let grouped = aggregate_display_levels(&levels, 1, 4, 101).unwrap();

        assert_eq!(grouped.len(), 2);
        assert_eq!((grouped[0].price_low, grouped[0].price_high), (100, 104));
        assert_eq!((grouped[0].buy_qty, grouped[0].sell_qty), (9, 12));
        assert!(grouped[0].is_poc);
        assert_eq!((grouped[1].buy_qty, grouped[1].sell_qty), (7, 8));
        assert!(!grouped[1].is_poc);
    }

    #[test]
    fn footprint_sides_use_blue_and_red_while_candles_use_green_and_red() {
        assert_eq!(candle_color(100, 101), LIME);
        assert_eq!(candle_color(100, 99), PROFILE_BEARISH);
        assert_eq!(
            candle_body_fill_color(100, 101),
            Color32::from_rgba_unmultiplied(0, 255, 0, 96)
        );
        assert_eq!(
            candle_body_fill_color(100, 99),
            Color32::from_rgba_unmultiplied(255, 0, 0, 96)
        );
        assert_eq!(delta_color(1), PROFILE_BULLISH);
        assert_eq!(delta_color(-1), PROFILE_BEARISH);
        assert_eq!(delta_color(0), TEXT);
    }
}

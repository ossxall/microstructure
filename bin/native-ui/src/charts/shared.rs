use eframe::egui::{self, RichText};

use crate::{market::BarSnapshot, theme::MUTED};

pub(super) fn show_waiting(ui: &mut egui::Ui) {
    ui.centered_and_justified(|ui| {
        ui.label(RichText::new("Esperando history/live…").color(MUTED));
    });
}

pub(super) fn timeframe_seconds(timeframe: &str) -> f64 {
    match timeframe {
        "1m" => 60.0,
        "5m" => 300.0,
        "15m" => 900.0,
        "30m" => 1_800.0,
        "1h" => 3_600.0,
        "4h" => 14_400.0,
        "1d" => 86_400.0,
        _ => 60.0,
    }
}

pub(super) fn x_bounds(bars: &[BarSnapshot], interval: f64) -> (f64, f64) {
    let x_min = bars[0].timestamp_ms as f64 / 1_000.0 - interval * 0.75;
    let x_max = bars[bars.len() - 1].timestamp_ms as f64 / 1_000.0 + interval * 0.75;
    (x_min, x_max.max(x_min + interval))
}

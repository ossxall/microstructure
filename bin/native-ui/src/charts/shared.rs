use std::ops::{Range, RangeInclusive};

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

/// Find the inclusive range of sorted points whose x coordinates are visible.
pub(super) fn visible_range<T>(
    points: &[T],
    x_bounds: &RangeInclusive<f64>,
    x: impl Fn(&T) -> f64,
) -> Range<usize> {
    let start = points.partition_point(|point| x(point) < *x_bounds.start());
    let end = points.partition_point(|point| x(point) <= *x_bounds.end());
    start..end
}

#[cfg(test)]
mod tests {
    use super::visible_range;

    #[test]
    fn visible_range_includes_points_on_both_bounds() {
        let points = [(1.0, "first"), (2.0, "middle"), (3.0, "last")];

        assert_eq!(visible_range(&points, &(1.0..=3.0), |point| point.0), 0..3);
        assert_eq!(visible_range(&points, &(1.5..=2.5), |point| point.0), 1..2);
    }

    #[test]
    fn visible_range_handles_ranges_outside_the_data_and_empty_data() {
        let points = [(1.0, "first"), (2.0, "last")];

        assert_eq!(visible_range(&points, &(-2.0..=0.0), |point| point.0), 0..0);
        assert_eq!(visible_range(&points, &(3.0..=4.0), |point| point.0), 2..2);
        assert_eq!(
            visible_range::<(f64, &str)>(&[], &(1.0..=2.0), |point| point.0),
            0..0
        );
    }
}

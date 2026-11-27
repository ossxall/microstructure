pub(in crate::charts::main) const EMA_55_PERIOD: usize = 55;
pub(in crate::charts::main) const EMA_200_PERIOD: usize = 200;

/// Builds EMA points from ordered `(x, close)` samples.
///
/// The oldest loaded close seeds the EMA so a line is available even before
/// `period` candles have arrived. As more history is loaded, its warm-up
/// naturally improves.
pub(in crate::charts::main) fn points(samples: &[(f64, f64)], period: usize) -> Vec<[f64; 2]> {
    let Some(&(first_x, first_close)) = samples.first() else {
        return Vec::new();
    };
    if period == 0 {
        return Vec::new();
    }

    let alpha = 2.0 / (period as f64 + 1.0);
    let mut ema = first_close;
    let mut points = Vec::with_capacity(samples.len());
    points.push([first_x, ema]);

    for &(x, close) in &samples[1..] {
        ema += alpha * (close - ema);
        points.push([x, ema]);
    }

    points
}

#[cfg(test)]
mod tests {
    use super::points;

    #[test]
    fn ema_uses_the_period_alpha_and_oldest_close_as_its_seed() {
        let values = [(1.0, 10.0), (2.0, 12.0), (3.0, 14.0), (4.0, 16.0)];

        assert_eq!(
            points(&values, 3),
            vec![[1.0, 10.0], [2.0, 11.0], [3.0, 12.5], [4.0, 14.25]]
        );
    }

    #[test]
    fn ema_returns_no_points_for_an_empty_input_or_zero_period() {
        assert!(points(&[], 55).is_empty());
        assert!(points(&[(1.0, 10.0)], 0).is_empty());
    }
}

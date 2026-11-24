// BLACKER
// Copyright (C) 2026 Juan José Caballero Rey
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation version 3 of the License.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License
// along with this program. If not, see <https://www.gnu.org/licenses/>.

use anyhow::{Context, Result, ensure};
use rust_decimal::Decimal;
use serde::Serialize;

/// Converts a positive exchange decimal to the repository's fixed 1e-8 scale
/// without rounding away source precision.
pub fn decimal_to_scaled_u64(value: &str) -> Result<u64> {
    let decimal: Decimal = value.parse().context("invalid decimal value")?;
    ensure!(decimal > Decimal::ZERO, "decimal value must be positive");

    let scale = decimal.scale();
    let mantissa = decimal.mantissa();
    let scaled = if scale <= 8 {
        mantissa
            .checked_mul(10_i128.pow(8 - scale))
            .context("scaled decimal value overflows")?
    } else {
        let divisor = 10_i128
            .checked_pow(scale - 8)
            .context("decimal precision divisor overflows")?;
        ensure!(
            mantissa % divisor == 0,
            "decimal value has more than 8 significant fractional places"
        );
        mantissa / divisor
    };

    u64::try_from(scaled).context("scaled decimal value is outside the u64 range")
}

#[derive(Debug, Clone, Serialize)]
pub struct Tick {
    pub source: String,
    pub symbol: String,
    pub id: String,
    pub time: u64,
    pub price: u64,
    pub qty: u64,
    pub is_buyer_maker: u8,
}

#[cfg(test)]
mod tests {
    use super::decimal_to_scaled_u64;

    #[test]
    fn converts_exact_values_at_the_fixed_scale() {
        assert_eq!(
            decimal_to_scaled_u64("60900.70").unwrap(),
            6_090_070_000_000
        );
        assert_eq!(decimal_to_scaled_u64("0.00100000").unwrap(), 100_000);
        assert_eq!(decimal_to_scaled_u64("1.1234567800").unwrap(), 112_345_678);
        assert_eq!(
            decimal_to_scaled_u64("1.12345678000000000000").unwrap(),
            112_345_678
        );
    }

    #[test]
    fn rejects_values_that_need_rounding_or_are_not_positive() {
        assert!(decimal_to_scaled_u64("1.000000001").is_err());
        assert!(decimal_to_scaled_u64("0").is_err());
        assert!(decimal_to_scaled_u64("0.00000000").is_err());
        assert!(decimal_to_scaled_u64("-1").is_err());
    }
}

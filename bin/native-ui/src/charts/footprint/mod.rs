mod indicators;
mod render;

use crate::market::{BarFields, ChartDataRequest, ChartSnapshot};

pub use indicators::{FootprintCandle, FootprintIndicatorData, FootprintLevel};
pub(super) use render::show;

pub const TITLE: &str = "FOOTPRINT";
pub const MENU_LABEL: &str = "Footprint";
pub const DEFAULT_TIMEFRAME_MS: u64 = 3_600_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TimeframeOption {
    pub label: &'static str,
    pub millis: u64,
}

const TIMEFRAMES: &[TimeframeOption] = &[
    TimeframeOption {
        label: "1m",
        millis: 60_000,
    },
    TimeframeOption {
        label: "2m",
        millis: 120_000,
    },
    TimeframeOption {
        label: "3m",
        millis: 180_000,
    },
    TimeframeOption {
        label: "5m",
        millis: 300_000,
    },
    TimeframeOption {
        label: "10m",
        millis: 600_000,
    },
    TimeframeOption {
        label: "15m",
        millis: 900_000,
    },
    TimeframeOption {
        label: "20m",
        millis: 1_200_000,
    },
    TimeframeOption {
        label: "30m",
        millis: 1_800_000,
    },
    TimeframeOption {
        label: "45m",
        millis: 2_700_000,
    },
    TimeframeOption {
        label: "1h",
        millis: 3_600_000,
    },
    TimeframeOption {
        label: "90m",
        millis: 5_400_000,
    },
    TimeframeOption {
        label: "2h",
        millis: 7_200_000,
    },
    TimeframeOption {
        label: "3h",
        millis: 10_800_000,
    },
    TimeframeOption {
        label: "4h",
        millis: 14_400_000,
    },
    TimeframeOption {
        label: "6h",
        millis: 21_600_000,
    },
    TimeframeOption {
        label: "8h",
        millis: 28_800_000,
    },
    TimeframeOption {
        label: "12h",
        millis: 43_200_000,
    },
    TimeframeOption {
        label: "1d",
        millis: 86_400_000,
    },
];

pub fn timeframe_options(source_timeframe: &str) -> impl Iterator<Item = TimeframeOption> + '_ {
    let source_ms = timeframe_millis(source_timeframe);
    TIMEFRAMES.iter().copied().filter(move |option| {
        source_ms.is_some_and(|source_ms| is_valid_target(source_ms, option.millis))
    })
}

pub fn timeframe_label(timeframe_ms: u64) -> String {
    if let Some(label) = TIMEFRAMES
        .iter()
        .find(|option| option.millis == timeframe_ms)
        .map(|option| option.label)
    {
        return label.to_owned();
    }
    if timeframe_ms.is_multiple_of(86_400_000) {
        format!("{}d", timeframe_ms / 86_400_000)
    } else if timeframe_ms.is_multiple_of(3_600_000) {
        format!("{}h", timeframe_ms / 3_600_000)
    } else if timeframe_ms.is_multiple_of(60_000) {
        format!("{}m", timeframe_ms / 60_000)
    } else {
        format!("{timeframe_ms}ms")
    }
}

pub fn default_timeframe(source_timeframe: &str) -> u64 {
    if timeframe_options(source_timeframe).any(|option| option.millis == DEFAULT_TIMEFRAME_MS) {
        DEFAULT_TIMEFRAME_MS
    } else {
        timeframe_options(source_timeframe)
            .next()
            .map_or(DEFAULT_TIMEFRAME_MS, |option| option.millis)
    }
}

pub fn data_request(
    history_limit: u64,
    source_timeframe: &str,
    target_timeframe_ms: u64,
) -> ChartDataRequest {
    let required_bars = timeframe_millis(source_timeframe)
        .filter(|source_ms| is_valid_target(*source_ms, target_timeframe_ms))
        .map_or(1, |source_ms| target_timeframe_ms / source_ms + 2);

    ChartDataRequest {
        history_limit: history_limit.max(required_bars),
        live: true,
        fields: BarFields {
            price: true,
            volume_flow: true,
            footprint: true,
            ..Default::default()
        },
    }
}

pub fn calculate(
    snapshot: &ChartSnapshot,
    source: &str,
    symbol: &str,
    source_timeframe: &str,
    target_timeframe_ms: u64,
    now_ms: u64,
) -> FootprintIndicatorData {
    match indicators::calculate(
        &snapshot.full_bars,
        source,
        symbol,
        source_timeframe,
        target_timeframe_ms,
        now_ms,
    ) {
        Ok(candle) => FootprintIndicatorData {
            target_timeframe_ms,
            candle,
            error: None,
        },
        Err(error) => FootprintIndicatorData {
            target_timeframe_ms,
            candle: None,
            error: Some(error),
        },
    }
}

pub(crate) fn timeframe_millis(timeframe: &str) -> Option<u64> {
    match timeframe {
        "1m" => Some(60_000),
        "5m" => Some(300_000),
        "15m" => Some(900_000),
        "30m" => Some(1_800_000),
        "1h" => Some(3_600_000),
        "4h" => Some(14_400_000),
        "1d" => Some(86_400_000),
        _ => None,
    }
}

pub(crate) fn is_valid_timeframe(source_timeframe: &str, target_ms: u64) -> bool {
    timeframe_millis(source_timeframe)
        .is_some_and(|source_ms| is_valid_target(source_ms, target_ms))
}

fn is_valid_target(source_ms: u64, target_ms: u64) -> bool {
    source_ms > 0 && target_ms >= source_ms && target_ms.is_multiple_of(source_ms)
}

#[cfg(test)]
mod tests {
    use super::{
        DEFAULT_TIMEFRAME_MS, data_request, default_timeframe, timeframe_label, timeframe_options,
    };
    use crate::market::BarFields;

    #[test]
    fn one_hour_is_the_default_when_the_tab_timeframe_can_resample_to_it() {
        assert_eq!(default_timeframe("5m"), DEFAULT_TIMEFRAME_MS);
        assert_eq!(default_timeframe("4h"), 14_400_000);
    }

    #[test]
    fn timeframe_options_only_include_integral_multiples_of_the_tab_timeframe() {
        let options = timeframe_options("5m").collect::<Vec<_>>();

        assert!(options.iter().any(|option| option.label == "1h"));
        assert!(!options.iter().any(|option| option.label == "2m"));
        assert!(options.iter().any(|option| option.label == "1d"));
    }

    #[test]
    fn footprint_request_uses_the_current_feed_and_enough_history_for_target() {
        let request = data_request(10, "5m", 3_600_000);

        assert_eq!(request.history_limit, 14);
        assert!(request.live);
        assert_eq!(
            request.fields,
            BarFields {
                price: true,
                volume_flow: true,
                footprint: true,
                ..Default::default()
            }
        );
    }

    #[test]
    fn custom_timeframes_are_supported_when_they_are_integral_source_multiples() {
        let request = data_request(1, "1m", 7 * 60_000);

        assert_eq!(request.history_limit, 9);
        assert_eq!(timeframe_label(7 * 60_000), "7m");
        assert!(!timeframe_options("5m").any(|option| option.millis == 7 * 60_000));
    }

    #[test]
    fn custom_timeframes_must_be_exact_multiples_at_or_above_the_source() {
        assert!(super::is_valid_timeframe("5m", 25 * 60_000));
        assert!(!super::is_valid_timeframe("5m", 26 * 60_000));
        assert!(!super::is_valid_timeframe("5m", 4 * 60_000));
    }
}

pub const DEFAULT_TIMEFRAME: &str = "1m";

pub fn bars_table_name(timeframe: &str) -> Option<&'static str> {
    match timeframe {
        "1m" => Some("bars_1m"),
        "5m" => Some("bars_5m"),
        "15m" => Some("bars_15m"),
        "30m" => Some("bars_30m"),
        "1h" => Some("bars_1h"),
        "4h" => Some("bars_4h"),
        "1d" => Some("bars_1d"),
        _ => None,
    }
}

pub fn topic_name(namespace: &str, source: &str, symbol: &str, timeframe: &str) -> Option<String> {
    if bars_table_name(timeframe).is_none()
        || !is_topic_segment(source)
        || !is_topic_segment(symbol)
    {
        return None;
    }

    Some(format!(
        "{}/bars-{source}-{symbol}-{timeframe}",
        namespace.trim_end_matches('/')
    ))
}

fn is_topic_segment(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

#[cfg(test)]
mod tests {
    use super::{bars_table_name, topic_name};

    #[test]
    fn builds_a_topic_per_source_symbol_and_timeframe() {
        assert_eq!(
            topic_name("persistent://public/default", "binance", "BTCUSDT", "1m"),
            Some("persistent://public/default/bars-binance-BTCUSDT-1m".to_string())
        );
        assert_eq!(bars_table_name("5m"), Some("bars_5m"));
    }

    #[test]
    fn rejects_unsupported_timeframes_and_topic_segments() {
        assert_eq!(
            topic_name("persistent://public/default", "binance", "BTC/USDT", "1m"),
            None
        );
        assert_eq!(
            topic_name("persistent://public/default", "binance", "BTCUSDT", "2m"),
            None
        );
    }
}

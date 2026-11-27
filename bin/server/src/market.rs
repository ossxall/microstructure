use std::path::{Path, PathBuf};

pub const DEFAULT_TIMEFRAME: &str = "1m";
pub const SUPPORTED_TIMEFRAMES: [&str; 7] = ["1m", "5m", "15m", "30m", "1h", "4h", "1d"];

pub fn bar_store_path(base: &Path, timeframe: &str) -> PathBuf {
    if timeframe == DEFAULT_TIMEFRAME {
        return base.to_path_buf();
    }
    let Some(file_name) = base.file_stem().and_then(|name| name.to_str()) else {
        return base.to_path_buf();
    };
    let name = match base.extension().and_then(|extension| extension.to_str()) {
        Some(extension) => format!("{file_name}-{timeframe}.{extension}"),
        None => format!("{file_name}-{timeframe}"),
    };
    base.with_file_name(name)
}

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
    use super::{bar_store_path, bars_table_name, topic_name};
    use std::path::Path;

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

    #[test]
    fn bar_store_paths_match_aggregator_timeframe_isolation() {
        assert_eq!(
            bar_store_path(Path::new("/data/bars"), "1m"),
            Path::new("/data/bars")
        );
        assert_eq!(
            bar_store_path(Path::new("/data/bars"), "5m"),
            Path::new("/data/bars-5m")
        );
        assert_eq!(
            bar_store_path(Path::new("/data/bars.store"), "5m"),
            Path::new("/data/bars-5m.store")
        );
    }
}

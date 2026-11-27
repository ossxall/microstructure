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

pub fn is_supported_timeframe(timeframe: &str) -> bool {
    SUPPORTED_TIMEFRAMES.contains(&timeframe)
}

pub fn topic_name(namespace: &str, source: &str, symbol: &str, timeframe: &str) -> Option<String> {
    if !is_supported_timeframe(timeframe) || !is_topic_segment(source) || !is_topic_segment(symbol)
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
    use super::{bar_store_path, is_supported_timeframe, topic_name};
    use std::path::Path;

    #[test]
    fn builds_a_topic_per_source_symbol_and_timeframe() {
        assert_eq!(
            topic_name("persistent://public/default", "binance", "BTCUSDT", "1m"),
            Some("persistent://public/default/bars-binance-BTCUSDT-1m".to_string())
        );
        assert!(is_supported_timeframe("5m"));
    }

    #[test]
    fn rejects_unsupported_timeframes_and_topic_segments() {
        assert_eq!(
            topic_name("persistent://public/default", "binance", "BTC/USDT", "1m"),
            None
        );
        assert!(!is_supported_timeframe("2m"));
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

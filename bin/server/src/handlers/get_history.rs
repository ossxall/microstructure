use anyhow::{Context, Result as AnyResult};
use axum::{Json, extract::State, http::StatusCode};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::{
    market::{DEFAULT_TIMEFRAME, is_supported_timeframe},
    state::AppState,
};

fn default_timeframe() -> String {
    DEFAULT_TIMEFRAME.to_string()
}

#[derive(Debug, Deserialize)]
pub struct HistoryRequest {
    pub source: String,
    pub symbol: String,
    #[serde(default = "default_timeframe")]
    pub timeframe: String,
    pub limit: u64,
    #[serde(default = "default_include_raw_ticks")]
    pub include_raw_ticks: bool,
}

fn default_include_raw_ticks() -> bool {
    true
}

pub async fn handler(
    State(state): State<AppState>,
    Json(request): Json<HistoryRequest>,
) -> std::result::Result<Json<Value>, (StatusCode, String)> {
    if request.source.trim().is_empty() || request.symbol.trim().is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            "source and symbol must not be empty".to_string(),
        ));
    }
    if !is_supported_timeframe(&request.timeframe) {
        return Err((
            StatusCode::BAD_REQUEST,
            "timeframe is not supported".to_string(),
        ));
    }
    let limit = usize::try_from(request.limit).map_err(|_| {
        (
            StatusCode::BAD_REQUEST,
            "limit does not fit this server's address space".to_string(),
        )
    })?;

    let bar_store = state.bar_stores.get(&request.timeframe).cloned().ok_or((
        StatusCode::BAD_REQUEST,
        "no Bar store is configured for this timeframe".to_string(),
    ))?;
    let source = request.source;
    let symbol = request.symbol;
    let timeframe = request.timeframe;
    let include_raw_ticks = request.include_raw_ticks;
    let history = tokio::task::spawn_blocking(move || {
        bar_store.refresh()?;
        let bars = bar_store.read_last_n(&source, &symbol, &timeframe, limit)?;
        serialize_history(bars, include_raw_ticks)
    })
    .await
    .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
    .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;

    Ok(Json(json!({
        "success": true,
        "message": "ok",
        "data": {
            "history": history
        }
    })))
}

fn serialize_history(
    mut bars: Vec<aggregator::model::Bar>,
    include_raw_ticks: bool,
) -> AnyResult<Vec<Value>> {
    // BarStore returns newest-first; the API contract is ascending time order.
    bars.reverse();
    bars.into_iter()
        .map(|bar| {
            let mut value =
                serde_json::to_value(bar).context("Failed to encode stored Bar for history")?;
            if !include_raw_ticks {
                strip_raw_ticks(&mut value);
            }
            Ok(value)
        })
        .collect()
}

fn strip_raw_ticks(bar: &mut Value) {
    if let Some(fields) = bar.as_object_mut() {
        fields.remove("raw_ticks");
    }
}

#[cfg(test)]
mod tests {
    use super::{HistoryRequest, serialize_history, strip_raw_ticks};
    use aggregator::model::{Tick, build_bar_for_timeframe};
    use serde_json::json;

    fn bar(timestamp: u64) -> aggregator::model::Bar {
        build_bar_for_timeframe(
            &[Tick {
                time_us: i64::try_from(timestamp * 1_000).unwrap(),
                source: "exchange".to_string(),
                symbol: "BTCUSD".to_string(),
                id: timestamp.to_string(),
                price: 100,
                qty: 1,
                is_buyer_maker: 0,
            }],
            timestamp,
            "1m",
            60_000,
            1,
            1,
        )
        .unwrap()
    }

    #[test]
    fn history_requests_default_to_one_minute() {
        let request: HistoryRequest =
            serde_json::from_str(r#"{"source":"binance","symbol":"BTCUSDT","limit":500}"#).unwrap();

        assert_eq!(request.timeframe, "1m");
        assert!(request.include_raw_ticks);
    }

    #[test]
    fn history_requests_can_omit_raw_ticks() {
        let request: HistoryRequest = serde_json::from_str(
            r#"{"source":"binance","symbol":"BTCUSDT","limit":500,"include_raw_ticks":false}"#,
        )
        .unwrap();

        assert!(!request.include_raw_ticks);
    }

    #[test]
    fn compact_history_omits_raw_ticks_but_preserves_bar_fields() {
        let mut bar = json!({
            "timestamp": "123",
            "raw_ticks": [{"price": "100", "qty": "2"}],
            "footprint": {"levels": [{"price": "100"}]}
        });

        strip_raw_ticks(&mut bar);

        assert!(bar.get("raw_ticks").is_none());
        assert_eq!(bar["timestamp"], "123");
        assert_eq!(bar["footprint"]["levels"][0]["price"], "100");
    }

    #[test]
    fn local_history_is_ascending_and_can_omit_raw_ticks() {
        let history = serialize_history(vec![bar(120_000), bar(60_000)], false).unwrap();

        assert_eq!(history.len(), 2);
        assert_eq!(history[0]["timestamp"], "60000");
        assert_eq!(history[1]["timestamp"], "120000");
        assert!(history[0].get("raw_ticks").is_none());
    }
}

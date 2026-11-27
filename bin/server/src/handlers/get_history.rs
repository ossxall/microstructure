use anyhow::{Context, Result as AnyResult};
use axum::{Json, extract::State, http::StatusCode};
use questdb::{QuestDb, egress::ColumnView};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{collections::BTreeMap, sync::Arc};

use crate::{
    market::{DEFAULT_TIMEFRAME, bars_table_name},
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
    let table = bars_table_name(&request.timeframe).ok_or((
        StatusCode::BAD_REQUEST,
        "timeframe is not supported".to_string(),
    ))?;
    let limit_i64 = i64::try_from(request.limit).map_err(|_| {
        (
            StatusCode::BAD_REQUEST,
            "limit must fit QuestDB's signed 64-bit LIMIT range".to_string(),
        )
    })?;
    let limit = usize::try_from(request.limit).map_err(|_| {
        (
            StatusCode::BAD_REQUEST,
            "limit does not fit this server's address space".to_string(),
        )
    })?;

    let questdb = Arc::clone(&state.questdb);
    let bar_store = state.bar_stores.get(&request.timeframe).cloned().ok_or((
        StatusCode::BAD_REQUEST,
        "no Bar store is configured for this timeframe".to_string(),
    ))?;
    let source = request.source;
    let symbol = request.symbol;
    let timeframe = request.timeframe;
    let include_raw_ticks = request.include_raw_ticks;
    let payloads = tokio::task::spawn_blocking(move || {
        bar_store.refresh()?;
        let stored = bar_store.read_last_n(&source, &symbol, &timeframe, limit)?;
        let local_payloads = stored
            .into_iter()
            .map(serde_json::to_value)
            .collect::<Result<Vec<_>, _>>()
            .context("Failed to encode archived Bars for history")?;

        // QuestDB is retained as a compatibility source while older history is
        // being backfilled into the dedicated store. Local records win on key.
        let legacy_payloads = if local_payloads.len() < limit {
            match query_history(
                &questdb,
                table,
                &source,
                &symbol,
                limit_i64,
                include_raw_ticks,
            ) {
                Ok(payloads) => payloads,
                Err(error) => {
                    tracing::warn!(
                        source,
                        symbol,
                        timeframe,
                        error = %format!("{error:#}"),
                        "QuestDB history fallback failed; serving Bars from the local store"
                    );
                    Vec::new()
                }
            }
        } else {
            Vec::new()
        };
        merge_history(local_payloads, legacy_payloads, limit, include_raw_ticks)
    })
    .await
    .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
    .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;

    Ok(Json(json!({
        "success": true,
        "message": "ok",
        "data": {
            "history": payloads
        }
    })))
}

fn merge_history(
    local_payloads: Vec<Value>,
    legacy_payloads: Vec<Value>,
    limit: usize,
    include_raw_ticks: bool,
) -> AnyResult<Vec<Value>> {
    let mut by_timestamp = BTreeMap::<u64, Value>::new();
    for mut bar in legacy_payloads {
        if !include_raw_ticks {
            strip_raw_ticks(&mut bar);
        }
        by_timestamp.insert(bar_timestamp(&bar)?, bar);
    }
    for mut bar in local_payloads {
        if !include_raw_ticks {
            strip_raw_ticks(&mut bar);
        }
        // Dedicated-store contents take precedence over legacy QuestDB rows.
        by_timestamp.insert(bar_timestamp(&bar)?, bar);
    }
    let skip = by_timestamp.len().saturating_sub(limit);
    Ok(by_timestamp.into_values().skip(skip).collect())
}

fn bar_timestamp(bar: &Value) -> AnyResult<u64> {
    let timestamp = bar
        .get("timestamp")
        .context("Bar history payload has no timestamp")?;
    match timestamp {
        Value::String(timestamp) => timestamp
            .parse()
            .context("Bar history timestamp is not an unsigned integer"),
        Value::Number(timestamp) => timestamp
            .as_u64()
            .context("Bar history timestamp is not an unsigned integer"),
        _ => anyhow::bail!("Bar history timestamp is not an unsigned integer"),
    }
}

fn query_history(
    db: &QuestDb,
    table: &str,
    source: &str,
    symbol: &str,
    limit: i64,
    include_raw_ticks: bool,
) -> AnyResult<Vec<Value>> {
    let mut reader = db
        .borrow_reader()
        .context("Failed to open QuestDB bars reader")?;
    let query = format!(
        "SELECT payload FROM {table} \
         WHERE source = $1 AND symbol = $2 \
         ORDER BY time DESC LIMIT $3"
    );
    let mut cursor = reader
        .prepare(&query)
        .bind_varchar(source)
        .bind_varchar(symbol)
        .bind_i64(limit)
        .execute()
        .context("Failed to query QuestDB bars")?;

    let mut history = Vec::new();
    while let Some(batch) = cursor
        .next_batch()
        .context("Failed while reading QuestDB bars")?
    {
        let values = match batch.column(0)? {
            ColumnView::Varchar(values) => values,
            _ => anyhow::bail!("QuestDB bar payload is not VARCHAR"),
        };
        for row in 0..batch.row_count() {
            let payload = values
                .value(row)
                .context("QuestDB returned a null bar payload")?;
            let decoded_payload = crate::payload_codec::decode(payload)
                .context("QuestDB bar payload could not be decompressed")?;
            let mut bar = serde_json::from_str::<Value>(&decoded_payload)
                .context("QuestDB returned an invalid bar payload")?;
            if !include_raw_ticks {
                strip_raw_ticks(&mut bar);
            }
            history.push(bar);
        }
    }

    // The query selects the most recent `limit` rows first; return the
    // retained history in ascending time order for chart consumers.
    history.reverse();
    Ok(history)
}

fn strip_raw_ticks(bar: &mut Value) {
    if let Some(fields) = bar.as_object_mut() {
        fields.remove("raw_ticks");
    }
}

#[cfg(test)]
mod tests {
    use super::{HistoryRequest, merge_history, strip_raw_ticks};
    use serde_json::json;

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
    fn history_merge_deduplicates_and_returns_the_latest_bars_in_time_order() {
        let legacy = vec![
            json!({"timestamp": "1000", "source": "exchange", "raw_ticks": [1]}),
            json!({"timestamp": "2000", "source": "exchange", "value": "old"}),
        ];
        let local = vec![
            json!({"timestamp": "2000", "source": "exchange", "value": "new", "raw_ticks": [2]}),
            json!({"timestamp": "3000", "source": "exchange", "value": "latest"}),
        ];

        let history = merge_history(local, legacy, 2, false).unwrap();
        assert_eq!(history.len(), 2);
        assert_eq!(history[0]["timestamp"], "2000");
        assert_eq!(history[0]["value"], "new");
        assert_eq!(history[1]["timestamp"], "3000");
        assert!(history[0].get("raw_ticks").is_none());
    }
}

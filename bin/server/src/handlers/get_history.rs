use anyhow::{Context, Result as AnyResult};
use axum::{Json, extract::State, http::StatusCode};
use questdb::{QuestDb, egress::ColumnView};
use serde::Deserialize;
use serde_json::{Value, json};
use std::sync::Arc;

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
    let limit = i64::try_from(request.limit).map_err(|_| {
        (
            StatusCode::BAD_REQUEST,
            "limit must fit QuestDB's signed 64-bit LIMIT range".to_string(),
        )
    })?;

    let questdb = Arc::clone(&state.questdb);
    let source = request.source;
    let symbol = request.symbol;
    let payloads = tokio::task::spawn_blocking(move || {
        query_history(&questdb, table, &source, &symbol, limit)
    })
    .await
    .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
    .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;

    let history = payloads
        .iter()
        .map(|payload| serde_json::from_str::<Value>(payload))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;

    Ok(Json(json!({
        "success": true,
        "message": "ok",
        "data": {
            "history": history
        }
    })))
}

fn query_history(
    db: &QuestDb,
    table: &str,
    source: &str,
    symbol: &str,
    limit: i64,
) -> AnyResult<Vec<String>> {
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

    let mut payloads = Vec::new();
    while let Some(batch) = cursor
        .next_batch()
        .context("Failed while reading QuestDB bars")?
    {
        let values = match batch.column(0)? {
            ColumnView::Varchar(values) => values,
            _ => anyhow::bail!("QuestDB bar payload is not VARCHAR"),
        };
        for row in 0..batch.row_count() {
            payloads.push(
                values
                    .value(row)
                    .context("QuestDB returned a null bar payload")?
                    .to_string(),
            );
        }
    }

    // The query selects the most recent `limit` rows first; return the
    // retained history in ascending time order for chart consumers.
    payloads.reverse();
    Ok(payloads)
}

#[cfg(test)]
mod tests {
    use super::HistoryRequest;

    #[test]
    fn history_requests_default_to_one_minute() {
        let request: HistoryRequest =
            serde_json::from_str(r#"{"source":"binance","symbol":"BTCUSDT","limit":500}"#).unwrap();

        assert_eq!(request.timeframe, "1m");
    }
}

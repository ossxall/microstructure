use std::{
    collections::BTreeMap,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
        mpsc::{Receiver, SyncSender},
    },
    thread,
};

use bytes::Bytes;
use eframe::egui;
use serde::Deserialize;
use serde_json::Value;
use tokio::sync::{mpsc::UnboundedSender, watch};

use crate::app_log::AppLogger;
use crate::network::NetworkCommand;

pub const DEFAULT_HISTORY_LIMIT: u64 = 50;

#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub struct FeedKey {
    pub source: String,
    pub symbol: String,
    pub timeframe: String,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BarFields {
    pub price: bool,
    pub volume_flow: bool,
    pub footprint: bool,
    pub raw_ticks: bool,
    pub complete_bar: bool,
}

#[derive(Clone, Debug)]
pub struct ChartDataRequest {
    pub history_limit: u64,
    pub live: bool,
    pub fields: BarFields,
}

#[derive(Clone, Copy, Debug)]
pub struct Candle {
    pub timestamp_ms: u64,
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
    pub volume: f64,
}

impl Candle {
    pub fn is_bullish(self) -> bool {
        self.close >= self.open
    }
}

#[derive(Clone, Debug)]
pub enum FeedStatus {
    Loading,
    Connecting,
    Live,
    Disconnected,
    Error(String),
}

#[derive(Clone, Debug)]
pub struct ChartSnapshot {
    /// Increments whenever a new market snapshot is published.
    pub generation: u64,
    pub status: FeedStatus,
    pub history_error: Option<String>,
    /// Complete, reconciled Bars shared by every Chart in this feed.
    pub full_bars: Arc<[BarSnapshot]>,
}

/// Immutable view of a complete Bar published to the UI.
///
/// `payload` remains shared with the calculation worker, so publishing a
/// snapshot clones only its `Arc`, not the full JSON value.
#[derive(Clone, Debug)]
pub struct BarSnapshot {
    pub timestamp_ms: u64,
    pub candle: Candle,
    pub revision: u64,
    pub is_final: bool,
    /// True when the feed supplied different contents for the same Bar
    /// identity and revision.
    pub revision_conflict: bool,
    payload: Arc<Value>,
}

impl BarSnapshot {
    /// Returns the complete Bar attributes, unwrapping the `data` envelope
    /// used by live WebSocket events.
    pub fn attributes(&self) -> &Value {
        self.payload
            .get("data")
            .filter(|value| value.is_object())
            .unwrap_or(&self.payload)
    }

    /// Returns the original complete history Bar or live WebSocket event.
    pub fn source_payload(&self) -> &Value {
        &self.payload
    }
}

impl Default for ChartSnapshot {
    fn default() -> Self {
        Self {
            generation: 0,
            status: FeedStatus::Loading,
            history_error: None,
            full_bars: Arc::from([]),
        }
    }
}

pub enum CalculationInput {
    Live(String),
    History(Result<Bytes, String>),
    Status(FeedStatus),
    ChartRequests(Vec<ChartDataRequest>),
}

pub struct CalculationWorkerConfig {
    pub feed_id: u64,
    pub requests: Vec<ChartDataRequest>,
    pub receiver: Receiver<CalculationInput>,
    pub snapshot_tx: watch::Sender<Arc<ChartSnapshot>>,
    pub indicator_update_tx: SyncSender<()>,
    pub network_tx: UnboundedSender<NetworkCommand>,
    pub egui_ctx: egui::Context,
    pub active_feed_id: Arc<AtomicU64>,
    pub feed_key: FeedKey,
    pub logger: AppLogger,
}

struct StoredBar {
    /// Retain the original Bar/event JSON in full. Charts derive only the values
    /// they need from it; no fields are removed at the network boundary.
    payload: Arc<Value>,
    candle: Candle,
    revision: u64,
    is_final: bool,
    revision_conflict: bool,
    event_id: Option<String>,
    trade_count: Option<u64>,
    checksum: Option<String>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum DecimalU64 {
    Number(u64),
    String(String),
}

impl DecimalU64 {
    fn value(&self) -> Option<u64> {
        match self {
            Self::Number(value) => Some(*value),
            Self::String(value) => value.parse().ok(),
        }
    }
}

#[derive(Deserialize)]
struct LiveBarIndex {
    timestamp: Option<DecimalU64>,
    revision: Option<DecimalU64>,
}

#[derive(Deserialize)]
struct LiveEventIndex {
    timestamp: Option<DecimalU64>,
    revision: Option<DecimalU64>,
    is_final: Option<bool>,
    data: Option<LiveBarIndex>,
}

pub fn spawn_calculation_worker(config: CalculationWorkerConfig) -> thread::JoinHandle<()> {
    let CalculationWorkerConfig {
        feed_id,
        requests,
        receiver,
        snapshot_tx,
        indicator_update_tx,
        network_tx,
        egui_ctx,
        active_feed_id,
        feed_key,
        logger,
    } = config;
    thread::Builder::new()
        .name(format!("chart-calc-{feed_id}"))
        .spawn(move || {
            let mut requests = requests;
            let history_limit = requested_history_limit(&requests);

            // The calculation layer requests the history required by its Charts.
            let _ = network_tx.send(NetworkCommand::FetchHistory {
                feed_id,
                history_limit,
            });

            let mut bars = BTreeMap::<u64, StoredBar>::new();
            let mut status = FeedStatus::Loading;
            let mut history_error = None;
            let mut generation = 0_u64;

            while let Ok(first) = receiver.recv() {
                let previous_history_limit = requested_history_limit(&requests);
                let previous_live_request = requests.iter().any(|request| request.live);
                let mut batch = vec![first];
                batch.extend(receiver.try_iter());
                let batch = coalesce_live_snapshots(batch);
                let mut dirty = false;

                // Process history/status/config before live snapshots in a batch.
                // Reconciliation uses timestamp, finality, and revision.
                for input in batch {
                    dirty |= apply_input(
                        input,
                        &mut bars,
                        &mut status,
                        &mut history_error,
                        &mut requests,
                        &feed_key,
                        &logger,
                    );
                }

                let history_limit = requested_history_limit(&requests);
                let current_bar_count = u64::try_from(bars.len()).unwrap_or(u64::MAX);
                if history_limit > previous_history_limit && current_bar_count < history_limit {
                    let _ = network_tx.send(NetworkCommand::FetchHistory {
                        feed_id,
                        history_limit,
                    });
                }
                let live_requested = requests.iter().any(|request| request.live);
                if live_requested != previous_live_request {
                    let _ = network_tx.send(NetworkCommand::SetLiveSubscription {
                        feed_id,
                        enabled: live_requested,
                    });
                }

                if dirty {
                    generation = generation.wrapping_add(1);
                    let full_bars = bars.values().map(snapshot_bar).collect::<Vec<_>>();
                    let snapshot = Arc::new(ChartSnapshot {
                        generation,
                        status: status.clone(),
                        history_error: history_error.clone(),
                        full_bars: full_bars.into(),
                    });
                    snapshot_tx.send_replace(snapshot);
                    // The indicator worker reads the latest watch value. A
                    // capacity-one signal coalesces updates while it is busy.
                    let _ = indicator_update_tx.try_send(());

                    if active_feed_id.load(Ordering::Relaxed) == feed_id {
                        egui_ctx.request_repaint();
                    }
                }
            }
        })
        .expect("failed to start chart calculation worker")
}

fn coalesce_live_snapshots(inputs: Vec<CalculationInput>) -> Vec<CalculationInput> {
    let mut passthrough = Vec::new();
    let mut latest_by_timestamp = BTreeMap::<u64, (bool, u64, String)>::new();

    for input in inputs {
        let CalculationInput::Live(payload) = input else {
            passthrough.push(input);
            continue;
        };

        let Some((timestamp, is_final, revision)) = live_event_index(&payload) else {
            passthrough.push(CalculationInput::Live(payload));
            continue;
        };

        let replace = latest_by_timestamp.get(&timestamp).is_none_or(
            |(current_final, current_revision, _)| {
                if *current_final && !is_final {
                    false
                } else if is_final && !current_final {
                    true
                } else if is_final {
                    revision >= *current_revision
                } else {
                    // Pulsar preserves order for a subscription; a live Bar is
                    // a complete snapshot, so only the last pending one matters.
                    true
                }
            },
        );

        if let Some((true, current_revision, current_payload)) = latest_by_timestamp.get(&timestamp)
            && is_final
            && *current_revision == revision
            && !same_bar_attributes_json(current_payload, &payload)
        {
            // Preserve both finalized payloads so reconciliation can report
            // the same-revision conflict rather than hiding it in this batch.
            passthrough.push(CalculationInput::Live(current_payload.clone()));
        }

        if replace {
            latest_by_timestamp.insert(timestamp, (is_final, revision, payload));
        }
    }

    passthrough.extend(
        latest_by_timestamp
            .into_values()
            .map(|(_, _, payload)| CalculationInput::Live(payload)),
    );
    passthrough
}

fn live_event_index(payload: &str) -> Option<(u64, bool, u64)> {
    let event = serde_json::from_str::<LiveEventIndex>(payload).ok()?;
    let data_timestamp = event
        .data
        .as_ref()
        .and_then(|data| data.timestamp.as_ref())
        .and_then(DecimalU64::value);
    let timestamp =
        data_timestamp.or_else(|| event.timestamp.as_ref().and_then(DecimalU64::value))?;
    let data_revision = event
        .data
        .as_ref()
        .and_then(|data| data.revision.as_ref())
        .and_then(DecimalU64::value);
    let revision = data_revision
        .or_else(|| event.revision.as_ref().and_then(DecimalU64::value))
        .unwrap_or_default();
    Some((timestamp, event.is_final.unwrap_or(true), revision))
}

fn apply_input(
    input: CalculationInput,
    bars: &mut BTreeMap<u64, StoredBar>,
    status: &mut FeedStatus,
    history_error: &mut Option<String>,
    requests: &mut Vec<ChartDataRequest>,
    feed_key: &FeedKey,
    logger: &AppLogger,
) -> bool {
    match input {
        CalculationInput::Status(next_status) => {
            *status = next_status;
            true
        }
        CalculationInput::History(Err(error)) => {
            *history_error = Some(error);
            true
        }
        CalculationInput::History(Ok(bytes)) => {
            let mut response = match serde_json::from_slice::<Value>(&bytes) {
                Ok(response) => response,
                Err(error) => {
                    let message = format!("History JSON inválido: {error}");
                    logger.error(Some(feed_label(feed_key)), message.clone());
                    *history_error = Some(message);
                    return true;
                }
            };
            if response["success"] == false {
                let message = response["message"]
                    .as_str()
                    .map(ToOwned::to_owned)
                    .unwrap_or_else(|| "El servidor rechazó el histórico".to_string());
                logger.error(Some(feed_label(feed_key)), message.clone());
                *history_error = Some(message);
                return true;
            }

            let Some(history) = response
                .get_mut("data")
                .and_then(Value::as_object_mut)
                .and_then(|data| data.remove("history"))
                .and_then(|history| match history {
                    Value::Array(history) => Some(history),
                    _ => None,
                })
            else {
                let message = "La respuesta no contiene data.history".to_string();
                logger.error(Some(feed_label(feed_key)), message.clone());
                *history_error = Some(message);
                return true;
            };

            *history_error = None;
            let (needs_price, needs_volume) = requested_fields(requests);
            let received = history.len();
            let mut parsed = 0;
            for payload in history {
                if let Some(bar) = parse_bar(payload, true, needs_price, needs_volume) {
                    upsert_bar(bars, bar);
                    parsed += 1;
                }
            }
            logger.info(
                Some(feed_label(feed_key)),
                format!(
                    "History procesado: {parsed}/{received} Bars válidas ({} almacenadas)",
                    bars.len()
                ),
            );
            true
        }
        CalculationInput::ChartRequests(updated_requests) => {
            *requests = updated_requests;
            let (needs_price, needs_volume) = requested_fields(requests);
            for bar in bars.values_mut() {
                if let Some(candle) = project_candle(&bar.payload, needs_price, needs_volume) {
                    bar.candle = candle;
                }
            }
            true
        }
        CalculationInput::Live(payload) => {
            let payload = match serde_json::from_str::<Value>(&payload) {
                Ok(payload) => payload,
                Err(error) => {
                    *status = FeedStatus::Error(format!("Bar live inválida: {error}"));
                    return true;
                }
            };
            if payload.get("type").and_then(Value::as_str) == Some("error") {
                *status = FeedStatus::Error(
                    payload["message"]
                        .as_str()
                        .unwrap_or("Error de suscripción WebSocket")
                        .to_string(),
                );
                return true;
            }

            let (needs_price, needs_volume) = requested_fields(requests);
            if let Some(bar) = parse_bar(payload, false, needs_price, needs_volume) {
                upsert_bar(bars, bar)
            } else {
                false
            }
        }
    }
}

fn feed_label(key: &FeedKey) -> String {
    format!("{}/{}/{}", key.source, key.symbol, key.timeframe)
}

fn requested_history_limit(requests: &[ChartDataRequest]) -> u64 {
    requests
        .iter()
        .map(|request| request.history_limit)
        .max()
        .unwrap_or(DEFAULT_HISTORY_LIMIT)
        .max(1)
}

fn requested_fields(requests: &[ChartDataRequest]) -> (bool, bool) {
    let needs_price = requests.iter().any(|request| {
        request.fields.price || request.fields.complete_bar || request.fields.footprint
    });
    let needs_volume = requests.iter().any(|request| {
        request.fields.volume_flow || request.fields.complete_bar || request.fields.raw_ticks
    });
    (needs_price, needs_volume)
}

fn parse_bar(
    payload: Value,
    is_history: bool,
    needs_price: bool,
    needs_volume: bool,
) -> Option<StoredBar> {
    let event = &payload;
    let bar = event
        .get("data")
        .filter(|value| value.is_object())
        .unwrap_or(event);
    let candle = project_candle(event, needs_price, needs_volume)?;

    let revision =
        value_as_u64(bar.get("revision").or_else(|| event.get("revision"))).unwrap_or_default();
    let event_id = event
        .get("event_id")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned);
    let is_final = if is_history {
        true
    } else {
        event
            .get("is_final")
            .and_then(Value::as_bool)
            // Legacy WebSocket events are finalized Bars.
            .unwrap_or(true)
    };
    let trade_count = bar
        .get("identity")
        .and_then(|identity| value_as_u64(identity.get("n_trades")));
    let checksum = bar
        .get("integrity")
        .and_then(|integrity| integrity.get("checksum"))
        .and_then(Value::as_str)
        .map(ToOwned::to_owned);

    Some(StoredBar {
        payload: Arc::new(payload),
        candle,
        revision,
        is_final,
        revision_conflict: false,
        event_id,
        trade_count,
        checksum,
    })
}

fn snapshot_bar(bar: &StoredBar) -> BarSnapshot {
    BarSnapshot {
        timestamp_ms: bar.candle.timestamp_ms,
        candle: bar.candle,
        revision: bar.revision,
        is_final: bar.is_final,
        revision_conflict: bar.revision_conflict,
        payload: bar.payload.clone(),
    }
}

#[cfg(test)]
pub(crate) fn test_bar_snapshot(
    payload: Value,
    revision: u64,
    is_final: bool,
    revision_conflict: bool,
) -> BarSnapshot {
    let candle = project_candle(&payload, true, true).expect("valid test Bar");
    BarSnapshot {
        timestamp_ms: candle.timestamp_ms,
        candle,
        revision,
        is_final,
        revision_conflict,
        payload: Arc::new(payload),
    }
}

fn project_candle(payload: &Value, needs_price: bool, needs_volume: bool) -> Option<Candle> {
    let bar = payload
        .get("data")
        .filter(|value| value.is_object())
        .unwrap_or(payload);
    let timestamp_ms = value_as_u64(Some(bar.get("timestamp").or_else(|| bar.get("time"))?))?;
    let header = bar.get("header");
    let price_scale = if bar.get("price").is_some() {
        header
            .and_then(|header| value_as_u64(header.get("price_scale")))
            .unwrap_or(8)
    } else {
        0
    };
    let quantity_scale = header
        .and_then(|header| value_as_u64(header.get("quantity_scale")))
        .unwrap_or(8);
    let price_divisor = 10_f64.powi(price_scale.min(18) as i32);
    let quantity_divisor = 10_f64.powi(quantity_scale.min(18) as i32);

    let price = bar.get("price").unwrap_or(bar);
    let (open, high, low, close) = if needs_price {
        (
            value_as_f64(price.get("open")?)? / price_divisor,
            value_as_f64(price.get("high")?)? / price_divisor,
            value_as_f64(price.get("low")?)? / price_divisor,
            value_as_f64(price.get("close")?)? / price_divisor,
        )
    } else {
        (0.0, 0.0, 0.0, 0.0)
    };

    let volume_flow = bar.get("volume_flow");
    let volume = if needs_volume {
        volume_flow
            .and_then(|flow| flow.get("total_qty"))
            .and_then(value_as_f64)
            .or_else(|| bar.get("volume").and_then(value_as_f64))
            .unwrap_or(0.0)
            / if volume_flow.is_some() {
                quantity_divisor
            } else {
                1.0
            }
    } else {
        0.0
    };

    if ![open, high, low, close, volume]
        .iter()
        .all(|value| value.is_finite())
        || (needs_price && high < low)
    {
        return None;
    }

    Some(Candle {
        timestamp_ms,
        open,
        high,
        low,
        close,
        volume,
    })
}

fn upsert_bar(bars: &mut BTreeMap<u64, StoredBar>, incoming: StoredBar) -> bool {
    let timestamp = incoming.candle.timestamp_ms;
    if let Some(current) = bars.get_mut(&timestamp) {
        if current.is_final && incoming.is_final && current.revision == incoming.revision {
            if !same_bar_attributes_value(current.payload.as_ref(), incoming.payload.as_ref())
                && !current.revision_conflict
            {
                current.revision_conflict = true;
                return true;
            }
            return false;
        }
        if !should_replace(current, &incoming) {
            return false;
        }
    }
    bars.insert(timestamp, incoming);
    true
}

fn same_bar_attributes_json(left: &str, right: &str) -> bool {
    let (Ok(left), Ok(right)) = (
        serde_json::from_str::<Value>(left),
        serde_json::from_str::<Value>(right),
    ) else {
        return left == right;
    };
    same_bar_attributes_value(&left, &right)
}

fn same_bar_attributes_value(left: &Value, right: &Value) -> bool {
    bar_attributes(left) == bar_attributes(right)
}

fn bar_attributes(value: &Value) -> &Value {
    value
        .get("data")
        .filter(|data| data.is_object())
        .unwrap_or(value)
}

fn should_replace(current: &StoredBar, incoming: &StoredBar) -> bool {
    if current.is_final && !incoming.is_final {
        return false;
    }
    if incoming.is_final && !current.is_final {
        return true;
    }
    if current.event_id.is_some() && current.event_id == incoming.event_id {
        return false;
    }
    if current.is_final && incoming.revision <= current.revision {
        return false;
    }

    // Live revisions can carry the same Bar revision number. Their trade count
    // is the useful monotonic signal; event IDs identify snapshots but are not
    // lexicographically ordered sequence numbers.
    if !current.is_final
        && let (Some(current_count), Some(incoming_count)) =
            (current.trade_count, incoming.trade_count)
        && incoming_count < current_count
    {
        return false;
    }

    current.is_final
        || current.trade_count != incoming.trade_count
        || current.checksum != incoming.checksum
}

fn value_as_u64(value: Option<&Value>) -> Option<u64> {
    match value? {
        Value::Number(number) => number.as_u64(),
        Value::String(string) => string.parse().ok(),
        _ => None,
    }
}

fn value_as_f64(value: &Value) -> Option<f64> {
    match value {
        Value::Number(number) => number.as_f64(),
        Value::String(string) => string.parse().ok(),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        AppLogger, BarFields, Bytes, CalculationInput, ChartDataRequest, FeedKey, FeedStatus,
        apply_input, coalesce_live_snapshots, parse_bar, snapshot_bar, upsert_bar,
    };
    use eframe::egui;
    use serde_json::json;
    use std::collections::BTreeMap;

    fn full_bar(timestamp: &str, revision: &str, trades: &str) -> serde_json::Value {
        json!({
            "schema_version": 4,
            "timestamp": timestamp,
            "source": "binance",
            "symbol": "BTCUSDT",
            "timeframe": "1m",
            "revision": revision,
            "header": {"price_scale": 2, "quantity_scale": 3},
            "identity": {"n_trades": trades},
            "price": {"open": "10000", "high": "10200", "low": "9900", "close": "10100"},
            "volume_flow": {"total_qty": "2500"},
            "footprint": {"levels": [{"price": "10000", "buy_qty": "1"}]},
            "raw_ticks": [{"id": "1", "price": "10000", "qty": "2500"}],
            "integrity": {"checksum": format!("checksum-{trades}")}
        })
    }

    fn test_feed() -> FeedKey {
        FeedKey {
            source: "binance".to_string(),
            symbol: "BTCUSDT".to_string(),
            timeframe: "1m".to_string(),
        }
    }

    #[test]
    fn parses_scaled_ohlcv_and_retains_the_complete_bar() {
        let payload = full_bar("60000", "1", "3");
        let stored = parse_bar(payload, true, true, true).unwrap();

        assert_eq!(stored.candle.timestamp_ms, 60_000);
        assert_eq!(stored.candle.open, 100.0);
        assert_eq!(stored.candle.volume, 2.5);
        assert_eq!(stored.payload["raw_ticks"][0]["id"], "1");
        assert_eq!(stored.payload["footprint"]["levels"][0]["price"], "10000");
    }

    #[test]
    fn shared_bar_snapshot_exposes_full_history_and_live_attributes() {
        let history = parse_bar(full_bar("60000", "1", "3"), true, true, true).unwrap();
        let history_snapshot = snapshot_bar(&history);

        assert_eq!(history_snapshot.attributes()["raw_ticks"][0]["id"], "1");
        assert_eq!(history_snapshot.revision, 1);
        assert!(history_snapshot.is_final);
        assert!(std::sync::Arc::ptr_eq(
            &history.payload,
            &history_snapshot.payload
        ));

        let live_event = json!({
            "event_id": "bar:live:3:abc",
            "is_final": false,
            "data": full_bar("60000", "1", "3")
        });
        let live = parse_bar(live_event, false, true, true).unwrap();
        let live_snapshot = snapshot_bar(&live);

        assert_eq!(
            live_snapshot.attributes()["footprint"]["levels"][0]["price"],
            "10000"
        );
        assert_eq!(live_snapshot.source_payload()["event_id"], "bar:live:3:abc");
        assert!(!live_snapshot.is_final);
        assert!(std::sync::Arc::ptr_eq(
            &live.payload,
            &live_snapshot.payload
        ));
    }

    #[test]
    fn each_chart_projects_its_requested_fields_without_dropping_the_full_bar() {
        let payload = full_bar("60000", "1", "3");
        let stored = parse_bar(payload, true, false, true).unwrap();

        assert_eq!(stored.candle.open, 0.0);
        assert_eq!(stored.candle.volume, 2.5);
        assert_eq!(stored.payload["price"]["open"], "10000");
        assert_eq!(stored.payload["raw_ticks"][0]["id"], "1");
        assert_eq!(stored.payload["footprint"]["levels"][0]["price"], "10000");
    }

    #[test]
    fn final_bar_wins_over_live_and_history_reconciles_full_payloads() {
        let mut bars = BTreeMap::new();
        let live = json!({
            "event_id": "bar:live:3:abc",
            "is_final": false,
            "data": full_bar("60000", "1", "3")
        });
        let final_bar = full_bar("60000", "1", "4");
        let mut status = FeedStatus::Connecting;
        let mut history_error = None;
        let feed_key = test_feed();
        let logger = AppLogger::new(egui::Context::default());
        let mut requests = vec![ChartDataRequest {
            history_limit: 10,
            live: true,
            fields: BarFields {
                price: true,
                volume_flow: true,
                ..Default::default()
            },
        }];

        apply_input(
            CalculationInput::Live(live.to_string()),
            &mut bars,
            &mut status,
            &mut history_error,
            &mut requests,
            &feed_key,
            &logger,
        );
        apply_input(
            CalculationInput::History(Ok(Bytes::from(
                serde_json::to_vec(&json!({
                    "success": true,
                    "data": {"history": [final_bar]}
                }))
                .unwrap(),
            ))),
            &mut bars,
            &mut status,
            &mut history_error,
            &mut requests,
            &feed_key,
            &logger,
        );

        assert_eq!(bars.len(), 1);
        assert!(bars.get(&60_000).unwrap().is_final);
        assert_eq!(
            bars.get(&60_000).unwrap().payload["raw_ticks"][0]["id"],
            "1"
        );
        assert!(history_error.is_none());
    }

    #[test]
    fn same_final_revision_with_different_contents_is_marked_as_a_conflict() {
        let first = parse_bar(full_bar("60000", "1", "3"), true, true, true).unwrap();
        let different = parse_bar(full_bar("60000", "1", "4"), true, true, true).unwrap();
        let mut bars = BTreeMap::new();

        assert!(upsert_bar(&mut bars, first));
        assert!(upsert_bar(&mut bars, different));
        assert!(snapshot_bar(bars.get(&60_000).unwrap()).revision_conflict);
    }

    #[test]
    fn live_coalescing_preserves_same_revision_final_conflicts_for_reconciliation() {
        let first = json!({
            "is_final": true,
            "data": full_bar("60000", "1", "3")
        });
        let different = json!({
            "is_final": true,
            "data": full_bar("60000", "1", "4")
        });

        assert_eq!(
            coalesce_live_snapshots(vec![
                CalculationInput::Live(first.to_string()),
                CalculationInput::Live(different.to_string()),
            ])
            .len(),
            2
        );
    }

    #[test]
    fn newer_live_snapshot_replaces_the_same_bar_without_comparing_event_ids() {
        let mut bars = BTreeMap::new();
        let mut status = FeedStatus::Connecting;
        let mut history_error = None;
        let feed_key = test_feed();
        let logger = AppLogger::new(egui::Context::default());
        let mut requests = vec![ChartDataRequest {
            history_limit: 10,
            live: true,
            fields: BarFields {
                price: true,
                ..Default::default()
            },
        }];
        let old_live = json!({
            "event_id": "bar:live:2:zzz",
            "is_final": false,
            "data": full_bar("60000", "1", "2")
        });
        let new_live = json!({
            "event_id": "bar:live:3:aaa",
            "is_final": false,
            "data": full_bar("60000", "1", "3")
        });

        apply_input(
            CalculationInput::Live(old_live.to_string()),
            &mut bars,
            &mut status,
            &mut history_error,
            &mut requests,
            &feed_key,
            &logger,
        );
        apply_input(
            CalculationInput::Live(new_live.to_string()),
            &mut bars,
            &mut status,
            &mut history_error,
            &mut requests,
            &feed_key,
            &logger,
        );

        assert_eq!(bars.get(&60_000).unwrap().trade_count, Some(3));
        assert_eq!(
            bars.get(&60_000).unwrap().payload["event_id"],
            "bar:live:3:aaa"
        );
        assert_eq!(
            bars.get(&60_000).unwrap().payload["data"]["raw_ticks"][0]["id"],
            "1"
        );
    }

    #[test]
    fn calculation_batch_keeps_latest_live_snapshot_but_never_replaces_final_with_live() {
        let live_older = json!({
            "event_id": "bar:live:2:zzz",
            "is_final": false,
            "data": full_bar("60000", "1", "2")
        });
        let live_newer = json!({
            "event_id": "bar:live:3:aaa",
            "is_final": false,
            "data": full_bar("60000", "1", "3")
        });
        let final_bar = json!({
            "event_id": "bar:r1",
            "is_final": true,
            "data": full_bar("60000", "1", "4")
        });
        let stale_live = json!({
            "event_id": "bar:live:3:zzz",
            "is_final": false,
            "data": full_bar("60000", "1", "3")
        });

        let coalesced = coalesce_live_snapshots(vec![
            CalculationInput::Live(live_older.to_string()),
            CalculationInput::Live(live_newer.to_string()),
            CalculationInput::Live(final_bar.to_string()),
            CalculationInput::Live(stale_live.to_string()),
        ]);

        assert_eq!(coalesced.len(), 1);
        let CalculationInput::Live(payload) = &coalesced[0] else {
            panic!("expected coalesced live event");
        };
        let payload: serde_json::Value = serde_json::from_str(payload).unwrap();
        assert_eq!(payload["is_final"], true);
        assert_eq!(payload["event_id"], "bar:r1");
        assert_eq!(payload["data"]["raw_ticks"][0]["id"], "1");
    }
}

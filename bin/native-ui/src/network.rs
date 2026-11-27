use std::{
    collections::HashMap,
    env,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
        mpsc::Sender,
    },
    time::Duration,
};

use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use serde_json::{json, value::RawValue};
use tokio::{
    runtime::{Builder, Runtime},
    sync::{
        Mutex,
        mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel},
        oneshot,
    },
    task::JoinHandle,
    time,
};
use tokio_tungstenite::{connect_async, tungstenite::Message};

use crate::{
    app_log::AppLogger,
    market::{CalculationInput, FeedKey, FeedStatus},
};

const DEFAULT_SERVER_URL: &str = "http://127.0.0.1:3001/api/market";
const HISTORY_TIMEOUT: Duration = Duration::from_secs(300);
const RECONNECT_DELAY: Duration = Duration::from_secs(1);

#[derive(Deserialize)]
struct WsEnvelope<'a> {
    #[serde(rename = "type", borrow)]
    message_type: Option<&'a str>,
    #[serde(borrow)]
    source: Option<&'a str>,
    #[serde(borrow)]
    symbol: Option<&'a str>,
    #[serde(borrow)]
    timeframe: Option<&'a str>,
    #[serde(rename = "data", borrow)]
    _data: Option<&'a RawValue>,
    #[serde(borrow)]
    message: Option<&'a str>,
}

pub enum NetworkCommand {
    OpenFeed {
        feed_id: u64,
        key: FeedKey,
        history_limit: u64,
        subscribe_live: bool,
        calculation_tx: Sender<CalculationInput>,
    },
    SetLiveSubscription {
        feed_id: u64,
        enabled: bool,
    },
    FetchHistory {
        feed_id: u64,
        history_limit: u64,
    },
    CloseFeed {
        feed_id: u64,
    },
}

pub struct NetworkRuntime {
    // Keeping the runtime alive keeps the network manager and feed tasks alive.
    _runtime: Runtime,
    pub sender: UnboundedSender<NetworkCommand>,
}

impl NetworkRuntime {
    pub fn new(logger: AppLogger) -> Self {
        let runtime = Builder::new_multi_thread()
            .worker_threads(2)
            .thread_name("native-ui-network")
            .enable_all()
            .build()
            .expect("failed to create Tokio runtime");
        let (sender, receiver) = unbounded_channel();
        let endpoints = Endpoints::from_env();
        logger.info(
            None,
            format!(
                "Endpoints configurados: HTTP {} · WebSocket {}",
                endpoints.history_url, endpoints.websocket_url
            ),
        );
        runtime.spawn(network_manager(receiver, endpoints, logger));

        Self {
            _runtime: runtime,
            sender,
        }
    }
}

#[derive(Clone)]
struct Endpoints {
    history_url: String,
    websocket_url: String,
}

impl Endpoints {
    fn from_env() -> Self {
        let server_url =
            env::var("NATIVE_UI_SERVER_URL").unwrap_or_else(|_| DEFAULT_SERVER_URL.to_string());
        let base_url = server_url.trim_end_matches('/');
        let history_url = format!("{base_url}/get-history");
        let websocket_url = env::var("NATIVE_UI_WEBSOCKET_URL").unwrap_or_else(|_| {
            let websocket_base = if let Some(rest) = base_url.strip_prefix("https://") {
                format!("wss://{rest}")
            } else if let Some(rest) = base_url.strip_prefix("http://") {
                format!("ws://{rest}")
            } else {
                base_url.to_string()
            };
            format!("{websocket_base}/ws")
        });

        Self {
            history_url,
            websocket_url,
        }
    }
}

struct FeedSession {
    key: FeedKey,
    calculation_tx: Sender<CalculationInput>,
    cancel_tx: Option<oneshot::Sender<()>>,
    task: Option<JoinHandle<()>>,
    history_limit: Arc<AtomicU64>,
    history_tasks: Arc<Mutex<Vec<JoinHandle<()>>>>,
}

struct FeedTaskContext {
    key: FeedKey,
    history_limit: Arc<AtomicU64>,
    calculation_tx: Sender<CalculationInput>,
    client: reqwest::Client,
    endpoints: Endpoints,
    history_tasks: Arc<Mutex<Vec<JoinHandle<()>>>>,
    logger: AppLogger,
    cancel: oneshot::Receiver<()>,
}

async fn network_manager(
    mut commands: UnboundedReceiver<NetworkCommand>,
    endpoints: Endpoints,
    logger: AppLogger,
) {
    let client = reqwest::Client::builder()
        .timeout(HISTORY_TIMEOUT)
        .build()
        .expect("failed to create HTTP client");
    let mut feeds = HashMap::<u64, FeedSession>::new();
    let mut pending_history = HashMap::<u64, u64>::new();

    while let Some(command) = commands.recv().await {
        match command {
            NetworkCommand::OpenFeed {
                feed_id,
                key,
                history_limit,
                subscribe_live,
                calculation_tx,
            } => {
                close_feed(&mut feeds, feed_id).await;
                let history_tasks = Arc::new(Mutex::new(Vec::new()));
                let mut feed = FeedSession {
                    key,
                    calculation_tx,
                    cancel_tx: None,
                    task: None,
                    history_limit: Arc::new(AtomicU64::new(history_limit)),
                    history_tasks,
                };
                if subscribe_live {
                    start_live_subscription(
                        &mut feed,
                        client.clone(),
                        endpoints.clone(),
                        logger.clone(),
                    );
                }
                feeds.insert(feed_id, feed);
                if let Some(limit) = pending_history.remove(&feed_id)
                    && let Some(feed) = feeds.get_mut(&feed_id)
                {
                    feed.history_limit.fetch_max(limit, Ordering::Relaxed);
                    let task = spawn_history_request(
                        client.clone(),
                        endpoints.history_url.clone(),
                        feed.key.clone(),
                        limit,
                        feed.calculation_tx.clone(),
                        logger.clone(),
                    );
                    register_history_task(&feed.history_tasks, task).await;
                }
            }
            NetworkCommand::FetchHistory {
                feed_id,
                history_limit,
            } => {
                if let Some(feed) = feeds.get_mut(&feed_id) {
                    feed.history_limit
                        .fetch_max(history_limit, Ordering::Relaxed);
                    let task = spawn_history_request(
                        client.clone(),
                        endpoints.history_url.clone(),
                        feed.key.clone(),
                        history_limit,
                        feed.calculation_tx.clone(),
                        logger.clone(),
                    );
                    register_history_task(&feed.history_tasks, task).await;
                } else {
                    pending_history
                        .entry(feed_id)
                        .and_modify(|limit| *limit = (*limit).max(history_limit))
                        .or_insert(history_limit);
                }
            }
            NetworkCommand::SetLiveSubscription { feed_id, enabled } => {
                if let Some(feed) = feeds.get_mut(&feed_id) {
                    if enabled {
                        start_live_subscription(
                            feed,
                            client.clone(),
                            endpoints.clone(),
                            logger.clone(),
                        );
                    } else {
                        stop_live_subscription(feed);
                    }
                }
            }
            NetworkCommand::CloseFeed { feed_id } => {
                pending_history.remove(&feed_id);
                close_feed(&mut feeds, feed_id).await;
            }
        }
    }

    for (_, mut feed) in feeds {
        stop_live_subscription(&mut feed);
        abort_history_tasks(&feed.history_tasks).await;
    }
}

async fn close_feed(feeds: &mut HashMap<u64, FeedSession>, feed_id: u64) {
    if let Some(mut feed) = feeds.remove(&feed_id) {
        stop_live_subscription(&mut feed);
        abort_history_tasks(&feed.history_tasks).await;
    }
}

fn start_live_subscription(
    feed: &mut FeedSession,
    client: reqwest::Client,
    endpoints: Endpoints,
    logger: AppLogger,
) {
    if feed.task.is_some() {
        return;
    }
    let (cancel_tx, cancel_rx) = oneshot::channel();
    let task = tokio::spawn(run_feed_session(FeedTaskContext {
        key: feed.key.clone(),
        history_limit: feed.history_limit.clone(),
        calculation_tx: feed.calculation_tx.clone(),
        client,
        endpoints,
        history_tasks: feed.history_tasks.clone(),
        logger,
        cancel: cancel_rx,
    }));
    feed.cancel_tx = Some(cancel_tx);
    feed.task = Some(task);
}

fn stop_live_subscription(feed: &mut FeedSession) {
    if let Some(cancel) = feed.cancel_tx.take() {
        let _ = cancel.send(());
    }
    if let Some(task) = feed.task.take() {
        task.abort();
    }
}

fn spawn_history_request(
    client: reqwest::Client,
    history_url: String,
    key: FeedKey,
    history_limit: u64,
    calculation_tx: Sender<CalculationInput>,
    logger: AppLogger,
) -> JoinHandle<()> {
    logger.info(
        Some(feed_label(&key)),
        format!("Solicitando history completo (limit={history_limit})"),
    );
    tokio::spawn(async move {
        let body = json!({
            "source": key.source,
            "symbol": key.symbol,
            "timeframe": key.timeframe,
            "limit": history_limit,
            // The wire contract is deliberately complete: retain raw_ticks and
            // every other Bar attribute for Charts that need them.
            "include_raw_ticks": true
        });

        let response = client
            .post(history_url)
            .json(&body)
            .send()
            .await
            .and_then(reqwest::Response::error_for_status);
        let history = match response {
            Ok(response) => match response.bytes().await {
                Ok(bytes) => {
                    logger.info(
                        Some(feed_label(&key)),
                        format!("History recibido: {} bytes", bytes.len()),
                    );
                    Ok(bytes)
                }
                Err(error) => {
                    let message = format!("No se pudo leer el histórico: {error}");
                    logger.error(Some(feed_label(&key)), message.clone());
                    Err(message)
                }
            },
            Err(error) => {
                let message = format!("No se pudo solicitar el histórico: {error}");
                logger.error(Some(feed_label(&key)), message.clone());
                Err(message)
            }
        };
        let _ = calculation_tx.send(CalculationInput::History(history));
    })
}

async fn register_history_task(tasks: &Arc<Mutex<Vec<JoinHandle<()>>>>, task: JoinHandle<()>) {
    let mut tasks = tasks.lock().await;
    tasks.retain(|task| !task.is_finished());
    tasks.push(task);
}

async fn abort_history_tasks(tasks: &Arc<Mutex<Vec<JoinHandle<()>>>>) {
    let mut tasks = tasks.lock().await;
    for task in tasks.drain(..) {
        task.abort();
    }
}

async fn run_feed_session(context: FeedTaskContext) {
    let FeedTaskContext {
        key,
        history_limit,
        calculation_tx,
        client,
        endpoints,
        history_tasks,
        logger,
        mut cancel,
    } = context;
    let mut has_subscribed = false;

    logger.info(Some(feed_label(&key)), "Conectando WebSocket");
    loop {
        send_status(&calculation_tx, &logger, &key, FeedStatus::Connecting);

        let connection = tokio::select! {
            _ = &mut cancel => return,
            connection = connect_async(&endpoints.websocket_url) => connection,
        };
        let (mut socket, _) = match connection {
            Ok(connection) => connection,
            Err(error) => {
                send_status(
                    &calculation_tx,
                    &logger,
                    &key,
                    FeedStatus::Error(format!("WebSocket: {error}")),
                );
                if wait_or_cancel(&mut cancel, RECONNECT_DELAY).await {
                    return;
                }
                continue;
            }
        };

        let subscribe = json!({
            "action": "subscribe",
            "source": key.source,
            "symbol": key.symbol,
            "timeframe": key.timeframe
        });
        if let Err(error) = socket
            .send(Message::Text(subscribe.to_string().into()))
            .await
        {
            send_status(
                &calculation_tx,
                &logger,
                &key,
                FeedStatus::Error(format!("No se pudo suscribir: {error}")),
            );
            if wait_or_cancel(&mut cancel, RECONNECT_DELAY).await {
                return;
            }
            continue;
        }

        let mut connection_ended = false;
        while !connection_ended {
            tokio::select! {
                _ = &mut cancel => return,
                message = socket.next() => {
                    match message {
                        Some(Ok(Message::Text(text))) => {
                            match serde_json::from_str::<WsEnvelope<'_>>(text.as_ref()) {
                                Ok(envelope) if envelope.message_type == Some("subscribed") => {
                                    if subscription_matches(&envelope, &key) {
                                        send_status(
                                            &calculation_tx,
                                            &logger,
                                            &key,
                                            FeedStatus::Live,
                                        );
                                        if has_subscribed {
                                            // Reconcile finalized Bars after reconnect. The
                                            // calculation worker merges this with live data.
                                            let task = spawn_history_request(
                                                client.clone(),
                                                endpoints.history_url.clone(),
                                                key.clone(),
                                                history_limit.load(Ordering::Relaxed),
                                                calculation_tx.clone(),
                                                logger.clone(),
                                            );
                                            register_history_task(&history_tasks, task).await;
                                        }
                                        has_subscribed = true;
                                    }
                                }
                                Ok(envelope) if envelope.message_type == Some("error") => {
                                    send_status(
                                        &calculation_tx,
                                        &logger,
                                        &key,
                                        FeedStatus::Error(
                                            envelope
                                                .message
                                                .unwrap_or("El servidor rechazó la suscripción")
                                                .to_string(),
                                        ),
                                    );
                                }
                                Ok(envelope) => {
                                    if event_matches(&envelope, &key) {
                                        let _ = calculation_tx.send(CalculationInput::Live(
                                            text.to_string(),
                                        ));
                                    }
                                }
                                Err(error) => {
                                    send_status(
                                        &calculation_tx,
                                        &logger,
                                        &key,
                                        FeedStatus::Error(format!(
                                            "Mensaje WebSocket inválido: {error}"
                                        )),
                                    );
                                }
                            }
                        }
                        Some(Ok(Message::Ping(data))) => {
                            if socket.send(Message::Pong(data)).await.is_err() {
                                connection_ended = true;
                            }
                        }
                        Some(Ok(Message::Close(_))) | None => connection_ended = true,
                        Some(Ok(_)) => {}
                        Some(Err(error)) => {
                            send_status(
                                &calculation_tx,
                                &logger,
                                &key,
                                FeedStatus::Error(format!("WebSocket: {error}")),
                            );
                            connection_ended = true;
                        }
                    }
                }
            }
        }

        send_status(&calculation_tx, &logger, &key, FeedStatus::Disconnected);
        if wait_or_cancel(&mut cancel, RECONNECT_DELAY).await {
            return;
        }
    }
}

fn subscription_matches(envelope: &WsEnvelope<'_>, key: &FeedKey) -> bool {
    envelope.source == Some(&key.source)
        && envelope.symbol == Some(&key.symbol)
        && envelope.timeframe == Some(&key.timeframe)
}

fn event_matches(envelope: &WsEnvelope<'_>, key: &FeedKey) -> bool {
    envelope.source.is_none_or(|source| source == key.source)
        && envelope.symbol.is_none_or(|symbol| symbol == key.symbol)
        && envelope
            .timeframe
            .is_none_or(|timeframe| timeframe == key.timeframe)
}

fn send_status(
    calculation_tx: &Sender<CalculationInput>,
    logger: &AppLogger,
    key: &FeedKey,
    status: FeedStatus,
) {
    let feed = Some(feed_label(key));
    match &status {
        FeedStatus::Loading => {}
        FeedStatus::Connecting => {}
        FeedStatus::Live => logger.info(feed, "WebSocket suscrito; feed live activo"),
        FeedStatus::Disconnected => logger.warning(feed, "WebSocket desconectado; reintentando"),
        FeedStatus::Error(error) => logger.error(feed, error.clone()),
    }
    let _ = calculation_tx.send(CalculationInput::Status(status));
}

fn feed_label(key: &FeedKey) -> String {
    format!("{}/{}/{}", key.source, key.symbol, key.timeframe)
}

async fn wait_or_cancel(cancel: &mut oneshot::Receiver<()>, duration: Duration) -> bool {
    tokio::select! {
        _ = cancel => true,
        _ = time::sleep(duration) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::{FeedKey, WsEnvelope, event_matches, subscription_matches};

    #[test]
    fn live_envelope_is_routed_without_materializing_or_dropping_bar_fields() {
        let text = r#"{
            "event_id":"binance/BTCUSDT/1m/0:live:2:123",
            "event_type":"bar.1m",
            "timestamp":"60000",
            "source":"binance",
            "symbol":"BTCUSDT",
            "timeframe":"1m",
            "revision":"1",
            "is_final":false,
            "data":{
                "timestamp":"60000",
                "price":{"open":"10000"},
                "footprint":{"levels":[{"price":"10000"}]},
                "raw_ticks":[{"id":"trade-1"}]
            }
        }"#;
        let key = FeedKey {
            source: "binance".to_string(),
            symbol: "BTCUSDT".to_string(),
            timeframe: "1m".to_string(),
        };
        let envelope = serde_json::from_str::<WsEnvelope<'_>>(text).unwrap();

        assert!(event_matches(&envelope, &key));
        assert!(envelope._data.unwrap().get().contains("\"raw_ticks\""));

        let acknowledgement = serde_json::from_str::<WsEnvelope<'_>>(
            r#"{"type":"subscribed","source":"binance","symbol":"BTCUSDT","timeframe":"1m"}"#,
        )
        .unwrap();
        assert!(subscription_matches(&acknowledgement, &key));
    }
}

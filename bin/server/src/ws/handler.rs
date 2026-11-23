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

use crate::state::AppState;
use axum::{
    extract::{
        State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    response::Response,
};
use futures::StreamExt;
use pulsar::{Consumer, ConsumerOptions, SubType, TokioExecutor, consumer::InitialPosition};
use serde::Deserialize;
use tracing::{error, info};

const BARS_1M_TOPIC: &str = "persistent://public/default/bars-1m";

//----------------------------------------------------------------------------------------------------------------------
// WEB SOCKET IMPLEMENTATION
//----------------------------------------------------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct WsCommand {
    action: String,
    source: String,
    symbol: String,
}

#[derive(Debug, Deserialize)]
struct BarEventIdentity {
    source: String,
    symbol: String,
}

pub async fn ws_handler(ws: WebSocketUpgrade, State(state): State<AppState>) -> Response {
    ws.on_upgrade(move |socket: WebSocket| handle_socket(socket, state))
}

//----------------------------------------------------------------------------------------------------------------------
// WEB SOCKET HANDLER
//----------------------------------------------------------------------------------------------------------------------

async fn handle_socket(mut socket: WebSocket, state: AppState) {
    info!("WS connected");

    let mut consumer: Option<Consumer<Vec<u8>, TokioExecutor>> = None;

    let mut current_subscription: Option<(String, String)> = None;

    loop {
        tokio::select! {
                // Wait for the next message received from the WebSocket client.
                ws_msg = socket.recv() => {

                    // Continue only if the message is a valid text frame.
                    // Exit the loop if the connection is closed or another frame type is received.
                    let Some(Ok(Message::Text(text))) = ws_msg else {
                        break;
                    };

                    // Attempt to deserialize the incoming JSON payload into a WebSocket command.
                    // Ignore malformed or unsupported messages.
                    let Ok(cmd) = serde_json::from_str::<WsCommand>(&text) else {
                        continue;
                    };

                    let subscription = (cmd.source.clone(), cmd.symbol.clone());

                    // Handle a subscription request from the client.
                    if cmd.action == "subscribe" {
                        if current_subscription.as_ref() == Some(&subscription) {
                            continue;
                        }
                        drop(consumer.take());

                        // Every client consumes the shared 1-minute bar topic.
                        match state
                            .pulsar
                            .consumer()
                            .with_topic(BARS_1M_TOPIC.to_string())
                            .with_subscription_type(SubType::Exclusive)
                            .with_subscription(
                                format!("ui-{}", uuid::Uuid::now_v7())
                            )
                            .with_options(ConsumerOptions {
                                initial_position: InitialPosition::Latest,
                                ..Default::default()
                            })
                            .build()
                            .await
                        {
                            Ok(c) => {
                                consumer = Some(c);
                                current_subscription = Some(subscription);
                                info!(
                                    source = %cmd.source,
                                    symbol = %cmd.symbol,
                                    topic = BARS_1M_TOPIC,
                                    "Subscribed to 1-minute bars"
                                );
                            }

                            Err(e) => {
                                error!("Error creating the consumer: {:?}", e);

                                let _ = socket
                                    .send(Message::Text(
                                        serde_json::json!({
                                            "type": "error",
                                            "message": "The subscription could not be created."
                                        })
                                        .to_string()
                                        .into(),
                                    ))
                                    .await;
                                current_subscription = None;
                            }
                        }
                    }

                    // Handle cancel subscription request from the client.
                    if cmd.action == "unsubscribe" {
                        consumer = None;
                        current_subscription = None;
                    }
                 }


                // Wait for the next message from the active Pulsar consumer.
                // If no consumer is active, wait indefinitely until one is created.
                pulsar_msg = async {
                    match consumer.as_mut() {
                        // Read the next available message from Pulsar.
                        Some(c) => c.next().await,

                        // Suspend this branch while there is no active consumer.
                        None => futures::future::pending().await,
                    }
                } =>  match pulsar_msg {

                    Some(Ok(mut msg)) => {
                        let should_forward = current_subscription.as_ref().is_some_and(
                            |(source, symbol)| {
                                serde_json::from_slice::<BarEventIdentity>(&msg.payload.data)
                                    .is_ok_and(|event| {
                                        event.source == *source && event.symbol == *symbol
                                    })
                            },
                        );

                        if should_forward {
                            // Events are JSON; preserve the full persisted envelope and bar
                            // payload and send it as a WebSocket text frame.
                            let data = std::mem::take(&mut msg.payload.data);
                            let text = String::from_utf8(data)
                                .expect("a valid JSON event is valid UTF-8");
                            if let Err(err) = socket.send(Message::Text(text.into())).await {
                                info!("WebSocket send failed: {:?}", err);
                                break;
                            }
                        }

                        // Acknowledge both forwarded events and events filtered out for this
                        // source/symbol so the per-client subscription does not retain them.
                        if let Some(c) = consumer.as_mut() {
                            if let Err(err) = c.ack(&msg).await {
                                error!("Failed to acknowledge Pulsar bar event: {:?}", err);
                            }
                        }
                    }


                    Some(Err(e)) => {
                         info!("Pulsar error: {:?}", e);
                         consumer = None;
                         current_subscription = None;
                    }


                    None => {
                         consumer = None;
                         current_subscription = None;
                    }
                 }

        }
    }

    info!("WS closed");
}

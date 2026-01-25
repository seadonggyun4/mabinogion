//! WebSocket handler.

use std::sync::Arc;

use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        State,
    },
    response::IntoResponse,
};
use futures::{SinkExt, StreamExt};
use serde::Serialize;
use tracing::debug;

use crate::state::AppState;

/// WebSocket handler.
pub async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    ws.on_upgrade(|socket| handle_socket(socket, state))
}

async fn handle_socket(socket: WebSocket, state: Arc<AppState>) {
    let (mut sender, mut receiver) = socket.split();

    // Subscribe to events
    let mut event_rx = state.event_tx.subscribe();

    // Spawn task to receive client messages
    let recv_task = tokio::spawn(async move {
        while let Some(Ok(msg)) = receiver.next().await {
            match msg {
                Message::Text(text) => {
                    debug!(message = %text, "WebSocket message received");
                    // Handle client commands here
                }
                Message::Close(_) => {
                    debug!("WebSocket closed by client");
                    break;
                }
                _ => {}
            }
        }
    });

    // Spawn task to send events to client
    let send_task = tokio::spawn(async move {
        loop {
            tokio::select! {
                Ok(event) = event_rx.recv() => {
                    if sender.send(Message::Text(event)).await.is_err() {
                        break;
                    }
                }
            }
        }
    });

    // Wait for either task to complete
    tokio::select! {
        _ = recv_task => {},
        _ = send_task => {},
    }

    debug!("WebSocket connection closed");
}

/// WebSocket message types.
#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WsMessage {
    /// Status update.
    Status {
        device_count: usize,
        tick_count: u64,
    },

    /// Value update.
    ValueUpdate {
        device_id: String,
        point_id: String,
        value: serde_json::Value,
        timestamp: String,
    },

    /// Chaos event.
    ChaosEvent {
        event_type: String,
        target: String,
        details: String,
    },

    /// Error.
    Error { message: String },
}

impl WsMessage {
    /// Serialize to JSON string.
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| "{}".to_string())
    }
}

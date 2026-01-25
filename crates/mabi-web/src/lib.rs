//! # mabi-web
//!
//! Web UI and REST API for the TRAP protocol simulator.
//!
//! This crate provides:
//! - REST API for simulator control
//! - WebSocket for real-time updates
//! - Static file serving for Web UI

pub mod api;
pub mod state;
pub mod websocket;

pub use api::create_router;
pub use state::AppState;

use std::net::SocketAddr;
use std::sync::Arc;

use tokio::net::TcpListener;
use tracing::info;

use thiserror::Error;

/// Web server error.
#[derive(Debug, Error)]
pub enum WebError {
    #[error("Server error: {0}")]
    Server(String),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

/// Web server result.
pub type WebResult<T> = Result<T, WebError>;

/// Web server configuration.
#[derive(Debug, Clone)]
pub struct WebConfig {
    /// Bind address.
    pub bind_address: SocketAddr,

    /// Enable CORS.
    pub enable_cors: bool,

    /// Static files directory.
    pub static_dir: Option<String>,
}

impl Default for WebConfig {
    fn default() -> Self {
        Self {
            bind_address: "0.0.0.0:8080".parse().unwrap(),
            enable_cors: true,
            static_dir: None,
        }
    }
}

/// Run the web server.
pub async fn run_server(state: Arc<AppState>, config: WebConfig) -> WebResult<()> {
    let router = create_router(state);

    let listener = TcpListener::bind(config.bind_address).await?;
    info!(address = %config.bind_address, "Web server started");

    axum::serve(listener, router)
        .await
        .map_err(|e| WebError::Server(e.to_string()))?;

    Ok(())
}

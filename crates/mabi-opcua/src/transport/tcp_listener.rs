//! OPC UA TCP Listener — accepts connections and spawns per-connection handlers.

use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use tokio::net::TcpListener;
use tokio::sync::{broadcast, Semaphore};
use tracing::{error, info, warn};

use crate::error::{OpcUaError, OpcUaResult};
use crate::service::registry::ServiceRegistry;
use crate::transport::connection::{handle_connection, ConnectionConfig, ServiceContextTemplate};
use crate::transport::metrics::TransportMetrics;

/// Configuration for the OPC UA TCP listener.
#[derive(Debug, Clone)]
pub struct TcpTransportConfig {
    /// Address to bind to.
    pub bind_address: SocketAddr,
    /// Maximum concurrent connections.
    pub max_connections: usize,
    /// Per-connection idle timeout.
    pub connection_timeout: std::time::Duration,
    /// Server buffer size for Hello/Acknowledge negotiation.
    pub server_buffer_size: u32,
}

impl Default for TcpTransportConfig {
    fn default() -> Self {
        Self {
            bind_address: SocketAddr::from(([0, 0, 0, 0], 4840)),
            max_connections: 1000,
            connection_timeout: std::time::Duration::from_secs(60),
            server_buffer_size: 65535,
        }
    }
}

/// OPC UA TCP listener that accepts connections and dispatches them.
pub struct OpcUaTcpListener {
    config: TcpTransportConfig,
    service_registry: Arc<ServiceRegistry>,
    service_context: Arc<ServiceContextTemplate>,
    metrics: Arc<TransportMetrics>,
    shutdown: Arc<AtomicBool>,
    shutdown_tx: broadcast::Sender<()>,
}

impl OpcUaTcpListener {
    pub fn new(
        config: TcpTransportConfig,
        service_registry: Arc<ServiceRegistry>,
        service_context: Arc<ServiceContextTemplate>,
    ) -> Self {
        let (shutdown_tx, _) = broadcast::channel(1);
        Self {
            config,
            service_registry,
            service_context,
            metrics: Arc::new(TransportMetrics::new()),
            shutdown: Arc::new(AtomicBool::new(false)),
            shutdown_tx,
        }
    }

    /// Get a reference to the transport metrics.
    pub fn metrics(&self) -> &Arc<TransportMetrics> {
        &self.metrics
    }

    /// Signal the listener to shut down.
    pub fn shutdown(&self) {
        self.shutdown.store(true, Ordering::SeqCst);
        let _ = self.shutdown_tx.send(());
    }

    /// Run the TCP listener — binds, accepts, and spawns connection handlers.
    ///
    /// This is a blocking call that runs until shutdown is signaled.
    pub async fn run(&self) -> OpcUaResult<()> {
        let listener = TcpListener::bind(self.config.bind_address)
            .await
            .map_err(|e| OpcUaError::Bind {
                address: self.config.bind_address,
                reason: e.to_string(),
            })?;

        info!(address = %self.config.bind_address, "OPC UA TCP server listening");

        let semaphore = Arc::new(Semaphore::new(self.config.max_connections));
        let mut shutdown_rx = self.shutdown_tx.subscribe();

        loop {
            tokio::select! {
                result = listener.accept() => {
                    match result {
                        Ok((stream, peer_addr)) => {
                            // Try to acquire a connection permit
                            let permit = match semaphore.clone().try_acquire_owned() {
                                Ok(permit) => permit,
                                Err(_) => {
                                    warn!(peer = %peer_addr, "Max connections reached, rejecting");
                                    self.metrics.record_rejection();
                                    drop(stream);
                                    continue;
                                }
                            };

                            self.metrics.record_connection();

                            let registry = self.service_registry.clone();
                            let context = self.service_context.clone();
                            let config = ConnectionConfig {
                                server_buffer_size: self.config.server_buffer_size,
                                connection_timeout: self.config.connection_timeout,
                            };
                            let metrics = self.metrics.clone();
                            let shutdown = self.shutdown.clone();

                            tokio::spawn(async move {
                                if let Err(e) = handle_connection(
                                    stream, registry, context, config, metrics.clone(), shutdown,
                                ).await {
                                    warn!(peer = %peer_addr, error = %e, "Connection error");
                                }
                                metrics.record_disconnection();
                                drop(permit);
                            });
                        }
                        Err(e) => {
                            error!(error = %e, "Failed to accept connection");
                            self.metrics.record_error();
                        }
                    }
                }
                _ = shutdown_rx.recv() => {
                    info!("TCP listener shutdown signal received");
                    break;
                }
            }
        }

        info!("OPC UA TCP server stopped");
        Ok(())
    }
}

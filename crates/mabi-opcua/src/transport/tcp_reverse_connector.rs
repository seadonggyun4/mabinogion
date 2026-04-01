//! OPC UA TCP reverse connector.

use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use tokio::net::TcpStream;
use tokio::sync::broadcast;
use tracing::{info, warn};

use crate::error::OpcUaResult;
use crate::transport::adapter::ConnectionInitiationMode;
use crate::transport::runtime::TransportRuntime;

/// Configuration for outbound UA-TCP reverse-connect sessions.
#[derive(Debug, Clone)]
pub struct TcpReverseConnectConfig {
    /// Remote listener the server should connect to.
    pub target_address: SocketAddr,
    /// Fixed retry interval between connect attempts.
    pub retry_interval: Duration,
    /// Timeout applied to outbound connect and per-connection idle handling.
    pub connection_timeout: Duration,
    /// Server buffer size used during Hello/Acknowledge negotiation.
    pub server_buffer_size: u32,
    /// Internal initiation seam, kept aligned with public config.
    pub(crate) initiation_mode: ConnectionInitiationMode,
}

impl Default for TcpReverseConnectConfig {
    fn default() -> Self {
        Self {
            target_address: SocketAddr::from(([127, 0, 0, 1], 4840)),
            retry_interval: Duration::from_secs(5),
            connection_timeout: Duration::from_secs(60),
            server_buffer_size: 65_535,
            initiation_mode: ConnectionInitiationMode::ReverseConnect,
        }
    }
}

/// Reverse-connect adapter that repeatedly opens outbound UA-TCP connections.
pub struct TcpReverseConnector {
    config: TcpReverseConnectConfig,
    runtime: Arc<TransportRuntime>,
    shutdown: Arc<AtomicBool>,
    shutdown_tx: broadcast::Sender<()>,
}

impl TcpReverseConnector {
    pub(crate) fn new(config: TcpReverseConnectConfig, runtime: Arc<TransportRuntime>) -> Self {
        let (shutdown_tx, _) = broadcast::channel(1);
        Self {
            config,
            runtime,
            shutdown: Arc::new(AtomicBool::new(false)),
            shutdown_tx,
        }
    }

    pub fn metrics(&self) -> &Arc<crate::transport::metrics::TransportMetrics> {
        self.runtime.metrics()
    }

    pub fn shutdown(&self) {
        self.shutdown.store(true, Ordering::SeqCst);
        let _ = self.shutdown_tx.send(());
    }

    pub async fn run(&self) -> OpcUaResult<()> {
        info!(
            target = %self.config.target_address,
            retry_ms = self.config.retry_interval.as_millis(),
            server_buffer_size = self.config.server_buffer_size,
            "OPC UA TCP reverse connector running"
        );
        let mut shutdown_rx = self.shutdown_tx.subscribe();

        loop {
            if self.shutdown.load(Ordering::Relaxed) {
                break;
            }

            let connect_result = tokio::select! {
                result = tokio::time::timeout(
                    self.config.connection_timeout,
                    TcpStream::connect(self.config.target_address),
                ) => result,
                _ = shutdown_rx.recv() => break,
            };

            let stream = match connect_result {
                Ok(Ok(stream)) => stream,
                Ok(Err(error)) => {
                    self.runtime.record_error();
                    warn!(
                        target = %self.config.target_address,
                        error = %error,
                        "OPC UA reverse-connect attempt failed"
                    );
                    if !self.wait_for_retry(&mut shutdown_rx).await {
                        break;
                    }
                    continue;
                }
                Err(_) => {
                    self.runtime.record_error();
                    warn!(
                        target = %self.config.target_address,
                        timeout_ms = self.config.connection_timeout.as_millis(),
                        "OPC UA reverse-connect attempt timed out"
                    );
                    if !self.wait_for_retry(&mut shutdown_rx).await {
                        break;
                    }
                    continue;
                }
            };

            info!(target = %self.config.target_address, "OPC UA reverse-connect established");
            if let Err(error) = self
                .runtime
                .handle_tcp_stream(stream, self.shutdown.clone())
                .await
            {
                self.runtime.record_error();
                warn!(
                    target = %self.config.target_address,
                    error = %error,
                    "OPC UA reverse-connect session ended with error"
                );
            }

            if !self.wait_for_retry(&mut shutdown_rx).await {
                break;
            }
        }

        info!(target = %self.config.target_address, "OPC UA TCP reverse connector stopped");
        Ok(())
    }

    async fn wait_for_retry(&self, shutdown_rx: &mut broadcast::Receiver<()>) -> bool {
        if self.shutdown.load(Ordering::Relaxed) {
            return false;
        }

        tokio::select! {
            _ = tokio::time::sleep(self.config.retry_interval) => true,
            _ = shutdown_rx.recv() => false,
        }
    }
}

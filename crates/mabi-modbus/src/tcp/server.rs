//! Modbus TCP server implementation (version 2).
//!
//! This module provides a high-performance, extensible Modbus TCP server with:
//!
//! - Connection pooling with limits
//! - Modular handler architecture
//! - Metrics collection
//! - Graceful shutdown support
//! - Device management

use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use dashmap::DashMap;
use futures::{SinkExt, StreamExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{broadcast, Semaphore};
use tokio_util::codec::Framed;
use tracing::{debug, error, info, instrument, warn};

use crate::config::ModbusServerConfig;
use crate::device::ModbusDevice;
use crate::error::ModbusResult;
use crate::fault_injection::connection_disruption::{
    ConnectionDisruptionConfig, ConnectionDisruptionState, DisruptionAction,
};
use crate::fault_injection::{FaultAction, FaultPipeline, ModbusFaultContext};
use crate::handler::{build_exception_pdu, ExceptionCode, HandlerContext, HandlerRegistry};
use crate::register::RegisterStore;

use super::codec::{MbapCodec, MbapFrame};
use super::connection::ConnectionPool;
use super::metrics::{LatencyTimer, ServerMetrics};

/// Configuration for the Modbus TCP server v2.
#[derive(Debug, Clone)]
pub struct ServerConfigV2 {
    /// Address to bind to.
    pub bind_address: SocketAddr,

    /// Maximum concurrent connections.
    pub max_connections: usize,

    /// Connection timeout (idle connections will be closed).
    pub connection_timeout: Duration,

    /// Request processing timeout.
    pub request_timeout: Duration,

    /// Enable TCP keep-alive.
    pub tcp_keepalive: Option<Duration>,

    /// Enable TCP nodelay (disable Nagle algorithm).
    pub tcp_nodelay: bool,

    /// Shutdown timeout (time to wait for connections to close).
    pub shutdown_timeout: Duration,
}

impl Default for ServerConfigV2 {
    fn default() -> Self {
        Self {
            bind_address: "0.0.0.0:502".parse().unwrap(),
            max_connections: 10_000,
            connection_timeout: Duration::from_secs(300),
            request_timeout: Duration::from_secs(5),
            tcp_keepalive: Some(Duration::from_secs(60)),
            tcp_nodelay: true,
            shutdown_timeout: Duration::from_secs(30),
        }
    }
}

impl From<ModbusServerConfig> for ServerConfigV2 {
    fn from(config: ModbusServerConfig) -> Self {
        Self {
            bind_address: config.bind_address,
            max_connections: config.max_connections,
            connection_timeout: Duration::from_secs(config.timeout_secs),
            request_timeout: Duration::from_secs(5),
            tcp_keepalive: if config.keep_alive {
                Some(Duration::from_secs(60))
            } else {
                None
            },
            tcp_nodelay: config.tcp_nodelay,
            shutdown_timeout: Duration::from_secs(30),
        }
    }
}

/// Server event types.
#[derive(Debug, Clone)]
pub enum ServerEvent {
    /// Server started.
    Started { address: SocketAddr },
    /// Server stopped.
    Stopped,
    /// Error occurred.
    Error { message: String },
}

/// Modbus TCP server (version 2).
///
/// This server provides a production-ready Modbus TCP implementation with:
///
/// - Connection management with limits and tracking
/// - Extensible handler architecture
/// - Multi-device support
/// - Comprehensive metrics
/// - Graceful shutdown
pub struct ModbusTcpServerV2 {
    /// Server configuration.
    config: ServerConfigV2,

    /// Handler registry for function codes.
    handlers: Arc<HandlerRegistry>,

    /// Devices by unit ID.
    devices: DashMap<u8, Arc<ModbusDevice>>,

    /// Default register store (for unit ID 0 / unknown units).
    default_registers: Arc<RegisterStore>,

    /// Connection pool.
    connections: Arc<ConnectionPool>,

    /// Server metrics.
    metrics: Arc<ServerMetrics>,

    /// Connection semaphore for limiting concurrent connections.
    connection_semaphore: Arc<Semaphore>,

    /// Shutdown flag.
    shutdown: Arc<AtomicBool>,

    /// Shutdown signal broadcaster.
    shutdown_tx: broadcast::Sender<()>,

    /// Event broadcaster.
    event_tx: broadcast::Sender<ServerEvent>,

    /// Optional fault injection pipeline.
    fault_pipeline: Option<Arc<FaultPipeline>>,

    /// Optional TCP connection disruption config.
    connection_disruption: Option<Arc<ConnectionDisruptionConfig>>,
}

impl ModbusTcpServerV2 {
    /// Create a new Modbus TCP server with default handlers.
    pub fn new(config: ServerConfigV2) -> Self {
        let (shutdown_tx, _) = broadcast::channel(1);
        let (event_tx, _) = broadcast::channel(64);

        Self {
            connection_semaphore: Arc::new(Semaphore::new(config.max_connections)),
            connections: Arc::new(ConnectionPool::new(config.max_connections)),
            config,
            handlers: Arc::new(HandlerRegistry::with_defaults()),
            devices: DashMap::new(),
            default_registers: Arc::new(RegisterStore::with_defaults()),
            metrics: Arc::new(ServerMetrics::new()),
            shutdown: Arc::new(AtomicBool::new(false)),
            shutdown_tx,
            event_tx,
            fault_pipeline: None,
            connection_disruption: None,
        }
    }

    /// Create a new server from old config.
    pub fn from_config(config: ModbusServerConfig) -> Self {
        Self::new(config.into())
    }

    /// Set custom handler registry.
    pub fn with_handlers(mut self, handlers: HandlerRegistry) -> Self {
        self.handlers = Arc::new(handlers);
        self
    }

    /// Set fault injection pipeline.
    pub fn with_fault_pipeline(mut self, pipeline: FaultPipeline) -> Self {
        self.fault_pipeline = Some(Arc::new(pipeline));
        self
    }

    /// Set TCP connection disruption configuration.
    pub fn with_connection_disruption(mut self, config: ConnectionDisruptionConfig) -> Self {
        self.connection_disruption = Some(Arc::new(config));
        self
    }

    /// Set default register store.
    pub fn with_default_registers(mut self, registers: RegisterStore) -> Self {
        self.default_registers = Arc::new(registers);
        self
    }

    /// Add a device to the server.
    pub fn add_device(&self, device: ModbusDevice) {
        let unit_id = device.unit_id();
        self.devices.insert(unit_id, Arc::new(device));
    }

    /// Remove a device from the server.
    pub fn remove_device(&self, unit_id: u8) -> Option<Arc<ModbusDevice>> {
        self.devices.remove(&unit_id).map(|(_, d)| d)
    }

    /// Get a device by unit ID.
    pub fn device(&self, unit_id: u8) -> Option<Arc<ModbusDevice>> {
        self.devices.get(&unit_id).map(|d| d.clone())
    }

    /// Get the default register store.
    pub fn default_registers(&self) -> &Arc<RegisterStore> {
        &self.default_registers
    }

    /// Get server metrics.
    pub fn metrics(&self) -> &Arc<ServerMetrics> {
        &self.metrics
    }

    /// Get connection pool.
    pub fn connections(&self) -> &Arc<ConnectionPool> {
        &self.connections
    }

    /// Subscribe to server events.
    pub fn subscribe(&self) -> broadcast::Receiver<ServerEvent> {
        self.event_tx.subscribe()
    }

    /// Check if shutdown has been requested.
    pub fn is_shutdown(&self) -> bool {
        self.shutdown.load(Ordering::SeqCst)
    }

    /// Request server shutdown.
    pub fn shutdown(&self) {
        if !self.shutdown.swap(true, Ordering::SeqCst) {
            info!("Shutdown requested");
            let _ = self.shutdown_tx.send(());
        }
    }

    /// Run the server.
    #[instrument(skip(self))]
    pub async fn run(&self) -> ModbusResult<()> {
        let listener = TcpListener::bind(self.config.bind_address).await?;
        info!(address = %self.config.bind_address, "Modbus TCP server started");

        let _ = self.event_tx.send(ServerEvent::Started {
            address: self.config.bind_address,
        });

        let mut shutdown_rx = self.shutdown_tx.subscribe();

        loop {
            tokio::select! {
                // Accept new connections
                result = listener.accept() => {
                    match result {
                        Ok((stream, peer_addr)) => {
                            self.handle_new_connection(stream, peer_addr).await;
                        }
                        Err(e) => {
                            error!(error = %e, "Failed to accept connection");
                            self.metrics.record_error();
                        }
                    }
                }

                // Handle shutdown signal
                _ = shutdown_rx.recv() => {
                    info!("Shutdown signal received");
                    break;
                }
            }
        }

        // Graceful shutdown
        self.graceful_shutdown().await;

        let _ = self.event_tx.send(ServerEvent::Stopped);
        info!("Modbus TCP server stopped");

        Ok(())
    }

    /// Handle a new incoming connection.
    async fn handle_new_connection(&self, stream: TcpStream, peer_addr: SocketAddr) {
        // Try to acquire connection permit
        let permit = match self.connection_semaphore.clone().try_acquire_owned() {
            Ok(permit) => permit,
            Err(_) => {
                warn!(peer = %peer_addr, "Max connections reached, rejecting");
                self.metrics.record_connection_rejected();
                return;
            }
        };

        // Register connection
        let connection_id = match self.connections.try_register(peer_addr) {
            Some(id) => id,
            None => {
                warn!(peer = %peer_addr, "Connection pool full, rejecting");
                self.metrics.record_connection_rejected();
                return;
            }
        };

        self.metrics.record_connection();

        // Spawn connection handler
        let handlers = self.handlers.clone();
        let devices = self.devices.clone();
        let default_registers = self.default_registers.clone();
        let connections = self.connections.clone();
        let metrics = self.metrics.clone();
        let shutdown = self.shutdown.clone();
        let config = self.config.clone();
        let fault_pipeline = self.fault_pipeline.clone();
        let connection_disruption = self.connection_disruption.clone();

        tokio::spawn(async move {
            let result = handle_connection(
                stream,
                peer_addr,
                connection_id,
                handlers,
                devices,
                default_registers,
                connections.clone(),
                metrics.clone(),
                shutdown,
                config,
                fault_pipeline,
                connection_disruption,
            )
            .await;

            if let Err(e) = result {
                debug!(peer = %peer_addr, error = %e, "Connection handler error");
            }

            connections.unregister(connection_id);
            metrics.record_disconnection();
            drop(permit);
        });
    }

    /// Perform graceful shutdown.
    async fn graceful_shutdown(&self) {
        info!("Starting graceful shutdown");

        // Wait for connections to close (with timeout)
        let start = std::time::Instant::now();
        loop {
            let active = self.connections.active_count();
            if active == 0 {
                info!("All connections closed");
                break;
            }

            if start.elapsed() > self.config.shutdown_timeout {
                warn!(
                    active_connections = active,
                    "Shutdown timeout reached, forcing close"
                );
                break;
            }

            debug!(active_connections = active, "Waiting for connections to close");
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }
}

/// Handle a single connection.
async fn handle_connection(
    stream: TcpStream,
    peer_addr: SocketAddr,
    connection_id: u64,
    handlers: Arc<HandlerRegistry>,
    devices: DashMap<u8, Arc<ModbusDevice>>,
    default_registers: Arc<RegisterStore>,
    connections: Arc<ConnectionPool>,
    metrics: Arc<ServerMetrics>,
    shutdown: Arc<AtomicBool>,
    config: ServerConfigV2,
    fault_pipeline: Option<Arc<FaultPipeline>>,
    connection_disruption: Option<Arc<ConnectionDisruptionConfig>>,
) -> ModbusResult<()> {
    debug!(peer = %peer_addr, connection_id, "Connection established");

    // Configure TCP socket
    if config.tcp_nodelay {
        stream.set_nodelay(true)?;
    }

    // Create framed codec
    let mut framed = Framed::new(stream, MbapCodec::new());
    let mut request_number: u64 = 0;

    // Initialize connection disruption state if configured
    let disruption_state = connection_disruption
        .as_ref()
        .map(|cfg| ConnectionDisruptionState::new((**cfg).clone()));

    loop {
        // Check shutdown
        if shutdown.load(Ordering::SeqCst) {
            debug!(peer = %peer_addr, "Shutdown requested, closing connection");
            break;
        }

        // Read frame with timeout
        let read_result = tokio::time::timeout(config.connection_timeout, framed.next()).await;

        let frame = match read_result {
            Ok(Some(Ok(frame))) => frame,
            Ok(Some(Err(e))) => {
                debug!(peer = %peer_addr, error = %e, "Frame decode error");
                metrics.record_frame_error();
                continue;
            }
            Ok(None) => {
                debug!(peer = %peer_addr, "Connection closed by client");
                break;
            }
            Err(_) => {
                debug!(peer = %peer_addr, "Connection timeout");
                metrics.record_timeout();
                break;
            }
        };

        // Process request
        let timer = LatencyTimer::start();
        let request_bytes = frame.frame_size() as u64;
        let unit_id = frame.header.unit_id;
        let function_code = frame.function_code().unwrap_or(0);

        metrics.record_request(function_code);

        // Get registers for this unit
        let registers = if let Some(device) = devices.get(&unit_id) {
            device.registers().clone()
        } else if unit_id == 0 {
            default_registers.clone()
        } else {
            // Unknown unit - send exception
            let exception_pdu = build_exception_pdu(
                function_code,
                ExceptionCode::GatewayTargetDeviceFailedToRespond,
            );

            let response = MbapFrame::response(&frame, exception_pdu);
            let response_bytes = response.frame_size() as u64;

            if let Err(e) = framed.send(response).await {
                warn!(peer = %peer_addr, error = %e, "Failed to send exception response");
                break;
            }

            let latency = timer.elapsed_us();
            metrics.record_exception(latency, request_bytes, response_bytes);
            connections.record_request(
                connection_id,
                unit_id,
                function_code,
                false,
                latency,
                request_bytes,
                response_bytes,
            );

            continue;
        };

        // Create handler context
        let ctx = HandlerContext::new(unit_id, registers, frame.header.transaction_id);

        // Process request with timeout
        let process_result = tokio::time::timeout(
            config.request_timeout,
            async {
                handlers.dispatch(&frame.pdu, &ctx)
            }
        ).await;

        let response_pdu = match process_result {
            Ok(Ok(pdu)) => pdu,
            Ok(Err(exception_code)) => {
                build_exception_pdu(function_code, exception_code)
            }
            Err(_) => {
                warn!(peer = %peer_addr, "Request processing timeout");
                metrics.record_timeout();
                build_exception_pdu(function_code, ExceptionCode::SlaveDeviceBusy)
            }
        };

        // Apply fault injection pipeline (if configured)
        request_number += 1;
        let fault_action = if let Some(ref pipeline) = fault_pipeline {
            let fault_ctx = ModbusFaultContext::tcp(
                unit_id,
                function_code,
                &frame.pdu,
                &response_pdu,
                frame.header.transaction_id,
                request_number,
            );
            pipeline.apply(&fault_ctx)
        } else {
            None
        };

        match fault_action {
            Some(FaultAction::DropResponse) => {
                // Silent drop - no response sent
                debug!(peer = %peer_addr, unit_id, fc = function_code, "Fault: dropping response");
                let latency = timer.elapsed_us();
                metrics.record_success(latency, request_bytes, 0);
                continue;
            }
            Some(FaultAction::DelayThenSend { delay, response: fault_pdu }) => {
                tokio::time::sleep(delay).await;
                let is_exception = fault_pdu.first().map(|&fc| fc & 0x80 != 0).unwrap_or(false);
                let response = MbapFrame::response(&frame, fault_pdu);
                let response_bytes = response.frame_size() as u64;
                if let Err(e) = framed.send(response).await {
                    warn!(peer = %peer_addr, error = %e, "Failed to send delayed response");
                    break;
                }
                let latency = timer.elapsed_us();
                if is_exception {
                    metrics.record_exception(latency, request_bytes, response_bytes);
                } else {
                    metrics.record_success(latency, request_bytes, response_bytes);
                }
                connections.record_request(connection_id, unit_id, function_code, !is_exception, latency, request_bytes, response_bytes);
            }
            Some(FaultAction::OverrideTransactionId { transaction_id, response: fault_pdu }) => {
                let is_exception = fault_pdu.first().map(|&fc| fc & 0x80 != 0).unwrap_or(false);
                let mut response = MbapFrame::response(&frame, fault_pdu);
                response.header.transaction_id = transaction_id;
                let response_bytes = response.frame_size() as u64;
                if let Err(e) = framed.send(response).await {
                    warn!(peer = %peer_addr, error = %e, "Failed to send response with overridden TID");
                    break;
                }
                let latency = timer.elapsed_us();
                if is_exception {
                    metrics.record_exception(latency, request_bytes, response_bytes);
                } else {
                    metrics.record_success(latency, request_bytes, response_bytes);
                }
                connections.record_request(connection_id, unit_id, function_code, !is_exception, latency, request_bytes, response_bytes);
            }
            Some(FaultAction::SendRawBytes(raw_bytes)) => {
                // For TCP: raw bytes include the complete MBAP frame, send directly
                use tokio::io::AsyncWriteExt;
                let inner = framed.get_mut();
                let response_bytes = raw_bytes.len() as u64;
                if let Err(e) = inner.write_all(&raw_bytes).await {
                    warn!(peer = %peer_addr, error = %e, "Failed to send raw bytes");
                    break;
                }
                let _ = inner.flush().await;
                let latency = timer.elapsed_us();
                metrics.record_success(latency, request_bytes, response_bytes);
                connections.record_request(connection_id, unit_id, function_code, true, latency, request_bytes, response_bytes);
            }
            Some(FaultAction::SendResponse(fault_pdu)) => {
                let is_exception = fault_pdu.first().map(|&fc| fc & 0x80 != 0).unwrap_or(false);
                let response = MbapFrame::response(&frame, fault_pdu);
                let response_bytes = response.frame_size() as u64;
                if let Err(e) = framed.send(response).await {
                    warn!(peer = %peer_addr, error = %e, "Failed to send faulted response");
                    break;
                }
                let latency = timer.elapsed_us();
                if is_exception {
                    metrics.record_exception(latency, request_bytes, response_bytes);
                } else {
                    metrics.record_success(latency, request_bytes, response_bytes);
                }
                connections.record_request(connection_id, unit_id, function_code, !is_exception, latency, request_bytes, response_bytes);
            }
            Some(FaultAction::SendPartial { bytes }) => {
                // Partial frames are RTU-only, but handle gracefully for TCP
                use tokio::io::AsyncWriteExt;
                let inner = framed.get_mut();
                let response_bytes = bytes.len() as u64;
                if let Err(e) = inner.write_all(&bytes).await {
                    warn!(peer = %peer_addr, error = %e, "Failed to send partial bytes");
                    break;
                }
                let _ = inner.flush().await;
                let latency = timer.elapsed_us();
                metrics.record_success(latency, request_bytes, response_bytes);
                connections.record_request(connection_id, unit_id, function_code, true, latency, request_bytes, response_bytes);
            }
            None => {
                // No fault — normal response path
                let is_exception = response_pdu.first().map(|&fc| fc & 0x80 != 0).unwrap_or(false);
                let response = MbapFrame::response(&frame, response_pdu);
                let response_bytes = response.frame_size() as u64;

                if let Err(e) = framed.send(response).await {
                    warn!(peer = %peer_addr, error = %e, "Failed to send response");
                    break;
                }

                // Record metrics
                let latency = timer.elapsed_us();
                if is_exception {
                    metrics.record_exception(latency, request_bytes, response_bytes);
                } else {
                    metrics.record_success(latency, request_bytes, response_bytes);
                }

                connections.record_request(
                    connection_id,
                    unit_id,
                    function_code,
                    !is_exception,
                    latency,
                    request_bytes,
                    response_bytes,
                );
            }
        }

        // Apply connection disruption (if configured)
        if let Some(ref state) = disruption_state {
            match state.record_request() {
                DisruptionAction::None => {}
                DisruptionAction::Disconnect { close_delay, use_rst: _ } => {
                    debug!(peer = %peer_addr, "Connection disruption: disconnect");
                    if let Some(delay) = close_delay {
                        tokio::time::sleep(delay).await;
                    }
                    // Drop the framed transport to close the connection.
                    // TCP RST vs FIN is OS-controlled; the abrupt drop without
                    // a graceful shutdown will typically produce a RST if there's
                    // unread data in the receive buffer.
                    break;
                }
                DisruptionAction::DropMidFrame { close_delay, use_rst: _ } => {
                    debug!(peer = %peer_addr, "Connection disruption: drop mid-frame");
                    if let Some(delay) = close_delay {
                        tokio::time::sleep(delay).await;
                    }
                    break;
                }
                DisruptionAction::RstAfterPartial { byte_count, close_delay, use_rst: _ } => {
                    debug!(peer = %peer_addr, byte_count, "Connection disruption: RST after partial");
                    // Send partial garbage bytes then close
                    use tokio::io::AsyncWriteExt;
                    let garbage: Vec<u8> = (0..byte_count).map(|i| i as u8).collect();
                    let inner = framed.get_mut();
                    let _ = inner.write_all(&garbage).await;
                    let _ = inner.flush().await;
                    if let Some(delay) = close_delay {
                        tokio::time::sleep(delay).await;
                    }
                    break;
                }
                DisruptionAction::HoldOpen { duration } => {
                    debug!(peer = %peer_addr, ?duration, "Connection disruption: hold open");
                    state.set_holding_open(true);
                    tokio::time::sleep(duration).await;
                    state.set_holding_open(false);
                    break;
                }
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ModbusDeviceConfig;

    #[tokio::test]
    async fn test_server_creation() {
        let config = ServerConfigV2::default();
        let server = ModbusTcpServerV2::new(config);

        assert!(!server.is_shutdown());
        assert_eq!(server.connections().active_count(), 0);
    }

    #[tokio::test]
    async fn test_device_management() {
        let server = ModbusTcpServerV2::new(ServerConfigV2::default());

        // Add device
        let device = ModbusDevice::new(ModbusDeviceConfig::new(5, "Test"));
        server.add_device(device);

        assert!(server.device(5).is_some());
        assert!(server.device(10).is_none());

        // Remove device
        let removed = server.remove_device(5);
        assert!(removed.is_some());
        assert!(server.device(5).is_none());
    }

    #[tokio::test]
    async fn test_shutdown_flag() {
        let server = ModbusTcpServerV2::new(ServerConfigV2::default());

        assert!(!server.is_shutdown());
        server.shutdown();
        assert!(server.is_shutdown());

        // Multiple shutdowns should be safe
        server.shutdown();
        assert!(server.is_shutdown());
    }

    // Integration test with actual TCP connection would require more setup
    // and is better suited for integration tests in the tests/ directory
}

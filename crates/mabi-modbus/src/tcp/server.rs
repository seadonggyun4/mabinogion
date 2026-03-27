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
use serde::{Deserialize, Serialize};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{broadcast, Semaphore};
use tokio_util::codec::Framed;
use tracing::{debug, error, info, instrument, warn};

use crate::config::ModbusServerConfig;
use crate::context::{BroadcastPolicy, ServerContext, SharedAddressSpace};
use crate::device::ModbusDevice;
use crate::error::ModbusResult;
use crate::fault_injection::connection_disruption::{
    ConnectionDisruptionConfig, ConnectionDisruptionState, DisruptionAction,
};
use crate::fault_injection::{FaultAction, FaultPipeline, ModbusFaultContext};
use crate::handler::{ExceptionCode, HandlerRegistry};
use crate::register::RegisterStore;
use crate::service::{
    execute_transport_request, ExtensionRegistry, StandardModbusService, TransportDisposition,
    TransportServicePolicy, UnknownUnitBehavior,
};
use crate::transport_runtime::TransportHookBundle;

use super::codec::{MbapCodec, MbapFrame};
use super::connection::{ConnectionPool, LifecycleEventOptions, RequestRecordOptions};
use super::metrics::{LatencyTimer, ServerMetrics};

/// Performance preset for the TCP server.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum PerformancePreset {
    /// Full observability and compatibility behavior.
    #[default]
    Default,
    /// Lower-overhead request processing for high-throughput workloads.
    HighThroughput,
}

impl PerformancePreset {
    #[inline]
    fn runtime_policy(self) -> TcpRuntimePolicy {
        TcpRuntimePolicy::resolve(self)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EventEmissionMode {
    SubscriberAware,
}

impl EventEmissionMode {
    #[inline]
    fn should_emit(self, subscriber_count: usize) -> bool {
        subscriber_count > 0
    }
}

#[derive(Debug, Clone, Copy)]
struct TcpRuntimePolicy {
    enforce_request_timeout: bool,
    detailed_metrics: bool,
    latency_sample_mask: u64,
    connection_metadata_sample_mask: u64,
    server_event_mode: EventEmissionMode,
    lifecycle_event_mode: EventEmissionMode,
}

impl TcpRuntimePolicy {
    fn resolve(preset: PerformancePreset) -> Self {
        match preset {
            PerformancePreset::Default => Self {
                enforce_request_timeout: true,
                detailed_metrics: true,
                latency_sample_mask: 0,
                connection_metadata_sample_mask: 0,
                server_event_mode: EventEmissionMode::SubscriberAware,
                lifecycle_event_mode: EventEmissionMode::SubscriberAware,
            },
            PerformancePreset::HighThroughput => Self {
                enforce_request_timeout: true,
                detailed_metrics: true,
                latency_sample_mask: 0,
                // Sample connection metadata updates so short-lived connections do
                // not pay a full wall-clock/update-unit-access cost per request.
                connection_metadata_sample_mask: 0x0f,
                server_event_mode: EventEmissionMode::SubscriberAware,
                lifecycle_event_mode: EventEmissionMode::SubscriberAware,
            },
        }
    }

    #[inline]
    fn request_timeout(self, request_timeout: Duration) -> Option<Duration> {
        self.enforce_request_timeout.then_some(request_timeout)
    }

    #[inline]
    fn detailed_metrics(self) -> bool {
        self.detailed_metrics
    }

    #[cfg(test)]
    #[inline]
    fn should_record_latency(self, request_number: u64) -> bool {
        request_number & self.latency_sample_mask == 0
    }

    #[inline]
    fn should_emit_server_events(self, subscriber_count: usize) -> bool {
        self.server_event_mode.should_emit(subscriber_count)
    }

    #[inline]
    fn lifecycle_event_options(self, subscriber_count: usize) -> LifecycleEventOptions {
        if self.lifecycle_event_mode.should_emit(subscriber_count) {
            LifecycleEventOptions::enabled()
        } else {
            LifecycleEventOptions::disabled()
        }
    }

    #[cfg(test)]
    #[inline]
    fn connection_record_options(self, request_number: u64) -> RequestRecordOptions {
        if self.connection_metadata_sample_mask == 0 {
            RequestRecordOptions::default()
        } else {
            RequestRecordOptions {
                update_last_activity: request_number & self.connection_metadata_sample_mask == 0,
                track_unit_access: false,
                emit_event: false,
            }
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct TcpRequestHooks {
    transport: TransportHookBundle,
    detailed_metrics: bool,
    latency_sample_mask: u64,
    connection_metadata_sample_mask: u64,
}

impl TcpRequestHooks {
    fn new(policy: TcpRuntimePolicy, request_timeout: Duration) -> Self {
        Self {
            transport: TransportHookBundle::new()
                .with_request_timeout(policy.request_timeout(request_timeout)),
            detailed_metrics: policy.detailed_metrics(),
            latency_sample_mask: policy.latency_sample_mask,
            connection_metadata_sample_mask: policy.connection_metadata_sample_mask,
        }
    }

    #[inline]
    fn should_record_latency(self, request_number: u64) -> bool {
        request_number & self.latency_sample_mask == 0
    }

    #[inline]
    fn connection_record_options(self, request_number: u64) -> RequestRecordOptions {
        if self.connection_metadata_sample_mask == 0 {
            RequestRecordOptions::default()
        } else {
            RequestRecordOptions {
                update_last_activity: request_number & self.connection_metadata_sample_mask == 0,
                track_unit_access: false,
                emit_event: false,
            }
        }
    }
}

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

    /// Performance tuning preset for request processing.
    pub performance_preset: PerformancePreset,
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
            performance_preset: PerformancePreset::Default,
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
            performance_preset: PerformancePreset::Default,
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

    /// Request execution service.
    service: Arc<StandardModbusService>,

    /// Devices by unit ID.
    devices: DashMap<u8, Arc<ModbusDevice>>,

    /// Shared server context for request routing.
    server_context: Arc<ServerContext>,

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
            service: Arc::new(StandardModbusService::default()),
            devices: DashMap::new(),
            server_context: Arc::new(ServerContext::new(Arc::new(RegisterStore::with_defaults()))),
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
        self.service = Arc::new(StandardModbusService::new(handlers));
        self
    }

    /// Set a typed extension registry.
    pub fn with_extensions(mut self, extensions: ExtensionRegistry) -> Self {
        self.service = Arc::new(StandardModbusService::with_extensions(extensions));
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
    pub fn with_default_registers(self, registers: RegisterStore) -> Self {
        self.server_context.set_default_space(Arc::new(registers));
        self
    }

    /// Add a device to the server.
    pub fn add_device(&self, device: ModbusDevice) {
        let unit_id = device.unit_id();
        let device = Arc::new(device);
        self.server_context.register(device.context().clone());
        self.devices.insert(unit_id, device);
    }

    /// Remove a device from the server.
    pub fn remove_device(&self, unit_id: u8) -> Option<Arc<ModbusDevice>> {
        self.server_context.remove(unit_id);
        self.devices.remove(&unit_id).map(|(_, d)| d)
    }

    /// Get a device by unit ID.
    pub fn device(&self, unit_id: u8) -> Option<Arc<ModbusDevice>> {
        self.devices.get(&unit_id).map(|d| d.clone())
    }

    /// Get all configured unit IDs.
    pub fn device_ids(&self) -> Vec<u8> {
        self.devices.iter().map(|entry| *entry.key()).collect()
    }

    /// Get the default register space.
    pub fn default_registers(&self) -> SharedAddressSpace {
        self.server_context.default_space()
    }

    /// Set whether broadcast requests are accepted.
    pub fn set_broadcast_enabled(&self, enabled: bool) {
        self.server_context.set_broadcast_enabled(enabled);
    }

    /// Set the canonical broadcast routing policy.
    pub fn set_broadcast_policy(&self, policy: BroadcastPolicy) {
        self.server_context.set_broadcast_policy(policy);
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
        let runtime_policy = self.config.performance_preset.runtime_policy();
        info!(address = %self.config.bind_address, "Modbus TCP server started");

        if runtime_policy.should_emit_server_events(self.event_tx.receiver_count()) {
            let _ = self.event_tx.send(ServerEvent::Started {
                address: self.config.bind_address,
            });
        }

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

        if runtime_policy.should_emit_server_events(self.event_tx.receiver_count()) {
            let _ = self.event_tx.send(ServerEvent::Stopped);
        }
        info!("Modbus TCP server stopped");

        Ok(())
    }

    /// Handle a new incoming connection.
    async fn handle_new_connection(&self, stream: TcpStream, peer_addr: SocketAddr) {
        let runtime_policy = self.config.performance_preset.runtime_policy();

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
        let connection_id = match self.connections.try_register_with_options(
            peer_addr,
            runtime_policy.lifecycle_event_options(self.connections.subscriber_count()),
        ) {
            Some(id) => id,
            None => {
                warn!(peer = %peer_addr, "Connection pool full, rejecting");
                self.metrics.record_connection_rejected();
                return;
            }
        };

        self.metrics.record_connection();

        // Spawn connection handler
        let service = self.service.clone();
        let server_context = self.server_context.clone();
        let connections = self.connections.clone();
        let metrics = self.metrics.clone();
        let shutdown = self.shutdown.clone();
        let config = self.config.clone();
        let fault_pipeline = self.fault_pipeline.clone();
        let connection_disruption = self.connection_disruption.clone();
        let lifecycle_policy = runtime_policy;

        tokio::spawn(async move {
            let result = handle_connection(
                stream,
                peer_addr,
                connection_id,
                service,
                server_context,
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

            connections.unregister_with_options(
                connection_id,
                lifecycle_policy.lifecycle_event_options(connections.subscriber_count()),
            );
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

            debug!(
                active_connections = active,
                "Waiting for connections to close"
            );
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }
}

/// Handle a single connection.
async fn handle_connection(
    stream: TcpStream,
    peer_addr: SocketAddr,
    connection_id: u64,
    service: Arc<StandardModbusService>,
    server_context: Arc<ServerContext>,
    connections: Arc<ConnectionPool>,
    metrics: Arc<ServerMetrics>,
    shutdown: Arc<AtomicBool>,
    config: ServerConfigV2,
    fault_pipeline: Option<Arc<FaultPipeline>>,
    connection_disruption: Option<Arc<ConnectionDisruptionConfig>>,
) -> ModbusResult<()> {
    debug!(peer = %peer_addr, connection_id, "Connection established");
    let runtime_policy = config.performance_preset.runtime_policy();
    let request_hooks = TcpRequestHooks::new(runtime_policy, config.request_timeout);

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
    let fast_response_path = fault_pipeline.is_none() && disruption_state.is_none();

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

        metrics.record_request_with_options(function_code, request_hooks.detailed_metrics);

        request_number += 1;
        let record_latency = request_hooks.should_record_latency(request_number);
        let connection_record_options = request_hooks.connection_record_options(request_number);

        let execution = execute_transport_request(
            service.as_ref(),
            server_context.as_ref(),
            unit_id,
            frame.header.transaction_id,
            frame.pdu.as_slice(),
            TransportServicePolicy::new(UnknownUnitBehavior::Exception(
                ExceptionCode::GatewayTargetDeviceFailedToRespond,
            ))
            .with_request_timeout(request_hooks.transport.request_timeout),
        )
        .await;

        if execution.timed_out {
            warn!(peer = %peer_addr, "Request processing timeout");
            metrics.record_timeout();
        }

        let execution_summary = execution.summary();
        let response_pdu = match execution.disposition {
            TransportDisposition::Ignore => continue,
            TransportDisposition::BroadcastSuppressed(_response) => {
                let latency = timer.elapsed_us();
                record_tcp_outcome(
                    metrics.as_ref(),
                    connections.as_ref(),
                    connection_id,
                    unit_id,
                    function_code,
                    latency,
                    request_bytes,
                    TcpRecordedOutcome::new(execution_summary.is_exception, 0),
                    record_latency,
                    connection_record_options,
                );
                continue;
            }
            TransportDisposition::Reply(response) => response.into_bytes(),
        };

        if fast_response_path {
            let response_bytes = match send_tcp_response(&mut framed, &frame, response_pdu).await {
                Ok(response_bytes) => response_bytes,
                Err(e) => {
                    warn!(peer = %peer_addr, error = %e, "Failed to send response");
                    break;
                }
            };

            let latency = timer.elapsed_us();
            record_tcp_outcome(
                metrics.as_ref(),
                connections.as_ref(),
                connection_id,
                unit_id,
                function_code,
                latency,
                request_bytes,
                TcpRecordedOutcome::new(execution_summary.is_exception, response_bytes),
                record_latency,
                connection_record_options,
            );
            continue;
        }

        // Apply fault injection pipeline (if configured)
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
                record_tcp_outcome(
                    metrics.as_ref(),
                    connections.as_ref(),
                    connection_id,
                    unit_id,
                    function_code,
                    latency,
                    request_bytes,
                    TcpRecordedOutcome::success(0),
                    record_latency,
                    connection_record_options,
                );
                continue;
            }
            Some(FaultAction::DelayThenSend {
                delay,
                response: fault_pdu,
            }) => {
                tokio::time::sleep(delay).await;
                let outcome = TcpRecordedOutcome::from_pdu(&fault_pdu, 0);
                let response_bytes = match send_tcp_response(&mut framed, &frame, fault_pdu).await {
                    Ok(response_bytes) => response_bytes,
                    Err(e) => {
                        warn!(peer = %peer_addr, error = %e, "Failed to send delayed response");
                        break;
                    }
                };
                let latency = timer.elapsed_us();
                record_tcp_outcome(
                    metrics.as_ref(),
                    connections.as_ref(),
                    connection_id,
                    unit_id,
                    function_code,
                    latency,
                    request_bytes,
                    outcome.with_response_bytes(response_bytes),
                    record_latency,
                    connection_record_options,
                );
            }
            Some(FaultAction::OverrideTransactionId {
                transaction_id,
                response: fault_pdu,
            }) => {
                let outcome = TcpRecordedOutcome::from_pdu(&fault_pdu, 0);
                let mut response = MbapFrame::response(&frame, fault_pdu);
                response.header.transaction_id = transaction_id;
                let response_bytes = response.frame_size() as u64;
                if let Err(e) = framed.send(response).await {
                    warn!(peer = %peer_addr, error = %e, "Failed to send response with overridden TID");
                    break;
                }
                let latency = timer.elapsed_us();
                record_tcp_outcome(
                    metrics.as_ref(),
                    connections.as_ref(),
                    connection_id,
                    unit_id,
                    function_code,
                    latency,
                    request_bytes,
                    outcome.with_response_bytes(response_bytes),
                    record_latency,
                    connection_record_options,
                );
            }
            Some(FaultAction::SendRawBytes(raw_bytes)) => {
                // For TCP: raw bytes include the complete MBAP frame, send directly
                let response_bytes = match write_tcp_raw_bytes(&mut framed, &raw_bytes).await {
                    Ok(response_bytes) => response_bytes,
                    Err(e) => {
                        warn!(peer = %peer_addr, error = %e, "Failed to send raw bytes");
                        break;
                    }
                };
                let latency = timer.elapsed_us();
                record_tcp_outcome(
                    metrics.as_ref(),
                    connections.as_ref(),
                    connection_id,
                    unit_id,
                    function_code,
                    latency,
                    request_bytes,
                    TcpRecordedOutcome::success(response_bytes),
                    record_latency,
                    connection_record_options,
                );
            }
            Some(FaultAction::SendResponse(fault_pdu)) => {
                let outcome = TcpRecordedOutcome::from_pdu(&fault_pdu, 0);
                let response_bytes = match send_tcp_response(&mut framed, &frame, fault_pdu).await {
                    Ok(response_bytes) => response_bytes,
                    Err(e) => {
                        warn!(peer = %peer_addr, error = %e, "Failed to send faulted response");
                        break;
                    }
                };
                let latency = timer.elapsed_us();
                record_tcp_outcome(
                    metrics.as_ref(),
                    connections.as_ref(),
                    connection_id,
                    unit_id,
                    function_code,
                    latency,
                    request_bytes,
                    outcome.with_response_bytes(response_bytes),
                    record_latency,
                    connection_record_options,
                );
            }
            Some(FaultAction::SendPartial { bytes }) => {
                // Partial frames are RTU-only, but handle gracefully for TCP
                let response_bytes = match write_tcp_raw_bytes(&mut framed, &bytes).await {
                    Ok(response_bytes) => response_bytes,
                    Err(e) => {
                        warn!(peer = %peer_addr, error = %e, "Failed to send partial bytes");
                        break;
                    }
                };
                let latency = timer.elapsed_us();
                record_tcp_outcome(
                    metrics.as_ref(),
                    connections.as_ref(),
                    connection_id,
                    unit_id,
                    function_code,
                    latency,
                    request_bytes,
                    TcpRecordedOutcome::success(response_bytes),
                    record_latency,
                    connection_record_options,
                );
            }
            None => {
                let response_bytes =
                    match send_tcp_response(&mut framed, &frame, response_pdu).await {
                        Ok(response_bytes) => response_bytes,
                        Err(e) => {
                            warn!(peer = %peer_addr, error = %e, "Failed to send response");
                            break;
                        }
                    };
                let latency = timer.elapsed_us();
                record_tcp_outcome(
                    metrics.as_ref(),
                    connections.as_ref(),
                    connection_id,
                    unit_id,
                    function_code,
                    latency,
                    request_bytes,
                    TcpRecordedOutcome::new(execution_summary.is_exception, response_bytes),
                    record_latency,
                    connection_record_options,
                );
            }
        }

        // Apply connection disruption (if configured)
        if let Some(ref state) = disruption_state {
            match state.record_request() {
                DisruptionAction::None => {}
                DisruptionAction::Disconnect {
                    close_delay,
                    use_rst: _,
                } => {
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
                DisruptionAction::DropMidFrame {
                    close_delay,
                    use_rst: _,
                } => {
                    debug!(peer = %peer_addr, "Connection disruption: drop mid-frame");
                    if let Some(delay) = close_delay {
                        tokio::time::sleep(delay).await;
                    }
                    break;
                }
                DisruptionAction::RstAfterPartial {
                    byte_count,
                    close_delay,
                    use_rst: _,
                } => {
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

#[derive(Debug, Clone, Copy)]
struct TcpRecordedOutcome {
    is_exception: bool,
    response_bytes: u64,
}

impl TcpRecordedOutcome {
    fn new(is_exception: bool, response_bytes: u64) -> Self {
        Self {
            is_exception,
            response_bytes,
        }
    }

    fn success(response_bytes: u64) -> Self {
        Self::new(false, response_bytes)
    }

    fn from_pdu(response_pdu: &[u8], response_bytes: u64) -> Self {
        Self::new(
            response_pdu
                .first()
                .map(|function_code| function_code & 0x80 != 0)
                .unwrap_or(false),
            response_bytes,
        )
    }

    fn with_response_bytes(self, response_bytes: u64) -> Self {
        Self {
            response_bytes,
            ..self
        }
    }
}

fn record_tcp_outcome(
    metrics: &ServerMetrics,
    connections: &ConnectionPool,
    connection_id: u64,
    unit_id: u8,
    function_code: u8,
    latency_us: u64,
    request_bytes: u64,
    outcome: TcpRecordedOutcome,
    record_latency: bool,
    connection_record_options: RequestRecordOptions,
) {
    if outcome.is_exception {
        metrics.record_exception_with_options(
            latency_us,
            request_bytes,
            outcome.response_bytes,
            record_latency,
        );
    } else {
        metrics.record_success_with_options(
            latency_us,
            request_bytes,
            outcome.response_bytes,
            record_latency,
        );
    }

    connections.record_request_with_options(
        connection_id,
        unit_id,
        function_code,
        !outcome.is_exception,
        latency_us,
        request_bytes,
        outcome.response_bytes,
        connection_record_options,
    );
}

async fn send_tcp_response(
    framed: &mut Framed<TcpStream, MbapCodec>,
    request_frame: &MbapFrame,
    response_pdu: Vec<u8>,
) -> ModbusResult<u64> {
    let response = MbapFrame::response(request_frame, response_pdu);
    let response_bytes = response.frame_size() as u64;
    framed.send(response).await?;
    Ok(response_bytes)
}

async fn write_tcp_raw_bytes(
    framed: &mut Framed<TcpStream, MbapCodec>,
    raw_bytes: &[u8],
) -> std::io::Result<u64> {
    use tokio::io::AsyncWriteExt;

    let inner = framed.get_mut();
    inner.write_all(raw_bytes).await?;
    inner.flush().await?;
    Ok(raw_bytes.len() as u64)
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

    #[test]
    fn test_runtime_policy_default_keeps_full_request_tracking() {
        let policy = PerformancePreset::Default.runtime_policy();
        let options = policy.connection_record_options(1);

        assert_eq!(
            policy.request_timeout(Duration::from_secs(1)),
            Some(Duration::from_secs(1))
        );
        assert!(policy.detailed_metrics());
        assert!(policy.should_record_latency(1));
        assert!(options.update_last_activity);
        assert!(options.track_unit_access);
        assert!(options.emit_event);
        assert!(!policy.should_emit_server_events(0));
        assert!(policy.should_emit_server_events(1));
    }

    #[test]
    fn test_runtime_policy_high_throughput_samples_connection_metadata() {
        let policy = PerformancePreset::HighThroughput.runtime_policy();
        let options = policy.connection_record_options(16);
        let unsampled_options = policy.connection_record_options(1);
        let no_subscribers = policy.lifecycle_event_options(0);
        let with_subscribers = policy.lifecycle_event_options(1);

        assert_eq!(
            policy.request_timeout(Duration::from_secs(1)),
            Some(Duration::from_secs(1))
        );
        assert!(policy.detailed_metrics());
        assert!(policy.should_record_latency(7));
        assert!(policy.should_record_latency(8));
        assert!(options.update_last_activity);
        assert!(!options.track_unit_access);
        assert!(!options.emit_event);
        assert!(!unsampled_options.update_last_activity);
        assert!(!unsampled_options.track_unit_access);
        assert!(!unsampled_options.emit_event);
        assert!(!no_subscribers.emit_connected);
        assert!(!no_subscribers.emit_disconnected);
        assert!(!no_subscribers.emit_rejected);
        assert!(with_subscribers.emit_connected);
        assert!(with_subscribers.emit_disconnected);
        assert!(with_subscribers.emit_rejected);
    }

    // Integration test with actual TCP connection would require more setup
    // and is better suited for integration tests in the tests/ directory
}

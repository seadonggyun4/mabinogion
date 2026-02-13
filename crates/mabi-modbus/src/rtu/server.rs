//! Modbus RTU server implementation.
//!
//! This module provides a high-performance RTU server with support for:
//!
//! - Multiple transport types (virtual serial, TCP bridge, channel)
//! - Multiple device simulation
//! - Extensible handler architecture
//! - Metrics and monitoring
//! - Graceful shutdown
//!
//! # Example
//!
//! ```rust,no_run
//! use mabi_modbus::rtu::{ModbusRtuServer, RtuServerConfig, VirtualSerialConfig};
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let config = RtuServerConfig::default()
//!         .with_unit_ids(vec![1, 2, 3]);
//!
//!     let server = ModbusRtuServer::new(config);
//!     server.run().await?;
//!     Ok(())
//! }
//! ```

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use dashmap::DashMap;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;
use tracing::{debug, error, info};

use crate::device::ModbusDevice;
use crate::error::{ModbusError, ModbusResult};
use crate::fault_injection::rtu_timing::RtuTimingFaultConfig;
use crate::fault_injection::{FaultAction, FaultPipeline, ModbusFaultContext};
use crate::handler::{build_exception_pdu, ExceptionCode, HandlerContext, HandlerRegistry};
use crate::register::RegisterStore;

use super::codec::RtuTiming;
use super::frame::{RtuFrame, RtuFrameError};
use super::transport::{ChannelConfig, ChannelTransport, RtuTransport, TransportConfig, TransportMetrics};

/// RTU server configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RtuServerConfig {
    /// Transport configuration.
    #[serde(default)]
    pub transport: TransportConfig,

    /// Supported unit IDs (empty = all).
    #[serde(default)]
    pub unit_ids: Vec<u8>,

    /// Enable broadcast (unit ID 0).
    #[serde(default = "default_broadcast")]
    pub broadcast_enabled: bool,

    /// Request processing timeout.
    #[serde(default = "default_request_timeout")]
    pub request_timeout: Duration,

    /// Shutdown timeout.
    #[serde(default = "default_shutdown_timeout")]
    pub shutdown_timeout: Duration,

    /// Enable response delay simulation.
    #[serde(default)]
    pub simulate_response_delay: bool,

    /// Additional response delay (beyond transmission time).
    #[serde(default)]
    pub additional_response_delay: Duration,
}

fn default_broadcast() -> bool {
    true
}

fn default_request_timeout() -> Duration {
    Duration::from_secs(5)
}

fn default_shutdown_timeout() -> Duration {
    Duration::from_secs(10)
}

impl Default for RtuServerConfig {
    fn default() -> Self {
        Self {
            transport: TransportConfig::default(),
            unit_ids: vec![1], // Default to unit ID 1
            broadcast_enabled: true,
            request_timeout: default_request_timeout(),
            shutdown_timeout: default_shutdown_timeout(),
            simulate_response_delay: true,
            additional_response_delay: Duration::ZERO,
        }
    }
}

impl RtuServerConfig {
    /// Create a new configuration with default settings.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set transport configuration.
    pub fn with_transport(mut self, transport: TransportConfig) -> Self {
        self.transport = transport;
        self
    }

    /// Set supported unit IDs.
    pub fn with_unit_ids(mut self, ids: Vec<u8>) -> Self {
        self.unit_ids = ids;
        self
    }

    /// Enable or disable broadcast support.
    pub fn with_broadcast(mut self, enabled: bool) -> Self {
        self.broadcast_enabled = enabled;
        self
    }

    /// Set request timeout.
    pub fn with_request_timeout(mut self, timeout: Duration) -> Self {
        self.request_timeout = timeout;
        self
    }

    /// Enable response delay simulation.
    pub fn with_response_delay_simulation(mut self, enabled: bool) -> Self {
        self.simulate_response_delay = enabled;
        self
    }

    /// Create a configuration for testing with channel transport.
    pub fn for_testing() -> Self {
        Self {
            transport: TransportConfig::Channel(ChannelConfig::default()),
            unit_ids: vec![1],
            broadcast_enabled: true,
            request_timeout: Duration::from_secs(1),
            shutdown_timeout: Duration::from_secs(1),
            simulate_response_delay: false,
            additional_response_delay: Duration::ZERO,
        }
    }
}

/// RTU server events.
#[derive(Debug, Clone)]
pub enum RtuServerEvent {
    /// Server started.
    Started,

    /// Server stopped.
    Stopped,

    /// Request received.
    RequestReceived {
        unit_id: u8,
        function_code: u8,
        timestamp: Instant,
    },

    /// Response sent.
    ResponseSent {
        unit_id: u8,
        function_code: u8,
        is_exception: bool,
        latency_us: u64,
    },

    /// Error occurred.
    Error { message: String },

    /// Frame error (CRC, framing, etc.).
    FrameError { error: String },
}

/// RTU server state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RtuServerState {
    /// Server is stopped.
    Stopped,
    /// Server is starting.
    Starting,
    /// Server is running.
    Running,
    /// Server is stopping.
    Stopping,
}

/// RTU server statistics.
#[derive(Debug, Clone, Default)]
pub struct RtuServerStats {
    /// Total requests processed.
    pub requests_processed: u64,

    /// Successful requests.
    pub requests_success: u64,

    /// Exception responses.
    pub requests_exception: u64,

    /// CRC errors.
    pub crc_errors: u64,

    /// Framing errors.
    pub framing_errors: u64,

    /// Timeouts.
    pub timeouts: u64,

    /// Total bytes received.
    pub bytes_received: u64,

    /// Total bytes sent.
    pub bytes_sent: u64,

    /// Average latency (microseconds).
    pub avg_latency_us: f64,
}

/// Modbus RTU server.
///
/// Provides a high-performance RTU server implementation with
/// support for multiple devices and extensible handlers.
pub struct ModbusRtuServer {
    /// Server configuration.
    config: RtuServerConfig,

    /// Handler registry.
    handlers: Arc<HandlerRegistry>,

    /// Devices by unit ID.
    devices: DashMap<u8, Arc<ModbusDevice>>,

    /// Default register store (for unknown units).
    default_registers: Arc<RegisterStore>,

    /// Server state.
    state: RwLock<RtuServerState>,

    /// Shutdown flag.
    shutdown: Arc<AtomicBool>,

    /// Event broadcaster.
    event_tx: broadcast::Sender<RtuServerEvent>,

    /// Statistics.
    stats: RwLock<RtuServerStats>,

    /// Transport metrics.
    transport_metrics: RwLock<TransportMetrics>,

    /// Request counter for latency tracking.
    request_count: AtomicU64,
    latency_sum: AtomicU64,

    /// Optional fault injection pipeline.
    fault_pipeline: Option<Arc<FaultPipeline>>,

    /// Optional RTU timing fault configuration.
    rtu_timing_fault: Option<Arc<RtuTimingFaultConfig>>,
}

impl ModbusRtuServer {
    /// Create a new RTU server.
    pub fn new(config: RtuServerConfig) -> Self {
        let (event_tx, _) = broadcast::channel(256);

        Self {
            config,
            handlers: Arc::new(HandlerRegistry::with_defaults()),
            devices: DashMap::new(),
            default_registers: Arc::new(RegisterStore::with_defaults()),
            state: RwLock::new(RtuServerState::Stopped),
            shutdown: Arc::new(AtomicBool::new(false)),
            event_tx,
            stats: RwLock::new(RtuServerStats::default()),
            transport_metrics: RwLock::new(TransportMetrics::new()),
            request_count: AtomicU64::new(0),
            latency_sum: AtomicU64::new(0),
            fault_pipeline: None,
            rtu_timing_fault: None,
        }
    }

    /// Set fault injection pipeline.
    pub fn with_fault_pipeline(mut self, pipeline: FaultPipeline) -> Self {
        self.fault_pipeline = Some(Arc::new(pipeline));
        self
    }

    /// Set RTU timing fault configuration.
    pub fn with_rtu_timing_fault(mut self, config: RtuTimingFaultConfig) -> Self {
        self.rtu_timing_fault = Some(Arc::new(config));
        self
    }

    /// Create with custom handler registry.
    pub fn with_handlers(mut self, handlers: HandlerRegistry) -> Self {
        self.handlers = Arc::new(handlers);
        self
    }

    /// Create with custom default registers.
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

    /// Get default registers.
    pub fn default_registers(&self) -> &Arc<RegisterStore> {
        &self.default_registers
    }

    /// Subscribe to server events.
    pub fn subscribe(&self) -> broadcast::Receiver<RtuServerEvent> {
        self.event_tx.subscribe()
    }

    /// Get current server state.
    pub fn state(&self) -> RtuServerState {
        *self.state.read()
    }

    /// Check if shutdown has been requested.
    pub fn is_shutdown(&self) -> bool {
        self.shutdown.load(Ordering::SeqCst)
    }

    /// Request server shutdown.
    pub fn shutdown(&self) {
        if !self.shutdown.swap(true, Ordering::SeqCst) {
            info!("RTU server shutdown requested");
        }
    }

    /// Get server statistics.
    pub fn stats(&self) -> RtuServerStats {
        let mut stats = self.stats.read().clone();

        // Calculate average latency
        let count = self.request_count.load(Ordering::Relaxed);
        if count > 0 {
            let sum = self.latency_sum.load(Ordering::Relaxed);
            stats.avg_latency_us = sum as f64 / count as f64;
        }

        stats
    }

    /// Get transport metrics.
    pub fn transport_metrics(&self) -> TransportMetrics {
        self.transport_metrics.read().clone()
    }

    /// Run the server with the configured transport.
    pub async fn run(&self) -> ModbusResult<()> {
        // For now, we'll use the channel transport for testing
        // In production, this would create the appropriate transport
        match &self.config.transport {
            TransportConfig::Channel(config) => {
                let (transport, _peer) = ChannelTransport::pair(config.clone());
                self.run_with_transport(transport).await
            }
            _ => {
                Err(ModbusError::Internal(
                    "Only channel transport is currently supported".into(),
                ))
            }
        }
    }

    /// Run with a specific transport.
    pub async fn run_with_transport<T: RtuTransport + 'static>(
        &self,
        mut transport: T,
    ) -> ModbusResult<()> {
        // Update state
        {
            let mut state = self.state.write();
            if *state != RtuServerState::Stopped {
                return Err(ModbusError::Internal("Server already running".into()));
            }
            *state = RtuServerState::Starting;
        }

        self.shutdown.store(false, Ordering::SeqCst);
        let _ = self.event_tx.send(RtuServerEvent::Started);

        {
            let mut state = self.state.write();
            *state = RtuServerState::Running;
        }

        info!("RTU server started");

        // Main processing loop
        let mut read_buffer = vec![0u8; 256];
        let mut frame_buffer = Vec::with_capacity(256);
        let mut rtu_request_number: u64 = 0;
        let serial_config = transport.serial_config().clone();
        let timing = RtuTiming::from_baud_rate(serial_config.baud_rate);

        loop {
            // Check shutdown
            if self.shutdown.load(Ordering::SeqCst) {
                break;
            }

            // Read with timeout
            let read_result = tokio::time::timeout(
                timing.inter_frame_timeout * 2,
                transport.read(&mut read_buffer),
            )
            .await;

            match read_result {
                Ok(Ok(0)) => {
                    // No data available or connection closed
                    tokio::task::yield_now().await;
                    continue;
                }
                Ok(Ok(n)) => {
                    // Data received
                    frame_buffer.extend_from_slice(&read_buffer[..n]);
                    self.transport_metrics.write().record_bytes_received(n);

                    // Try to parse frame
                    if let Some(frame) = self.try_parse_frame(&mut frame_buffer)? {
                        // Process request
                        let response = self.process_request(&frame).await;
                        rtu_request_number += 1;

                        // Apply fault injection pipeline (if configured)
                        let fault_action = if let Some(ref pipeline) = self.fault_pipeline {
                            let unit_id = frame.unit_id;
                            let function_code = frame.function_code().unwrap_or(0);
                            let fault_ctx = ModbusFaultContext::rtu(
                                unit_id,
                                function_code,
                                &frame.pdu,
                                &response.pdu,
                                rtu_request_number,
                            );
                            pipeline.apply(&fault_ctx)
                        } else {
                            None
                        };

                        match fault_action {
                            Some(FaultAction::DropResponse) => {
                                // Silent drop - no response sent
                                debug!("Fault: dropping RTU response");
                            }
                            Some(FaultAction::DelayThenSend { delay, response: fault_pdu }) => {
                                tokio::time::sleep(delay).await;
                                // Re-encode with faulted PDU
                                let fault_frame = RtuFrame::response(&frame, fault_pdu);
                                let response_bytes = fault_frame.encode();
                                if self.config.simulate_response_delay {
                                    let tx_delay = transport.transmission_delay(response_bytes.len());
                                    tokio::time::sleep(tx_delay + self.config.additional_response_delay).await;
                                }
                                if let Err(e) = transport.write(&response_bytes).await {
                                    error!("Failed to send delayed response: {}", e);
                                } else {
                                    self.transport_metrics.write().record_bytes_sent(response_bytes.len());
                                }
                            }
                            Some(FaultAction::SendRawBytes(raw_bytes)) => {
                                // Send raw wire bytes (used for CRC corruption, wrong unit_id)
                                // Apply RTU timing faults if configured
                                if let Some(ref timing_config) = self.rtu_timing_fault {
                                    if timing_config.is_active() {
                                        let plan = timing_config.build_timing_plan(&raw_bytes);
                                        let mut total_sent = 0usize;
                                        for segment in &plan.segments {
                                            if !segment.delay_before.is_zero() {
                                                tokio::time::sleep(segment.delay_before).await;
                                            }
                                            if let Err(e) = transport.write(&segment.data).await {
                                                error!("Failed to send timing segment (raw): {}", e);
                                                break;
                                            }
                                            total_sent += segment.data.len();
                                        }
                                        self.transport_metrics.write().record_bytes_sent(total_sent);
                                    } else {
                                        if self.config.simulate_response_delay {
                                            let delay = transport.transmission_delay(raw_bytes.len());
                                            tokio::time::sleep(delay + self.config.additional_response_delay).await;
                                        }
                                        if let Err(e) = transport.write(&raw_bytes).await {
                                            error!("Failed to send raw bytes: {}", e);
                                        } else {
                                            self.transport_metrics.write().record_bytes_sent(raw_bytes.len());
                                        }
                                    }
                                } else {
                                    if self.config.simulate_response_delay {
                                        let delay = transport.transmission_delay(raw_bytes.len());
                                        tokio::time::sleep(delay + self.config.additional_response_delay).await;
                                    }
                                    if let Err(e) = transport.write(&raw_bytes).await {
                                        error!("Failed to send raw bytes: {}", e);
                                    } else {
                                        self.transport_metrics.write().record_bytes_sent(raw_bytes.len());
                                    }
                                }
                            }
                            Some(FaultAction::SendPartial { bytes }) => {
                                // Send partial frame bytes
                                if self.config.simulate_response_delay {
                                    let delay = transport.transmission_delay(bytes.len());
                                    tokio::time::sleep(delay + self.config.additional_response_delay).await;
                                }
                                if let Err(e) = transport.write(&bytes).await {
                                    error!("Failed to send partial frame: {}", e);
                                } else {
                                    self.transport_metrics.write().record_bytes_sent(bytes.len());
                                }
                            }
                            Some(FaultAction::SendResponse(fault_pdu)) => {
                                let fault_frame = RtuFrame::response(&frame, fault_pdu);
                                let response_bytes = fault_frame.encode();
                                if self.config.simulate_response_delay {
                                    let delay = transport.transmission_delay(response_bytes.len());
                                    tokio::time::sleep(delay + self.config.additional_response_delay).await;
                                }
                                if let Err(e) = transport.write(&response_bytes).await {
                                    error!("Failed to send faulted response: {}", e);
                                } else {
                                    self.transport_metrics.write().record_bytes_sent(response_bytes.len());
                                }
                            }
                            Some(FaultAction::OverrideTransactionId { .. }) => {
                                // TID override is TCP-only, send normal response for RTU
                                let response_bytes = response.encode();
                                if self.config.simulate_response_delay {
                                    let delay = transport.transmission_delay(response_bytes.len());
                                    tokio::time::sleep(delay + self.config.additional_response_delay).await;
                                }
                                if let Err(e) = transport.write(&response_bytes).await {
                                    error!("Failed to send response: {}", e);
                                } else {
                                    self.transport_metrics.write().record_bytes_sent(response_bytes.len());
                                }
                            }
                            None => {
                                // Normal response path (no fault)
                                let response_bytes = response.encode();

                                // Apply RTU timing faults if configured
                                if let Some(ref timing_config) = self.rtu_timing_fault {
                                    if timing_config.is_active() {
                                        let plan = timing_config.build_timing_plan(&response_bytes);
                                        let mut total_sent = 0usize;
                                        for segment in &plan.segments {
                                            if !segment.delay_before.is_zero() {
                                                tokio::time::sleep(segment.delay_before).await;
                                            }
                                            if let Err(e) = transport.write(&segment.data).await {
                                                error!("Failed to send timing segment: {}", e);
                                                let _ = self.event_tx.send(RtuServerEvent::Error {
                                                    message: e.to_string(),
                                                });
                                                break;
                                            }
                                            total_sent += segment.data.len();
                                        }
                                        self.transport_metrics.write().record_bytes_sent(total_sent);
                                    } else {
                                        // Timing config present but not active
                                        if self.config.simulate_response_delay {
                                            let delay = transport.transmission_delay(response_bytes.len());
                                            tokio::time::sleep(delay + self.config.additional_response_delay).await;
                                        }
                                        if let Err(e) = transport.write(&response_bytes).await {
                                            error!("Failed to send response: {}", e);
                                            let _ = self.event_tx.send(RtuServerEvent::Error {
                                                message: e.to_string(),
                                            });
                                        } else {
                                            self.transport_metrics.write().record_bytes_sent(response_bytes.len());
                                        }
                                    }
                                } else {
                                    // No timing config
                                    if self.config.simulate_response_delay {
                                        let delay = transport.transmission_delay(response_bytes.len());
                                        tokio::time::sleep(delay + self.config.additional_response_delay).await;
                                    }
                                    if let Err(e) = transport.write(&response_bytes).await {
                                        error!("Failed to send response: {}", e);
                                        let _ = self.event_tx.send(RtuServerEvent::Error {
                                            message: e.to_string(),
                                        });
                                    } else {
                                        self.transport_metrics
                                            .write()
                                            .record_bytes_sent(response_bytes.len());
                                    }
                                }
                            }
                        }
                    }
                }
                Ok(Err(e)) => {
                    error!("Transport read error: {}", e);
                    let _ = self.event_tx.send(RtuServerEvent::Error {
                        message: e.to_string(),
                    });
                }
                Err(_) => {
                    // Timeout - check for partial frame
                    if !frame_buffer.is_empty() {
                        // Incomplete frame, discard
                        debug!("Discarding incomplete frame ({} bytes)", frame_buffer.len());
                        frame_buffer.clear();
                        self.stats.write().framing_errors += 1;
                    }
                }
            }
        }

        // Shutdown
        {
            let mut state = self.state.write();
            *state = RtuServerState::Stopping;
        }

        let _ = transport.close().await;

        {
            let mut state = self.state.write();
            *state = RtuServerState::Stopped;
        }

        let _ = self.event_tx.send(RtuServerEvent::Stopped);
        info!("RTU server stopped");

        Ok(())
    }

    /// Try to parse a complete frame from the buffer.
    fn try_parse_frame(&self, buffer: &mut Vec<u8>) -> ModbusResult<Option<RtuFrame>> {
        if buffer.len() < 4 {
            return Ok(None);
        }

        // Try to decode
        match RtuFrame::try_decode(buffer) {
            Ok(Some(frame)) => {
                // Remove parsed bytes from buffer
                let frame_size = frame.frame_size();
                buffer.drain(..frame_size);

                self.transport_metrics.write().record_frame_received();

                Ok(Some(frame))
            }
            Ok(None) => {
                // Need more data
                Ok(None)
            }
            Err(RtuFrameError::CrcMismatch { .. }) => {
                // CRC error - discard frame
                buffer.clear();
                self.stats.write().crc_errors += 1;
                self.transport_metrics.write().record_crc_error();

                let _ = self.event_tx.send(RtuServerEvent::FrameError {
                    error: "CRC mismatch".into(),
                });

                Ok(None)
            }
            Err(e) => {
                // Other error
                buffer.clear();
                self.stats.write().framing_errors += 1;
                self.transport_metrics.write().record_framing_error();

                let _ = self.event_tx.send(RtuServerEvent::FrameError {
                    error: e.to_string(),
                });

                Ok(None)
            }
        }
    }

    /// Process a request and generate a response.
    async fn process_request(&self, request: &RtuFrame) -> RtuFrame {
        let start = Instant::now();
        let unit_id = request.unit_id;
        let function_code = request.function_code().unwrap_or(0);

        // Emit request event
        let _ = self.event_tx.send(RtuServerEvent::RequestReceived {
            unit_id,
            function_code,
            timestamp: start,
        });

        // Check unit ID
        if !self.should_respond_to_unit(unit_id) {
            // Silent ignore for non-matching unit IDs
            debug!("Ignoring request for unit {}", unit_id);
            return RtuFrame::new(unit_id, vec![]);
        }

        // Get registers for this unit
        let registers = if let Some(device) = self.devices.get(&unit_id) {
            device.registers().clone()
        } else if unit_id == 0 {
            // Broadcast - use default
            self.default_registers.clone()
        } else {
            // Unknown unit but in our filter - use default
            self.default_registers.clone()
        };

        // Create handler context
        let ctx = HandlerContext::new(unit_id, registers, 0);

        // Process with timeout
        let response_pdu = match tokio::time::timeout(
            self.config.request_timeout,
            async { self.handlers.dispatch(&request.pdu, &ctx) },
        )
        .await
        {
            Ok(Ok(pdu)) => pdu,
            Ok(Err(exception_code)) => {
                build_exception_pdu(function_code, exception_code)
            }
            Err(_) => {
                self.stats.write().timeouts += 1;
                build_exception_pdu(function_code, ExceptionCode::SlaveDeviceBusy)
            }
        };

        // Create response frame
        let response = RtuFrame::response(request, response_pdu);
        let is_exception = response.is_exception();

        // Update statistics
        let latency_us = start.elapsed().as_micros() as u64;
        self.request_count.fetch_add(1, Ordering::Relaxed);
        self.latency_sum.fetch_add(latency_us, Ordering::Relaxed);

        {
            let mut stats = self.stats.write();
            stats.requests_processed += 1;
            if is_exception {
                stats.requests_exception += 1;
            } else {
                stats.requests_success += 1;
            }
        }

        // Emit response event
        let _ = self.event_tx.send(RtuServerEvent::ResponseSent {
            unit_id,
            function_code,
            is_exception,
            latency_us,
        });

        response
    }

    /// Check if we should respond to a given unit ID.
    fn should_respond_to_unit(&self, unit_id: u8) -> bool {
        // Broadcast
        if unit_id == 0 {
            return self.config.broadcast_enabled;
        }

        // Empty filter = accept all
        if self.config.unit_ids.is_empty() {
            return true;
        }

        // Check filter
        self.config.unit_ids.contains(&unit_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_server_config_default() {
        let config = RtuServerConfig::default();
        assert_eq!(config.unit_ids, vec![1]);
        assert!(config.broadcast_enabled);
    }

    #[test]
    fn test_server_config_builder() {
        let config = RtuServerConfig::new()
            .with_unit_ids(vec![1, 2, 3])
            .with_broadcast(false)
            .with_request_timeout(Duration::from_secs(10));

        assert_eq!(config.unit_ids, vec![1, 2, 3]);
        assert!(!config.broadcast_enabled);
        assert_eq!(config.request_timeout, Duration::from_secs(10));
    }

    #[test]
    fn test_server_creation() {
        let config = RtuServerConfig::for_testing();
        let server = ModbusRtuServer::new(config);

        assert_eq!(server.state(), RtuServerState::Stopped);
        assert!(!server.is_shutdown());
    }

    #[test]
    fn test_server_device_management() {
        use crate::config::ModbusDeviceConfig;

        let server = ModbusRtuServer::new(RtuServerConfig::for_testing());

        // Add device
        let device = ModbusDevice::new(ModbusDeviceConfig::new(5, "Test Device"));
        server.add_device(device);

        assert!(server.device(5).is_some());
        assert!(server.device(10).is_none());

        // Remove device
        let removed = server.remove_device(5);
        assert!(removed.is_some());
        assert!(server.device(5).is_none());
    }

    #[test]
    fn test_should_respond_to_unit() {
        let config = RtuServerConfig::new()
            .with_unit_ids(vec![1, 2, 3])
            .with_broadcast(true);
        let server = ModbusRtuServer::new(config);

        // Matching units
        assert!(server.should_respond_to_unit(1));
        assert!(server.should_respond_to_unit(2));
        assert!(server.should_respond_to_unit(3));

        // Non-matching
        assert!(!server.should_respond_to_unit(4));
        assert!(!server.should_respond_to_unit(255));

        // Broadcast
        assert!(server.should_respond_to_unit(0));
    }

    #[test]
    fn test_should_respond_broadcast_disabled() {
        let config = RtuServerConfig::new()
            .with_unit_ids(vec![1])
            .with_broadcast(false);
        let server = ModbusRtuServer::new(config);

        assert!(server.should_respond_to_unit(1));
        assert!(!server.should_respond_to_unit(0)); // Broadcast disabled
    }

    #[test]
    fn test_should_respond_empty_filter() {
        let config = RtuServerConfig::new().with_unit_ids(vec![]);
        let server = ModbusRtuServer::new(config);

        // Empty filter = accept all
        assert!(server.should_respond_to_unit(1));
        assert!(server.should_respond_to_unit(100));
        assert!(server.should_respond_to_unit(255));
    }

    #[tokio::test]
    async fn test_server_stats() {
        let server = ModbusRtuServer::new(RtuServerConfig::for_testing());
        let stats = server.stats();

        assert_eq!(stats.requests_processed, 0);
        assert_eq!(stats.crc_errors, 0);
    }
}

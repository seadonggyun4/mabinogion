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

use crate::context::{BroadcastPolicy, ServerContext, SharedAddressSpace};
use crate::device::ModbusDevice;
use crate::error::{ModbusError, ModbusResult};
use crate::fault_injection::rtu_timing::RtuTimingFaultConfig;
use crate::fault_injection::{FaultAction, FaultPipeline, ModbusFaultContext};
use crate::handler::HandlerRegistry;
use crate::register::RegisterStore;
use crate::service::{
    execute_transport_request, ExtensionRegistry, StandardModbusService, TransportDisposition,
    TransportServicePolicy, UnknownUnitBehavior,
};
use crate::transport_runtime::TransportHookBundle;

use super::codec::RtuTiming;
use super::frame::{RtuFrame, RtuFrameError};
use super::transport::{
    ChannelConfig, RtuTransport, TransportConfig, TransportFactory, TransportMetrics, TransportType,
};

/// Performance preset for the RTU server.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum PerformancePreset {
    /// Full observability and compatibility behavior.
    #[default]
    Default,
    /// Lower-overhead request processing for high-throughput workloads.
    HighThroughput,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EventEmissionMode {
    Always,
    SubscriberAware,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct RtuRuntimePolicy {
    enforce_request_timeout: bool,
    event_mode: EventEmissionMode,
    record_transport_metrics: bool,
}

impl RtuRuntimePolicy {
    fn resolve(
        preset: PerformancePreset,
        transport_type: TransportType,
        has_fault_pipeline: bool,
        has_timing_fault: bool,
        simulate_response_delay: bool,
    ) -> Self {
        match preset {
            PerformancePreset::Default => Self {
                enforce_request_timeout: true,
                event_mode: EventEmissionMode::Always,
                record_transport_metrics: true,
            },
            PerformancePreset::HighThroughput => match transport_type {
                TransportType::Channel => Self {
                    // Channel transport is already CPU-local; keep default semantics to avoid regressions.
                    enforce_request_timeout: true,
                    event_mode: EventEmissionMode::Always,
                    record_transport_metrics: true,
                },
                TransportType::TcpBridge => Self {
                    enforce_request_timeout: false,
                    event_mode: EventEmissionMode::SubscriberAware,
                    record_transport_metrics: true,
                },
                TransportType::VirtualSerial => {
                    let keep_timeout =
                        has_fault_pipeline || has_timing_fault || simulate_response_delay;
                    Self {
                        enforce_request_timeout: keep_timeout,
                        event_mode: EventEmissionMode::SubscriberAware,
                        record_transport_metrics: true,
                    }
                }
            },
        }
    }

    #[inline]
    fn request_timeout(self, timeout: Duration) -> Option<Duration> {
        self.enforce_request_timeout.then_some(timeout)
    }

    #[inline]
    fn should_emit_events(self, subscriber_count: usize) -> bool {
        match self.event_mode {
            EventEmissionMode::Always => true,
            EventEmissionMode::SubscriberAware => subscriber_count > 0,
        }
    }

    #[inline]
    fn should_record_transport_metrics(self) -> bool {
        self.record_transport_metrics
    }
}

#[derive(Debug, Clone, Copy)]
struct RtuHookBundle {
    transport: TransportHookBundle,
    simulate_response_delay: bool,
    additional_response_delay: Duration,
    apply_timing_faults: bool,
}

impl RtuHookBundle {
    fn new(
        policy: RtuRuntimePolicy,
        request_timeout: Duration,
        simulate_response_delay: bool,
        additional_response_delay: Duration,
        apply_timing_faults: bool,
    ) -> Self {
        Self {
            transport: TransportHookBundle::new()
                .with_request_timeout(policy.request_timeout(request_timeout))
                .with_transport_metrics(policy.should_record_transport_metrics()),
            simulate_response_delay,
            additional_response_delay,
            apply_timing_faults,
        }
    }
}

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

    /// Performance tuning preset for request processing.
    #[serde(default)]
    pub performance_preset: PerformancePreset,
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
            performance_preset: PerformancePreset::Default,
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

    /// Set the performance preset.
    pub fn with_performance_preset(mut self, preset: PerformancePreset) -> Self {
        self.performance_preset = preset;
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
            performance_preset: PerformancePreset::Default,
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

#[derive(Debug, Default)]
struct RtuStatsCounters {
    requests_processed: AtomicU64,
    requests_success: AtomicU64,
    requests_exception: AtomicU64,
    crc_errors: AtomicU64,
    framing_errors: AtomicU64,
    timeouts: AtomicU64,
    bytes_received: AtomicU64,
    bytes_sent: AtomicU64,
}

impl RtuStatsCounters {
    #[inline]
    fn record_request(
        &self,
        is_exception: bool,
        latency_us: u64,
        bytes_received: u64,
        bytes_sent: u64,
    ) {
        let _ = latency_us;
        self.requests_processed.fetch_add(1, Ordering::Relaxed);
        if is_exception {
            self.requests_exception.fetch_add(1, Ordering::Relaxed);
        } else {
            self.requests_success.fetch_add(1, Ordering::Relaxed);
        }
        self.bytes_received
            .fetch_add(bytes_received, Ordering::Relaxed);
        self.bytes_sent.fetch_add(bytes_sent, Ordering::Relaxed);
    }

    #[inline]
    fn record_crc_error(&self) {
        self.crc_errors.fetch_add(1, Ordering::Relaxed);
    }

    #[inline]
    fn record_framing_error(&self) {
        self.framing_errors.fetch_add(1, Ordering::Relaxed);
    }

    #[inline]
    fn record_timeout(&self) {
        self.timeouts.fetch_add(1, Ordering::Relaxed);
    }

    fn snapshot(&self, request_count: u64, latency_sum: u64) -> RtuServerStats {
        let avg_latency_us = if request_count > 0 {
            latency_sum as f64 / request_count as f64
        } else {
            0.0
        };

        RtuServerStats {
            requests_processed: self.requests_processed.load(Ordering::Relaxed),
            requests_success: self.requests_success.load(Ordering::Relaxed),
            requests_exception: self.requests_exception.load(Ordering::Relaxed),
            crc_errors: self.crc_errors.load(Ordering::Relaxed),
            framing_errors: self.framing_errors.load(Ordering::Relaxed),
            timeouts: self.timeouts.load(Ordering::Relaxed),
            bytes_received: self.bytes_received.load(Ordering::Relaxed),
            bytes_sent: self.bytes_sent.load(Ordering::Relaxed),
            avg_latency_us,
        }
    }
}

#[derive(Debug, Default)]
struct AtomicTransportMetrics {
    bytes_received: AtomicU64,
    bytes_sent: AtomicU64,
    frames_received: AtomicU64,
    frames_sent: AtomicU64,
    crc_errors: AtomicU64,
    framing_errors: AtomicU64,
    timeouts: AtomicU64,
}

impl AtomicTransportMetrics {
    #[inline]
    fn record_bytes_received(&self, bytes: usize) {
        self.bytes_received
            .fetch_add(bytes as u64, Ordering::Relaxed);
    }

    #[inline]
    fn record_bytes_sent(&self, bytes: usize) {
        self.bytes_sent.fetch_add(bytes as u64, Ordering::Relaxed);
        self.frames_sent.fetch_add(1, Ordering::Relaxed);
    }

    #[inline]
    fn record_frame_received(&self) {
        self.frames_received.fetch_add(1, Ordering::Relaxed);
    }

    #[inline]
    fn record_crc_error(&self) {
        self.crc_errors.fetch_add(1, Ordering::Relaxed);
    }

    #[inline]
    fn record_framing_error(&self) {
        self.framing_errors.fetch_add(1, Ordering::Relaxed);
    }

    #[inline]
    fn record_timeout(&self) {
        self.timeouts.fetch_add(1, Ordering::Relaxed);
    }

    fn snapshot(&self) -> TransportMetrics {
        TransportMetrics {
            bytes_received: self.bytes_received.load(Ordering::Relaxed),
            bytes_sent: self.bytes_sent.load(Ordering::Relaxed),
            frames_received: self.frames_received.load(Ordering::Relaxed),
            frames_sent: self.frames_sent.load(Ordering::Relaxed),
            crc_errors: self.crc_errors.load(Ordering::Relaxed),
            framing_errors: self.framing_errors.load(Ordering::Relaxed),
            timeouts: self.timeouts.load(Ordering::Relaxed),
        }
    }
}

#[derive(Debug, Clone)]
enum UnitFilter {
    All,
    Selected(Box<[bool; 256]>),
}

impl UnitFilter {
    fn new(unit_ids: &[u8]) -> Self {
        if unit_ids.is_empty() {
            Self::All
        } else {
            let mut selected = Box::new([false; 256]);
            for unit_id in unit_ids {
                selected[*unit_id as usize] = true;
            }
            Self::Selected(selected)
        }
    }

    #[inline]
    fn allows(&self, unit_id: u8) -> bool {
        match self {
            Self::All => true,
            Self::Selected(selected) => selected[unit_id as usize],
        }
    }
}

/// Modbus RTU server.
///
/// Provides a high-performance RTU server implementation with
/// support for multiple devices and extensible handlers.
pub struct ModbusRtuServer {
    /// Server configuration.
    config: RtuServerConfig,

    /// Shared request execution service.
    service: Arc<StandardModbusService>,

    /// Devices by unit ID.
    devices: DashMap<u8, Arc<ModbusDevice>>,

    /// Shared server context for request routing.
    server_context: Arc<ServerContext>,

    /// Fast unit-id filter for request admission.
    unit_filter: UnitFilter,

    /// Server state.
    state: RwLock<RtuServerState>,

    /// Shutdown flag.
    shutdown: Arc<AtomicBool>,

    /// Event broadcaster.
    event_tx: broadcast::Sender<RtuServerEvent>,

    /// Low-overhead request statistics.
    stats: RtuStatsCounters,

    /// Low-overhead transport metrics.
    transport_metrics: AtomicTransportMetrics,

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
        let server_context = Arc::new(ServerContext::new(Arc::new(RegisterStore::with_defaults())));
        server_context.set_broadcast_enabled(config.broadcast_enabled);
        let unit_filter = UnitFilter::new(&config.unit_ids);

        Self {
            config,
            service: Arc::new(StandardModbusService::default()),
            devices: DashMap::new(),
            server_context,
            unit_filter,
            state: RwLock::new(RtuServerState::Stopped),
            shutdown: Arc::new(AtomicBool::new(false)),
            event_tx,
            stats: RtuStatsCounters::default(),
            transport_metrics: AtomicTransportMetrics::default(),
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
        self.service = Arc::new(StandardModbusService::new(handlers));
        self
    }

    /// Set a typed extension registry.
    pub fn with_extensions(mut self, extensions: ExtensionRegistry) -> Self {
        self.service = Arc::new(StandardModbusService::with_extensions(extensions));
        self
    }

    /// Create with custom default registers.
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

    /// Get default register space.
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

    /// Subscribe to server events.
    pub fn subscribe(&self) -> broadcast::Receiver<RtuServerEvent> {
        self.event_tx.subscribe()
    }

    fn runtime_policy(&self, transport_type: TransportType) -> RtuRuntimePolicy {
        RtuRuntimePolicy::resolve(
            self.config.performance_preset,
            transport_type,
            self.fault_pipeline.is_some(),
            self.rtu_timing_fault
                .as_ref()
                .map(|config| config.is_active())
                .unwrap_or(false),
            self.config.simulate_response_delay,
        )
    }

    #[inline]
    fn should_emit_events(&self, policy: RtuRuntimePolicy) -> bool {
        policy.should_emit_events(self.event_tx.receiver_count())
    }

    #[inline]
    fn emit_event(&self, policy: RtuRuntimePolicy, event: RtuServerEvent) {
        if self.should_emit_events(policy) {
            let _ = self.event_tx.send(event);
        }
    }

    #[inline]
    fn record_transport_bytes_received(&self, policy: RtuRuntimePolicy, bytes: usize) {
        if policy.should_record_transport_metrics() {
            self.transport_metrics.record_bytes_received(bytes);
        }
    }

    #[inline]
    fn record_transport_bytes_sent(&self, policy: RtuRuntimePolicy, bytes: usize) {
        if policy.should_record_transport_metrics() {
            self.transport_metrics.record_bytes_sent(bytes);
        }
    }

    #[inline]
    fn record_transport_frame_received(&self, policy: RtuRuntimePolicy) {
        if policy.should_record_transport_metrics() {
            self.transport_metrics.record_frame_received();
        }
    }

    #[inline]
    fn record_transport_crc_error(&self, policy: RtuRuntimePolicy) {
        if policy.should_record_transport_metrics() {
            self.transport_metrics.record_crc_error();
        }
    }

    #[inline]
    fn record_transport_framing_error(&self, policy: RtuRuntimePolicy) {
        if policy.should_record_transport_metrics() {
            self.transport_metrics.record_framing_error();
        }
    }

    #[inline]
    fn record_request_observation(
        &self,
        is_exception: bool,
        latency_us: u64,
        request_bytes: u64,
        response_bytes: u64,
    ) {
        self.request_count.fetch_add(1, Ordering::Relaxed);
        self.latency_sum.fetch_add(latency_us, Ordering::Relaxed);
        self.stats
            .record_request(is_exception, latency_us, request_bytes, response_bytes);
    }

    async fn send_response_bytes(
        &self,
        transport: &mut dyn RtuTransport,
        policy: RtuRuntimePolicy,
        hooks: RtuHookBundle,
        bytes: &[u8],
        allow_timing_faults: bool,
        error_context: &str,
    ) -> bool {
        if allow_timing_faults && hooks.apply_timing_faults {
            if let Some(ref timing_config) = self.rtu_timing_fault {
                let plan = timing_config.build_timing_plan(bytes);
                let mut total_sent = 0usize;
                for segment in &plan.segments {
                    if !segment.delay_before.is_zero() {
                        tokio::time::sleep(segment.delay_before).await;
                    }
                    if let Err(error) = transport.write(&segment.data).await {
                        error!("{error_context}: {error}");
                        self.emit_event(
                            policy,
                            RtuServerEvent::Error {
                                message: error.to_string(),
                            },
                        );
                        return false;
                    }
                    total_sent += segment.data.len();
                }
                self.record_transport_bytes_sent(policy, total_sent);
                return true;
            }
        }

        if hooks.simulate_response_delay {
            let delay =
                transport.transmission_delay(bytes.len()) + hooks.additional_response_delay;
            tokio::time::sleep(delay).await;
        }

        match transport.write(bytes).await {
            Ok(_) => {
                self.record_transport_bytes_sent(policy, bytes.len());
                true
            }
            Err(error) => {
                error!("{error_context}: {error}");
                self.emit_event(
                    policy,
                    RtuServerEvent::Error {
                        message: error.to_string(),
                    },
                );
                false
            }
        }
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
        let count = self.request_count.load(Ordering::Relaxed);
        let sum = self.latency_sum.load(Ordering::Relaxed);
        self.stats.snapshot(count, sum)
    }

    /// Get transport metrics.
    pub fn transport_metrics(&self) -> TransportMetrics {
        self.transport_metrics.snapshot()
    }

    /// Run the server with the configured transport.
    pub async fn run(&self) -> ModbusResult<()> {
        let transport = TransportFactory::create(self.config.transport.clone()).await?;
        self.run_with_boxed_transport(transport).await
    }

    /// Run with a specific transport.
    pub async fn run_with_transport<T: RtuTransport + 'static>(
        &self,
        transport: T,
    ) -> ModbusResult<()> {
        self.run_with_boxed_transport(Box::new(transport)).await
    }

    async fn run_with_boxed_transport(
        &self,
        mut transport: Box<dyn RtuTransport>,
    ) -> ModbusResult<()> {
        let policy = self.runtime_policy(transport.transport_type());
        let hooks = RtuHookBundle::new(
            policy,
            self.config.request_timeout,
            self.config.simulate_response_delay,
            self.config.additional_response_delay,
            self.rtu_timing_fault
                .as_ref()
                .map(|config| config.is_active())
                .unwrap_or(false),
        );

        // Update state
        {
            let mut state = self.state.write();
            if *state != RtuServerState::Stopped {
                return Err(ModbusError::Internal("Server already running".into()));
            }
            *state = RtuServerState::Starting;
        }

        self.shutdown.store(false, Ordering::SeqCst);
        self.emit_event(policy, RtuServerEvent::Started);

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
                    self.record_transport_bytes_received(policy, n);

                    // Try to parse frame
                    if let Some(frame) = self.try_parse_frame(&mut frame_buffer, policy)? {
                        let emit_events = self.should_emit_events(policy);
                        // Process request
                        let response = self.process_request(&frame, hooks, emit_events).await;
                        rtu_request_number += 1;

                        if response.pdu.is_empty() {
                            continue;
                        }

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
                            Some(FaultAction::DelayThenSend {
                                delay,
                                response: fault_pdu,
                            }) => {
                                tokio::time::sleep(delay).await;
                                let response_bytes = RtuFrame::response(&frame, fault_pdu).encode();
                                let _ = self
                                    .send_response_bytes(
                                        transport.as_mut(),
                                        policy,
                                        hooks,
                                        &response_bytes,
                                        false,
                                        "Failed to send delayed response",
                                    )
                                    .await;
                            }
                            Some(FaultAction::SendRawBytes(raw_bytes)) => {
                                let _ = self
                                    .send_response_bytes(
                                        transport.as_mut(),
                                        policy,
                                        hooks,
                                        &raw_bytes,
                                        true,
                                        "Failed to send raw bytes",
                                    )
                                    .await;
                            }
                            Some(FaultAction::SendPartial { bytes }) => {
                                let _ = self
                                    .send_response_bytes(
                                        transport.as_mut(),
                                        policy,
                                        hooks,
                                        &bytes,
                                        false,
                                        "Failed to send partial frame",
                                    )
                                    .await;
                            }
                            Some(FaultAction::SendResponse(fault_pdu)) => {
                                let response_bytes = RtuFrame::response(&frame, fault_pdu).encode();
                                let _ = self
                                    .send_response_bytes(
                                        transport.as_mut(),
                                        policy,
                                        hooks,
                                        &response_bytes,
                                        false,
                                        "Failed to send faulted response",
                                    )
                                    .await;
                            }
                            Some(FaultAction::OverrideTransactionId { .. }) => {
                                // TID override is TCP-only, send normal response for RTU
                                let response_bytes = response.encode();
                                let _ = self
                                    .send_response_bytes(
                                        transport.as_mut(),
                                        policy,
                                        hooks,
                                        &response_bytes,
                                        false,
                                        "Failed to send response",
                                    )
                                    .await;
                            }
                            None => {
                                let response_bytes = response.encode();
                                let _ = self
                                    .send_response_bytes(
                                        transport.as_mut(),
                                        policy,
                                        hooks,
                                        &response_bytes,
                                        true,
                                        "Failed to send response",
                                    )
                                    .await;
                            }
                        }
                    }
                }
                Ok(Err(e)) => {
                    error!("Transport read error: {}", e);
                    self.emit_event(
                        policy,
                        RtuServerEvent::Error {
                            message: e.to_string(),
                        },
                    );
                }
                Err(_) => {
                    // Timeout - check for partial frame
                    if !frame_buffer.is_empty() {
                        // Incomplete frame, discard
                        debug!("Discarding incomplete frame ({} bytes)", frame_buffer.len());
                        frame_buffer.clear();
                        self.stats.record_framing_error();
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

        self.emit_event(policy, RtuServerEvent::Stopped);
        info!("RTU server stopped");

        Ok(())
    }

    /// Try to parse a complete frame from the buffer.
    fn try_parse_frame(
        &self,
        buffer: &mut Vec<u8>,
        policy: RtuRuntimePolicy,
    ) -> ModbusResult<Option<RtuFrame>> {
        if buffer.len() < 4 {
            return Ok(None);
        }

        // Try to decode
        match RtuFrame::try_decode(buffer) {
            Ok(Some(frame)) => {
                // Remove parsed bytes from buffer
                let frame_size = frame.frame_size();
                buffer.drain(..frame_size);

                self.record_transport_frame_received(policy);

                Ok(Some(frame))
            }
            Ok(None) => {
                // Need more data
                Ok(None)
            }
            Err(RtuFrameError::CrcMismatch { .. }) => {
                // CRC error - discard frame
                buffer.clear();
                self.stats.record_crc_error();
                self.record_transport_crc_error(policy);
                self.emit_event(
                    policy,
                    RtuServerEvent::FrameError {
                        error: "CRC mismatch".into(),
                    },
                );

                Ok(None)
            }
            Err(e) => {
                // Other error
                buffer.clear();
                self.stats.record_framing_error();
                self.record_transport_framing_error(policy);
                self.emit_event(
                    policy,
                    RtuServerEvent::FrameError {
                        error: e.to_string(),
                    },
                );

                Ok(None)
            }
        }
    }

    /// Process a request and generate a response.
    async fn process_request(
        &self,
        request: &RtuFrame,
        hooks: RtuHookBundle,
        emit_events: bool,
    ) -> RtuFrame {
        let start = Instant::now();
        let unit_id = request.unit_id;
        let function_code = request.function_code().unwrap_or(0);
        let is_broadcast = unit_id == 0;

        // Emit request event
        if emit_events {
            let _ = self.event_tx.send(RtuServerEvent::RequestReceived {
                unit_id,
                function_code,
                timestamp: start,
            });
        }

        // Check unit ID
        if !self.should_respond_to_unit(unit_id) {
            // Silent ignore for non-matching unit IDs
            debug!("Ignoring request for unit {}", unit_id);
            return RtuFrame::new(unit_id, vec![]);
        }

        let execution = execute_transport_request(
            self.service.as_ref(),
            self.server_context.as_ref(),
            unit_id,
            0,
            request.pdu.as_slice(),
            TransportServicePolicy::new(UnknownUnitBehavior::Ignore)
                .with_request_timeout(hooks.transport.request_timeout),
        )
        .await;

        if execution.timed_out {
            self.stats.record_timeout();
            if hooks.transport.record_transport_metrics {
                self.transport_metrics.record_timeout();
            }
        }

        let (is_exception, response) = match execution.disposition {
            TransportDisposition::Ignore => return RtuFrame::new(unit_id, vec![]),
            TransportDisposition::BroadcastSuppressed(response) => {
                (response.is_exception(), RtuFrame::new(unit_id, vec![]))
            }
            TransportDisposition::Reply(response) => {
                let is_exception = response.is_exception();
                (
                    is_exception,
                    RtuFrame::response(request, response.into_bytes()),
                )
            }
        };

        // Update statistics
        let latency_us = start.elapsed().as_micros() as u64;
        self.record_request_observation(
            is_exception,
            latency_us,
            request.frame_size() as u64,
            response.frame_size() as u64,
        );

        // Emit response event
        if emit_events && !is_broadcast {
            let _ = self.event_tx.send(RtuServerEvent::ResponseSent {
                unit_id,
                function_code,
                is_exception,
                latency_us,
            });
        }

        response
    }

    /// Check if we should respond to a given unit ID.
    fn should_respond_to_unit(&self, unit_id: u8) -> bool {
        // Broadcast
        if unit_id == 0 {
            return self.server_context.broadcast_enabled();
        }

        self.unit_filter.allows(unit_id)
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

    #[test]
    fn test_runtime_policy_default_is_fully_observable() {
        let policy = RtuRuntimePolicy::resolve(
            PerformancePreset::Default,
            TransportType::Channel,
            false,
            false,
            false,
        );

        assert_eq!(
            policy.request_timeout(Duration::from_secs(1)),
            Some(Duration::from_secs(1))
        );
        assert!(policy.should_emit_events(0));
        assert!(policy.should_record_transport_metrics());
    }

    #[test]
    fn test_runtime_policy_channel_high_throughput_matches_default() {
        let policy = RtuRuntimePolicy::resolve(
            PerformancePreset::HighThroughput,
            TransportType::Channel,
            false,
            false,
            false,
        );

        assert_eq!(
            policy.request_timeout(Duration::from_secs(1)),
            Some(Duration::from_secs(1))
        );
        assert!(policy.should_emit_events(0));
    }

    #[test]
    fn test_runtime_policy_tcp_bridge_high_throughput_is_subscriber_aware() {
        let policy = RtuRuntimePolicy::resolve(
            PerformancePreset::HighThroughput,
            TransportType::TcpBridge,
            false,
            false,
            false,
        );

        assert_eq!(policy.request_timeout(Duration::from_secs(1)), None);
        assert!(!policy.should_emit_events(0));
        assert!(policy.should_emit_events(1));
    }

    #[test]
    fn test_runtime_policy_virtual_serial_keeps_timeout_when_timing_semantics_are_active() {
        let policy = RtuRuntimePolicy::resolve(
            PerformancePreset::HighThroughput,
            TransportType::VirtualSerial,
            true,
            true,
            true,
        );

        assert_eq!(
            policy.request_timeout(Duration::from_secs(1)),
            Some(Duration::from_secs(1))
        );
        assert!(!policy.should_emit_events(0));
        assert!(policy.should_emit_events(2));
    }

    #[tokio::test]
    async fn test_server_stats() {
        let server = ModbusRtuServer::new(RtuServerConfig::for_testing());
        let stats = server.stats();

        assert_eq!(stats.requests_processed, 0);
        assert_eq!(stats.crc_errors, 0);
    }
}

//! KNXnet/IP Server with production-grade tunnel FSM, sequence validation, and L_Data.con.
//!
//! This module provides a complete KNXnet/IP server supporting:
//! - knxd-compatible sequence validation (Duplicate/OutOfOrder/FatalDesync)
//! - L_Data.con confirmation flow (MC=0x2E with Ctrl1 bit 0 success/failure)
//! - 7-state per-connection FSM mapped to knxd mod 0-3
//! - Configurable heartbeat status codes for reconnection testing
//! - Server→client ACK tracking with timeout/retry
//! - Bus Monitor frame generation (MC=0x2B) with Additional Info TLV
//! - cEMI property service handling (M_PropRead, M_PropWrite, M_Reset)
//! - L_Data.ind broadcast to other tunnel connections
//! - Flow control filter chain (PaceFilter, QueueFilter, RetryFilter)
//!   for realistic KNX bus timing simulation
//! - Unified 62-field metrics collection with Prometheus exposition format
//! - 9-rule automatic diagnostic analysis with configurable thresholds

use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::Arc;
use std::time::Duration;

use dashmap::DashMap;
use tokio::net::UdpSocket;
use tokio::sync::{broadcast, mpsc};
use tracing::{debug, error, info, trace, warn};

use crate::address::{GroupAddress, IndividualAddress};
use crate::cemi::{Apci, CemiFrame, MessageCode};
use crate::config::KnxServerConfig;
use crate::diagnostics::{DiagnosticConfig, KnxDiagnostics};
use crate::error::{KnxError, KnxResult};
use crate::error_tracker::{ErrorCategory, SendErrorTracker, TrackingResult};
use crate::filter::{CircuitBreakerState, FilterChain, FilterResult, FrameEnvelope, PaceState};
use crate::frame::{DibDeviceInfo, Hpai, KnxFrame, ServiceType, SupportedServiceFamilies};
use crate::group::GroupObjectTable;
use crate::group_cache::GroupValueCache;
use crate::heartbeat::HeartbeatScheduler;
use crate::metrics::{ConnectionMetricsSnapshot, KnxMetricsCollector, KnxMetricsSnapshot};
use crate::tunnel::{
    AckMessage, ConnectRequest, ConnectResponse, ConnectStatus, ConnectionResponseData,
    ConnectionStateRequest, ConnectionStateResponse, DisconnectRequest, DisconnectResponse,
    ReceivedValidation, TunnelConnection, TunnellingAck, TunnellingRequest,
};

// ============================================================================
// Server Event
// ============================================================================

/// Server event.
#[derive(Debug, Clone)]
pub enum ServerEvent {
    /// Server started.
    Started { address: SocketAddr },
    /// Server stopped.
    Stopped,
    /// Client connected.
    ClientConnected { channel_id: u8, address: SocketAddr },
    /// Client disconnected.
    ClientDisconnected { channel_id: u8 },
    /// Group value written.
    GroupValueWrite {
        address: GroupAddress,
        value: Vec<u8>,
        source: IndividualAddress,
    },
    /// Group value read.
    GroupValueRead {
        address: GroupAddress,
        source: IndividualAddress,
    },
    /// Sequence validation event.
    SequenceEvent {
        channel_id: u8,
        validation: SequenceEventType,
    },
    /// L_Data.con sent.
    ConfirmationSent { channel_id: u8, success: bool },
    /// Bus monitor frame sent.
    BusMonitorFrameSent {
        channel_id: u8,
        message_code: u8,
        raw_frame_len: usize,
    },
    /// Property read request processed.
    PropertyRead {
        channel_id: u8,
        object_index: u16,
        property_id: u16,
    },
    /// Property write request processed.
    PropertyWrite {
        channel_id: u8,
        object_index: u16,
        property_id: u16,
    },
    /// Device reset requested.
    DeviceReset { channel_id: u8 },
    /// L_Data.ind broadcast to other connections.
    DataIndBroadcast {
        source_channel_id: u8,
        target_channel_count: usize,
        group_address: GroupAddress,
    },
    /// Frame delayed by PaceFilter (bus timing simulation).
    FrameDelayed {
        channel_id: u8,
        delay_ms: u64,
        pace_state: String,
    },
    /// Frame queued by QueueFilter (backpressure active).
    FrameQueued {
        channel_id: u8,
        priority: String,
        queue_depth: usize,
    },
    /// Frame dropped by flow control filter.
    FrameDropped { channel_id: u8, reason: String },
    /// Circuit breaker state changed.
    CircuitBreakerStateChanged {
        new_state: String,
        failure_count: u32,
    },
    /// Queued frames drained after ACK received.
    QueueDrained {
        channel_id: u8,
        drained_count: usize,
    },
    /// Heartbeat action taken (non-Continue).
    HeartbeatAction {
        channel_id: u8,
        action: String,
        status_code: Option<u8>,
    },
    /// Heartbeat suppressed (NoResponse action).
    HeartbeatSuppressed { channel_id: u8 },
    /// Group value cache updated.
    GroupValueCacheUpdated {
        address: GroupAddress,
        source: String,
        cache_size: usize,
    },
    /// Send error threshold exceeded — tunnel restart recommended.
    SendErrorThreshold {
        channel_id: u8,
        consecutive_errors: u32,
        threshold: u32,
    },
    /// Send error rate warning.
    SendErrorRateWarning {
        channel_id: u8,
        error_count: usize,
        window_ms: u64,
        rate_percent: u32,
    },
    /// Error occurred.
    Error { message: String },
}

/// Sequence validation event type for observability.
#[derive(Debug, Clone)]
pub enum SequenceEventType {
    /// Valid sequence processed.
    Valid { sequence: u8 },
    /// Duplicate frame detected (ACK sent, not processed).
    Duplicate { sequence: u8, expected: u8 },
    /// Out-of-order frame (processed with warning).
    OutOfOrder {
        sequence: u8,
        expected: u8,
        distance: u8,
    },
    /// Fatal desync — tunnel restart required.
    FatalDesync {
        sequence: u8,
        expected: u8,
        distance: u8,
    },
}

// ============================================================================
// Server State
// ============================================================================

/// Server state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ServerState {
    /// Server is stopped.
    #[default]
    Stopped,
    /// Server is starting.
    Starting,
    /// Server is running.
    Running,
    /// Server is stopping.
    Stopping,
}

// ============================================================================
// Connection Manager
// ============================================================================

/// Manages tunnel connections with per-connection FSM and sequence tracking.
pub struct ConnectionManager {
    connections: DashMap<u8, Arc<TunnelConnection>>,
    next_channel_id: AtomicU8,
    max_connections: usize,
    heartbeat_timeout: Duration,
    individual_address_base: IndividualAddress,
    desync_threshold: u8,
}

impl ConnectionManager {
    /// Create a new connection manager.
    pub fn new(
        max_connections: usize,
        heartbeat_timeout: Duration,
        individual_address_base: IndividualAddress,
    ) -> Self {
        Self {
            connections: DashMap::new(),
            next_channel_id: AtomicU8::new(1),
            max_connections,
            heartbeat_timeout,
            individual_address_base,
            desync_threshold: 5,
        }
    }

    /// Create with custom fatal desync threshold.
    pub fn with_desync_threshold(mut self, threshold: u8) -> Self {
        self.desync_threshold = threshold;
        self
    }

    /// Allocate a new channel.
    pub fn allocate_channel(&self) -> Option<u8> {
        if self.connections.len() >= self.max_connections {
            return None;
        }

        // Find next available channel ID
        for _ in 0..255 {
            let channel_id = self.next_channel_id.fetch_add(1, Ordering::SeqCst);
            if channel_id != 0 && !self.connections.contains_key(&channel_id) {
                return Some(channel_id);
            }
        }

        None
    }

    /// Create a new connection with Phase 1 components.
    pub fn create_connection(
        &self,
        channel_id: u8,
        client_addr: SocketAddr,
        data_endpoint: SocketAddr,
    ) -> Arc<TunnelConnection> {
        // Assign individual address based on channel
        let individual_address = IndividualAddress::new(
            self.individual_address_base.area(),
            self.individual_address_base.line(),
            100 + channel_id,
        );

        let connection = Arc::new(TunnelConnection::with_desync_threshold(
            channel_id,
            client_addr,
            data_endpoint,
            individual_address,
            self.heartbeat_timeout,
            self.desync_threshold,
        ));

        // Transition FSM: Connecting → Idle (handshake completed at this point)
        connection.fsm.on_connected();

        self.connections.insert(channel_id, connection.clone());
        connection
    }

    /// Get a connection.
    pub fn get(&self, channel_id: u8) -> Option<Arc<TunnelConnection>> {
        self.connections.get(&channel_id).map(|c| c.clone())
    }

    /// Remove a connection.
    pub fn remove(&self, channel_id: u8) -> Option<Arc<TunnelConnection>> {
        self.connections.remove(&channel_id).map(|(_, c)| {
            c.fsm.on_disconnected();
            c
        })
    }

    /// Get all connections.
    pub fn all(&self) -> Vec<Arc<TunnelConnection>> {
        self.connections.iter().map(|r| r.value().clone()).collect()
    }

    /// Get connection count.
    pub fn len(&self) -> usize {
        self.connections.len()
    }

    /// Check if empty.
    pub fn is_empty(&self) -> bool {
        self.connections.is_empty()
    }

    /// Clean up timed out connections.
    pub fn cleanup_timed_out(&self) -> Vec<u8> {
        let timed_out: Vec<_> = self
            .connections
            .iter()
            .filter(|r| r.value().is_timed_out())
            .map(|r| *r.key())
            .collect();

        for channel_id in &timed_out {
            if let Some((_, conn)) = self.connections.remove(channel_id) {
                conn.fsm.on_disconnected();
            }
        }

        timed_out
    }
}

// ============================================================================
// KNX Server
// ============================================================================

/// KNXnet/IP Server with full Phase 1-5 support.
///
/// Phase 4 additions:
/// - HeartbeatScheduler: 5 heartbeat action types with configurable scheduling
/// - GroupValueCache: TTL/LRU cache with auto-update on indication
/// - SendErrorTracker: Consecutive and sliding window error rate monitoring
///
/// Phase 5 additions:
/// - KnxMetricsCollector: Unified 62-field metrics snapshot with Prometheus exposition
/// - KnxDiagnostics: 9-rule automatic health analysis with configurable thresholds
/// - ConnectionMetricsSnapshot: Per-connection tunnel/FSM/sequence detail export
pub struct KnxServer {
    config: KnxServerConfig,
    state: parking_lot::RwLock<ServerState>,
    connections: ConnectionManager,
    group_objects: Arc<GroupObjectTable>,
    filter_chain: FilterChain,
    heartbeat_scheduler: HeartbeatScheduler,
    group_value_cache: GroupValueCache,
    error_tracker: SendErrorTracker,
    metrics_collector: KnxMetricsCollector,
    event_tx: broadcast::Sender<ServerEvent>,
    shutdown_tx: parking_lot::Mutex<Option<mpsc::Sender<()>>>,
    running: AtomicBool,
}

impl KnxServer {
    /// Create a new KNX server.
    pub fn new(config: KnxServerConfig) -> Self {
        let (event_tx, _) = broadcast::channel(1000);
        let desync_threshold = config
            .tunnel_behavior
            .sequence_validation_enabled
            .then_some(5u8)
            .unwrap_or(255); // 255 effectively disables desync detection

        let filter_chain = FilterChain::new(config.tunnel_behavior.flow_control.clone());

        // Phase 4: initialize heartbeat scheduler
        let heartbeat_scheduler = if config.tunnel_behavior.heartbeat_scheduler.enabled {
            HeartbeatScheduler::new(config.tunnel_behavior.heartbeat_scheduler.schedule.clone())
        } else {
            HeartbeatScheduler::normal()
        };

        // Phase 4: initialize group value cache
        let group_value_cache =
            GroupValueCache::new(config.tunnel_behavior.group_value_cache.clone());

        // Phase 4: initialize send error tracker
        let error_tracker =
            SendErrorTracker::new(config.tunnel_behavior.send_error_tracker.clone());

        Self {
            connections: ConnectionManager::new(
                config.max_connections,
                config.connection_timeout(),
                config.individual_address,
            )
            .with_desync_threshold(desync_threshold),
            filter_chain,
            heartbeat_scheduler,
            group_value_cache,
            error_tracker,
            metrics_collector: KnxMetricsCollector::new(),
            config,
            state: parking_lot::RwLock::new(ServerState::Stopped),
            group_objects: Arc::new(GroupObjectTable::new()),
            event_tx,
            shutdown_tx: parking_lot::Mutex::new(None),
            running: AtomicBool::new(false),
        }
    }

    /// Create with custom group object table.
    pub fn with_group_objects(mut self, table: Arc<GroupObjectTable>) -> Self {
        self.group_objects = table;
        self
    }

    /// Get server state.
    pub fn state(&self) -> ServerState {
        *self.state.read()
    }

    /// Check if running.
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }

    /// Get config.
    pub fn config(&self) -> &KnxServerConfig {
        &self.config
    }

    /// Get group object table.
    pub fn group_objects(&self) -> Arc<GroupObjectTable> {
        self.group_objects.clone()
    }

    /// Subscribe to server events.
    pub fn subscribe(&self) -> broadcast::Receiver<ServerEvent> {
        self.event_tx.subscribe()
    }

    /// Get connection count.
    pub fn connection_count(&self) -> usize {
        self.connections.len()
    }

    /// Get a connection by channel ID (for testing/diagnostics).
    pub fn get_connection(&self, channel_id: u8) -> Option<Arc<TunnelConnection>> {
        self.connections.get(channel_id)
    }

    /// Get a reference to the flow control filter chain.
    pub fn filter_chain(&self) -> &FilterChain {
        &self.filter_chain
    }

    /// Get the current PaceFilter state.
    pub fn pace_state(&self) -> PaceState {
        self.filter_chain.pace_state()
    }

    /// Get the current CircuitBreaker state.
    pub fn circuit_breaker_state(&self) -> CircuitBreakerState {
        self.filter_chain.circuit_breaker_state()
    }

    /// Get a reference to the heartbeat scheduler.
    pub fn heartbeat_scheduler(&self) -> &HeartbeatScheduler {
        &self.heartbeat_scheduler
    }

    /// Get a reference to the group value cache.
    pub fn group_value_cache(&self) -> &GroupValueCache {
        &self.group_value_cache
    }

    /// Get a reference to the send error tracker.
    pub fn error_tracker(&self) -> &SendErrorTracker {
        &self.error_tracker
    }

    /// Get a reference to the metrics collector.
    pub fn metrics_collector(&self) -> &KnxMetricsCollector {
        &self.metrics_collector
    }

    // ========================================================================
    // Phase 5: Observability & Diagnostics
    // ========================================================================

    /// Collect a unified metrics snapshot from all server components.
    ///
    /// This gathers statistics from every subsystem (heartbeat, cache,
    /// error tracker, filter chain, pace/queue/retry filters, tunnel
    /// connections) and produces a single [`KnxMetricsSnapshot`] with
    /// 62 fields. The snapshot can be exported as Prometheus text, JSON,
    /// or YAML.
    ///
    /// # Performance
    ///
    /// All underlying counters use `Ordering::Relaxed` atomic loads,
    /// so this method is lock-free and safe to call at any frequency
    /// without impacting server performance.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let snapshot = server.metrics_snapshot();
    /// println!("{}", snapshot.to_prometheus());
    /// println!("{}", snapshot.summary());
    /// ```
    pub fn metrics_snapshot(&self) -> KnxMetricsSnapshot {
        // Server-level state
        let server_state = match self.state() {
            ServerState::Stopped => 0u8,
            ServerState::Starting => 1,
            ServerState::Running => 2,
            ServerState::Stopping => 3,
        };

        // Phase 4 component snapshots
        let heartbeat = self.heartbeat_scheduler.stats_snapshot();
        let cache = self.group_value_cache.stats_snapshot();
        let cache_entries = self.group_value_cache.len();
        let error_tracker = self.error_tracker.stats_snapshot();

        // Phase 3 filter chain snapshots
        let filter_chain = self.filter_chain.stats_snapshot();
        let pace = self.filter_chain.pace_filter().stats_snapshot();
        let queue = self.filter_chain.queue_filter().stats_snapshot();
        let retry = self.filter_chain.retry_filter().stats_snapshot();

        // Per-connection tunnel snapshots (aggregated)
        let connections = self.connections.all();
        let seq_stats: Vec<_> = connections
            .iter()
            .map(|c| c.sequence_tracker.stats_snapshot())
            .collect();
        let fsm_stats: Vec<_> = connections.iter().map(|c| c.fsm.stats_snapshot()).collect();

        self.metrics_collector.collect(
            server_state,
            connections.len(),
            self.config.max_connections,
            &heartbeat,
            &cache,
            cache_entries,
            &error_tracker,
            &filter_chain,
            &pace,
            &queue,
            &retry,
            &seq_stats,
            &fsm_stats,
        )
    }

    /// Run automatic diagnostic analysis on current server metrics.
    ///
    /// Evaluates all 9 diagnostic rules using default thresholds and
    /// returns a [`KnxDiagnostics`] result with severity, messages,
    /// and recommendations for each rule.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let diag = server.diagnostics();
    /// if !diag.is_healthy() {
    ///     for problem in diag.problems() {
    ///         eprintln!("[{}] {}: {}", problem.severity, problem.rule, problem.message);
    ///         eprintln!("  → {}", problem.recommendation);
    ///     }
    /// }
    /// ```
    pub fn diagnostics(&self) -> KnxDiagnostics {
        let snapshot = self.metrics_snapshot();
        KnxDiagnostics::analyze(&snapshot)
    }

    /// Run diagnostic analysis with custom thresholds.
    ///
    /// Allows tuning warning/critical thresholds for each of the 9
    /// diagnostic rules. See [`DiagnosticConfig`] for available options.
    pub fn diagnostics_with_config(&self, config: &DiagnosticConfig) -> KnxDiagnostics {
        let snapshot = self.metrics_snapshot();
        KnxDiagnostics::analyze_with_config(&snapshot, config)
    }

    /// Collect per-connection metrics snapshots for detailed diagnostics.
    ///
    /// Returns a [`ConnectionMetricsSnapshot`] for each active tunnel
    /// connection, containing FSM state, sequence statistics, idle
    /// duration, and timeout status.
    pub fn connection_metrics(&self) -> Vec<ConnectionMetricsSnapshot> {
        self.connections
            .all()
            .iter()
            .map(|conn| {
                let seq = conn.sequence_tracker.stats_snapshot();
                let fsm = conn.fsm.stats_snapshot();
                ConnectionMetricsSnapshot {
                    channel_id: conn.channel_id,
                    individual_address: conn.individual_address.to_string(),
                    fsm_state: conn.fsm.state().to_string(),
                    fsm_transitions: fsm.transitions,
                    frames_sent: seq.frames_sent,
                    frames_received: seq.frames_received,
                    duplicates_detected: seq.duplicates_detected,
                    out_of_order_detected: seq.out_of_order_detected,
                    fatal_desyncs: seq.fatal_desyncs,
                    resets: seq.resets,
                    idle_duration_ms: conn.idle_duration().as_millis() as u64,
                    is_timed_out: conn.is_timed_out(),
                }
            })
            .collect()
    }

    /// Start the server.
    pub async fn start(&self) -> KnxResult<()> {
        if self.is_running() {
            return Err(KnxError::ServerAlreadyRunning);
        }

        *self.state.write() = ServerState::Starting;

        // Bind UDP socket
        let socket =
            UdpSocket::bind(&self.config.bind_addr)
                .await
                .map_err(|e| KnxError::BindError {
                    address: self.config.bind_addr,
                    reason: e.to_string(),
                })?;

        let local_addr = socket.local_addr()?;
        info!(address = %local_addr, "KNXnet/IP server started");

        self.running.store(true, Ordering::SeqCst);
        *self.state.write() = ServerState::Running;

        let _ = self.event_tx.send(ServerEvent::Started {
            address: local_addr,
        });

        // Create shutdown channel
        let (shutdown_tx, mut shutdown_rx) = mpsc::channel::<()>(1);
        *self.shutdown_tx.lock() = Some(shutdown_tx);

        let socket = Arc::new(socket);

        // Start receive loop
        let mut buf = vec![0u8; 1024];

        loop {
            tokio::select! {
                result = socket.recv_from(&mut buf) => {
                    match result {
                        Ok((len, addr)) => {
                            if let Err(e) = self.handle_packet(&socket, &buf[..len], addr).await {
                                debug!(error = %e, "Error handling packet");
                            }
                        }
                        Err(e) => {
                            error!(error = %e, "Error receiving packet");
                        }
                    }
                }
                _ = shutdown_rx.recv() => {
                    info!("Server shutdown requested");
                    break;
                }
            }
        }

        self.running.store(false, Ordering::SeqCst);
        *self.state.write() = ServerState::Stopped;
        let _ = self.event_tx.send(ServerEvent::Stopped);

        Ok(())
    }

    /// Stop the server.
    pub async fn stop(&self) -> KnxResult<()> {
        if !self.is_running() {
            return Ok(());
        }

        *self.state.write() = ServerState::Stopping;

        let shutdown_tx = self.shutdown_tx.lock().take();
        if let Some(tx) = shutdown_tx {
            let _ = tx.send(()).await;
        }

        Ok(())
    }

    /// Handle incoming packet.
    async fn handle_packet(
        &self,
        socket: &UdpSocket,
        data: &[u8],
        addr: SocketAddr,
    ) -> KnxResult<()> {
        let frame = KnxFrame::decode(data)?;

        trace!(
            service_type = ?frame.service_type,
            from = %addr,
            "Received KNXnet/IP frame"
        );

        match frame.service_type {
            ServiceType::SearchRequest => {
                self.handle_search_request(socket, addr).await?;
            }
            ServiceType::DescriptionRequest => {
                self.handle_description_request(socket, addr).await?;
            }
            ServiceType::ConnectRequest => {
                self.handle_connect_request(socket, &frame.body, addr)
                    .await?;
            }
            ServiceType::ConnectionStateRequest => {
                self.handle_connection_state_request(socket, &frame.body, addr)
                    .await?;
            }
            ServiceType::DisconnectRequest => {
                self.handle_disconnect_request(socket, &frame.body, addr)
                    .await?;
            }
            ServiceType::TunnellingRequest => {
                self.handle_tunnelling_request(socket, &frame.body, addr)
                    .await?;
            }
            ServiceType::TunnellingAck => {
                if self.filter_chain.is_enabled() {
                    self.handle_tunnelling_ack_async(socket, &frame.body)
                        .await?;
                } else {
                    self.handle_tunnelling_ack(&frame.body)?;
                }
            }
            _ => {
                debug!(service_type = ?frame.service_type, "Unhandled service type");
            }
        }

        Ok(())
    }

    // ========================================================================
    // Discovery handlers (unchanged)
    // ========================================================================

    async fn handle_search_request(&self, socket: &UdpSocket, addr: SocketAddr) -> KnxResult<()> {
        debug!(from = %addr, "Handling SearchRequest");

        let local_addr = socket.local_addr()?;
        let local_ip = match local_addr {
            SocketAddr::V4(v4) => *v4.ip(),
            _ => std::net::Ipv4Addr::UNSPECIFIED,
        };

        let hpai = Hpai::udp_ipv4(local_ip, local_addr.port());
        let device_info =
            DibDeviceInfo::new(&self.config.device_name, self.config.individual_address)
                .with_serial_number(self.config.serial_number)
                .with_mac_address(self.config.mac_address);
        let families = SupportedServiceFamilies::default_families();

        let mut body = hpai.encode();
        body.extend(device_info.encode());
        body.extend(families.encode());

        let response = KnxFrame::new(ServiceType::SearchResponse, body);
        socket.send_to(&response.encode(), addr).await?;

        Ok(())
    }

    async fn handle_description_request(
        &self,
        socket: &UdpSocket,
        addr: SocketAddr,
    ) -> KnxResult<()> {
        debug!(from = %addr, "Handling DescriptionRequest");

        let device_info =
            DibDeviceInfo::new(&self.config.device_name, self.config.individual_address)
                .with_serial_number(self.config.serial_number)
                .with_mac_address(self.config.mac_address);
        let families = SupportedServiceFamilies::default_families();

        let mut body = device_info.encode();
        body.extend(families.encode());

        let response = KnxFrame::new(ServiceType::DescriptionResponse, body);
        socket.send_to(&response.encode(), addr).await?;

        Ok(())
    }

    // ========================================================================
    // Connection lifecycle
    // ========================================================================

    async fn handle_connect_request(
        &self,
        socket: &UdpSocket,
        data: &[u8],
        addr: SocketAddr,
    ) -> KnxResult<()> {
        let request = ConnectRequest::decode(data)?;
        debug!(from = %addr, "Handling ConnectRequest");

        let channel_id = match self.connections.allocate_channel() {
            Some(id) => id,
            None => {
                let response = ConnectResponse::error(ConnectStatus::NoMoreConnections);
                let frame = KnxFrame::new(ServiceType::ConnectResponse, response.encode());
                socket.send_to(&frame.encode(), addr).await?;
                return Ok(());
            }
        };

        let data_endpoint = if request.data_endpoint.is_nat() {
            addr
        } else {
            request.data_endpoint.to_socket_addr_v()
        };

        let connection = self
            .connections
            .create_connection(channel_id, addr, data_endpoint);

        let local_addr = socket.local_addr()?;
        let local_ip = match local_addr {
            SocketAddr::V4(v4) => *v4.ip(),
            _ => std::net::Ipv4Addr::UNSPECIFIED,
        };

        let response = ConnectResponse::success(
            channel_id,
            Hpai::udp_ipv4(local_ip, local_addr.port()),
            ConnectionResponseData::new(connection.individual_address),
        );

        let frame = KnxFrame::new(ServiceType::ConnectResponse, response.encode());
        socket.send_to(&frame.encode(), addr).await?;

        info!(
            channel_id,
            client = %addr,
            individual_address = %connection.individual_address,
            "Client connected"
        );

        let _ = self.event_tx.send(ServerEvent::ClientConnected {
            channel_id,
            address: addr,
        });

        Ok(())
    }

    /// Handle heartbeat with configurable status code simulation.
    ///
    /// Heartbeat response determination priority:
    /// 1. If `heartbeat_scheduler` is enabled → use scheduler to determine action
    /// 2. If `heartbeat_status_override` is set → use static override (legacy)
    /// 3. Otherwise → normal behavior (0x00 if connected, 0x21 if unknown)
    ///
    /// The HeartbeatScheduler supports 5 action types:
    /// - Continue → 0x00 (normal)
    /// - ImmediateReconnect → 0x21 (E_CONNECTION_ID)
    /// - AbandonTunnel → 0x27 (E_KNX_CONNECTION)
    /// - DelayedReconnect → 0x29 (E_TUNNELLING_LAYER)
    /// - NoResponse → no packet sent (simulates timeout)
    async fn handle_connection_state_request(
        &self,
        socket: &UdpSocket,
        data: &[u8],
        addr: SocketAddr,
    ) -> KnxResult<()> {
        let request = ConnectionStateRequest::decode(data)?;

        // Phase 4: Use heartbeat scheduler if enabled
        if self.config.tunnel_behavior.heartbeat_scheduler.enabled {
            let action = self.heartbeat_scheduler.next_action(request.channel_id);

            match action.status_code() {
                Some(status) => {
                    if !action.is_normal() {
                        debug!(
                            channel_id = request.channel_id,
                            action = %action,
                            status = status,
                            "Heartbeat scheduler action"
                        );
                        let _ = self.event_tx.send(ServerEvent::HeartbeatAction {
                            channel_id: request.channel_id,
                            action: action.to_string(),
                            status_code: Some(status),
                        });
                    }

                    // Touch connection if it exists and action is Continue
                    if action.is_normal() {
                        if let Some(conn) = self.connections.get(request.channel_id) {
                            conn.touch();
                        }
                    }

                    let response = ConnectionStateResponse {
                        channel_id: request.channel_id,
                        status,
                    };
                    let frame =
                        KnxFrame::new(ServiceType::ConnectionStateResponse, response.encode());
                    socket.send_to(&frame.encode(), addr).await?;
                }
                None => {
                    // NoResponse action — suppress the heartbeat reply
                    debug!(
                        channel_id = request.channel_id,
                        "Heartbeat suppressed (NoResponse)"
                    );
                    let _ = self.event_tx.send(ServerEvent::HeartbeatSuppressed {
                        channel_id: request.channel_id,
                    });
                    // Intentionally do NOT send any response
                }
            }

            return Ok(());
        }

        // Legacy: static status override
        let response =
            if let Some(status_override) = self.config.tunnel_behavior.heartbeat_status_override {
                debug!(
                    channel_id = request.channel_id,
                    status = status_override,
                    "Heartbeat override"
                );
                ConnectionStateResponse {
                    channel_id: request.channel_id,
                    status: status_override,
                }
            } else if let Some(conn) = self.connections.get(request.channel_id) {
                conn.touch();
                ConnectionStateResponse::ok(request.channel_id)
            } else {
                // Unknown channel → 0x21 E_CONNECTION_ID (knxd standard)
                ConnectionStateResponse {
                    channel_id: request.channel_id,
                    status: 0x21,
                }
            };

        let frame = KnxFrame::new(ServiceType::ConnectionStateResponse, response.encode());
        socket.send_to(&frame.encode(), addr).await?;

        Ok(())
    }

    async fn handle_disconnect_request(
        &self,
        socket: &UdpSocket,
        data: &[u8],
        addr: SocketAddr,
    ) -> KnxResult<()> {
        let request = DisconnectRequest::decode(data)?;
        debug!(
            channel_id = request.channel_id,
            "Handling DisconnectRequest"
        );

        self.connections.remove(request.channel_id);

        // Clean up flow control queue state for this channel
        if self.filter_chain.is_enabled() {
            self.filter_chain
                .queue_filter()
                .clear_channel(request.channel_id);
        }

        // Phase 4: Clean up error tracker state for this channel
        self.error_tracker.remove_channel(request.channel_id);

        let response = DisconnectResponse::ok(request.channel_id);
        let frame = KnxFrame::new(ServiceType::DisconnectResponse, response.encode());
        socket.send_to(&frame.encode(), addr).await?;

        info!(channel_id = request.channel_id, "Client disconnected");

        let _ = self.event_tx.send(ServerEvent::ClientDisconnected {
            channel_id: request.channel_id,
        });

        Ok(())
    }

    // ========================================================================
    // Tunnelling with Phase 1 sequence validation
    // ========================================================================

    /// Handle TUNNELLING_REQUEST with knxd-compatible sequence validation.
    ///
    /// Implements the full validation chain:
    /// 1. Validate channel ID
    /// 2. Validate sequence number (Valid/Duplicate/OutOfOrder/FatalDesync)
    /// 3. Send ACK (with appropriate status)
    /// 4. Process cEMI frame (if Valid or OutOfOrder)
    /// 5. Send L_Data.con (if L_Data.req and confirmation enabled)
    async fn handle_tunnelling_request(
        &self,
        socket: &UdpSocket,
        data: &[u8],
        addr: SocketAddr,
    ) -> KnxResult<()> {
        let request = TunnellingRequest::decode(data)?;

        let connection = match self.connections.get(request.channel_id) {
            Some(conn) => conn,
            None => {
                // Unknown channel → error ACK with 0x21
                let ack = TunnellingAck::error(request.channel_id, request.sequence_counter, 0x21);
                let frame = KnxFrame::new(ServiceType::TunnellingAck, ack.encode());
                socket.send_to(&frame.encode(), addr).await?;
                return Ok(());
            }
        };

        connection.touch();

        // Validate sequence number with knxd-compatible logic
        let validation = connection.validate_recv_sequence(request.sequence_counter);

        // Emit sequence event for observability
        let seq_event = match &validation {
            ReceivedValidation::Valid { sequence } => SequenceEventType::Valid {
                sequence: *sequence,
            },
            ReceivedValidation::Duplicate { sequence, expected } => {
                debug!(
                    channel_id = request.channel_id,
                    sequence, expected, "Duplicate frame — ACK only"
                );
                SequenceEventType::Duplicate {
                    sequence: *sequence,
                    expected: *expected,
                }
            }
            ReceivedValidation::OutOfOrder {
                sequence,
                expected,
                distance,
            } => {
                warn!(
                    channel_id = request.channel_id,
                    sequence, expected, distance, "Out-of-order frame"
                );
                SequenceEventType::OutOfOrder {
                    sequence: *sequence,
                    expected: *expected,
                    distance: *distance,
                }
            }
            ReceivedValidation::FatalDesync {
                sequence,
                expected,
                distance,
            } => {
                error!(
                    channel_id = request.channel_id,
                    sequence, expected, distance, "Fatal sequence desync — tunnel restart required"
                );
                SequenceEventType::FatalDesync {
                    sequence: *sequence,
                    expected: *expected,
                    distance: *distance,
                }
            }
        };

        let _ = self.event_tx.send(ServerEvent::SequenceEvent {
            channel_id: request.channel_id,
            validation: seq_event,
        });

        // Send ACK (unless FatalDesync — knxd does not ACK desynced frames)
        if validation.should_ack() {
            let ack = TunnellingAck::ok(request.channel_id, request.sequence_counter);
            let frame = KnxFrame::new(ServiceType::TunnellingAck, ack.encode());
            socket.send_to(&frame.encode(), addr).await?;
        }

        // Process frame only if it should be processed
        if validation.should_process() {
            self.process_cemi(socket, addr, &request.cemi, &connection)
                .await?;
        }

        // FatalDesync → trigger disconnect event
        if validation.requires_restart() {
            // Clean up flow control queue state
            if self.filter_chain.is_enabled() {
                self.filter_chain
                    .queue_filter()
                    .clear_channel(request.channel_id);
            }

            // Phase 4: Clean up error tracker
            self.error_tracker.remove_channel(request.channel_id);

            self.connections.remove(request.channel_id);
            let _ = self.event_tx.send(ServerEvent::ClientDisconnected {
                channel_id: request.channel_id,
            });
        }

        Ok(())
    }

    /// Handle TUNNELLING_ACK from client (for server→client frame delivery tracking).
    ///
    /// When flow control is enabled, this also:
    /// - Releases QueueFilter backpressure for the channel
    /// - Feeds success/failure to the RetryFilter circuit breaker
    /// - Does NOT drain queued frames here (the async version does)
    fn handle_tunnelling_ack(&self, data: &[u8]) -> KnxResult<()> {
        let ack = TunnellingAck::decode(data)?;

        if let Some(conn) = self.connections.get(ack.channel_id) {
            conn.touch();

            // Feed ACK to the connection's ACK channel for AckWaiter
            conn.feed_ack(AckMessage {
                channel_id: ack.channel_id,
                sequence: ack.sequence_counter,
                status: ack.status,
            });

            // Transition FSM: WaitingForAck → Idle (simple ACK case)
            conn.fsm.on_ack_received_simple();

            // Release flow control backpressure
            if self.filter_chain.is_enabled() {
                if ack.status == 0 {
                    self.filter_chain.on_send_success(conn.channel_id);
                } else {
                    self.filter_chain.on_send_failure(
                        conn.channel_id,
                        &format!("ACK error status: {:#04x}", ack.status),
                    );
                }
            }

            // Phase 4: Track ACK errors
            if ack.status == 0 {
                self.error_tracker.on_send_success(conn.channel_id);
            } else {
                self.handle_send_error_tracking(conn.channel_id, ErrorCategory::AckError);
            }
        }

        Ok(())
    }

    /// Handle TUNNELLING_ACK from client with async queue draining.
    ///
    /// This is the async variant that also drains queued frames after
    /// backpressure is released.
    async fn handle_tunnelling_ack_async(&self, socket: &UdpSocket, data: &[u8]) -> KnxResult<()> {
        let ack = TunnellingAck::decode(data)?;

        if let Some(conn) = self.connections.get(ack.channel_id) {
            conn.touch();

            // Feed ACK to the connection's ACK channel for AckWaiter
            conn.feed_ack(AckMessage {
                channel_id: ack.channel_id,
                sequence: ack.sequence_counter,
                status: ack.status,
            });

            // Transition FSM: WaitingForAck → Idle (simple ACK case)
            conn.fsm.on_ack_received_simple();

            // Release flow control backpressure and drain queued frames
            if self.filter_chain.is_enabled() {
                if ack.status == 0 {
                    self.filter_chain.on_send_success(conn.channel_id);
                    // Drain queued frames now that backpressure is released
                    self.drain_queued_frames(socket, &conn).await;
                } else {
                    self.filter_chain.on_send_failure(
                        conn.channel_id,
                        &format!("ACK error status: {:#04x}", ack.status),
                    );
                }
            }

            // Phase 4: Track ACK errors
            if ack.status == 0 {
                self.error_tracker.on_send_success(conn.channel_id);
            } else {
                self.handle_send_error_tracking(conn.channel_id, ErrorCategory::AckError);
            }
        }

        Ok(())
    }

    // ========================================================================
    // cEMI processing — full message code dispatcher
    // ========================================================================

    /// Process cEMI frame — dispatches all message codes with L_Data.con support.
    ///
    /// This is the central cEMI processing pipeline:
    /// 1. Dispatch based on message_code (L_Data, M_Prop*, M_Reset, L_Busmon, L_Raw)
    /// 2. For L_Data.req: process group services, send L_Data.con, broadcast L_Data.ind
    /// 3. For M_PropRead.req: respond with M_PropRead.con
    /// 4. For M_PropWrite.req: respond with M_PropWrite.con
    /// 5. For M_Reset.req: respond with M_Reset.ind
    /// 6. Optionally generate Bus Monitor frames for all bus traffic
    async fn process_cemi(
        &self,
        socket: &UdpSocket,
        client_addr: SocketAddr,
        cemi: &CemiFrame,
        connection: &TunnelConnection,
    ) -> KnxResult<()> {
        match cemi.message_code {
            MessageCode::LDataReq => {
                self.handle_ldata_req(socket, client_addr, cemi, connection)
                    .await?;
            }
            MessageCode::LDataInd => {
                // Client sending L_Data.ind — unusual but process group services
                self.handle_ldata_data(socket, client_addr, cemi, connection)
                    .await?;
            }
            MessageCode::MPropReadReq => {
                if self.config.tunnel_behavior.property_service_enabled {
                    self.handle_prop_read_req(socket, client_addr, cemi, connection)
                        .await?;
                }
            }
            MessageCode::MPropWriteReq => {
                if self.config.tunnel_behavior.property_service_enabled {
                    self.handle_prop_write_req(socket, client_addr, cemi, connection)
                        .await?;
                }
            }
            MessageCode::MResetReq => {
                if self.config.tunnel_behavior.reset_service_enabled {
                    self.handle_reset_req(socket, client_addr, connection)
                        .await?;
                }
            }
            MessageCode::LRawReq => {
                // L_Raw.req: send L_Raw.con (success) — raw bus access
                self.send_ldata_con(socket, client_addr, connection, cemi, true)
                    .await?;
            }
            _ => {
                debug!(
                    message_code = ?cemi.message_code,
                    channel_id = connection.channel_id,
                    "Unhandled cEMI message code"
                );
            }
        }

        // Generate Bus Monitor frame if enabled
        if self.config.tunnel_behavior.bus_monitor_enabled {
            self.emit_bus_monitor_frame(socket, cemi, connection)
                .await?;
        }

        Ok(())
    }

    /// Handle L_Data.req — the primary data path.
    ///
    /// 1. Process group value services (Write/Read/Response)
    /// 2. Send L_Data.con (MC=0x2E) with success/failure status
    /// 3. Broadcast L_Data.ind to other tunnel connections
    async fn handle_ldata_req(
        &self,
        socket: &UdpSocket,
        client_addr: SocketAddr,
        cemi: &CemiFrame,
        connection: &TunnelConnection,
    ) -> KnxResult<()> {
        // Process group value services if applicable
        let bus_success = self
            .handle_ldata_data(socket, client_addr, cemi, connection)
            .await?;

        // Always send L_Data.con for L_Data.req when enabled
        if self.config.tunnel_behavior.ldata_con_enabled {
            let confirm_success = self.compute_confirmation_success(bus_success);

            // Optional bus delivery delay
            let delay_ms = self.config.tunnel_behavior.bus_delivery_delay_ms;
            if delay_ms > 0 {
                tokio::time::sleep(Duration::from_millis(delay_ms)).await;
            }

            self.send_ldata_con(socket, client_addr, connection, cemi, confirm_success)
                .await?;

            let _ = self.event_tx.send(ServerEvent::ConfirmationSent {
                channel_id: connection.channel_id,
                success: confirm_success,
            });
        }

        // Broadcast L_Data.ind to other tunnel connections
        if self.config.tunnel_behavior.ldata_ind_broadcast_enabled {
            if let Some(group_addr) = cemi.destination_group() {
                self.broadcast_ldata_ind(socket, cemi, connection, group_addr)
                    .await?;
            }
        }

        Ok(())
    }

    /// Handle L_Data group value services.
    ///
    /// Returns true if bus delivery succeeded.
    async fn handle_ldata_data(
        &self,
        socket: &UdpSocket,
        client_addr: SocketAddr,
        cemi: &CemiFrame,
        connection: &TunnelConnection,
    ) -> KnxResult<bool> {
        if !cemi.apci.is_group_value() {
            // Non-group-value APCIs (IndividualAddressRead, MemoryRead, etc.)
            // are still valid L_Data frames — bus delivery succeeds.
            return Ok(true);
        }

        let group_addr = match cemi.destination_group() {
            Some(addr) => addr,
            None => return Ok(true),
        };

        match cemi.apci {
            Apci::GroupValueWrite => {
                debug!(
                    address = %group_addr,
                    source = %cemi.source,
                    "Group Value Write"
                );

                let write_ok = self
                    .group_objects
                    .write(&group_addr, &cemi.data, Some(cemi.source.to_string()))
                    .is_ok();

                // Phase 4: Update group value cache on write
                self.group_value_cache.on_write(
                    group_addr,
                    cemi.data.clone(),
                    Some(cemi.source.to_string()),
                );

                let _ = self.event_tx.send(ServerEvent::GroupValueWrite {
                    address: group_addr,
                    value: cemi.data.clone(),
                    source: cemi.source,
                });

                Ok(write_ok)
            }
            Apci::GroupValueRead => {
                debug!(
                    address = %group_addr,
                    source = %cemi.source,
                    "Group Value Read"
                );

                let _ = self.event_tx.send(ServerEvent::GroupValueRead {
                    address: group_addr,
                    source: cemi.source,
                });

                // Send GroupValueResponse back via tunnelling
                let response_data = match self.group_objects.read(&group_addr) {
                    Ok(data) => data,
                    Err(_e) => vec![0u8],
                };

                let response_cemi = CemiFrame::group_value_response(
                    self.config.individual_address,
                    group_addr,
                    response_data,
                );

                self.send_tunnelling_request(socket, client_addr, connection, response_cemi)
                    .await?;
                Ok(true)
            }
            Apci::GroupValueResponse => {
                debug!(
                    address = %group_addr,
                    source = %cemi.source,
                    "Group Value Response"
                );

                // Store the response value in the group object table
                let _ = self.group_objects.write(
                    &group_addr,
                    &cemi.data,
                    Some(cemi.source.to_string()),
                );

                Ok(true)
            }
            _ => Ok(true),
        }
    }

    /// Compute confirmation success based on bus result and configured success rate.
    fn compute_confirmation_success(&self, bus_success: bool) -> bool {
        let rate = self.config.tunnel_behavior.confirmation_success_rate;
        if rate >= 1.0 {
            bus_success
        } else if rate <= 0.0 {
            false
        } else {
            bus_success && (rand_simple() < rate)
        }
    }

    // ========================================================================
    // Property service handlers (M_PropRead, M_PropWrite, M_Reset)
    // ========================================================================

    /// Handle M_PropRead.req — respond with M_PropRead.con.
    ///
    /// Simulates property read by returning well-known property values
    /// or a default response for unknown properties.
    async fn handle_prop_read_req(
        &self,
        socket: &UdpSocket,
        client_addr: SocketAddr,
        cemi: &CemiFrame,
        connection: &TunnelConnection,
    ) -> KnxResult<()> {
        let (object_index, property_id, count, start_index, _) = match cemi.parse_property_request()
        {
            Some(parsed) => parsed,
            None => {
                debug!(
                    channel_id = connection.channel_id,
                    "M_PropRead.req with invalid data format"
                );
                return Ok(());
            }
        };

        debug!(
            channel_id = connection.channel_id,
            object_index, property_id, count, start_index, "M_PropRead.req"
        );

        // Simulate well-known interface object properties (KNX AN033)
        let response_value = self.read_property(object_index, property_id);

        let response_cemi = CemiFrame::prop_read_con(
            object_index,
            property_id,
            count,
            start_index,
            response_value,
        );

        self.send_tunnelling_request(socket, client_addr, connection, response_cemi)
            .await?;

        let _ = self.event_tx.send(ServerEvent::PropertyRead {
            channel_id: connection.channel_id,
            object_index,
            property_id,
        });

        Ok(())
    }

    /// Handle M_PropWrite.req — respond with M_PropWrite.con.
    async fn handle_prop_write_req(
        &self,
        socket: &UdpSocket,
        client_addr: SocketAddr,
        cemi: &CemiFrame,
        connection: &TunnelConnection,
    ) -> KnxResult<()> {
        let (object_index, property_id, count, start_index, write_data) =
            match cemi.parse_property_request() {
                Some(parsed) => parsed,
                None => {
                    debug!(
                        channel_id = connection.channel_id,
                        "M_PropWrite.req with invalid data format"
                    );
                    return Ok(());
                }
            };

        debug!(
            channel_id = connection.channel_id,
            object_index,
            property_id,
            count,
            start_index,
            data_len = write_data.len(),
            "M_PropWrite.req"
        );

        // Property writes succeed for simulation purposes
        let response_cemi = CemiFrame::prop_write_con(
            object_index,
            property_id,
            count,
            start_index,
            true, // success
        );

        self.send_tunnelling_request(socket, client_addr, connection, response_cemi)
            .await?;

        let _ = self.event_tx.send(ServerEvent::PropertyWrite {
            channel_id: connection.channel_id,
            object_index,
            property_id,
        });

        Ok(())
    }

    /// Handle M_Reset.req — respond with M_Reset.ind.
    async fn handle_reset_req(
        &self,
        socket: &UdpSocket,
        client_addr: SocketAddr,
        connection: &TunnelConnection,
    ) -> KnxResult<()> {
        debug!(
            channel_id = connection.channel_id,
            "M_Reset.req — sending M_Reset.ind"
        );

        let response_cemi = CemiFrame::reset_ind();
        self.send_tunnelling_request(socket, client_addr, connection, response_cemi)
            .await?;

        let _ = self.event_tx.send(ServerEvent::DeviceReset {
            channel_id: connection.channel_id,
        });

        Ok(())
    }

    /// Read a simulated interface object property.
    ///
    /// Returns well-known property values for standard KNX interface objects.
    /// For unknown properties, returns a default 2-byte zero value.
    fn read_property(&self, object_index: u16, property_id: u16) -> Vec<u8> {
        match (object_index, property_id) {
            // Object 0 = Device Object
            // PID_OBJECT_TYPE (1)
            (0, 1) => vec![0x00, 0x00], // Device Object
            // PID_SERIAL_NUMBER (11)
            (0, 11) => self.config.serial_number.to_vec(),
            // PID_MANUFACTURER_ID (12)
            (0, 12) => vec![0x00, 0x00],
            // PID_DEVICE_CONTROL (14)
            (0, 14) => vec![0x00],
            // PID_ORDER_INFO (15)
            (0, 15) => vec![0x00; 10],
            // PID_FIRMWARE_REVISION (9)
            (0, 9) => vec![0x01],
            // PID_HARDWARE_TYPE (7)
            (0, 7) => vec![0x00; 6],
            // PID_SUBNET_ADDRESS (57)
            (0, 57) => vec![self.config.individual_address.area()],
            // PID_DEVICE_ADDRESS (58)
            (0, 58) => vec![self.config.individual_address.device()],
            // PID_MAX_APDULENGTH (56)
            (0, 56) => vec![0x00, 0xFE], // 254 bytes max APDU

            // Object 11 = KNXnet/IP Parameter Object
            // PID_KNX_INDIVIDUAL_ADDRESS (52)
            (11, 52) => self.config.individual_address.to_bytes().to_vec(),
            // PID_CURRENT_IP_ADDRESS (55)
            (11, 55) => match self.config.bind_addr {
                SocketAddr::V4(v4) => v4.ip().octets().to_vec(),
                _ => vec![0, 0, 0, 0],
            },
            // PID_FRIENDLY_NAME (7)
            (11, 7) => {
                let mut name = self.config.device_name.as_bytes().to_vec();
                name.truncate(30);
                name
            }
            // PID_MAC_ADDRESS (8)
            (11, 8) => self.config.mac_address.to_vec(),

            // Default: return 2-byte zero for unknown properties
            _ => vec![0x00, 0x00],
        }
    }

    // ========================================================================
    // Bus Monitor frame generation
    // ========================================================================

    /// Generate and emit a Bus Monitor indication (MC=0x2B) frame.
    ///
    /// Bus Monitor frames carry:
    /// - Additional Info TLV with BusMonitorInfo (0x03) status byte
    /// - Additional Info TLV with TimestampRelative (0x04) in ms
    /// - The raw bus frame data
    async fn emit_bus_monitor_frame(
        &self,
        socket: &UdpSocket,
        original_cemi: &CemiFrame,
        source_connection: &TunnelConnection,
    ) -> KnxResult<()> {
        // Encode the original cEMI to get raw bus frame bytes
        let raw_frame = original_cemi.encode();

        // Status byte: 0x00 = OK, bit 7 = frame error, bit 0 = parity error
        let status: u8 = if original_cemi.confirm { 0x80 } else { 0x00 };

        // Relative timestamp: 0 ms (we don't track inter-frame timing)
        let busmon_cemi = CemiFrame::bus_monitor_indication(&raw_frame, status, 0);
        let raw_frame_len = raw_frame.len();

        // Send bus monitor frame to ALL connections (including the source)
        let all_connections = self.connections.all();
        for conn in &all_connections {
            let target_addr = conn.data_endpoint;
            if let Err(e) = self
                .send_tunnelling_request(socket, target_addr, conn, busmon_cemi.clone())
                .await
            {
                debug!(
                    error = %e,
                    channel_id = conn.channel_id,
                    "Failed to send bus monitor frame"
                );
            }
        }

        let _ = self.event_tx.send(ServerEvent::BusMonitorFrameSent {
            channel_id: source_connection.channel_id,
            message_code: original_cemi.message_code as u8,
            raw_frame_len,
        });

        Ok(())
    }

    // ========================================================================
    // L_Data.ind broadcast to other tunnel connections
    // ========================================================================

    /// Broadcast L_Data.ind to all other tunnel connections.
    ///
    /// When a client sends a group value write, all other connected tunnels
    /// should receive the same data as L_Data.ind, simulating bus-level
    /// multicast behavior.
    async fn broadcast_ldata_ind(
        &self,
        socket: &UdpSocket,
        original_cemi: &CemiFrame,
        source_connection: &TunnelConnection,
        group_addr: GroupAddress,
    ) -> KnxResult<()> {
        let all_connections = self.connections.all();
        let mut target_count = 0usize;

        for conn in &all_connections {
            // Skip the source connection
            if conn.channel_id == source_connection.channel_id {
                continue;
            }

            // Create L_Data.ind frame from the original request
            let ind_cemi = CemiFrame {
                message_code: MessageCode::LDataInd,
                additional_info: Vec::new(),
                source: original_cemi.source,
                destination: original_cemi.destination,
                address_type: original_cemi.address_type,
                hop_count: original_cemi.hop_count,
                priority: original_cemi.priority,
                confirm: false,
                ack_request: false,
                system_broadcast: original_cemi.system_broadcast,
                apci: original_cemi.apci,
                data: original_cemi.data.clone(),
            };

            let target_addr = conn.data_endpoint;
            if let Err(e) = self
                .send_tunnelling_request(socket, target_addr, conn, ind_cemi)
                .await
            {
                debug!(
                    error = %e,
                    channel_id = conn.channel_id,
                    "Failed to broadcast L_Data.ind"
                );
            } else {
                target_count += 1;
            }
        }

        if target_count > 0 {
            // Phase 4: Update group value cache on indication broadcast
            self.group_value_cache.on_indication(
                group_addr,
                original_cemi.data.clone(),
                Some(original_cemi.source.to_string()),
            );

            let _ = self.event_tx.send(ServerEvent::DataIndBroadcast {
                source_channel_id: source_connection.channel_id,
                target_channel_count: target_count,
                group_address: group_addr,
            });

            let _ = self.event_tx.send(ServerEvent::GroupValueCacheUpdated {
                address: group_addr,
                source: format!("indication from {}", original_cemi.source),
                cache_size: self.group_value_cache.len(),
            });
        }

        Ok(())
    }

    // ========================================================================
    // Frame transmission helpers
    // ========================================================================

    /// Send a TUNNELLING_REQUEST frame through the flow control filter chain.
    ///
    /// This is the primary transmission path. Frames are processed through:
    /// 1. QueueFilter — may queue if backpressure is active
    /// 2. PaceFilter — may delay for bus timing simulation
    /// 3. RetryFilter — may drop if circuit breaker is open
    ///
    /// If the filter chain imposes a delay, the transmission is deferred.
    /// If the frame is queued, it will be sent later when the queue is drained.
    async fn send_tunnelling_request(
        &self,
        socket: &UdpSocket,
        client_addr: SocketAddr,
        connection: &TunnelConnection,
        cemi: CemiFrame,
    ) -> KnxResult<()> {
        // If filter chain is enabled, process through it
        if self.filter_chain.is_enabled() {
            let mut envelope = FrameEnvelope::new(cemi.clone(), connection.channel_id, client_addr);

            let result = self.filter_chain.send(&mut envelope);

            match result {
                FilterResult::Pass { delay } => {
                    if delay > Duration::ZERO {
                        let _ = self.event_tx.send(ServerEvent::FrameDelayed {
                            channel_id: connection.channel_id,
                            delay_ms: delay.as_millis() as u64,
                            pace_state: format!("{}", self.filter_chain.pace_state()),
                        });

                        tokio::time::sleep(delay).await;
                    }

                    // Proceed with actual transmission
                    let result = self
                        .send_tunnelling_request_raw(socket, client_addr, connection, cemi)
                        .await;

                    match &result {
                        Ok(()) => {
                            self.filter_chain.on_send_success(connection.channel_id);
                            self.error_tracker.on_send_success(connection.channel_id);
                        }
                        Err(e) => {
                            self.filter_chain
                                .on_send_failure(connection.channel_id, &e.to_string());
                            self.handle_send_error_tracking(
                                connection.channel_id,
                                ErrorCategory::SendFailure,
                            );
                        }
                    }

                    return result;
                }
                FilterResult::Queued => {
                    let _ = self.event_tx.send(ServerEvent::FrameQueued {
                        channel_id: connection.channel_id,
                        priority: format!("{}", envelope.priority),
                        queue_depth: self.filter_chain.pending_count(),
                    });
                    return Ok(());
                }
                FilterResult::Dropped { reason } => {
                    let _ = self.event_tx.send(ServerEvent::FrameDropped {
                        channel_id: connection.channel_id,
                        reason: reason.clone(),
                    });
                    debug!(
                        channel_id = connection.channel_id,
                        reason = %reason,
                        "Frame dropped by flow control"
                    );
                    return Ok(()); // Not an error — intentional drop
                }
                FilterResult::Error { message } => {
                    let _ = self.event_tx.send(ServerEvent::FrameDropped {
                        channel_id: connection.channel_id,
                        reason: message.clone(),
                    });
                    return Err(KnxError::FlowControlDrop { reason: message });
                }
            }
        }

        // Filter chain not enabled — direct send
        self.send_tunnelling_request_raw(socket, client_addr, connection, cemi)
            .await
    }

    /// Raw TUNNELLING_REQUEST send without filter chain processing.
    ///
    /// This bypasses the filter chain and sends directly to the UDP socket.
    /// Used internally by `send_tunnelling_request` after filtering, and also
    /// for queue draining where frames have already been filtered.
    async fn send_tunnelling_request_raw(
        &self,
        socket: &UdpSocket,
        client_addr: SocketAddr,
        connection: &TunnelConnection,
        cemi: CemiFrame,
    ) -> KnxResult<()> {
        let seq = connection.next_send_sequence();
        let tunnel_req = TunnellingRequest::new(connection.channel_id, seq, cemi);
        let frame = KnxFrame::new(ServiceType::TunnellingRequest, tunnel_req.encode());

        // Activate backpressure before sending (we're now waiting for ACK)
        if self.filter_chain.is_enabled() {
            self.filter_chain
                .queue_filter()
                .set_waiting_for_ack(connection.channel_id, true);
        }

        if let Err(e) = socket.send_to(&frame.encode(), client_addr).await {
            debug!(error = %e, channel_id = connection.channel_id, "Failed to send tunnelling request");
            return Err(e.into());
        }

        Ok(())
    }

    /// Drain and send queued frames for a channel after ACK received.
    ///
    /// When the QueueFilter has been holding frames due to WaitingForAck
    /// backpressure, this method drains and sends them in priority order
    /// once the ACK is received.
    async fn drain_queued_frames(&self, socket: &UdpSocket, connection: &TunnelConnection) {
        if !self.filter_chain.is_enabled() {
            return;
        }

        let envelopes = self.filter_chain.drain_pending(connection.channel_id, 16);

        if envelopes.is_empty() {
            return;
        }

        let drained_count = envelopes.len();

        for envelope in envelopes {
            // Re-apply pace filter timing for each drained frame
            let env = envelope;
            let pace_result = self.filter_chain.pace_filter().process_send(&env);

            if let FilterResult::Pass { delay } = pace_result {
                if delay > Duration::ZERO {
                    tokio::time::sleep(delay).await;
                }
            }

            if let Err(e) = self
                .send_tunnelling_request_raw(socket, env.target_addr, connection, env.cemi)
                .await
            {
                debug!(
                    error = %e,
                    channel_id = connection.channel_id,
                    "Failed to send drained frame"
                );
                self.filter_chain
                    .on_send_failure(connection.channel_id, &e.to_string());
                self.handle_send_error_tracking(connection.channel_id, ErrorCategory::SendFailure);
                break;
            } else {
                self.filter_chain.on_send_success(connection.channel_id);
                self.error_tracker.on_send_success(connection.channel_id);
            }
        }

        if drained_count > 0 {
            let _ = self.event_tx.send(ServerEvent::QueueDrained {
                channel_id: connection.channel_id,
                drained_count,
            });
        }
    }

    /// Handle send error tracking result — emit events if thresholds are exceeded.
    fn handle_send_error_tracking(&self, channel_id: u8, category: ErrorCategory) {
        let result = self.error_tracker.on_send_failure(channel_id, category);

        match result {
            TrackingResult::Recorded => {}
            TrackingResult::ConsecutiveThresholdExceeded {
                consecutive_errors,
                threshold,
            } => {
                warn!(
                    channel_id,
                    consecutive_errors,
                    threshold,
                    "Send error threshold exceeded — tunnel restart recommended"
                );
                let _ = self.event_tx.send(ServerEvent::SendErrorThreshold {
                    channel_id,
                    consecutive_errors,
                    threshold,
                });
            }
            TrackingResult::RateThresholdExceeded {
                error_count,
                window_ms,
                rate,
            } => {
                warn!(
                    channel_id,
                    error_count,
                    window_ms,
                    rate_percent = rate,
                    "Send error rate threshold exceeded"
                );
                let _ = self.event_tx.send(ServerEvent::SendErrorRateWarning {
                    channel_id,
                    error_count,
                    window_ms,
                    rate_percent: rate,
                });
            }
        }
    }

    /// Send L_Data.con (MC=0x2E) confirmation frame.
    ///
    /// After the server processes an L_Data.req, it sends back an L_Data.con with:
    /// - Same source/destination as the original frame
    /// - MC=0x2E (or the appropriate confirmation code for the message type)
    /// - Ctrl1 bit 0: 0=success, 1=failure (NACK)
    async fn send_ldata_con(
        &self,
        socket: &UdpSocket,
        client_addr: SocketAddr,
        connection: &TunnelConnection,
        original_cemi: &CemiFrame,
        success: bool,
    ) -> KnxResult<()> {
        // Use the appropriate confirmation message code
        let con_message_code = original_cemi
            .message_code
            .to_confirmation()
            .unwrap_or(MessageCode::LDataCon);

        let con_cemi = CemiFrame {
            message_code: con_message_code,
            additional_info: Vec::new(),
            source: original_cemi.source,
            destination: original_cemi.destination,
            address_type: original_cemi.address_type,
            hop_count: original_cemi.hop_count,
            priority: original_cemi.priority,
            confirm: !success, // Ctrl1 bit 0: 0=success, 1=failure
            ack_request: false,
            system_broadcast: original_cemi.system_broadcast,
            apci: original_cemi.apci,
            data: original_cemi.data.clone(),
        };

        debug!(
            channel_id = connection.channel_id,
            success,
            message_code = ?con_message_code,
            destination = original_cemi.destination,
            "Sending confirmation frame"
        );

        self.send_tunnelling_request(socket, client_addr, connection, con_cemi)
            .await
    }
}

/// Simple deterministic pseudo-random for confirmation success rate.
/// No external dependency needed — uses wrapping arithmetic on thread-local state.
fn rand_simple() -> f64 {
    use std::cell::Cell;
    thread_local! {
        static STATE: Cell<u64> = Cell::new(0x12345678_9ABCDEF0);
    }
    STATE.with(|s| {
        let mut x = s.get();
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        s.set(x);
        (x as f64) / (u64::MAX as f64)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_connection_manager() {
        let manager =
            ConnectionManager::new(10, Duration::from_secs(60), IndividualAddress::new(1, 1, 0));

        let channel_id = manager.allocate_channel().unwrap();
        assert!(channel_id > 0);

        let conn = manager.create_connection(
            channel_id,
            "192.168.1.100:3671".parse().unwrap(),
            "192.168.1.100:3672".parse().unwrap(),
        );

        assert_eq!(conn.channel_id, channel_id);
        assert_eq!(manager.len(), 1);

        // Connection should be in Idle state (Connecting → Idle in create_connection)
        assert!(conn.fsm.is_connected());
        assert!(conn.fsm.can_send());

        manager.remove(channel_id);
        assert!(manager.is_empty());
    }

    #[test]
    fn test_connection_with_sequence_tracker() {
        let manager =
            ConnectionManager::new(10, Duration::from_secs(60), IndividualAddress::new(1, 1, 0));

        let channel_id = manager.allocate_channel().unwrap();
        let conn = manager.create_connection(
            channel_id,
            "192.168.1.100:3671".parse().unwrap(),
            "192.168.1.100:3672".parse().unwrap(),
        );

        // Test sequence tracking
        let validation = conn.validate_recv_sequence(0);
        assert!(matches!(
            validation,
            ReceivedValidation::Valid { sequence: 0 }
        ));

        let validation = conn.validate_recv_sequence(0);
        assert!(matches!(validation, ReceivedValidation::Duplicate { .. }));

        let validation = conn.validate_recv_sequence(1);
        assert!(matches!(
            validation,
            ReceivedValidation::Valid { sequence: 1 }
        ));

        // Test send sequence
        assert_eq!(conn.next_send_sequence(), 0);
        assert_eq!(conn.next_send_sequence(), 1);
    }

    #[test]
    fn test_server_config() {
        let config = KnxServerConfig::default();
        assert!(config.validate().is_ok());
        assert!(config.tunneling_enabled);
        assert!(config.tunnel_behavior.ldata_con_enabled);
        assert_eq!(config.tunnel_behavior.confirmation_success_rate, 1.0);
    }

    #[test]
    fn test_connection_fsm_lifecycle() {
        let manager =
            ConnectionManager::new(10, Duration::from_secs(60), IndividualAddress::new(1, 1, 0));

        let channel_id = manager.allocate_channel().unwrap();
        let conn = manager.create_connection(
            channel_id,
            "192.168.1.100:3671".parse().unwrap(),
            "192.168.1.100:3672".parse().unwrap(),
        );

        // Should be Idle after creation
        use crate::tunnel::TunnelState;
        assert!(matches!(conn.fsm.state(), TunnelState::Idle));

        // Send frame → WaitingForAck
        conn.fsm.on_frame_sent(0);
        assert!(matches!(
            conn.fsm.state(),
            TunnelState::WaitingForAck { sequence: 0, .. }
        ));

        // ACK received → WaitingForConfirmation (for L_Data.req)
        conn.fsm.on_ack_received(0);
        assert!(matches!(
            conn.fsm.state(),
            TunnelState::WaitingForConfirmation { .. }
        ));

        // Confirmation received → Idle
        conn.fsm.on_confirmation_received();
        assert!(matches!(conn.fsm.state(), TunnelState::Idle));
    }

    #[test]
    fn test_connection_reset() {
        let manager =
            ConnectionManager::new(10, Duration::from_secs(60), IndividualAddress::new(1, 1, 0));

        let channel_id = manager.allocate_channel().unwrap();
        let conn = manager.create_connection(
            channel_id,
            "192.168.1.100:3671".parse().unwrap(),
            "192.168.1.100:3672".parse().unwrap(),
        );

        conn.validate_recv_sequence(0);
        conn.validate_recv_sequence(1);
        conn.next_send_sequence();

        conn.reset();

        assert_eq!(conn.sequence_tracker.current_rno(), 0);
        assert_eq!(conn.sequence_tracker.current_sno(), 0);
    }

    #[test]
    fn test_desync_threshold_custom() {
        let manager =
            ConnectionManager::new(10, Duration::from_secs(60), IndividualAddress::new(1, 1, 0))
                .with_desync_threshold(10);

        let channel_id = manager.allocate_channel().unwrap();
        let conn = manager.create_connection(
            channel_id,
            "192.168.1.100:3671".parse().unwrap(),
            "192.168.1.100:3672".parse().unwrap(),
        );

        // Distance 5 should be OutOfOrder (not FatalDesync) with threshold 10
        let validation = conn.validate_recv_sequence(5);
        assert!(matches!(validation, ReceivedValidation::OutOfOrder { .. }));
    }

    #[test]
    fn test_rand_simple_range() {
        for _ in 0..100 {
            let v = rand_simple();
            assert!(v >= 0.0 && v <= 1.0);
        }
    }

    #[test]
    fn test_server_config_new_fields() {
        let config = KnxServerConfig::default();
        assert!(!config.tunnel_behavior.bus_monitor_enabled);
        assert!(config.tunnel_behavior.ldata_ind_broadcast_enabled);
        assert!(config.tunnel_behavior.property_service_enabled);
        assert!(config.tunnel_behavior.reset_service_enabled);
    }

    #[test]
    fn test_read_property_device_object() {
        let config =
            KnxServerConfig::default().with_individual_address(IndividualAddress::new(1, 2, 3));
        let server = KnxServer::new(config);

        // PID_SERIAL_NUMBER
        let serial = server.read_property(0, 11);
        assert_eq!(serial.len(), 6);

        // PID_SUBNET_ADDRESS
        let subnet = server.read_property(0, 57);
        assert_eq!(subnet, vec![1]); // area = 1

        // PID_DEVICE_ADDRESS
        let device = server.read_property(0, 58);
        assert_eq!(device, vec![3]); // device = 3

        // PID_MAX_APDULENGTH
        let max_apdu = server.read_property(0, 56);
        assert_eq!(max_apdu, vec![0x00, 0xFE]); // 254

        // Unknown property
        let unknown = server.read_property(99, 99);
        assert_eq!(unknown, vec![0x00, 0x00]);
    }

    #[test]
    fn test_read_property_knxnet_ip_object() {
        let config = KnxServerConfig::default()
            .with_individual_address(IndividualAddress::new(1, 2, 3))
            .with_device_name("Test Device");
        let server = KnxServer::new(config);

        // PID_KNX_INDIVIDUAL_ADDRESS
        let addr = server.read_property(11, 52);
        assert_eq!(addr, IndividualAddress::new(1, 2, 3).to_bytes().to_vec());

        // PID_FRIENDLY_NAME
        let name = server.read_property(11, 7);
        assert_eq!(name, b"Test Device".to_vec());

        // PID_MAC_ADDRESS
        let mac = server.read_property(11, 8);
        assert_eq!(mac.len(), 6);
    }

    #[test]
    fn test_compute_confirmation_success() {
        let mut config = KnxServerConfig::default();
        config.tunnel_behavior.confirmation_success_rate = 1.0;
        let server = KnxServer::new(config.clone());

        // Rate 1.0 — always follows bus_success
        assert!(server.compute_confirmation_success(true));
        assert!(!server.compute_confirmation_success(false));

        // Rate 0.0 — always fails
        config.tunnel_behavior.confirmation_success_rate = 0.0;
        let server = KnxServer::new(config);
        assert!(!server.compute_confirmation_success(true));
        assert!(!server.compute_confirmation_success(false));
    }

    #[test]
    fn test_server_event_variants() {
        // Ensure all new ServerEvent variants are constructible
        let events: Vec<ServerEvent> = vec![
            ServerEvent::BusMonitorFrameSent {
                channel_id: 1,
                message_code: 0x11,
                raw_frame_len: 10,
            },
            ServerEvent::PropertyRead {
                channel_id: 1,
                object_index: 0,
                property_id: 11,
            },
            ServerEvent::PropertyWrite {
                channel_id: 1,
                object_index: 0,
                property_id: 14,
            },
            ServerEvent::DeviceReset { channel_id: 1 },
            ServerEvent::DataIndBroadcast {
                source_channel_id: 1,
                target_channel_count: 3,
                group_address: GroupAddress::three_level(1, 0, 1),
            },
        ];

        for event in &events {
            // Just ensure Debug formatting works
            let _ = format!("{:?}", event);
        }
        assert_eq!(events.len(), 5);
    }
}

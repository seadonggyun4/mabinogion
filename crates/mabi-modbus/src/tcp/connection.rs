//! Connection pool management for Modbus TCP server.
//!
//! This module keeps the stable connection-management surface while moving
//! request-path accounting onto a sharded internal core.

use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use chrono::{DateTime, Duration as ChronoDuration, Utc};
use dashmap::DashMap;
use tokio::sync::broadcast;
use tracing::{debug, warn};

use crate::connection_core::{ShardRouter, UnitIdSet};

/// State of a connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionState {
    /// Connection is active and processing requests.
    Active,
    /// Connection is idle (no recent activity).
    Idle,
    /// Connection is being closed.
    Closing,
}

/// Information about an active connection.
#[derive(Debug, Clone)]
pub struct ConnectionInfo {
    /// Peer address.
    pub peer_addr: SocketAddr,

    /// When the connection was established.
    pub connected_at: DateTime<Utc>,

    /// Internal timestamp for performance tracking.
    connected_instant: Instant,

    /// Current connection state.
    pub state: ConnectionState,

    /// Total number of requests processed.
    pub requests_processed: u64,

    /// Number of successful requests.
    pub requests_success: u64,

    /// Number of failed requests.
    pub requests_failed: u64,

    /// Last activity timestamp.
    pub last_activity: DateTime<Utc>,

    /// Unit IDs that have been accessed.
    pub accessed_unit_ids: Vec<u8>,

    /// Total bytes received.
    pub bytes_received: u64,

    /// Total bytes sent.
    pub bytes_sent: u64,
}

/// Optional request tracking controls used by high-throughput paths.
#[derive(Debug, Clone, Copy)]
pub struct RequestRecordOptions {
    pub update_last_activity: bool,
    pub track_unit_access: bool,
    pub emit_event: bool,
}

impl Default for RequestRecordOptions {
    fn default() -> Self {
        Self {
            update_last_activity: true,
            track_unit_access: true,
            emit_event: true,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct LifecycleEventOptions {
    pub emit_connected: bool,
    pub emit_disconnected: bool,
    pub emit_rejected: bool,
}

impl LifecycleEventOptions {
    pub(crate) const fn enabled() -> Self {
        Self {
            emit_connected: true,
            emit_disconnected: true,
            emit_rejected: true,
        }
    }

    pub(crate) const fn disabled() -> Self {
        Self {
            emit_connected: false,
            emit_disconnected: false,
            emit_rejected: false,
        }
    }
}

impl ConnectionInfo {
    /// Create a new connection info snapshot.
    pub fn new(peer_addr: SocketAddr) -> Self {
        let now = Utc::now();
        Self {
            peer_addr,
            connected_at: now,
            connected_instant: Instant::now(),
            state: ConnectionState::Active,
            requests_processed: 0,
            requests_success: 0,
            requests_failed: 0,
            last_activity: now,
            accessed_unit_ids: Vec::new(),
            bytes_received: 0,
            bytes_sent: 0,
        }
    }

    /// Get the connection duration.
    pub fn duration(&self) -> Duration {
        self.connected_instant.elapsed()
    }

    /// Record a successful request.
    pub fn record_success(&mut self, unit_id: u8, bytes_in: u64, bytes_out: u64) {
        self.record_success_with_options(
            unit_id,
            bytes_in,
            bytes_out,
            RequestRecordOptions::default(),
        );
    }

    /// Record a successful request with optional metadata tracking.
    pub fn record_success_with_options(
        &mut self,
        unit_id: u8,
        bytes_in: u64,
        bytes_out: u64,
        options: RequestRecordOptions,
    ) {
        self.requests_processed += 1;
        self.requests_success += 1;
        self.bytes_received += bytes_in;
        self.bytes_sent += bytes_out;

        if options.update_last_activity {
            self.last_activity = Utc::now();
        }

        if options.track_unit_access && !self.accessed_unit_ids.contains(&unit_id) {
            self.accessed_unit_ids.push(unit_id);
        }
    }

    /// Record a failed request.
    pub fn record_failure(&mut self, bytes_in: u64) {
        self.record_failure_with_options(bytes_in, RequestRecordOptions::default());
    }

    /// Record a failed request with optional metadata tracking.
    pub fn record_failure_with_options(&mut self, bytes_in: u64, options: RequestRecordOptions) {
        self.requests_processed += 1;
        self.requests_failed += 1;
        self.bytes_received += bytes_in;

        if options.update_last_activity {
            self.last_activity = Utc::now();
        }
    }

    /// Mark as closing.
    pub fn mark_closing(&mut self) {
        self.state = ConnectionState::Closing;
    }
}

#[derive(Debug, Clone)]
struct ConnectionRecord {
    peer_addr: SocketAddr,
    connected_at: DateTime<Utc>,
    connected_instant: Instant,
    state: ConnectionState,
    requests_processed: u64,
    requests_success: u64,
    requests_failed: u64,
    last_activity_tick_us: u64,
    accessed_units: UnitIdSet,
    bytes_received: u64,
    bytes_sent: u64,
}

impl ConnectionRecord {
    fn new(peer_addr: SocketAddr) -> Self {
        let connected_at = Utc::now();
        Self {
            peer_addr,
            connected_at,
            connected_instant: Instant::now(),
            state: ConnectionState::Active,
            requests_processed: 0,
            requests_success: 0,
            requests_failed: 0,
            last_activity_tick_us: 0,
            accessed_units: UnitIdSet::default(),
            bytes_received: 0,
            bytes_sent: 0,
        }
    }

    #[inline]
    fn elapsed_tick_us(&self) -> u64 {
        self.connected_instant
            .elapsed()
            .as_micros()
            .min(u64::MAX as u128) as u64
    }

    #[inline]
    fn touch(&mut self) {
        self.last_activity_tick_us = self.elapsed_tick_us();
    }

    #[inline]
    fn last_activity(&self) -> DateTime<Utc> {
        let micros = self.last_activity_tick_us.min(i64::MAX as u64) as i64;
        self.connected_at + ChronoDuration::microseconds(micros)
    }

    #[inline]
    fn idle_duration(&self) -> Duration {
        self.connected_instant
            .elapsed()
            .saturating_sub(Duration::from_micros(self.last_activity_tick_us))
    }

    fn record_success(
        &mut self,
        unit_id: u8,
        bytes_in: u64,
        bytes_out: u64,
        options: RequestRecordOptions,
    ) {
        self.requests_processed += 1;
        self.requests_success += 1;
        self.bytes_received += bytes_in;
        self.bytes_sent += bytes_out;
        self.state = ConnectionState::Active;

        if options.update_last_activity {
            self.touch();
        }

        if options.track_unit_access {
            let _ = self.accessed_units.record(unit_id);
        }
    }

    fn record_failure(&mut self, bytes_in: u64, options: RequestRecordOptions) {
        self.requests_processed += 1;
        self.requests_failed += 1;
        self.bytes_received += bytes_in;
        self.state = ConnectionState::Active;

        if options.update_last_activity {
            self.touch();
        }
    }

    fn snapshot(&self) -> ConnectionInfo {
        ConnectionInfo {
            peer_addr: self.peer_addr,
            connected_at: self.connected_at,
            connected_instant: self.connected_instant,
            state: self.state,
            requests_processed: self.requests_processed,
            requests_success: self.requests_success,
            requests_failed: self.requests_failed,
            last_activity: self.last_activity(),
            accessed_unit_ids: self.accessed_units.snapshot(),
            bytes_received: self.bytes_received,
            bytes_sent: self.bytes_sent,
        }
    }
}

struct ConnectionShard {
    connections: DashMap<u64, ConnectionRecord>,
    addr_to_id: DashMap<SocketAddr, u64>,
    active_count: AtomicUsize,
}

impl ConnectionShard {
    fn new(capacity: usize) -> Self {
        Self {
            connections: DashMap::with_capacity(capacity),
            addr_to_id: DashMap::with_capacity(capacity),
            active_count: AtomicUsize::new(0),
        }
    }

    fn insert(&self, connection_id: u64, peer_addr: SocketAddr) {
        self.connections
            .insert(connection_id, ConnectionRecord::new(peer_addr));
        self.addr_to_id.insert(peer_addr, connection_id);
        self.active_count.fetch_add(1, Ordering::Relaxed);
    }

    fn remove(&self, connection_id: u64) -> Option<ConnectionRecord> {
        if let Some((_, record)) = self.connections.remove(&connection_id) {
            self.addr_to_id.remove(&record.peer_addr);
            self.active_count.fetch_sub(1, Ordering::Relaxed);
            Some(record)
        } else {
            None
        }
    }

    fn get(&self, connection_id: u64) -> Option<ConnectionInfo> {
        self.connections
            .get(&connection_id)
            .map(|entry| entry.value().snapshot())
    }

    fn get_by_addr(&self, addr: &SocketAddr) -> Option<ConnectionInfo> {
        self.addr_to_id.get(addr).and_then(|id| self.get(*id))
    }

    fn update<R>(
        &self,
        connection_id: u64,
        f: impl FnOnce(&mut ConnectionRecord) -> R,
    ) -> Option<R> {
        self.connections
            .get_mut(&connection_id)
            .map(|mut entry| f(entry.value_mut()))
    }

    fn idle_connections(&self, threshold: Duration) -> Vec<u64> {
        self.connections
            .iter()
            .filter(|entry| entry.value().idle_duration() > threshold)
            .map(|entry| *entry.key())
            .collect()
    }

    fn snapshots(&self) -> Vec<(u64, ConnectionInfo)> {
        self.connections
            .iter()
            .map(|entry| (*entry.key(), entry.value().snapshot()))
            .collect()
    }
}

fn shard_count_for_capacity(max_connections: usize) -> usize {
    let desired = (max_connections / 256).max(1).next_power_of_two();
    desired.clamp(4, 64)
}

/// Event types for connection lifecycle.
#[derive(Debug, Clone)]
pub enum ConnectionEvent {
    /// A new connection was established.
    Connected {
        peer_addr: SocketAddr,
        connection_id: u64,
    },

    /// A connection was closed.
    Disconnected {
        peer_addr: SocketAddr,
        connection_id: u64,
        duration_secs: f64,
        requests_processed: u64,
    },

    /// A request was processed.
    RequestProcessed {
        peer_addr: SocketAddr,
        connection_id: u64,
        unit_id: u8,
        function_code: u8,
        success: bool,
        duration_us: u64,
    },

    /// Connection was rejected (limit reached).
    Rejected {
        peer_addr: SocketAddr,
        reason: String,
    },
}

/// Pool for managing active connections.
pub struct ConnectionPool {
    shards: Vec<ConnectionShard>,
    router: ShardRouter,
    next_id: AtomicU64,
    max_connections: usize,
    event_tx: broadcast::Sender<ConnectionEvent>,
    total_connections: AtomicU64,
    total_rejected: AtomicU64,
    active_connections: AtomicUsize,
    created_at: Instant,
}

impl ConnectionPool {
    /// Create a new connection pool.
    pub fn new(max_connections: usize) -> Self {
        let shard_count = shard_count_for_capacity(max_connections);
        let router = ShardRouter::new(shard_count);
        let capacity_per_shard = (max_connections / shard_count).max(16);
        let (event_tx, _) = broadcast::channel(1024);

        Self {
            shards: (0..shard_count)
                .map(|_| ConnectionShard::new(capacity_per_shard))
                .collect(),
            router,
            next_id: AtomicU64::new(1),
            max_connections,
            event_tx,
            total_connections: AtomicU64::new(0),
            total_rejected: AtomicU64::new(0),
            active_connections: AtomicUsize::new(0),
            created_at: Instant::now(),
        }
    }

    /// Try to register a new connection.
    ///
    /// Returns `Some(connection_id)` if successful, `None` if limit reached.
    pub fn try_register(&self, peer_addr: SocketAddr) -> Option<u64> {
        self.try_register_with_options(peer_addr, LifecycleEventOptions::enabled())
    }

    pub(crate) fn try_register_with_options(
        &self,
        peer_addr: SocketAddr,
        options: LifecycleEventOptions,
    ) -> Option<u64> {
        if self.active_count() >= self.max_connections {
            self.total_rejected.fetch_add(1, Ordering::Relaxed);

            self.emit_lifecycle_event(options.emit_rejected, || ConnectionEvent::Rejected {
                peer_addr,
                reason: format!("Max connections ({}) reached", self.max_connections),
            });

            warn!(
                peer = %peer_addr,
                max = self.max_connections,
                "Connection rejected: limit reached"
            );
            return None;
        }

        let connection_id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let shard_index = self.router.index_for_id(connection_id);

        self.shards[shard_index].insert(connection_id, peer_addr);
        self.active_connections.fetch_add(1, Ordering::Relaxed);
        self.total_connections.fetch_add(1, Ordering::Relaxed);

        self.emit_lifecycle_event(options.emit_connected, || ConnectionEvent::Connected {
            peer_addr,
            connection_id,
        });

        debug!(
            peer = %peer_addr,
            connection_id,
            active = self.active_count(),
            "Connection registered"
        );

        Some(connection_id)
    }

    /// Unregister a connection.
    pub fn unregister(&self, connection_id: u64) -> Option<ConnectionInfo> {
        self.unregister_with_options(connection_id, LifecycleEventOptions::enabled())
    }

    pub(crate) fn unregister_with_options(
        &self,
        connection_id: u64,
        options: LifecycleEventOptions,
    ) -> Option<ConnectionInfo> {
        let shard_index = self.router.index_for_id(connection_id);
        if let Some(record) = self.shards[shard_index].remove(connection_id) {
            self.active_connections.fetch_sub(1, Ordering::Relaxed);
            let info = record.snapshot();

            self.emit_lifecycle_event(options.emit_disconnected, || {
                ConnectionEvent::Disconnected {
                    peer_addr: info.peer_addr,
                    connection_id,
                    duration_secs: info.duration().as_secs_f64(),
                    requests_processed: info.requests_processed,
                }
            });

            debug!(
                peer = %info.peer_addr,
                connection_id,
                duration_secs = info.duration().as_secs_f64(),
                requests = info.requests_processed,
                "Connection unregistered"
            );

            Some(info)
        } else {
            None
        }
    }

    /// Record a successful request for a connection.
    pub fn record_request(
        &self,
        connection_id: u64,
        unit_id: u8,
        function_code: u8,
        success: bool,
        duration_us: u64,
        bytes_in: u64,
        bytes_out: u64,
    ) {
        self.record_request_with_options(
            connection_id,
            unit_id,
            function_code,
            success,
            duration_us,
            bytes_in,
            bytes_out,
            RequestRecordOptions::default(),
        );
    }

    /// Record a request with optional metadata and event tracking.
    pub fn record_request_with_options(
        &self,
        connection_id: u64,
        unit_id: u8,
        function_code: u8,
        success: bool,
        duration_us: u64,
        bytes_in: u64,
        bytes_out: u64,
        options: RequestRecordOptions,
    ) {
        let shard_index = self.router.index_for_id(connection_id);
        let maybe_event = self.shards[shard_index].update(connection_id, |record| {
            if success {
                record.record_success(unit_id, bytes_in, bytes_out, options);
            } else {
                record.record_failure(bytes_in, options);
            }

            options.emit_event.then_some(record.peer_addr)
        });

        if let Some(Some(peer_addr)) = maybe_event {
            if self.event_tx.receiver_count() > 0 {
                let _ = self.event_tx.send(ConnectionEvent::RequestProcessed {
                    peer_addr,
                    connection_id,
                    unit_id,
                    function_code,
                    success,
                    duration_us,
                });
            }
        }
    }

    /// Get connection info by ID.
    pub fn get(&self, connection_id: u64) -> Option<ConnectionInfo> {
        let shard_index = self.router.index_for_id(connection_id);
        self.shards[shard_index].get(connection_id)
    }

    /// Get connection info by peer address.
    pub fn get_by_addr(&self, addr: &SocketAddr) -> Option<ConnectionInfo> {
        let likely_shard = self.router.index_for_addr(addr);
        if let Some(info) = self.shards[likely_shard].get_by_addr(addr) {
            return Some(info);
        }

        for (index, shard) in self.shards.iter().enumerate() {
            if index == likely_shard {
                continue;
            }
            if let Some(info) = shard.get_by_addr(addr) {
                return Some(info);
            }
        }

        None
    }

    /// Get the number of active connections.
    pub fn active_count(&self) -> usize {
        self.active_connections.load(Ordering::Relaxed)
    }

    /// Get all connection info.
    pub fn list_all(&self) -> Vec<(u64, ConnectionInfo)> {
        self.shards
            .iter()
            .flat_map(ConnectionShard::snapshots)
            .collect()
    }

    /// Get connections that have been idle for longer than the specified duration.
    pub fn get_idle_connections(&self, idle_threshold: Duration) -> Vec<u64> {
        self.shards
            .iter()
            .flat_map(|shard| shard.idle_connections(idle_threshold))
            .collect()
    }

    /// Subscribe to connection events.
    pub fn subscribe(&self) -> broadcast::Receiver<ConnectionEvent> {
        self.event_tx.subscribe()
    }

    pub(crate) fn subscriber_count(&self) -> usize {
        self.event_tx.receiver_count()
    }

    /// Get the maximum connection limit.
    pub fn max_connections(&self) -> usize {
        self.max_connections
    }

    /// Get total connections ever established.
    pub fn total_connections(&self) -> u64 {
        self.total_connections.load(Ordering::Relaxed)
    }

    /// Get total connections rejected.
    pub fn total_rejected(&self) -> u64 {
        self.total_rejected.load(Ordering::Relaxed)
    }

    /// Get pool uptime.
    pub fn uptime(&self) -> Duration {
        self.created_at.elapsed()
    }

    /// Get statistics summary.
    pub fn stats(&self) -> ConnectionPoolStats {
        ConnectionPoolStats {
            active_connections: self.active_count(),
            max_connections: self.max_connections,
            total_connections: self.total_connections(),
            total_rejected: self.total_rejected(),
            uptime_secs: self.uptime().as_secs_f64(),
        }
    }

    fn emit_lifecycle_event<F>(&self, should_emit: bool, build_event: F)
    where
        F: FnOnce() -> ConnectionEvent,
    {
        if should_emit && self.event_tx.receiver_count() > 0 {
            let _ = self.event_tx.send(build_event());
        }
    }
}

impl Default for ConnectionPool {
    fn default() -> Self {
        Self::new(1000)
    }
}

/// Statistics about the connection pool.
#[derive(Debug, Clone)]
pub struct ConnectionPoolStats {
    /// Current active connections.
    pub active_connections: usize,

    /// Maximum allowed connections.
    pub max_connections: usize,

    /// Total connections ever established.
    pub total_connections: u64,

    /// Total connections rejected.
    pub total_rejected: u64,

    /// Pool uptime in seconds.
    pub uptime_secs: f64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{IpAddr, Ipv4Addr};
    use std::thread;
    use tokio::time::timeout;

    fn make_addr(port: u16) -> SocketAddr {
        SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), port)
    }

    #[test]
    fn test_connection_pool_register() {
        let pool = ConnectionPool::new(10);

        let addr1 = make_addr(1001);
        let addr2 = make_addr(1002);

        let id1 = pool.try_register(addr1);
        let id2 = pool.try_register(addr2);

        assert!(id1.is_some());
        assert!(id2.is_some());
        assert_ne!(id1, id2);
        assert_eq!(pool.active_count(), 2);
    }

    #[test]
    fn test_connection_pool_limit() {
        let pool = ConnectionPool::new(2);

        let id1 = pool.try_register(make_addr(1001));
        let id2 = pool.try_register(make_addr(1002));
        let id3 = pool.try_register(make_addr(1003));

        assert!(id1.is_some());
        assert!(id2.is_some());
        assert!(id3.is_none());
        assert_eq!(pool.total_rejected(), 1);
    }

    #[test]
    fn test_connection_pool_unregister() {
        let pool = ConnectionPool::new(10);

        let addr = make_addr(1001);
        let id = pool.try_register(addr).unwrap();

        assert_eq!(pool.active_count(), 1);

        let info = pool.unregister(id);
        assert!(info.is_some());
        assert_eq!(pool.active_count(), 0);

        let info = pool.unregister(id);
        assert!(info.is_none());
    }

    #[test]
    fn test_connection_info_tracking() {
        let pool = ConnectionPool::new(10);

        let addr = make_addr(1001);
        let id = pool.try_register(addr).unwrap();

        pool.record_request(id, 1, 0x03, true, 100, 10, 20);
        pool.record_request(id, 1, 0x06, true, 150, 15, 25);
        pool.record_request(id, 2, 0x03, false, 50, 10, 0);

        let info = pool.get(id).unwrap();
        assert_eq!(info.requests_processed, 3);
        assert_eq!(info.requests_success, 2);
        assert_eq!(info.requests_failed, 1);
        assert_eq!(info.bytes_received, 35);
        assert_eq!(info.bytes_sent, 45);
        assert_eq!(info.accessed_unit_ids, vec![1]);
    }

    #[test]
    fn test_connection_info_tracking_options_skip_detailed_metadata() {
        let pool = ConnectionPool::new(10);

        let addr = make_addr(1002);
        let id = pool.try_register(addr).unwrap();
        let before = pool.get(id).unwrap().last_activity;

        pool.record_request_with_options(
            id,
            7,
            0x03,
            true,
            50,
            8,
            12,
            RequestRecordOptions {
                update_last_activity: false,
                track_unit_access: false,
                emit_event: false,
            },
        );

        let info = pool.get(id).unwrap();
        assert_eq!(info.requests_processed, 1);
        assert_eq!(info.requests_success, 1);
        assert_eq!(info.bytes_received, 8);
        assert_eq!(info.bytes_sent, 12);
        assert_eq!(info.last_activity, before);
        assert!(info.accessed_unit_ids.is_empty());
    }

    #[test]
    fn test_last_activity_snapshot_advances_without_wall_clock_updates_on_hot_path() {
        let pool = ConnectionPool::new(10);
        let id = pool.try_register(make_addr(1004)).unwrap();
        let before = pool.get(id).unwrap().last_activity;

        thread::sleep(Duration::from_millis(2));
        pool.record_request_with_options(
            id,
            1,
            0x03,
            true,
            42,
            4,
            6,
            RequestRecordOptions {
                update_last_activity: true,
                track_unit_access: false,
                emit_event: false,
            },
        );

        let after = pool.get(id).unwrap().last_activity;
        assert!(after > before);
    }

    #[test]
    fn test_accessed_unit_ids_preserve_first_seen_order_without_linear_scan() {
        let pool = ConnectionPool::new(10);
        let id = pool.try_register(make_addr(1005)).unwrap();

        for unit in [7, 3, 7, 2, 3, 255] {
            pool.record_request_with_options(
                id,
                unit,
                0x03,
                true,
                10,
                2,
                4,
                RequestRecordOptions {
                    update_last_activity: false,
                    track_unit_access: true,
                    emit_event: false,
                },
            );
        }

        let info = pool.get(id).unwrap();
        assert_eq!(info.accessed_unit_ids, vec![7, 3, 2, 255]);
    }

    #[test]
    fn test_get_by_addr() {
        let pool = ConnectionPool::new(10);

        let addr = make_addr(1001);
        let _id = pool.try_register(addr).unwrap();

        let info = pool.get_by_addr(&addr);
        assert!(info.is_some());
        assert_eq!(info.unwrap().peer_addr, addr);
    }

    #[test]
    fn test_idle_detection_uses_monotonic_activity_tracking() {
        let pool = ConnectionPool::new(10);
        let id = pool.try_register(make_addr(1006)).unwrap();

        thread::sleep(Duration::from_millis(5));
        let idle = pool.get_idle_connections(Duration::from_millis(1));
        assert!(idle.contains(&id));
    }

    #[tokio::test]
    async fn test_connection_events() {
        let pool = ConnectionPool::new(10);
        let mut rx = pool.subscribe();

        let addr = make_addr(1001);
        let id = pool.try_register(addr).unwrap();

        let event = rx.recv().await.unwrap();
        match event {
            ConnectionEvent::Connected {
                peer_addr,
                connection_id,
            } => {
                assert_eq!(peer_addr, addr);
                assert_eq!(connection_id, id);
            }
            _ => panic!("Expected Connected event"),
        }

        pool.unregister(id);

        let event = rx.recv().await.unwrap();
        match event {
            ConnectionEvent::Disconnected {
                peer_addr,
                connection_id,
                ..
            } => {
                assert_eq!(peer_addr, addr);
                assert_eq!(connection_id, id);
            }
            _ => panic!("Expected Disconnected event"),
        }
    }

    #[tokio::test]
    async fn test_lifecycle_events_can_be_suppressed_without_affecting_pool_state() {
        let pool = ConnectionPool::new(10);
        let mut rx = pool.subscribe();
        let addr = make_addr(1007);

        let id = pool
            .try_register_with_options(addr, LifecycleEventOptions::disabled())
            .unwrap();
        assert_eq!(pool.active_count(), 1);
        assert!(timeout(Duration::from_millis(25), rx.recv()).await.is_err());

        let info = pool.unregister_with_options(id, LifecycleEventOptions::disabled());
        assert!(info.is_some());
        assert_eq!(pool.active_count(), 0);
        assert!(timeout(Duration::from_millis(25), rx.recv()).await.is_err());
    }
}

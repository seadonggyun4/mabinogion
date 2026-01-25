//! Connection pool management for Modbus TCP server.
//!
//! This module provides tracking and management of active connections,
//! including connection limits, statistics, and lifecycle events.

use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use chrono::{DateTime, Utc};
use dashmap::DashMap;
use tokio::sync::broadcast;
use tracing::{debug, warn};

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

impl ConnectionInfo {
    /// Create a new connection info.
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
    pub fn duration(&self) -> std::time::Duration {
        self.connected_instant.elapsed()
    }

    /// Record a successful request.
    pub fn record_success(&mut self, unit_id: u8, bytes_in: u64, bytes_out: u64) {
        self.requests_processed += 1;
        self.requests_success += 1;
        self.last_activity = Utc::now();
        self.bytes_received += bytes_in;
        self.bytes_sent += bytes_out;

        if !self.accessed_unit_ids.contains(&unit_id) {
            self.accessed_unit_ids.push(unit_id);
        }
    }

    /// Record a failed request.
    pub fn record_failure(&mut self, bytes_in: u64) {
        self.requests_processed += 1;
        self.requests_failed += 1;
        self.last_activity = Utc::now();
        self.bytes_received += bytes_in;
    }

    /// Mark as closing.
    pub fn mark_closing(&mut self) {
        self.state = ConnectionState::Closing;
    }
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
    Rejected { peer_addr: SocketAddr, reason: String },
}

/// Pool for managing active connections.
///
/// This struct provides:
/// - Connection tracking with detailed statistics
/// - Connection limits
/// - Lifecycle event broadcasting
/// - Thread-safe concurrent access
pub struct ConnectionPool {
    /// Active connections indexed by connection ID.
    connections: DashMap<u64, ConnectionInfo>,

    /// Peer address to connection ID mapping (for lookup).
    addr_to_id: DashMap<SocketAddr, u64>,

    /// Next connection ID.
    next_id: AtomicU64,

    /// Maximum allowed connections.
    max_connections: usize,

    /// Event broadcaster.
    event_tx: broadcast::Sender<ConnectionEvent>,

    /// Total connections ever established.
    total_connections: AtomicU64,

    /// Total connections rejected.
    total_rejected: AtomicU64,

    /// Pool creation time.
    created_at: Instant,
}

impl ConnectionPool {
    /// Create a new connection pool.
    pub fn new(max_connections: usize) -> Self {
        let (event_tx, _) = broadcast::channel(1024);

        Self {
            connections: DashMap::new(),
            addr_to_id: DashMap::new(),
            next_id: AtomicU64::new(1),
            max_connections,
            event_tx,
            total_connections: AtomicU64::new(0),
            total_rejected: AtomicU64::new(0),
            created_at: Instant::now(),
        }
    }

    /// Try to register a new connection.
    ///
    /// Returns `Some(connection_id)` if successful, `None` if limit reached.
    pub fn try_register(&self, peer_addr: SocketAddr) -> Option<u64> {
        // Check if we're at capacity
        if self.connections.len() >= self.max_connections {
            self.total_rejected.fetch_add(1, Ordering::Relaxed);

            let _ = self.event_tx.send(ConnectionEvent::Rejected {
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

        // Generate connection ID
        let connection_id = self.next_id.fetch_add(1, Ordering::Relaxed);

        // Create connection info
        let info = ConnectionInfo::new(peer_addr);

        // Register
        self.connections.insert(connection_id, info);
        self.addr_to_id.insert(peer_addr, connection_id);
        self.total_connections.fetch_add(1, Ordering::Relaxed);

        // Broadcast event
        let _ = self.event_tx.send(ConnectionEvent::Connected {
            peer_addr,
            connection_id,
        });

        debug!(
            peer = %peer_addr,
            connection_id,
            active = self.connections.len(),
            "Connection registered"
        );

        Some(connection_id)
    }

    /// Unregister a connection.
    pub fn unregister(&self, connection_id: u64) -> Option<ConnectionInfo> {
        if let Some((_, info)) = self.connections.remove(&connection_id) {
            self.addr_to_id.remove(&info.peer_addr);

            // Broadcast event
            let _ = self.event_tx.send(ConnectionEvent::Disconnected {
                peer_addr: info.peer_addr,
                connection_id,
                duration_secs: info.duration().as_secs_f64(),
                requests_processed: info.requests_processed,
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
        if let Some(mut info) = self.connections.get_mut(&connection_id) {
            if success {
                info.record_success(unit_id, bytes_in, bytes_out);
            } else {
                info.record_failure(bytes_in);
            }

            // Broadcast event
            let _ = self.event_tx.send(ConnectionEvent::RequestProcessed {
                peer_addr: info.peer_addr,
                connection_id,
                unit_id,
                function_code,
                success,
                duration_us,
            });
        }
    }

    /// Get connection info by ID.
    pub fn get(&self, connection_id: u64) -> Option<ConnectionInfo> {
        self.connections.get(&connection_id).map(|r| r.clone())
    }

    /// Get connection info by peer address.
    pub fn get_by_addr(&self, addr: &SocketAddr) -> Option<ConnectionInfo> {
        self.addr_to_id
            .get(addr)
            .and_then(|id| self.get(*id))
    }

    /// Get the number of active connections.
    pub fn active_count(&self) -> usize {
        self.connections.len()
    }

    /// Get all connection info.
    pub fn list_all(&self) -> Vec<(u64, ConnectionInfo)> {
        self.connections
            .iter()
            .map(|r| (*r.key(), r.value().clone()))
            .collect()
    }

    /// Get connections that have been idle for longer than the specified duration.
    pub fn get_idle_connections(&self, idle_threshold: std::time::Duration) -> Vec<u64> {
        let threshold = Utc::now() - chrono::Duration::from_std(idle_threshold).unwrap();

        self.connections
            .iter()
            .filter(|r| r.value().last_activity < threshold)
            .map(|r| *r.key())
            .collect()
    }

    /// Subscribe to connection events.
    pub fn subscribe(&self) -> broadcast::Receiver<ConnectionEvent> {
        self.event_tx.subscribe()
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
    pub fn uptime(&self) -> std::time::Duration {
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
        let id3 = pool.try_register(make_addr(1003)); // Should fail

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

        // Double unregister should return None
        let info = pool.unregister(id);
        assert!(info.is_none());
    }

    #[test]
    fn test_connection_info_tracking() {
        let pool = ConnectionPool::new(10);

        let addr = make_addr(1001);
        let id = pool.try_register(addr).unwrap();

        // Record some requests
        pool.record_request(id, 1, 0x03, true, 100, 10, 20);
        pool.record_request(id, 1, 0x06, true, 150, 15, 25);
        pool.record_request(id, 2, 0x03, false, 50, 10, 0);

        let info = pool.get(id).unwrap();
        assert_eq!(info.requests_processed, 3);
        assert_eq!(info.requests_success, 2);
        assert_eq!(info.requests_failed, 1);
        assert_eq!(info.bytes_received, 35);
        assert_eq!(info.bytes_sent, 45);
        assert!(info.accessed_unit_ids.contains(&1));
        // Note: unit_id 2 is not tracked because record_failure doesn't track unit_ids
        // (only successful requests track accessed unit_ids)
        assert!(!info.accessed_unit_ids.contains(&2));
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

    #[tokio::test]
    async fn test_connection_events() {
        let pool = ConnectionPool::new(10);
        let mut rx = pool.subscribe();

        let addr = make_addr(1001);
        let id = pool.try_register(addr).unwrap();

        // Should receive Connected event
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

        // Should receive Disconnected event
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
}

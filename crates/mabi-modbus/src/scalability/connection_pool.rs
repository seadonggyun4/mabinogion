//! Sharded connection pool for high-volume connection handling.
//!
//! This module provides a scalable connection pool that distributes connections
//! across multiple shards to minimize lock contention and maximize throughput.
//!
//! ## Design Principles
//!
//! 1. **Lock-free reads**: Most operations use atomic counters and DashMap
//! 2. **Sharding**: Connections are distributed across shards by hash
//! 3. **O(1) operations**: Register/unregister/lookup are constant time
//! 4. **Memory efficient**: Only stores essential connection metadata
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────┐
//! │                    ShardedConnectionPool                         │
//! │  ┌──────────────────────────────────────────────────────────┐   │
//! │  │                   Global Statistics                       │   │
//! │  │  total_connections │ active │ rejected │ bytes_in/out    │   │
//! │  └──────────────────────────────────────────────────────────┘   │
//! │                              │                                   │
//! │  ┌───────────────────────────┴───────────────────────────────┐  │
//! │  │                      Shard Array                           │  │
//! │  │  ┌─────────┐ ┌─────────┐ ┌─────────┐       ┌─────────┐   │  │
//! │  │  │ Shard 0 │ │ Shard 1 │ │ Shard 2 │  ...  │Shard N-1│   │  │
//! │  │  │DashMap  │ │DashMap  │ │DashMap  │       │DashMap  │   │  │
//! │  │  └─────────┘ └─────────┘ └─────────┘       └─────────┘   │  │
//! │  └───────────────────────────────────────────────────────────┘  │
//! │                              │                                   │
//! │  hash(connection_id) % shard_count → shard index                │
//! └─────────────────────────────────────────────────────────────────┘
//! ```

use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use dashmap::DashMap;
use tokio::sync::broadcast;
use tracing::{debug, trace};

use crate::connection_core::ShardRouter;

use super::config::ConnectionPoolConfig;

/// Unique identifier for a connection.
pub type ConnectionId = u64;

/// State of a connection in the pool.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionState {
    /// Connection is active and can process requests.
    Active,
    /// Connection is idle (no recent activity).
    Idle,
    /// Connection is draining (no new requests, finishing existing).
    Draining,
    /// Connection is being closed.
    Closing,
}

/// Compact connection information for minimal memory footprint.
#[derive(Debug, Clone)]
pub struct ShardedConnectionInfo {
    /// Connection ID.
    pub id: ConnectionId,

    /// Peer address.
    pub peer_addr: SocketAddr,

    /// Shard index.
    pub shard_index: usize,

    /// Connection state.
    pub state: ConnectionState,

    /// When the connection was established.
    pub connected_at: DateTime<Utc>,

    /// Internal timestamp for efficient duration calculation.
    connected_instant: Instant,

    /// Last activity timestamp (internal).
    last_activity_instant: Instant,

    /// Request counters (success, failure).
    pub requests: RequestCounters,

    /// Byte counters.
    pub bytes: ByteCounters,
}

/// Request counters for a connection.
#[derive(Debug, Clone, Default)]
pub struct RequestCounters {
    pub total: u64,
    pub success: u64,
    pub failed: u64,
}

/// Byte counters for a connection.
#[derive(Debug, Clone, Default)]
pub struct ByteCounters {
    pub received: u64,
    pub sent: u64,
}

impl ShardedConnectionInfo {
    /// Create new connection info.
    fn new(id: ConnectionId, peer_addr: SocketAddr, shard_index: usize) -> Self {
        let now = Instant::now();
        Self {
            id,
            peer_addr,
            shard_index,
            state: ConnectionState::Active,
            connected_at: Utc::now(),
            connected_instant: now,
            last_activity_instant: now,
            requests: RequestCounters::default(),
            bytes: ByteCounters::default(),
        }
    }

    /// Get connection duration.
    #[inline]
    pub fn duration(&self) -> Duration {
        self.connected_instant.elapsed()
    }

    /// Get time since last activity.
    #[inline]
    pub fn idle_duration(&self) -> Duration {
        self.last_activity_instant.elapsed()
    }

    /// Check if connection is idle beyond threshold.
    #[inline]
    pub fn is_idle(&self, threshold: Duration) -> bool {
        self.idle_duration() > threshold
    }

    /// Record successful request.
    #[inline]
    pub fn record_success(&mut self, bytes_in: u64, bytes_out: u64) {
        self.requests.total += 1;
        self.requests.success += 1;
        self.bytes.received += bytes_in;
        self.bytes.sent += bytes_out;
        self.last_activity_instant = Instant::now();
        self.state = ConnectionState::Active;
    }

    /// Record failed request.
    #[inline]
    pub fn record_failure(&mut self, bytes_in: u64) {
        self.requests.total += 1;
        self.requests.failed += 1;
        self.bytes.received += bytes_in;
        self.last_activity_instant = Instant::now();
    }

    /// Mark as draining (no new requests).
    #[inline]
    pub fn drain(&mut self) {
        self.state = ConnectionState::Draining;
    }

    /// Mark as closing.
    #[inline]
    pub fn close(&mut self) {
        self.state = ConnectionState::Closing;
    }
}

/// A single shard in the connection pool.
pub struct ConnectionShard {
    /// Connections in this shard, keyed by connection ID.
    connections: DashMap<ConnectionId, ShardedConnectionInfo>,

    /// Reverse lookup: peer address to connection ID.
    addr_to_id: DashMap<SocketAddr, ConnectionId>,

    /// Active connection count for this shard.
    active_count: AtomicUsize,

    /// Shard index.
    index: usize,
}

impl ConnectionShard {
    /// Create a new shard.
    fn new(index: usize, estimated_capacity: usize) -> Self {
        Self {
            connections: DashMap::with_capacity(estimated_capacity),
            addr_to_id: DashMap::with_capacity(estimated_capacity),
            active_count: AtomicUsize::new(0),
            index,
        }
    }

    /// Get connection count in this shard.
    #[inline]
    pub fn len(&self) -> usize {
        self.active_count.load(Ordering::Relaxed)
    }

    /// Check if shard is empty.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Insert a connection.
    fn insert(&self, id: ConnectionId, peer_addr: SocketAddr) -> ShardedConnectionInfo {
        let info = ShardedConnectionInfo::new(id, peer_addr, self.index);
        self.connections.insert(id, info.clone());
        self.addr_to_id.insert(peer_addr, id);
        self.active_count.fetch_add(1, Ordering::Relaxed);
        info
    }

    /// Remove a connection.
    fn remove(&self, id: ConnectionId) -> Option<ShardedConnectionInfo> {
        if let Some((_, info)) = self.connections.remove(&id) {
            self.addr_to_id.remove(&info.peer_addr);
            self.active_count.fetch_sub(1, Ordering::Relaxed);
            Some(info)
        } else {
            None
        }
    }

    /// Get connection info.
    fn get(&self, id: ConnectionId) -> Option<ShardedConnectionInfo> {
        self.connections.get(&id).map(|r| r.value().clone())
    }

    /// Get connection by address.
    fn get_by_addr(&self, addr: &SocketAddr) -> Option<ShardedConnectionInfo> {
        self.addr_to_id.get(addr).and_then(|id| self.get(*id))
    }

    /// Update connection with a closure.
    fn update<F>(&self, id: ConnectionId, f: F) -> bool
    where
        F: FnOnce(&mut ShardedConnectionInfo),
    {
        if let Some(mut entry) = self.connections.get_mut(&id) {
            f(entry.value_mut());
            true
        } else {
            false
        }
    }

    /// Get connections idle for longer than threshold.
    fn get_idle(&self, threshold: Duration) -> Vec<ConnectionId> {
        self.connections
            .iter()
            .filter(|r| r.value().is_idle(threshold))
            .map(|r| *r.key())
            .collect()
    }
}

/// Event types for connection lifecycle.
#[derive(Debug, Clone)]
pub enum PoolEvent {
    /// Connection registered.
    Connected {
        connection_id: ConnectionId,
        peer_addr: SocketAddr,
        shard_index: usize,
    },
    /// Connection unregistered.
    Disconnected {
        connection_id: ConnectionId,
        peer_addr: SocketAddr,
        duration: Duration,
        requests_total: u64,
    },
    /// Connection rejected.
    Rejected {
        peer_addr: SocketAddr,
        reason: String,
    },
    /// Shard rebalance triggered.
    Rebalance {
        shard_index: usize,
        count_before: usize,
        count_after: usize,
    },
}

/// Handle for a registered connection.
///
/// When dropped, automatically unregisters the connection from the pool.
pub struct ConnectionHandle {
    pool: Arc<ShardedConnectionPool>,
    id: ConnectionId,
    shard_index: usize,
}

impl ConnectionHandle {
    /// Get connection ID.
    #[inline]
    pub fn id(&self) -> ConnectionId {
        self.id
    }

    /// Get shard index.
    #[inline]
    pub fn shard_index(&self) -> usize {
        self.shard_index
    }

    /// Record successful request.
    #[inline]
    pub fn record_success(&self, bytes_in: u64, bytes_out: u64) {
        self.pool.record_success(self.id, bytes_in, bytes_out);
    }

    /// Record failed request.
    #[inline]
    pub fn record_failure(&self, bytes_in: u64) {
        self.pool.record_failure(self.id, bytes_in);
    }

    /// Get connection info snapshot.
    pub fn info(&self) -> Option<ShardedConnectionInfo> {
        self.pool.get(self.id)
    }

    /// Mark connection as draining.
    pub fn drain(&self) {
        self.pool.shards[self.shard_index].update(self.id, |info| {
            info.drain();
        });
    }
}

impl Drop for ConnectionHandle {
    fn drop(&mut self) {
        self.pool.unregister(self.id);
    }
}

/// Statistics for the connection pool.
#[derive(Debug, Clone, Default)]
pub struct PoolStatistics {
    /// Total connections ever registered.
    pub total_connections: u64,
    /// Currently active connections.
    pub active_connections: usize,
    /// Total connections rejected.
    pub total_rejected: u64,
    /// Total requests processed.
    pub total_requests: u64,
    /// Total bytes received.
    pub total_bytes_received: u64,
    /// Total bytes sent.
    pub total_bytes_sent: u64,
    /// Per-shard statistics.
    pub per_shard: Vec<ShardStatistics>,
    /// Pool uptime.
    pub uptime: Duration,
}

/// Statistics for a single shard.
#[derive(Debug, Clone, Default)]
pub struct ShardStatistics {
    /// Shard index.
    pub index: usize,
    /// Active connections.
    pub active_connections: usize,
    /// Utilization ratio (0.0-1.0).
    pub utilization: f64,
}

/// Sharded connection pool for high-volume connection handling.
///
/// This pool distributes connections across multiple shards to minimize
/// lock contention and provide O(1) operations even with 10,000+ connections.
pub struct ShardedConnectionPool {
    /// Configuration.
    config: ConnectionPoolConfig,

    /// Connection shards.
    shards: Vec<ConnectionShard>,

    /// Shared shard routing logic.
    router: ShardRouter,

    /// Global connection ID counter.
    next_id: AtomicU64,

    /// Total connections ever registered.
    total_connections: AtomicU64,

    /// Total connections rejected.
    total_rejected: AtomicU64,

    /// Total active connections (sum of all shards).
    active_connections: AtomicUsize,

    /// Total requests processed.
    total_requests: AtomicU64,

    /// Total bytes received.
    total_bytes_received: AtomicU64,

    /// Total bytes sent.
    total_bytes_sent: AtomicU64,

    /// Pool creation time.
    created_at: Instant,

    /// Event broadcaster.
    event_tx: broadcast::Sender<PoolEvent>,

    /// Maximum connections.
    max_connections: usize,
}

impl ShardedConnectionPool {
    /// Create a new sharded connection pool.
    pub fn new(config: ConnectionPoolConfig) -> Arc<Self> {
        let shard_count = config.shard_count;
        let connections_per_shard = config.connections_per_shard;

        // Pre-allocate shards
        let shards: Vec<_> = (0..shard_count)
            .map(|i| ConnectionShard::new(i, connections_per_shard))
            .collect();

        let (event_tx, _) = broadcast::channel(4096);

        Arc::new(Self {
            max_connections: config.max_connections,
            config,
            shards,
            router: ShardRouter::new(shard_count),
            next_id: AtomicU64::new(1),
            total_connections: AtomicU64::new(0),
            total_rejected: AtomicU64::new(0),
            active_connections: AtomicUsize::new(0),
            total_requests: AtomicU64::new(0),
            total_bytes_received: AtomicU64::new(0),
            total_bytes_sent: AtomicU64::new(0),
            created_at: Instant::now(),
            event_tx,
        })
    }

    /// Create pool with default medium configuration.
    pub fn default_medium() -> Arc<Self> {
        Self::new(ConnectionPoolConfig::default())
    }

    /// Get shard index for a connection ID.
    #[inline]
    fn shard_index(&self, id: ConnectionId) -> usize {
        self.router.index_for_id(id)
    }

    /// Get shard index for an address (for lookup).
    #[inline]
    fn shard_index_for_addr(&self, addr: &SocketAddr) -> usize {
        self.router.index_for_addr(addr)
    }

    /// Try to register a new connection.
    ///
    /// Returns a connection handle if successful, or None if the pool is at capacity.
    pub fn try_register(self: &Arc<Self>, peer_addr: SocketAddr) -> Option<ConnectionHandle> {
        // Fast path: check global limit
        let current = self.active_connections.load(Ordering::Relaxed);
        if current >= self.max_connections {
            self.total_rejected.fetch_add(1, Ordering::Relaxed);
            let _ = self.event_tx.send(PoolEvent::Rejected {
                peer_addr,
                reason: format!("Pool at capacity ({}/{})", current, self.max_connections),
            });
            trace!(
                peer = %peer_addr,
                current,
                max = self.max_connections,
                "Connection rejected: pool at capacity"
            );
            return None;
        }

        // Generate connection ID
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let shard_index = self.shard_index(id);

        // Register in shard
        let info = self.shards[shard_index].insert(id, peer_addr);

        // Update global counters
        self.total_connections.fetch_add(1, Ordering::Relaxed);
        self.active_connections.fetch_add(1, Ordering::Relaxed);

        // Broadcast event
        let _ = self.event_tx.send(PoolEvent::Connected {
            connection_id: id,
            peer_addr,
            shard_index,
        });

        debug!(
            peer = %peer_addr,
            connection_id = id,
            shard = shard_index,
            active = self.active_connections.load(Ordering::Relaxed),
            "Connection registered"
        );

        Some(ConnectionHandle {
            pool: Arc::clone(self),
            id,
            shard_index: info.shard_index,
        })
    }

    /// Register a connection (panics if pool is full - use try_register for safe registration).
    pub fn register(self: &Arc<Self>, peer_addr: SocketAddr) -> ConnectionHandle {
        self.try_register(peer_addr)
            .expect("Connection pool at capacity")
    }

    /// Unregister a connection.
    pub fn unregister(&self, id: ConnectionId) -> Option<ShardedConnectionInfo> {
        let shard_index = self.shard_index(id);
        if let Some(info) = self.shards[shard_index].remove(id) {
            self.active_connections.fetch_sub(1, Ordering::Relaxed);

            let _ = self.event_tx.send(PoolEvent::Disconnected {
                connection_id: id,
                peer_addr: info.peer_addr,
                duration: info.duration(),
                requests_total: info.requests.total,
            });

            debug!(
                peer = %info.peer_addr,
                connection_id = id,
                duration_ms = info.duration().as_millis(),
                requests = info.requests.total,
                "Connection unregistered"
            );

            Some(info)
        } else {
            None
        }
    }

    /// Get connection info by ID.
    pub fn get(&self, id: ConnectionId) -> Option<ShardedConnectionInfo> {
        let shard_index = self.shard_index(id);
        self.shards[shard_index].get(id)
    }

    /// Get connection info by peer address.
    pub fn get_by_addr(&self, addr: &SocketAddr) -> Option<ShardedConnectionInfo> {
        // Try likely shard first
        let likely_shard = self.shard_index_for_addr(addr);
        if let Some(info) = self.shards[likely_shard].get_by_addr(addr) {
            return Some(info);
        }

        // Fall back to searching all shards
        for (i, shard) in self.shards.iter().enumerate() {
            if i == likely_shard {
                continue;
            }
            if let Some(info) = shard.get_by_addr(addr) {
                return Some(info);
            }
        }

        None
    }

    /// Record successful request.
    #[inline]
    pub fn record_success(&self, id: ConnectionId, bytes_in: u64, bytes_out: u64) {
        let shard_index = self.shard_index(id);
        self.shards[shard_index].update(id, |info| {
            info.record_success(bytes_in, bytes_out);
        });
        self.total_requests.fetch_add(1, Ordering::Relaxed);
        self.total_bytes_received
            .fetch_add(bytes_in, Ordering::Relaxed);
        self.total_bytes_sent
            .fetch_add(bytes_out, Ordering::Relaxed);
    }

    /// Record failed request.
    #[inline]
    pub fn record_failure(&self, id: ConnectionId, bytes_in: u64) {
        let shard_index = self.shard_index(id);
        self.shards[shard_index].update(id, |info| {
            info.record_failure(bytes_in);
        });
        self.total_requests.fetch_add(1, Ordering::Relaxed);
        self.total_bytes_received
            .fetch_add(bytes_in, Ordering::Relaxed);
    }

    /// Get number of active connections.
    #[inline]
    pub fn active_count(&self) -> usize {
        self.active_connections.load(Ordering::Relaxed)
    }

    /// Get maximum connections.
    #[inline]
    pub fn max_connections(&self) -> usize {
        self.max_connections
    }

    /// Get shard count.
    #[inline]
    pub fn shard_count(&self) -> usize {
        self.router.shard_count()
    }

    /// Get pool uptime.
    #[inline]
    pub fn uptime(&self) -> Duration {
        self.created_at.elapsed()
    }

    /// Subscribe to pool events.
    pub fn subscribe(&self) -> broadcast::Receiver<PoolEvent> {
        self.event_tx.subscribe()
    }

    /// Get idle connections across all shards.
    pub fn get_idle_connections(&self, threshold: Duration) -> Vec<ConnectionId> {
        let mut idle = Vec::new();
        for shard in &self.shards {
            idle.extend(shard.get_idle(threshold));
        }
        idle
    }

    /// Clean up idle connections.
    pub fn cleanup_idle(&self, threshold: Duration) -> usize {
        let idle = self.get_idle_connections(threshold);
        let count = idle.len();

        for id in idle {
            self.unregister(id);
        }

        if count > 0 {
            debug!(count, "Cleaned up idle connections");
        }

        count
    }

    /// Get pool statistics.
    pub fn statistics(&self) -> PoolStatistics {
        let per_shard: Vec<_> = self
            .shards
            .iter()
            .enumerate()
            .map(|(i, shard)| {
                let active = shard.len();
                ShardStatistics {
                    index: i,
                    active_connections: active,
                    utilization: active as f64 / self.config.connections_per_shard as f64,
                }
            })
            .collect();

        PoolStatistics {
            total_connections: self.total_connections.load(Ordering::Relaxed),
            active_connections: self.active_count(),
            total_rejected: self.total_rejected.load(Ordering::Relaxed),
            total_requests: self.total_requests.load(Ordering::Relaxed),
            total_bytes_received: self.total_bytes_received.load(Ordering::Relaxed),
            total_bytes_sent: self.total_bytes_sent.load(Ordering::Relaxed),
            per_shard,
            uptime: self.uptime(),
        }
    }

    /// Get configuration.
    pub fn config(&self) -> &ConnectionPoolConfig {
        &self.config
    }

    /// Iterate over all connections (expensive - use sparingly).
    pub fn iter_all(&self) -> impl Iterator<Item = ShardedConnectionInfo> + '_ {
        self.shards
            .iter()
            .flat_map(|shard| shard.connections.iter().map(|r| r.value().clone()))
    }

    /// Get utilization ratio (0.0-1.0).
    #[inline]
    pub fn utilization(&self) -> f64 {
        self.active_count() as f64 / self.max_connections as f64
    }

    /// Check if pool is at capacity.
    #[inline]
    pub fn is_full(&self) -> bool {
        self.active_count() >= self.max_connections
    }

    /// Get available capacity.
    #[inline]
    pub fn available(&self) -> usize {
        self.max_connections.saturating_sub(self.active_count())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{IpAddr, Ipv4Addr};

    fn make_addr(port: u16) -> SocketAddr {
        SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), port)
    }

    fn make_pool(max_connections: usize, shard_count: usize) -> Arc<ShardedConnectionPool> {
        let config = ConnectionPoolConfig {
            max_connections,
            shard_count,
            connections_per_shard: max_connections / shard_count + 1,
            idle_timeout: Duration::from_secs(60),
            health_check_interval: Duration::from_secs(30),
            enable_metrics: true,
        };
        ShardedConnectionPool::new(config)
    }

    #[test]
    fn test_basic_registration() {
        let pool = make_pool(100, 4);

        let handle = pool.try_register(make_addr(1001));
        assert!(handle.is_some());

        let handle = handle.unwrap();
        assert_eq!(pool.active_count(), 1);

        let info = pool.get(handle.id());
        assert!(info.is_some());
        assert_eq!(info.unwrap().peer_addr.port(), 1001);
    }

    #[test]
    fn test_capacity_limit() {
        let pool = make_pool(3, 2);

        let h1 = pool.try_register(make_addr(1001));
        let h2 = pool.try_register(make_addr(1002));
        let h3 = pool.try_register(make_addr(1003));

        assert!(h1.is_some());
        assert!(h2.is_some());
        assert!(h3.is_some());

        // Should fail - at capacity
        let h4 = pool.try_register(make_addr(1004));
        assert!(h4.is_none());

        let stats = pool.statistics();
        assert_eq!(stats.total_rejected, 1);
    }

    #[test]
    fn test_automatic_unregister_on_drop() {
        let pool = make_pool(100, 4);

        {
            let _handle = pool.try_register(make_addr(1001)).unwrap();
            assert_eq!(pool.active_count(), 1);
        }

        // Handle dropped, should unregister
        assert_eq!(pool.active_count(), 0);
    }

    #[test]
    fn test_request_tracking() {
        let pool = make_pool(100, 4);
        let handle = pool.try_register(make_addr(1001)).unwrap();

        handle.record_success(100, 200);
        handle.record_success(150, 250);
        handle.record_failure(50);

        let info = handle.info().unwrap();
        assert_eq!(info.requests.total, 3);
        assert_eq!(info.requests.success, 2);
        assert_eq!(info.requests.failed, 1);
        assert_eq!(info.bytes.received, 300);
        assert_eq!(info.bytes.sent, 450);
    }

    #[test]
    fn test_shard_distribution() {
        let pool = make_pool(1000, 8);

        // Register many connections
        let handles: Vec<_> = (0..100)
            .map(|i| pool.try_register(make_addr(1000 + i)).unwrap())
            .collect();

        // Check distribution across shards
        let stats = pool.statistics();
        let total_in_shards: usize = stats.per_shard.iter().map(|s| s.active_connections).sum();

        assert_eq!(total_in_shards, 100);

        // All shards should have some connections (probabilistic, may occasionally fail)
        let non_empty_shards = stats
            .per_shard
            .iter()
            .filter(|s| s.active_connections > 0)
            .count();
        assert!(
            non_empty_shards >= 4,
            "Expected connections distributed across shards"
        );

        drop(handles);
    }

    #[test]
    fn test_idle_detection() {
        let pool = make_pool(100, 4);
        let _handle = pool.try_register(make_addr(1001)).unwrap();

        // No connections should be idle immediately
        let idle = pool.get_idle_connections(Duration::from_millis(100));
        assert!(idle.is_empty());

        // After a short sleep, should still not be idle
        std::thread::sleep(Duration::from_millis(10));
        let idle = pool.get_idle_connections(Duration::from_secs(60));
        assert!(idle.is_empty());
    }

    #[test]
    fn test_get_by_addr() {
        let pool = make_pool(100, 4);
        let addr = make_addr(1001);
        let _handle = pool.try_register(addr).unwrap();

        let info = pool.get_by_addr(&addr);
        assert!(info.is_some());
        assert_eq!(info.unwrap().peer_addr, addr);

        let missing = pool.get_by_addr(&make_addr(9999));
        assert!(missing.is_none());
    }

    #[test]
    fn test_statistics() {
        let pool = make_pool(100, 4);

        let h1 = pool.try_register(make_addr(1001)).unwrap();
        let h2 = pool.try_register(make_addr(1002)).unwrap();

        h1.record_success(100, 200);
        h2.record_failure(50);

        let stats = pool.statistics();
        assert_eq!(stats.active_connections, 2);
        assert_eq!(stats.total_connections, 2);
        assert_eq!(stats.total_requests, 2);
        assert_eq!(stats.total_bytes_received, 150);
        assert_eq!(stats.total_bytes_sent, 200);
        assert_eq!(stats.per_shard.len(), 4);
    }

    #[test]
    fn test_utilization() {
        let pool = make_pool(100, 4);

        assert_eq!(pool.utilization(), 0.0);
        assert!(!pool.is_full());
        assert_eq!(pool.available(), 100);

        let handles: Vec<_> = (0..50)
            .map(|i| pool.try_register(make_addr(1000 + i)).unwrap())
            .collect();

        assert_eq!(pool.utilization(), 0.5);
        assert!(!pool.is_full());
        assert_eq!(pool.available(), 50);

        drop(handles);
    }

    #[tokio::test]
    async fn test_event_subscription() {
        let pool = make_pool(100, 4);
        let mut rx = pool.subscribe();

        let addr = make_addr(1001);
        let handle = pool.try_register(addr).unwrap();
        let id = handle.id();

        // Should receive Connected event
        let event = rx.recv().await.unwrap();
        match event {
            PoolEvent::Connected {
                connection_id,
                peer_addr,
                ..
            } => {
                assert_eq!(connection_id, id);
                assert_eq!(peer_addr, addr);
            }
            _ => panic!("Expected Connected event"),
        }

        drop(handle);

        // Should receive Disconnected event
        let event = rx.recv().await.unwrap();
        match event {
            PoolEvent::Disconnected {
                connection_id,
                peer_addr,
                ..
            } => {
                assert_eq!(connection_id, id);
                assert_eq!(peer_addr, addr);
            }
            _ => panic!("Expected Disconnected event"),
        }
    }

    #[test]
    fn test_power_of_two_sharding() {
        let pool = make_pool(100, 8);
        assert_eq!(pool.router.shard_count(), 8);
        assert_eq!(pool.shard_index(7), 7);
        assert_eq!(pool.shard_index(8), 0);

        let pool = make_pool(100, 6);
        assert_eq!(pool.router.shard_count(), 6);
        assert_eq!(pool.shard_index(7), 1);
        assert_eq!(pool.shard_index(12), 0);
    }
}

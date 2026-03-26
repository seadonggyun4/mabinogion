//! Group Value Cache with TTL/LRU eviction and auto-update on L_Data.ind.
//!
//! This module provides a production-grade group value cache that simulates
//! the client-side caching behavior found in trap-knx:
//!
//! - **Automatic Update**: Cache entries are updated when L_Data.ind (indications)
//!   are received, ensuring all connected clients see consistent values
//! - **TTL Expiration**: Cache entries expire after a configurable time-to-live
//! - **LRU Eviction**: When the cache reaches its maximum capacity, the least
//!   recently used entries are evicted
//! - **Per-Address Statistics**: Track hit/miss rates, update counts, and staleness
//!
//! ## Architecture
//!
//! ```text
//! GroupValueWrite (from client) → GroupObjectTable → broadcast L_Data.ind
//!                                                  ↓
//!                                           GroupValueCache.on_indication()
//!                                                  ↓
//!                                           Cache entry updated (TTL refreshed)
//! ```
//!
//! ## Integration
//!
//! The cache is integrated into `KnxServer`:
//! - `on_group_value_write()` → updates cache when any client writes
//! - `on_ldata_ind_received()` → updates cache on indication broadcast
//! - `get_cached_value()` → returns cached value if valid (not expired)

use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use dashmap::DashMap;
use serde::{Deserialize, Serialize};

use crate::address::GroupAddress;

// ============================================================================
// Cache Entry
// ============================================================================

/// A single cached group value entry.
#[derive(Debug, Clone)]
pub struct CacheEntry {
    /// Group address this entry belongs to.
    pub address: GroupAddress,
    /// Raw value bytes.
    pub value: Vec<u8>,
    /// Source address that last wrote this value (if known).
    pub source: Option<String>,
    /// Timestamp when this entry was last written/updated.
    pub updated_at: Instant,
    /// Timestamp when this entry was last accessed (read).
    pub accessed_at: Instant,
    /// Number of times this entry has been updated.
    pub update_count: u64,
    /// Number of times this entry has been accessed.
    pub access_count: u64,
}

impl CacheEntry {
    /// Create a new cache entry.
    fn new(address: GroupAddress, value: Vec<u8>, source: Option<String>) -> Self {
        let now = Instant::now();
        Self {
            address,
            value,
            source,
            updated_at: now,
            accessed_at: now,
            update_count: 1,
            access_count: 0,
        }
    }

    /// Check if this entry has expired based on TTL.
    pub fn is_expired(&self, ttl: Duration) -> bool {
        if ttl.is_zero() {
            return false; // TTL=0 means never expire
        }
        self.updated_at.elapsed() > ttl
    }

    /// Get the age of this entry since last update.
    pub fn age(&self) -> Duration {
        self.updated_at.elapsed()
    }

    /// Get the idle time since last access.
    pub fn idle_time(&self) -> Duration {
        self.accessed_at.elapsed()
    }
}

// ============================================================================
// Cache Configuration
// ============================================================================

/// GroupValueCache configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroupValueCacheConfig {
    /// Whether the cache is enabled.
    #[serde(default = "default_true")]
    pub enabled: bool,

    /// Maximum number of entries in the cache.
    /// When exceeded, LRU eviction occurs.
    #[serde(default = "default_max_entries")]
    pub max_entries: usize,

    /// Time-to-live for cache entries in milliseconds.
    /// 0 = no expiration (entries only evicted by LRU).
    #[serde(default = "default_ttl_ms")]
    pub ttl_ms: u64,

    /// Whether to automatically update cache on L_Data.ind indications.
    #[serde(default = "default_true")]
    pub auto_update_on_indication: bool,

    /// Whether to automatically update cache on GroupValueWrite.
    #[serde(default = "default_true")]
    pub auto_update_on_write: bool,
}

fn default_true() -> bool {
    true
}

fn default_max_entries() -> usize {
    4096
}

fn default_ttl_ms() -> u64 {
    60_000 // 60 seconds default TTL
}

impl Default for GroupValueCacheConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            max_entries: default_max_entries(),
            ttl_ms: default_ttl_ms(),
            auto_update_on_indication: true,
            auto_update_on_write: true,
        }
    }
}

impl GroupValueCacheConfig {
    /// Get TTL as Duration.
    pub fn ttl(&self) -> Duration {
        Duration::from_millis(self.ttl_ms)
    }

    /// Validate the configuration.
    pub fn validate(&self) -> Result<(), String> {
        if self.max_entries == 0 {
            return Err("GroupValueCache max_entries must be > 0".to_string());
        }
        Ok(())
    }
}

// ============================================================================
// GroupValueCache
// ============================================================================

/// Production-grade group value cache with TTL/LRU eviction.
///
/// Thread-safe — uses `DashMap` for concurrent access without global locking.
pub struct GroupValueCache {
    /// Configuration.
    config: GroupValueCacheConfig,
    /// Cache entries keyed by group address.
    entries: DashMap<GroupAddress, CacheEntry>,
    /// LRU tracking: stores (address, last_accessed) for eviction ordering.
    /// We use DashMap for the main store and rely on `accessed_at` timestamps
    /// for LRU ordering when eviction is needed.
    stats: CacheStats,
}

/// Cache statistics.
pub struct CacheStats {
    /// Number of cache hits.
    pub hits: AtomicU64,
    /// Number of cache misses.
    pub misses: AtomicU64,
    /// Number of entries evicted by LRU.
    pub evictions: AtomicU64,
    /// Number of entries expired by TTL.
    pub expirations: AtomicU64,
    /// Total updates (write + indication).
    pub updates: AtomicU64,
    /// Updates from indications.
    pub indication_updates: AtomicU64,
    /// Updates from direct writes.
    pub write_updates: AtomicU64,
}

impl CacheStats {
    fn new() -> Self {
        Self {
            hits: AtomicU64::new(0),
            misses: AtomicU64::new(0),
            evictions: AtomicU64::new(0),
            expirations: AtomicU64::new(0),
            updates: AtomicU64::new(0),
            indication_updates: AtomicU64::new(0),
            write_updates: AtomicU64::new(0),
        }
    }

    /// Take a snapshot of the statistics.
    pub fn snapshot(&self) -> CacheStatsSnapshot {
        CacheStatsSnapshot {
            hits: self.hits.load(Ordering::Relaxed),
            misses: self.misses.load(Ordering::Relaxed),
            evictions: self.evictions.load(Ordering::Relaxed),
            expirations: self.expirations.load(Ordering::Relaxed),
            updates: self.updates.load(Ordering::Relaxed),
            indication_updates: self.indication_updates.load(Ordering::Relaxed),
            write_updates: self.write_updates.load(Ordering::Relaxed),
        }
    }
}

/// Immutable snapshot of cache statistics.
#[derive(Debug, Clone)]
pub struct CacheStatsSnapshot {
    pub hits: u64,
    pub misses: u64,
    pub evictions: u64,
    pub expirations: u64,
    pub updates: u64,
    pub indication_updates: u64,
    pub write_updates: u64,
}

impl CacheStatsSnapshot {
    /// Cache hit rate (0.0 - 1.0).
    pub fn hit_rate(&self) -> f64 {
        let total = self.hits + self.misses;
        if total == 0 {
            return 0.0;
        }
        self.hits as f64 / total as f64
    }

    /// Total lookups.
    pub fn total_lookups(&self) -> u64 {
        self.hits + self.misses
    }
}

/// The source of a cache update.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateSource {
    /// Updated from a direct GroupValueWrite.
    Write,
    /// Updated from an L_Data.ind indication broadcast.
    Indication,
}

impl GroupValueCache {
    /// Create a new cache with the given configuration.
    pub fn new(config: GroupValueCacheConfig) -> Self {
        Self {
            entries: DashMap::with_capacity(config.max_entries.min(256)),
            config,
            stats: CacheStats::new(),
        }
    }

    /// Create a cache with default configuration.
    pub fn with_defaults() -> Self {
        Self::new(GroupValueCacheConfig::default())
    }

    /// Check if the cache is enabled.
    pub fn is_enabled(&self) -> bool {
        self.config.enabled
    }

    /// Get the cache configuration.
    pub fn config(&self) -> &GroupValueCacheConfig {
        &self.config
    }

    /// Get a cached value for a group address.
    ///
    /// Returns `None` if:
    /// - Cache is disabled
    /// - Entry does not exist
    /// - Entry has expired (TTL exceeded)
    pub fn get(&self, address: &GroupAddress) -> Option<Vec<u8>> {
        if !self.config.enabled {
            self.stats.misses.fetch_add(1, Ordering::Relaxed);
            return None;
        }

        match self.entries.get_mut(address) {
            Some(mut entry) => {
                let ttl = self.config.ttl();
                if entry.is_expired(ttl) {
                    // Entry expired — drop it
                    drop(entry);
                    self.entries.remove(address);
                    self.stats.expirations.fetch_add(1, Ordering::Relaxed);
                    self.stats.misses.fetch_add(1, Ordering::Relaxed);
                    None
                } else {
                    // Cache hit — update access tracking
                    entry.accessed_at = Instant::now();
                    entry.access_count += 1;
                    let value = entry.value.clone();
                    self.stats.hits.fetch_add(1, Ordering::Relaxed);
                    Some(value)
                }
            }
            None => {
                self.stats.misses.fetch_add(1, Ordering::Relaxed);
                None
            }
        }
    }

    /// Get a cached entry with full metadata.
    ///
    /// Returns `None` if entry does not exist or has expired.
    pub fn get_entry(&self, address: &GroupAddress) -> Option<CacheEntry> {
        if !self.config.enabled {
            return None;
        }

        match self.entries.get_mut(address) {
            Some(mut entry) => {
                let ttl = self.config.ttl();
                if entry.is_expired(ttl) {
                    drop(entry);
                    self.entries.remove(address);
                    self.stats.expirations.fetch_add(1, Ordering::Relaxed);
                    None
                } else {
                    entry.accessed_at = Instant::now();
                    entry.access_count += 1;
                    let snapshot = entry.clone();
                    self.stats.hits.fetch_add(1, Ordering::Relaxed);
                    Some(snapshot)
                }
            }
            None => None,
        }
    }

    /// Update the cache with a new value.
    ///
    /// If the cache is at capacity, performs LRU eviction before inserting.
    pub fn update(
        &self,
        address: GroupAddress,
        value: Vec<u8>,
        source: Option<String>,
        update_source: UpdateSource,
    ) {
        if !self.config.enabled {
            return;
        }

        // Check source-specific auto-update flags
        match update_source {
            UpdateSource::Write if !self.config.auto_update_on_write => return,
            UpdateSource::Indication if !self.config.auto_update_on_indication => return,
            _ => {}
        }

        // Update or insert
        if let Some(mut entry) = self.entries.get_mut(&address) {
            entry.value = value;
            entry.source = source;
            entry.updated_at = Instant::now();
            entry.accessed_at = Instant::now();
            entry.update_count += 1;
        } else {
            // Check capacity and evict if needed
            self.evict_if_needed();
            self.entries
                .insert(address, CacheEntry::new(address, value, source));
        }

        // Update stats
        self.stats.updates.fetch_add(1, Ordering::Relaxed);
        match update_source {
            UpdateSource::Write => {
                self.stats.write_updates.fetch_add(1, Ordering::Relaxed);
            }
            UpdateSource::Indication => {
                self.stats
                    .indication_updates
                    .fetch_add(1, Ordering::Relaxed);
            }
        }
    }

    /// Called when an L_Data.ind indication is received.
    ///
    /// Updates the cache entry for the target group address if the cache
    /// is enabled and auto_update_on_indication is true.
    pub fn on_indication(&self, address: GroupAddress, value: Vec<u8>, source: Option<String>) {
        self.update(address, value, source, UpdateSource::Indication);
    }

    /// Called when a GroupValueWrite is processed.
    ///
    /// Updates the cache entry for the written group address.
    pub fn on_write(&self, address: GroupAddress, value: Vec<u8>, source: Option<String>) {
        self.update(address, value, source, UpdateSource::Write);
    }

    /// Invalidate (remove) a specific cache entry.
    pub fn invalidate(&self, address: &GroupAddress) -> bool {
        self.entries.remove(address).is_some()
    }

    /// Invalidate all cache entries.
    pub fn invalidate_all(&self) {
        self.entries.clear();
    }

    /// Remove all expired entries.
    ///
    /// Returns the number of entries removed.
    pub fn purge_expired(&self) -> usize {
        let ttl = self.config.ttl();
        if ttl.is_zero() {
            return 0; // No TTL configured
        }

        let expired: Vec<GroupAddress> = self
            .entries
            .iter()
            .filter(|r| r.value().is_expired(ttl))
            .map(|r| *r.key())
            .collect();

        let count = expired.len();
        for addr in expired {
            self.entries.remove(&addr);
        }

        self.stats
            .expirations
            .fetch_add(count as u64, Ordering::Relaxed);
        count
    }

    /// Get the number of cached entries.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Check if the cache is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Get all cached addresses.
    pub fn addresses(&self) -> Vec<GroupAddress> {
        self.entries.iter().map(|r| *r.key()).collect()
    }

    /// Get statistics snapshot.
    pub fn stats_snapshot(&self) -> CacheStatsSnapshot {
        self.stats.snapshot()
    }

    /// Evict the least recently accessed entry if at capacity.
    fn evict_if_needed(&self) {
        if self.entries.len() < self.config.max_entries {
            return;
        }

        // Find the entry with the oldest accessed_at (LRU)
        let lru_addr = self
            .entries
            .iter()
            .min_by_key(|r| r.value().accessed_at)
            .map(|r| *r.key());

        if let Some(addr) = lru_addr {
            self.entries.remove(&addr);
            self.stats.evictions.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Get a summary of cache state for a specific address.
    pub fn entry_info(&self, address: &GroupAddress) -> Option<CacheEntryInfo> {
        self.entries.get(address).map(|entry| {
            let ttl = self.config.ttl();
            CacheEntryInfo {
                address: entry.address,
                value_len: entry.value.len(),
                source: entry.source.clone(),
                age: entry.age(),
                idle_time: entry.idle_time(),
                update_count: entry.update_count,
                access_count: entry.access_count,
                is_expired: entry.is_expired(ttl),
            }
        })
    }

    /// Get info for all entries.
    pub fn all_entry_info(&self) -> Vec<CacheEntryInfo> {
        let ttl = self.config.ttl();
        self.entries
            .iter()
            .map(|r| {
                let entry = r.value();
                CacheEntryInfo {
                    address: entry.address,
                    value_len: entry.value.len(),
                    source: entry.source.clone(),
                    age: entry.age(),
                    idle_time: entry.idle_time(),
                    update_count: entry.update_count,
                    access_count: entry.access_count,
                    is_expired: entry.is_expired(ttl),
                }
            })
            .collect()
    }
}

/// Summary information about a cache entry.
#[derive(Debug, Clone)]
pub struct CacheEntryInfo {
    pub address: GroupAddress,
    pub value_len: usize,
    pub source: Option<String>,
    pub age: Duration,
    pub idle_time: Duration,
    pub update_count: u64,
    pub access_count: u64,
    pub is_expired: bool,
}

impl fmt::Debug for GroupValueCache {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GroupValueCache")
            .field("enabled", &self.config.enabled)
            .field("entries", &self.entries.len())
            .field("max_entries", &self.config.max_entries)
            .field("ttl_ms", &self.config.ttl_ms)
            .finish()
    }
}

impl Default for GroupValueCache {
    fn default() -> Self {
        Self::with_defaults()
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn make_addr(main: u8, middle: u8, sub: u8) -> GroupAddress {
        GroupAddress::three_level(main, middle, sub)
    }

    #[test]
    fn test_cache_basic_get_set() {
        let cache = GroupValueCache::with_defaults();

        let addr = make_addr(1, 0, 1);
        cache.on_write(addr, vec![0x01], Some("1.1.1".into()));

        let value = cache.get(&addr);
        assert_eq!(value, Some(vec![0x01]));
    }

    #[test]
    fn test_cache_miss() {
        let cache = GroupValueCache::with_defaults();
        let addr = make_addr(1, 0, 1);
        assert_eq!(cache.get(&addr), None);
    }

    #[test]
    fn test_cache_update_overwrites() {
        let cache = GroupValueCache::with_defaults();
        let addr = make_addr(1, 0, 1);

        cache.on_write(addr, vec![0x01], None);
        assert_eq!(cache.get(&addr), Some(vec![0x01]));

        cache.on_write(addr, vec![0x02], None);
        assert_eq!(cache.get(&addr), Some(vec![0x02]));
    }

    #[test]
    fn test_cache_indication_update() {
        let cache = GroupValueCache::with_defaults();
        let addr = make_addr(1, 0, 1);

        cache.on_indication(addr, vec![0x55], Some("1.2.3".into()));

        let value = cache.get(&addr);
        assert_eq!(value, Some(vec![0x55]));

        let stats = cache.stats_snapshot();
        assert_eq!(stats.indication_updates, 1);
    }

    #[test]
    fn test_cache_disabled() {
        let config = GroupValueCacheConfig {
            enabled: false,
            ..Default::default()
        };
        let cache = GroupValueCache::new(config);
        let addr = make_addr(1, 0, 1);

        cache.on_write(addr, vec![0x01], None);
        assert_eq!(cache.get(&addr), None);
        assert_eq!(cache.len(), 0);
    }

    #[test]
    fn test_cache_ttl_expiration() {
        let config = GroupValueCacheConfig {
            ttl_ms: 1, // 1ms TTL — will expire almost immediately
            ..Default::default()
        };
        let cache = GroupValueCache::new(config);
        let addr = make_addr(1, 0, 1);

        cache.on_write(addr, vec![0x01], None);

        // Wait for TTL to expire
        std::thread::sleep(Duration::from_millis(5));

        assert_eq!(cache.get(&addr), None);

        let stats = cache.stats_snapshot();
        assert_eq!(stats.expirations, 1);
    }

    #[test]
    fn test_cache_ttl_zero_no_expiration() {
        let config = GroupValueCacheConfig {
            ttl_ms: 0, // No expiration
            ..Default::default()
        };
        let cache = GroupValueCache::new(config);
        let addr = make_addr(1, 0, 1);

        cache.on_write(addr, vec![0x01], None);

        // Even after a short wait, should not expire
        std::thread::sleep(Duration::from_millis(5));
        assert_eq!(cache.get(&addr), Some(vec![0x01]));
    }

    #[test]
    fn test_cache_lru_eviction() {
        let config = GroupValueCacheConfig {
            max_entries: 3,
            ttl_ms: 0, // No TTL
            ..Default::default()
        };
        let cache = GroupValueCache::new(config);

        let addr1 = make_addr(1, 0, 1);
        let addr2 = make_addr(1, 0, 2);
        let addr3 = make_addr(1, 0, 3);
        let addr4 = make_addr(1, 0, 4);

        cache.on_write(addr1, vec![0x01], None);
        std::thread::sleep(Duration::from_millis(1));
        cache.on_write(addr2, vec![0x02], None);
        std::thread::sleep(Duration::from_millis(1));
        cache.on_write(addr3, vec![0x03], None);

        assert_eq!(cache.len(), 3);

        // Access addr1 to make it recently used
        cache.get(&addr1);
        std::thread::sleep(Duration::from_millis(1));

        // Insert addr4 — should evict addr2 (least recently accessed)
        cache.on_write(addr4, vec![0x04], None);

        assert_eq!(cache.len(), 3);
        assert_eq!(cache.get(&addr1), Some(vec![0x01])); // Still present (recently accessed)
        assert_eq!(cache.get(&addr2), None); // Evicted (LRU)
        assert_eq!(cache.get(&addr3), Some(vec![0x03])); // Still present
        assert_eq!(cache.get(&addr4), Some(vec![0x04])); // Newly inserted

        let stats = cache.stats_snapshot();
        assert_eq!(stats.evictions, 1);
    }

    #[test]
    fn test_cache_invalidate() {
        let cache = GroupValueCache::with_defaults();
        let addr = make_addr(1, 0, 1);

        cache.on_write(addr, vec![0x01], None);
        assert!(cache.invalidate(&addr));
        assert_eq!(cache.get(&addr), None);
        assert!(!cache.invalidate(&addr)); // Already removed
    }

    #[test]
    fn test_cache_invalidate_all() {
        let cache = GroupValueCache::with_defaults();
        cache.on_write(make_addr(1, 0, 1), vec![0x01], None);
        cache.on_write(make_addr(1, 0, 2), vec![0x02], None);
        cache.on_write(make_addr(1, 0, 3), vec![0x03], None);

        assert_eq!(cache.len(), 3);
        cache.invalidate_all();
        assert_eq!(cache.len(), 0);
    }

    #[test]
    fn test_cache_purge_expired() {
        let config = GroupValueCacheConfig {
            ttl_ms: 1,
            ..Default::default()
        };
        let cache = GroupValueCache::new(config);

        cache.on_write(make_addr(1, 0, 1), vec![0x01], None);
        cache.on_write(make_addr(1, 0, 2), vec![0x02], None);

        std::thread::sleep(Duration::from_millis(5));

        let purged = cache.purge_expired();
        assert_eq!(purged, 2);
        assert!(cache.is_empty());
    }

    #[test]
    fn test_cache_stats() {
        let cache = GroupValueCache::with_defaults();
        let addr = make_addr(1, 0, 1);

        // Miss
        cache.get(&addr);

        // Write
        cache.on_write(addr, vec![0x01], None);

        // Hit
        cache.get(&addr);

        // Indication update
        cache.on_indication(addr, vec![0x02], None);

        let stats = cache.stats_snapshot();
        assert_eq!(stats.hits, 1);
        assert_eq!(stats.misses, 1);
        assert_eq!(stats.updates, 2);
        assert_eq!(stats.write_updates, 1);
        assert_eq!(stats.indication_updates, 1);
        assert_eq!(stats.total_lookups(), 2);
        assert_eq!(stats.hit_rate(), 0.5);
    }

    #[test]
    fn test_cache_stats_hit_rate_zero() {
        let stats = CacheStatsSnapshot {
            hits: 0,
            misses: 0,
            evictions: 0,
            expirations: 0,
            updates: 0,
            indication_updates: 0,
            write_updates: 0,
        };
        assert_eq!(stats.hit_rate(), 0.0);
    }

    #[test]
    fn test_cache_get_entry() {
        let cache = GroupValueCache::with_defaults();
        let addr = make_addr(1, 0, 1);

        cache.on_write(addr, vec![0x42], Some("1.1.1".into()));

        let entry = cache.get_entry(&addr).unwrap();
        assert_eq!(entry.address, addr);
        assert_eq!(entry.value, vec![0x42]);
        assert_eq!(entry.source, Some("1.1.1".to_string()));
        assert_eq!(entry.update_count, 1);
        assert_eq!(entry.access_count, 1); // get_entry counts as an access
    }

    #[test]
    fn test_cache_entry_info() {
        let cache = GroupValueCache::with_defaults();
        let addr = make_addr(1, 0, 1);

        cache.on_write(addr, vec![0x42, 0x43], Some("1.1.1".into()));

        let info = cache.entry_info(&addr).unwrap();
        assert_eq!(info.address, addr);
        assert_eq!(info.value_len, 2);
        assert_eq!(info.source, Some("1.1.1".to_string()));
        assert_eq!(info.update_count, 1);
        assert!(!info.is_expired);
    }

    #[test]
    fn test_cache_addresses() {
        let cache = GroupValueCache::with_defaults();
        cache.on_write(make_addr(1, 0, 1), vec![0x01], None);
        cache.on_write(make_addr(1, 0, 2), vec![0x02], None);

        let addresses = cache.addresses();
        assert_eq!(addresses.len(), 2);
    }

    #[test]
    fn test_cache_auto_update_disabled() {
        let config = GroupValueCacheConfig {
            auto_update_on_indication: false,
            auto_update_on_write: true,
            ..Default::default()
        };
        let cache = GroupValueCache::new(config);
        let addr = make_addr(1, 0, 1);

        // Write should update
        cache.on_write(addr, vec![0x01], None);
        assert_eq!(cache.get(&addr), Some(vec![0x01]));

        // Indication should NOT update since auto_update_on_indication=false
        cache.on_indication(addr, vec![0x02], None);
        assert_eq!(cache.get(&addr), Some(vec![0x01])); // Still old value
    }

    #[test]
    fn test_cache_config_validate() {
        assert!(GroupValueCacheConfig::default().validate().is_ok());
        assert!(GroupValueCacheConfig {
            max_entries: 0,
            ..Default::default()
        }
        .validate()
        .is_err());
    }

    #[test]
    fn test_cache_config_defaults() {
        let config = GroupValueCacheConfig::default();
        assert!(config.enabled);
        assert_eq!(config.max_entries, 4096);
        assert_eq!(config.ttl_ms, 60_000);
        assert!(config.auto_update_on_indication);
        assert!(config.auto_update_on_write);
    }

    #[test]
    fn test_cache_debug() {
        let cache = GroupValueCache::with_defaults();
        let debug_str = format!("{:?}", cache);
        assert!(debug_str.contains("GroupValueCache"));
        assert!(debug_str.contains("enabled"));
    }

    #[test]
    fn test_update_count_increments() {
        let cache = GroupValueCache::with_defaults();
        let addr = make_addr(1, 0, 1);

        cache.on_write(addr, vec![0x01], None);
        cache.on_write(addr, vec![0x02], None);
        cache.on_indication(addr, vec![0x03], None);

        let entry = cache.get_entry(&addr).unwrap();
        assert_eq!(entry.update_count, 3);
        assert_eq!(entry.value, vec![0x03]);
    }

    #[test]
    fn test_cache_entry_expired() {
        let entry = CacheEntry::new(make_addr(1, 0, 1), vec![0x01], None);

        // Not expired with 1 hour TTL
        assert!(!entry.is_expired(Duration::from_secs(3600)));

        // Not expired with zero TTL (never expire)
        assert!(!entry.is_expired(Duration::ZERO));
    }

    #[test]
    fn test_cache_all_entry_info() {
        let cache = GroupValueCache::with_defaults();
        cache.on_write(make_addr(1, 0, 1), vec![0x01], None);
        cache.on_write(make_addr(1, 0, 2), vec![0x02], None);

        let info = cache.all_entry_info();
        assert_eq!(info.len(), 2);
    }
}

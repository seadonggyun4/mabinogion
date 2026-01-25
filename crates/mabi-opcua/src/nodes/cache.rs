//! Node caching for large address spaces.
//!
//! This module provides LRU caching for frequently accessed nodes,
//! improving performance when dealing with 100,000+ node address spaces.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use lru::LruCache;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use tracing::{debug, trace};

use crate::types::{NodeId, DataValue};
use super::base::{NodeClass, QualifiedName, LocalizedText};
use super::reference::ReferenceDescription;

/// Node cache configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeCacheConfig {
    /// Maximum number of nodes to cache.
    pub max_size: usize,
    /// Whether to enable prefetching.
    pub prefetch_enabled: bool,
    /// Prefetch depth (how many levels to prefetch).
    pub prefetch_depth: usize,
    /// Enable value caching for variables.
    pub cache_values: bool,
    /// Value cache TTL in milliseconds.
    pub value_cache_ttl_ms: u64,
}

impl Default for NodeCacheConfig {
    fn default() -> Self {
        Self {
            max_size: 100_000,
            prefetch_enabled: true,
            prefetch_depth: 2,
            cache_values: true,
            value_cache_ttl_ms: 1000, // 1 second
        }
    }
}

/// Cache statistics.
#[derive(Debug, Clone, Default)]
pub struct CacheStats {
    /// Cache hits.
    pub hits: u64,
    /// Cache misses.
    pub misses: u64,
    /// Cache evictions.
    pub evictions: u64,
    /// Prefetch operations.
    pub prefetches: u64,
    /// Current cache size.
    pub size: usize,
}

impl CacheStats {
    /// Calculate hit rate.
    pub fn hit_rate(&self) -> f64 {
        let total = self.hits + self.misses;
        if total == 0 {
            return 0.0;
        }
        self.hits as f64 / total as f64
    }
}

/// Cached node entry.
#[derive(Debug, Clone)]
pub struct CachedNode {
    /// Node ID.
    pub node_id: NodeId,
    /// Node class.
    pub node_class: NodeClass,
    /// Browse name.
    pub browse_name: QualifiedName,
    /// Display name.
    pub display_name: LocalizedText,
    /// Cached value (for variables).
    pub value: Option<CachedValue>,
    /// Cached references.
    pub references: Option<Vec<ReferenceDescription>>,
}

/// Cached variable value.
#[derive(Debug, Clone)]
pub struct CachedValue {
    /// The data value.
    pub data_value: DataValue,
    /// Timestamp when cached.
    pub cached_at: std::time::Instant,
}

impl CachedValue {
    /// Create a new cached value.
    pub fn new(data_value: DataValue) -> Self {
        Self {
            data_value,
            cached_at: std::time::Instant::now(),
        }
    }

    /// Check if the cached value is expired.
    pub fn is_expired(&self, ttl_ms: u64) -> bool {
        self.cached_at.elapsed().as_millis() > ttl_ms as u128
    }
}

/// Node cache for efficient node access.
///
/// Uses an LRU eviction policy to maintain a bounded cache size.
/// Thread-safe through internal locking.
///
/// # Examples
///
/// ```ignore
/// let cache = NodeCache::new(NodeCacheConfig::default());
///
/// // Put a node
/// cache.put(CachedNode {
///     node_id: NodeId::numeric(2, 1001),
///     node_class: NodeClass::Variable,
///     browse_name: QualifiedName::new(2, "Temperature"),
///     display_name: LocalizedText::invariant("Temperature"),
///     value: None,
///     references: None,
/// });
///
/// // Get a node
/// if let Some(node) = cache.get(&NodeId::numeric(2, 1001)) {
///     println!("Found: {}", node.display_name);
/// }
/// ```
pub struct NodeCache {
    config: NodeCacheConfig,
    cache: Mutex<LruCache<NodeId, CachedNode>>,
    hits: AtomicU64,
    misses: AtomicU64,
    evictions: AtomicU64,
    prefetches: AtomicU64,
}

impl NodeCache {
    /// Create a new node cache.
    pub fn new(config: NodeCacheConfig) -> Self {
        let max_size = std::num::NonZeroUsize::new(config.max_size)
            .unwrap_or(std::num::NonZeroUsize::new(1000).unwrap());

        Self {
            config,
            cache: Mutex::new(LruCache::new(max_size)),
            hits: AtomicU64::new(0),
            misses: AtomicU64::new(0),
            evictions: AtomicU64::new(0),
            prefetches: AtomicU64::new(0),
        }
    }

    /// Get a node from cache.
    pub fn get(&self, node_id: &NodeId) -> Option<CachedNode> {
        let mut cache = self.cache.lock();

        if let Some(node) = cache.get(node_id) {
            self.hits.fetch_add(1, Ordering::Relaxed);
            trace!(node_id = %node_id, "Cache hit");
            Some(node.clone())
        } else {
            self.misses.fetch_add(1, Ordering::Relaxed);
            trace!(node_id = %node_id, "Cache miss");
            None
        }
    }

    /// Get a node's cached value if not expired.
    pub fn get_value(&self, node_id: &NodeId) -> Option<DataValue> {
        if !self.config.cache_values {
            return None;
        }

        let cache = self.cache.lock();

        if let Some(node) = cache.peek(node_id) {
            if let Some(ref cached_value) = node.value {
                if !cached_value.is_expired(self.config.value_cache_ttl_ms) {
                    return Some(cached_value.data_value.clone());
                }
            }
        }

        None
    }

    /// Put a node into cache.
    pub fn put(&self, node: CachedNode) {
        let mut cache = self.cache.lock();

        // Check if we need to evict
        if cache.len() >= self.config.max_size {
            if cache.pop_lru().is_some() {
                self.evictions.fetch_add(1, Ordering::Relaxed);
            }
        }

        trace!(node_id = %node.node_id, "Cache put");
        cache.put(node.node_id.clone(), node);
    }

    /// Update a cached node's value.
    pub fn update_value(&self, node_id: &NodeId, data_value: DataValue) {
        if !self.config.cache_values {
            return;
        }

        let mut cache = self.cache.lock();

        if let Some(node) = cache.get_mut(node_id) {
            node.value = Some(CachedValue::new(data_value));
        }
    }

    /// Update a cached node's references.
    pub fn update_references(&self, node_id: &NodeId, references: Vec<ReferenceDescription>) {
        let mut cache = self.cache.lock();

        if let Some(node) = cache.get_mut(node_id) {
            node.references = Some(references);
        }
    }

    /// Invalidate a cached node.
    pub fn invalidate(&self, node_id: &NodeId) {
        let mut cache = self.cache.lock();
        cache.pop(node_id);
        debug!(node_id = %node_id, "Cache invalidate");
    }

    /// Invalidate all cached nodes.
    pub fn clear(&self) {
        let mut cache = self.cache.lock();
        cache.clear();
        debug!("Cache cleared");
    }

    /// Get cache statistics.
    pub fn stats(&self) -> CacheStats {
        let cache = self.cache.lock();
        CacheStats {
            hits: self.hits.load(Ordering::Relaxed),
            misses: self.misses.load(Ordering::Relaxed),
            evictions: self.evictions.load(Ordering::Relaxed),
            prefetches: self.prefetches.load(Ordering::Relaxed),
            size: cache.len(),
        }
    }

    /// Check if a node is cached.
    pub fn contains(&self, node_id: &NodeId) -> bool {
        let cache = self.cache.lock();
        cache.contains(node_id)
    }

    /// Get current cache size.
    pub fn len(&self) -> usize {
        self.cache.lock().len()
    }

    /// Check if cache is empty.
    pub fn is_empty(&self) -> bool {
        self.cache.lock().is_empty()
    }

    /// Get the cache configuration.
    pub fn config(&self) -> &NodeCacheConfig {
        &self.config
    }

    /// Record a prefetch operation.
    pub fn record_prefetch(&self, count: usize) {
        self.prefetches.fetch_add(count as u64, Ordering::Relaxed);
    }
}

impl Default for NodeCache {
    fn default() -> Self {
        Self::new(NodeCacheConfig::default())
    }
}

/// Cache warming utility for preloading nodes.
pub struct CacheWarmer {
    cache: Arc<NodeCache>,
}

impl CacheWarmer {
    /// Create a new cache warmer.
    pub fn new(cache: Arc<NodeCache>) -> Self {
        Self { cache }
    }

    /// Warm the cache with a list of nodes.
    pub fn warm(&self, nodes: impl IntoIterator<Item = CachedNode>) {
        let count = nodes.into_iter().map(|node| {
            self.cache.put(node);
        }).count();

        debug!(count, "Cache warmed");
    }

    /// Warm the cache with frequently accessed node IDs.
    /// This is a hint for prefetching from the address space.
    pub fn warm_hints(&self, _hints: &[NodeId]) {
        // This would be implemented to fetch from address space
        // and populate cache
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Variant;

    fn create_test_node(id: u32) -> CachedNode {
        CachedNode {
            node_id: NodeId::numeric(2, id),
            node_class: NodeClass::Variable,
            browse_name: QualifiedName::new(2, format!("Node{}", id)),
            display_name: LocalizedText::invariant(format!("Node {}", id)),
            value: None,
            references: None,
        }
    }

    #[test]
    fn test_cache_basic() {
        let cache = NodeCache::new(NodeCacheConfig {
            max_size: 10,
            ..Default::default()
        });

        // Put a node
        let node = create_test_node(1001);
        cache.put(node.clone());

        // Get it back
        let retrieved = cache.get(&NodeId::numeric(2, 1001)).unwrap();
        assert_eq!(retrieved.node_id, node.node_id);
    }

    #[test]
    fn test_cache_miss() {
        let cache = NodeCache::new(NodeCacheConfig::default());

        let result = cache.get(&NodeId::numeric(2, 9999));
        assert!(result.is_none());

        let stats = cache.stats();
        assert_eq!(stats.misses, 1);
    }

    #[test]
    fn test_cache_eviction() {
        let cache = NodeCache::new(NodeCacheConfig {
            max_size: 5,
            ..Default::default()
        });

        // Fill cache beyond capacity
        for i in 0..10 {
            cache.put(create_test_node(1000 + i));
        }

        assert_eq!(cache.len(), 5);

        let stats = cache.stats();
        assert!(stats.evictions >= 5);
    }

    #[test]
    fn test_cache_invalidate() {
        let cache = NodeCache::new(NodeCacheConfig::default());

        cache.put(create_test_node(1001));
        assert!(cache.contains(&NodeId::numeric(2, 1001)));

        cache.invalidate(&NodeId::numeric(2, 1001));
        assert!(!cache.contains(&NodeId::numeric(2, 1001)));
    }

    #[test]
    fn test_cache_clear() {
        let cache = NodeCache::new(NodeCacheConfig::default());

        for i in 0..10 {
            cache.put(create_test_node(1000 + i));
        }

        assert_eq!(cache.len(), 10);

        cache.clear();
        assert!(cache.is_empty());
    }

    #[test]
    fn test_cached_value_expiration() {
        let value = CachedValue::new(DataValue::new(Variant::double(25.5)));

        // Should not be expired immediately
        assert!(!value.is_expired(1000));

        // After artificial delay, should be expired
        // (In real tests, we'd use mock time)
    }

    #[test]
    fn test_cache_stats() {
        let cache = NodeCache::new(NodeCacheConfig::default());

        // Some hits and misses
        cache.put(create_test_node(1001));
        cache.get(&NodeId::numeric(2, 1001)); // hit
        cache.get(&NodeId::numeric(2, 1001)); // hit
        cache.get(&NodeId::numeric(2, 9999)); // miss

        let stats = cache.stats();
        assert_eq!(stats.hits, 2);
        assert_eq!(stats.misses, 1);
        assert!((stats.hit_rate() - 0.666).abs() < 0.01);
    }
}

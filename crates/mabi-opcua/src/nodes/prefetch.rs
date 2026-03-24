//! Intelligent Node Prefetching for OPC UA Address Space.
//!
//! This module provides predictive prefetching capabilities to improve
//! cache hit rates for frequently accessed node patterns.
//!
//! ## Features
//!
//! - **Pattern-based prefetching**: Learn access patterns and prefetch likely nodes
//! - **Hierarchical prefetching**: Prefetch children/parents based on browse operations
//! - **Async prefetching**: Background prefetch to avoid blocking operations
//! - **Adaptive strategies**: Adjust prefetch behavior based on cache statistics

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::{Mutex, RwLock};
use serde::{Deserialize, Serialize};
use tracing::{debug, trace};

use super::cache::{CachedNode, NodeCache};
use super::reference::BrowseDirection;
use super::store::AddressSpace;
use crate::types::NodeId;

/// Configuration for the prefetcher.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrefetchConfig {
    /// Whether prefetching is enabled.
    pub enabled: bool,
    /// Maximum depth for hierarchical prefetching.
    pub max_depth: usize,
    /// Maximum nodes to prefetch per operation.
    pub max_prefetch_count: usize,
    /// Prefetch queue size.
    pub queue_size: usize,
    /// Minimum interval between prefetch batches (ms).
    pub batch_interval_ms: u64,
    /// Enable pattern learning.
    pub learn_patterns: bool,
    /// Pattern history size.
    pub pattern_history_size: usize,
    /// Minimum pattern frequency before prefetching.
    pub min_pattern_frequency: usize,
    /// Prefetch children when parent is accessed.
    pub prefetch_children: bool,
    /// Prefetch siblings when node is accessed.
    pub prefetch_siblings: bool,
    /// Prefetch type definitions.
    pub prefetch_types: bool,
}

impl Default for PrefetchConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            max_depth: 2,
            max_prefetch_count: 100,
            queue_size: 1000,
            batch_interval_ms: 10,
            learn_patterns: true,
            pattern_history_size: 1000,
            min_pattern_frequency: 3,
            prefetch_children: true,
            prefetch_siblings: false,
            prefetch_types: true,
        }
    }
}

impl PrefetchConfig {
    /// Configuration for aggressive prefetching (high cache hit rate).
    pub fn aggressive() -> Self {
        Self {
            enabled: true,
            max_depth: 3,
            max_prefetch_count: 200,
            prefetch_children: true,
            prefetch_siblings: true,
            prefetch_types: true,
            ..Default::default()
        }
    }

    /// Configuration for conservative prefetching (low memory usage).
    pub fn conservative() -> Self {
        Self {
            enabled: true,
            max_depth: 1,
            max_prefetch_count: 50,
            prefetch_children: true,
            prefetch_siblings: false,
            prefetch_types: false,
            ..Default::default()
        }
    }

    /// Disable prefetching entirely.
    pub fn disabled() -> Self {
        Self {
            enabled: false,
            ..Default::default()
        }
    }
}

/// Statistics for prefetch operations.
#[derive(Debug, Clone, Default)]
pub struct PrefetchStats {
    /// Total nodes prefetched.
    pub nodes_prefetched: u64,
    /// Prefetch operations triggered.
    pub prefetch_operations: u64,
    /// Patterns detected.
    pub patterns_detected: u64,
    /// Pattern-based prefetches.
    pub pattern_prefetches: u64,
    /// Hierarchical prefetches.
    pub hierarchical_prefetches: u64,
    /// Prefetch queue overflow count.
    pub queue_overflows: u64,
    /// Average prefetch batch size.
    pub avg_batch_size: f64,
}

/// Access pattern for learning.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct AccessPattern {
    /// Sequence of node IDs accessed.
    sequence: Vec<NodeId>,
}

impl AccessPattern {
    fn new(sequence: Vec<NodeId>) -> Self {
        Self { sequence }
    }

    /// Get the trigger node (first in sequence).
    fn trigger(&self) -> Option<&NodeId> {
        self.sequence.first()
    }

    /// Get nodes to prefetch (rest of sequence).
    fn prefetch_targets(&self) -> &[NodeId] {
        if self.sequence.len() > 1 {
            &self.sequence[1..]
        } else {
            &[]
        }
    }
}

/// Pattern frequency tracker.
#[derive(Debug)]
struct PatternTracker {
    /// Pattern frequencies.
    frequencies: HashMap<AccessPattern, usize>,
    /// Recent access history for pattern detection.
    history: VecDeque<NodeId>,
    /// Pattern window size.
    window_size: usize,
    /// History size limit.
    max_history: usize,
}

impl PatternTracker {
    fn new(window_size: usize, max_history: usize) -> Self {
        Self {
            frequencies: HashMap::new(),
            history: VecDeque::with_capacity(max_history),
            window_size,
            max_history,
        }
    }

    /// Record an access.
    fn record(&mut self, node_id: &NodeId) {
        self.history.push_back(node_id.clone());

        // Maintain history size
        while self.history.len() > self.max_history {
            self.history.pop_front();
        }

        // Extract patterns from recent history
        if self.history.len() >= self.window_size {
            let start = self.history.len() - self.window_size;
            let pattern_sequence: Vec<NodeId> = self.history.iter().skip(start).cloned().collect();

            let pattern = AccessPattern::new(pattern_sequence);
            *self.frequencies.entry(pattern).or_insert(0) += 1;
        }
    }

    /// Get prefetch candidates based on patterns.
    fn get_prefetch_candidates(&self, trigger: &NodeId, min_freq: usize) -> Vec<NodeId> {
        let mut candidates = Vec::new();

        for (pattern, freq) in &self.frequencies {
            if *freq >= min_freq {
                if pattern.trigger() == Some(trigger) {
                    candidates.extend(pattern.prefetch_targets().iter().cloned());
                }
            }
        }

        candidates
    }

    /// Clear old patterns.
    fn prune(&mut self, min_freq: usize) {
        self.frequencies.retain(|_, freq| *freq >= min_freq);
    }
}

/// Prefetch request.
#[derive(Debug)]
struct PrefetchRequest {
    /// Nodes to prefetch.
    node_ids: Vec<NodeId>,
    #[allow(dead_code)]
    /// Request priority (higher = more important).
    priority: u8,
    #[allow(dead_code)]
    /// Request timestamp.
    timestamp: Instant,
}

/// Node prefetcher for proactive cache population.
pub struct NodePrefetcher {
    config: PrefetchConfig,
    cache: Arc<NodeCache>,
    address_space: Arc<AddressSpace>,
    pattern_tracker: Mutex<PatternTracker>,
    pending_requests: Mutex<VecDeque<PrefetchRequest>>,
    #[allow(dead_code)]
    is_running: AtomicBool,
    stats: RwLock<PrefetchStats>,
    stats_nodes_prefetched: AtomicU64,
    stats_operations: AtomicU64,
}

impl NodePrefetcher {
    /// Create a new prefetcher.
    pub fn new(
        config: PrefetchConfig,
        cache: Arc<NodeCache>,
        address_space: Arc<AddressSpace>,
    ) -> Self {
        let pattern_history_size = config.pattern_history_size;

        Self {
            config,
            cache,
            address_space,
            pattern_tracker: Mutex::new(PatternTracker::new(3, pattern_history_size)),
            pending_requests: Mutex::new(VecDeque::new()),
            is_running: AtomicBool::new(false),
            stats: RwLock::new(PrefetchStats::default()),
            stats_nodes_prefetched: AtomicU64::new(0),
            stats_operations: AtomicU64::new(0),
        }
    }

    /// Record a node access and trigger prefetching.
    pub fn on_node_access(&self, node_id: &NodeId) {
        if !self.config.enabled {
            return;
        }

        trace!(node_id = %node_id, "Recording node access for prefetch");

        // Record for pattern learning
        if self.config.learn_patterns {
            let mut tracker = self.pattern_tracker.lock();
            tracker.record(node_id);
        }

        // Collect prefetch candidates
        let mut prefetch_nodes = Vec::new();

        // Pattern-based prefetching
        if self.config.learn_patterns {
            let tracker = self.pattern_tracker.lock();
            let pattern_candidates =
                tracker.get_prefetch_candidates(node_id, self.config.min_pattern_frequency);
            prefetch_nodes.extend(pattern_candidates);
        }

        // Hierarchical prefetching
        if self.config.prefetch_children {
            let children = self.get_children(node_id, self.config.max_depth);
            prefetch_nodes.extend(children);
        }

        if self.config.prefetch_siblings {
            let siblings = self.get_siblings(node_id);
            prefetch_nodes.extend(siblings);
        }

        // Filter out already cached nodes
        let to_prefetch: Vec<NodeId> = prefetch_nodes
            .into_iter()
            .filter(|id| !self.cache.contains(id))
            .take(self.config.max_prefetch_count)
            .collect();

        if !to_prefetch.is_empty() {
            self.enqueue_prefetch(to_prefetch, 1);
        }
    }

    /// Record a browse operation and prefetch related nodes.
    pub fn on_browse(&self, start_node: &NodeId, direction: BrowseDirection, results: &[NodeId]) {
        if !self.config.enabled {
            return;
        }

        trace!(
            node_id = %start_node,
            direction = ?direction,
            results_count = results.len(),
            "Recording browse for prefetch"
        );

        // Prefetch browse results that aren't cached
        let to_prefetch: Vec<NodeId> = results
            .iter()
            .filter(|id| !self.cache.contains(id))
            .cloned()
            .take(self.config.max_prefetch_count)
            .collect();

        if !to_prefetch.is_empty() {
            self.enqueue_prefetch(to_prefetch, 2); // Higher priority for browse results
        }
    }

    /// Explicitly request prefetching for specific nodes.
    pub fn prefetch(&self, node_ids: Vec<NodeId>) {
        if !self.config.enabled {
            return;
        }

        let to_prefetch: Vec<NodeId> = node_ids
            .into_iter()
            .filter(|id| !self.cache.contains(id))
            .collect();

        if !to_prefetch.is_empty() {
            self.enqueue_prefetch(to_prefetch, 3); // Highest priority for explicit requests
        }
    }

    /// Prefetch a subtree starting from a node.
    pub fn prefetch_subtree(&self, root_id: &NodeId, max_depth: usize) {
        if !self.config.enabled {
            return;
        }

        let nodes = self.collect_subtree(root_id, max_depth);
        let to_prefetch: Vec<NodeId> = nodes
            .into_iter()
            .filter(|id| !self.cache.contains(id))
            .collect();

        if !to_prefetch.is_empty() {
            self.enqueue_prefetch(to_prefetch, 2);
        }
    }

    /// Process pending prefetch requests.
    pub fn process_pending(&self) -> usize {
        if !self.config.enabled {
            return 0;
        }

        let requests: Vec<PrefetchRequest> = {
            let mut pending = self.pending_requests.lock();
            pending.drain(..).collect()
        };

        if requests.is_empty() {
            return 0;
        }

        // Merge and deduplicate all pending requests
        let mut all_node_ids: HashSet<NodeId> = HashSet::new();
        for request in requests {
            all_node_ids.extend(request.node_ids);
        }

        let node_ids: Vec<NodeId> = all_node_ids.into_iter().collect();
        let count = self.do_prefetch(&node_ids);

        self.stats_operations.fetch_add(1, Ordering::Relaxed);
        self.stats_nodes_prefetched
            .fetch_add(count as u64, Ordering::Relaxed);

        count
    }

    /// Get prefetch statistics.
    pub fn stats(&self) -> PrefetchStats {
        let mut stats = self.stats.read().clone();
        stats.nodes_prefetched = self.stats_nodes_prefetched.load(Ordering::Relaxed);
        stats.prefetch_operations = self.stats_operations.load(Ordering::Relaxed);

        if stats.prefetch_operations > 0 {
            stats.avg_batch_size = stats.nodes_prefetched as f64 / stats.prefetch_operations as f64;
        }

        stats
    }

    /// Reset statistics.
    pub fn reset_stats(&self) {
        self.stats_nodes_prefetched.store(0, Ordering::Relaxed);
        self.stats_operations.store(0, Ordering::Relaxed);
        *self.stats.write() = PrefetchStats::default();
    }

    /// Prune old patterns from the tracker.
    pub fn prune_patterns(&self) {
        let mut tracker = self.pattern_tracker.lock();
        tracker.prune(self.config.min_pattern_frequency);
    }

    // Private helpers

    fn enqueue_prefetch(&self, node_ids: Vec<NodeId>, priority: u8) {
        let mut pending = self.pending_requests.lock();

        // Check queue size limit
        if pending.len() >= self.config.queue_size {
            let mut stats = self.stats.write();
            stats.queue_overflows += 1;
            // Remove lowest priority request
            pending.pop_front();
        }

        pending.push_back(PrefetchRequest {
            node_ids,
            priority,
            timestamp: Instant::now(),
        });
    }

    fn do_prefetch(&self, node_ids: &[NodeId]) -> usize {
        let mut count = 0;

        for node_id in node_ids {
            // Skip if already cached
            if self.cache.contains(node_id) {
                continue;
            }

            // Get node from address space
            if let Some(shared_node) = self.address_space.get_node(node_id) {
                let node = shared_node.read();
                let cached_node = CachedNode {
                    node_id: node_id.clone(),
                    node_class: node.node_class(),
                    browse_name: node.browse_name().clone(),
                    display_name: node.display_name().clone(),
                    value: None, // Don't prefetch values by default
                    references: None,
                };
                drop(node);

                self.cache.put(cached_node);
                count += 1;
            }
        }

        if count > 0 {
            self.cache.record_prefetch(count);
            debug!(count, "Prefetched nodes");
        }

        count
    }

    fn get_children(&self, node_id: &NodeId, depth: usize) -> Vec<NodeId> {
        if depth == 0 {
            return Vec::new();
        }

        let mut children = Vec::new();

        // Get direct children (forward references are typically parent->child)
        let references = self
            .address_space
            .get_references(node_id, BrowseDirection::Forward);

        // Filter hierarchical references
        for reference in references {
            if reference.reference_type_id.is_hierarchical() {
                children.push(reference.target_node_id.clone());

                // Recursively get children
                if depth > 1 {
                    let grandchildren = self.get_children(&reference.target_node_id, depth - 1);
                    children.extend(grandchildren);
                }
            }
        }

        children
    }

    fn get_siblings(&self, node_id: &NodeId) -> Vec<NodeId> {
        let mut siblings = Vec::new();

        // Get parent (inverse hierarchical references)
        let parents = self
            .address_space
            .get_references(node_id, BrowseDirection::Inverse);

        for parent_ref in &parents {
            if parent_ref.reference_type_id.is_hierarchical() {
                // Get all children of parent (siblings)
                let parent_children = self
                    .address_space
                    .get_references(&parent_ref.target_node_id, BrowseDirection::Forward);

                for child_ref in parent_children {
                    if child_ref.reference_type_id.is_hierarchical()
                        && &child_ref.target_node_id != node_id
                    {
                        siblings.push(child_ref.target_node_id);
                    }
                }
                break; // Only use first parent
            }
        }

        siblings
    }

    fn collect_subtree(&self, root_id: &NodeId, max_depth: usize) -> Vec<NodeId> {
        let mut nodes = Vec::new();
        let mut visited = HashSet::new();
        let mut queue: VecDeque<(NodeId, usize)> = VecDeque::new();

        queue.push_back((root_id.clone(), 0));

        while let Some((node_id, depth)) = queue.pop_front() {
            if visited.contains(&node_id) || depth > max_depth {
                continue;
            }

            visited.insert(node_id.clone());
            nodes.push(node_id.clone());

            // Get children
            let references = self
                .address_space
                .get_references(&node_id, BrowseDirection::Forward);

            for reference in references {
                if reference.reference_type_id.is_hierarchical()
                    && !visited.contains(&reference.target_node_id)
                {
                    queue.push_back((reference.target_node_id, depth + 1));
                }
            }
        }

        nodes
    }
}

/// Async prefetch worker that processes requests in the background.
pub struct AsyncPrefetchWorker {
    prefetcher: Arc<NodePrefetcher>,
    shutdown: Arc<AtomicBool>,
}

impl AsyncPrefetchWorker {
    /// Create a new async worker.
    pub fn new(prefetcher: Arc<NodePrefetcher>) -> Self {
        Self {
            prefetcher,
            shutdown: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Start the worker.
    pub async fn run(&self) {
        let batch_interval = Duration::from_millis(self.prefetcher.config.batch_interval_ms);

        while !self.shutdown.load(Ordering::Relaxed) {
            let processed = self.prefetcher.process_pending();

            if processed == 0 {
                // No work, sleep longer
                tokio::time::sleep(batch_interval * 10).await;
            } else {
                tokio::time::sleep(batch_interval).await;
            }
        }
    }

    /// Signal the worker to shut down.
    pub fn shutdown(&self) {
        self.shutdown.store(true, Ordering::Relaxed);
    }
}

/// Helper to integrate prefetcher with AddressSpace operations.
pub struct PrefetchingAddressSpace {
    address_space: Arc<AddressSpace>,
    cache: Arc<NodeCache>,
    prefetcher: Arc<NodePrefetcher>,
}

impl PrefetchingAddressSpace {
    /// Create a new prefetching address space wrapper.
    pub fn new(
        address_space: Arc<AddressSpace>,
        cache: Arc<NodeCache>,
        config: PrefetchConfig,
    ) -> Self {
        let prefetcher = Arc::new(NodePrefetcher::new(
            config,
            cache.clone(),
            address_space.clone(),
        ));

        Self {
            address_space,
            cache,
            prefetcher,
        }
    }

    /// Get a node with caching and prefetching.
    /// Returns true if the node exists and was cached.
    pub fn get_node_cached(&self, node_id: &NodeId) -> bool {
        // Check cache first
        if self.cache.get(node_id).is_some() {
            self.prefetcher.on_node_access(node_id);
            return true;
        }

        // Cache miss - get from address space
        let Some(shared_node) = self.address_space.get_node(node_id) else {
            return false;
        };

        // Cache it
        let node = shared_node.read();
        let cached_node = CachedNode {
            node_id: node_id.clone(),
            node_class: node.node_class(),
            browse_name: node.browse_name().clone(),
            display_name: node.display_name().clone(),
            value: None,
            references: None,
        };
        drop(node);
        self.cache.put(cached_node);

        // Trigger prefetching
        self.prefetcher.on_node_access(node_id);

        true
    }

    /// Browse with caching and prefetching.
    pub fn browse(&self, node_id: &NodeId, direction: BrowseDirection) -> Vec<NodeId> {
        let references = self.address_space.get_references(node_id, direction);

        let result_ids: Vec<NodeId> = references
            .iter()
            .map(|r| r.target_node_id.clone())
            .collect();

        // Trigger prefetch for browse results
        self.prefetcher.on_browse(node_id, direction, &result_ids);

        result_ids
    }

    /// Get the prefetcher for manual control.
    pub fn prefetcher(&self) -> &Arc<NodePrefetcher> {
        &self.prefetcher
    }

    /// Process pending prefetch requests synchronously.
    pub fn process_prefetch(&self) -> usize {
        self.prefetcher.process_pending()
    }

    /// Get underlying address space.
    pub fn address_space(&self) -> &Arc<AddressSpace> {
        &self.address_space
    }

    /// Get cache.
    pub fn cache(&self) -> &Arc<NodeCache> {
        &self.cache
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nodes::cache::NodeCacheConfig;
    use crate::nodes::{
        AddressSpaceConfig, NodeBuilder, ObjectBuilder, Reference, VariableBuilder,
    };
    use crate::types::variant::DataTypeId;

    fn create_test_address_space() -> Arc<AddressSpace> {
        let address_space = AddressSpace::new(AddressSpaceConfig::default());

        // Create a hierarchy:
        // Root
        //   ├── Folder1
        //   │   ├── Var1
        //   │   └── Var2
        //   └── Folder2
        //       ├── Var3
        //       └── Var4

        let root_id = NodeId::numeric(0, 85);

        let folder1 = ObjectBuilder::new(NodeId::numeric(2, 1000))
            .browse_name(2, "Folder1")
            .display_name("Folder 1")
            .build();
        address_space.insert_node(folder1);
        address_space.add_reference(Reference::organizes(
            root_id.clone(),
            NodeId::numeric(2, 1000),
        ));

        let folder2 = ObjectBuilder::new(NodeId::numeric(2, 2000))
            .browse_name(2, "Folder2")
            .display_name("Folder 2")
            .build();
        address_space.insert_node(folder2);
        address_space.add_reference(Reference::organizes(
            root_id.clone(),
            NodeId::numeric(2, 2000),
        ));

        // Variables under Folder1
        for i in 1..=2 {
            let var = VariableBuilder::new(NodeId::numeric(2, 1000 + i))
                .browse_name(2, format!("Var{}", i))
                .display_name(format!("Variable {}", i))
                .data_type(DataTypeId::Double)
                .value(i as f64)
                .build();
            address_space.insert_node(var);
            address_space.add_reference(Reference::has_component(
                NodeId::numeric(2, 1000),
                NodeId::numeric(2, 1000 + i),
            ));
        }

        // Variables under Folder2
        for i in 3..=4 {
            let var = VariableBuilder::new(NodeId::numeric(2, 2000 + i))
                .browse_name(2, format!("Var{}", i))
                .display_name(format!("Variable {}", i))
                .data_type(DataTypeId::Double)
                .value(i as f64)
                .build();
            address_space.insert_node(var);
            address_space.add_reference(Reference::has_component(
                NodeId::numeric(2, 2000),
                NodeId::numeric(2, 2000 + i),
            ));
        }

        Arc::new(address_space)
    }

    #[test]
    fn test_prefetch_config() {
        let default = PrefetchConfig::default();
        assert!(default.enabled);
        assert_eq!(default.max_depth, 2);

        let aggressive = PrefetchConfig::aggressive();
        assert_eq!(aggressive.max_depth, 3);
        assert!(aggressive.prefetch_siblings);

        let conservative = PrefetchConfig::conservative();
        assert_eq!(conservative.max_depth, 1);
        assert!(!conservative.prefetch_siblings);

        let disabled = PrefetchConfig::disabled();
        assert!(!disabled.enabled);
    }

    #[test]
    fn test_pattern_tracker() {
        let mut tracker = PatternTracker::new(3, 100);

        // Record a pattern multiple times
        for _ in 0..5 {
            tracker.record(&NodeId::numeric(2, 1));
            tracker.record(&NodeId::numeric(2, 2));
            tracker.record(&NodeId::numeric(2, 3));
        }

        // Should detect pattern starting with node 1
        let candidates = tracker.get_prefetch_candidates(&NodeId::numeric(2, 1), 3);
        assert!(!candidates.is_empty());
    }

    #[test]
    fn test_prefetcher_creation() {
        let address_space = create_test_address_space();
        let cache = Arc::new(NodeCache::new(NodeCacheConfig::default()));
        let prefetcher =
            NodePrefetcher::new(PrefetchConfig::default(), cache.clone(), address_space);

        assert_eq!(prefetcher.stats().nodes_prefetched, 0);
    }

    #[test]
    fn test_explicit_prefetch() {
        let address_space = create_test_address_space();
        let cache = Arc::new(NodeCache::new(NodeCacheConfig::default()));
        let prefetcher =
            NodePrefetcher::new(PrefetchConfig::default(), cache.clone(), address_space);

        // Request prefetch for specific nodes
        prefetcher.prefetch(vec![NodeId::numeric(2, 1001), NodeId::numeric(2, 1002)]);

        // Process the request
        let processed = prefetcher.process_pending();
        assert_eq!(processed, 2);

        // Nodes should be cached
        assert!(cache.contains(&NodeId::numeric(2, 1001)));
        assert!(cache.contains(&NodeId::numeric(2, 1002)));
    }

    #[test]
    fn test_subtree_prefetch() {
        let address_space = create_test_address_space();
        let cache = Arc::new(NodeCache::new(NodeCacheConfig::default()));
        let prefetcher =
            NodePrefetcher::new(PrefetchConfig::default(), cache.clone(), address_space);

        // Prefetch subtree under Folder1
        prefetcher.prefetch_subtree(&NodeId::numeric(2, 1000), 2);
        let processed = prefetcher.process_pending();

        // Folder1, Var1, Var2 should be prefetched
        assert!(processed >= 3);
    }

    #[test]
    fn test_prefetching_address_space() {
        let address_space = create_test_address_space();
        let cache = Arc::new(NodeCache::new(NodeCacheConfig::default()));

        let prefetching_as =
            PrefetchingAddressSpace::new(address_space, cache.clone(), PrefetchConfig::default());

        // Access a node
        let exists = prefetching_as.get_node_cached(&NodeId::numeric(2, 1000));
        assert!(exists);

        // Node should be cached
        assert!(cache.contains(&NodeId::numeric(2, 1000)));

        // Process prefetch
        prefetching_as.process_prefetch();

        // Children should be prefetched (depending on config)
        // With default config, children should be prefetched
    }

    #[test]
    fn test_disabled_prefetch() {
        let address_space = create_test_address_space();
        let cache = Arc::new(NodeCache::new(NodeCacheConfig::default()));
        let prefetcher =
            NodePrefetcher::new(PrefetchConfig::disabled(), cache.clone(), address_space);

        // Request prefetch - should be ignored
        prefetcher.prefetch(vec![NodeId::numeric(2, 1001)]);

        let processed = prefetcher.process_pending();
        assert_eq!(processed, 0);
    }

    #[test]
    fn test_prefetch_stats() {
        let address_space = create_test_address_space();
        let cache = Arc::new(NodeCache::new(NodeCacheConfig::default()));
        let prefetcher =
            NodePrefetcher::new(PrefetchConfig::default(), cache.clone(), address_space);

        prefetcher.prefetch(vec![NodeId::numeric(2, 1001), NodeId::numeric(2, 1002)]);
        prefetcher.process_pending();

        let stats = prefetcher.stats();
        assert_eq!(stats.nodes_prefetched, 2);
        assert_eq!(stats.prefetch_operations, 1);
        assert_eq!(stats.avg_batch_size, 2.0);
    }
}

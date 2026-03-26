//! Memory profiling implementation.
//!
//! Provides detailed memory usage tracking with region-based accounting,
//! historical snapshots, and usage analysis.

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};

/// Memory profiler configuration.
#[derive(Debug, Clone)]
pub struct MemoryProfilerConfig {
    /// Interval between automatic snapshots.
    pub sampling_interval: Duration,

    /// Maximum number of historical snapshots to retain.
    pub max_snapshots: usize,

    /// Whether to track individual allocations.
    pub track_allocations: bool,

    /// Minimum allocation size to track individually (bytes).
    pub allocation_threshold: usize,
}

impl Default for MemoryProfilerConfig {
    fn default() -> Self {
        Self {
            sampling_interval: Duration::from_secs(10),
            max_snapshots: 1000,
            track_allocations: true,
            allocation_threshold: 1024,
        }
    }
}

/// Memory profiler for tracking allocations and usage patterns.
pub struct MemoryProfiler {
    config: MemoryProfilerConfig,

    /// Memory regions (e.g., "devices", "registers", "connections")
    regions: RwLock<HashMap<String, MemoryRegion>>,

    /// Historical snapshots
    snapshots: RwLock<VecDeque<TimestampedSnapshot>>,

    /// Global counters
    total_allocated: AtomicU64,
    total_deallocated: AtomicU64,
    current_bytes: AtomicU64,
    peak_bytes: AtomicU64,
    allocation_count: AtomicU64,
    deallocation_count: AtomicU64,

    /// Profiler state
    started_at: RwLock<Option<Instant>>,
    last_snapshot_at: RwLock<Option<Instant>>,
    is_running: std::sync::atomic::AtomicBool,
}

impl MemoryProfiler {
    /// Create a new memory profiler.
    pub fn new(config: MemoryProfilerConfig) -> Self {
        Self {
            config,
            regions: RwLock::new(HashMap::new()),
            snapshots: RwLock::new(VecDeque::new()),
            total_allocated: AtomicU64::new(0),
            total_deallocated: AtomicU64::new(0),
            current_bytes: AtomicU64::new(0),
            peak_bytes: AtomicU64::new(0),
            allocation_count: AtomicU64::new(0),
            deallocation_count: AtomicU64::new(0),
            started_at: RwLock::new(None),
            last_snapshot_at: RwLock::new(None),
            is_running: std::sync::atomic::AtomicBool::new(false),
        }
    }

    /// Start profiling.
    pub fn start(&mut self) {
        let now = Instant::now();
        *self.started_at.write() = Some(now);
        *self.last_snapshot_at.write() = Some(now);
        self.is_running.store(true, Ordering::SeqCst);
    }

    /// Stop profiling.
    pub fn stop(&mut self) {
        self.is_running.store(false, Ordering::SeqCst);
    }

    /// Check if profiling is active.
    pub fn is_running(&self) -> bool {
        self.is_running.load(Ordering::SeqCst)
    }

    /// Record a memory allocation.
    pub fn record_allocation(&self, label: &str, size: usize) {
        let size = size as u64;

        // Update global counters
        self.total_allocated.fetch_add(size, Ordering::Relaxed);
        self.allocation_count.fetch_add(1, Ordering::Relaxed);

        let new_current = self.current_bytes.fetch_add(size, Ordering::Relaxed) + size;

        // Update peak if necessary
        let mut peak = self.peak_bytes.load(Ordering::Relaxed);
        while new_current > peak {
            match self.peak_bytes.compare_exchange_weak(
                peak,
                new_current,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => break,
                Err(p) => peak = p,
            }
        }

        // Update region
        let mut regions = self.regions.write();
        regions
            .entry(label.to_string())
            .or_insert_with(|| MemoryRegion::new(label))
            .record_allocation(size);

        // Check if we need a new snapshot
        drop(regions);
        self.maybe_take_snapshot();
    }

    /// Record a memory deallocation.
    pub fn record_deallocation(&self, label: &str, size: usize) {
        let size = size as u64;

        // Update global counters
        self.total_deallocated.fetch_add(size, Ordering::Relaxed);
        self.deallocation_count.fetch_add(1, Ordering::Relaxed);
        self.current_bytes.fetch_sub(size, Ordering::Relaxed);

        // Update region
        let mut regions = self.regions.write();
        if let Some(region) = regions.get_mut(label) {
            region.record_deallocation(size);
        }
    }

    /// Take a snapshot if enough time has passed.
    fn maybe_take_snapshot(&self) {
        let mut last_snapshot = self.last_snapshot_at.write();
        if let Some(last) = *last_snapshot {
            if last.elapsed() >= self.config.sampling_interval {
                *last_snapshot = Some(Instant::now());
                drop(last_snapshot);

                let snapshot = TimestampedSnapshot {
                    timestamp: chrono::Utc::now(),
                    snapshot: self.snapshot(),
                };

                let mut snapshots = self.snapshots.write();
                snapshots.push_back(snapshot);

                // Maintain max snapshots limit
                while snapshots.len() > self.config.max_snapshots {
                    snapshots.pop_front();
                }
            }
        }
    }

    /// Get current memory snapshot.
    pub fn snapshot(&self) -> MemorySnapshot {
        let regions = self.regions.read();
        let region_snapshots: HashMap<String, RegionSnapshot> = regions
            .iter()
            .map(|(name, region)| (name.clone(), region.snapshot()))
            .collect();

        MemorySnapshot {
            current_bytes: self.current_bytes.load(Ordering::Relaxed),
            peak_bytes: self.peak_bytes.load(Ordering::Relaxed),
            total_allocated: self.total_allocated.load(Ordering::Relaxed),
            total_deallocated: self.total_deallocated.load(Ordering::Relaxed),
            allocation_count: self.allocation_count.load(Ordering::Relaxed),
            deallocation_count: self.deallocation_count.load(Ordering::Relaxed),
            regions: region_snapshots,
        }
    }

    /// Get historical snapshots.
    pub fn history(&self) -> Vec<TimestampedSnapshot> {
        self.snapshots.read().iter().cloned().collect()
    }

    /// Generate a memory report.
    pub fn generate_report(&self) -> MemoryReport {
        let snapshot = self.snapshot();
        let history = self.history();

        // Calculate growth rate if we have history
        let growth_rate = if history.len() >= 2 {
            let first = &history[0];
            let last = &history[history.len() - 1];
            let time_diff = (last.timestamp - first.timestamp).num_seconds() as f64;
            if time_diff > 0.0 {
                let byte_diff =
                    last.snapshot.current_bytes as f64 - first.snapshot.current_bytes as f64;
                Some(byte_diff / time_diff) // bytes per second
            } else {
                None
            }
        } else {
            None
        };

        MemoryReport {
            current_bytes: snapshot.current_bytes,
            peak_bytes: snapshot.peak_bytes,
            total_allocated: snapshot.total_allocated,
            total_deallocated: snapshot.total_deallocated,
            allocation_count: snapshot.allocation_count,
            deallocation_count: snapshot.deallocation_count,
            regions: snapshot.regions,
            growth_rate_bytes_per_sec: growth_rate,
            snapshot_count: history.len(),
        }
    }

    /// Reset all profiling data.
    pub fn reset(&mut self) {
        self.regions.write().clear();
        self.snapshots.write().clear();
        self.total_allocated.store(0, Ordering::Relaxed);
        self.total_deallocated.store(0, Ordering::Relaxed);
        self.current_bytes.store(0, Ordering::Relaxed);
        self.peak_bytes.store(0, Ordering::Relaxed);
        self.allocation_count.store(0, Ordering::Relaxed);
        self.deallocation_count.store(0, Ordering::Relaxed);
    }

    /// Get memory usage for a specific region.
    pub fn region_usage(&self, label: &str) -> Option<RegionSnapshot> {
        self.regions.read().get(label).map(|r| r.snapshot())
    }

    /// List all tracked regions.
    pub fn regions(&self) -> Vec<String> {
        self.regions.read().keys().cloned().collect()
    }
}

/// A memory region for tracking allocations by category.
#[derive(Debug)]
pub struct MemoryRegion {
    name: String,
    current_bytes: AtomicU64,
    peak_bytes: AtomicU64,
    total_allocated: AtomicU64,
    total_deallocated: AtomicU64,
    allocation_count: AtomicU64,
    deallocation_count: AtomicU64,
}

impl MemoryRegion {
    /// Create a new memory region.
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            current_bytes: AtomicU64::new(0),
            peak_bytes: AtomicU64::new(0),
            total_allocated: AtomicU64::new(0),
            total_deallocated: AtomicU64::new(0),
            allocation_count: AtomicU64::new(0),
            deallocation_count: AtomicU64::new(0),
        }
    }

    /// Record an allocation.
    pub fn record_allocation(&self, size: u64) {
        self.total_allocated.fetch_add(size, Ordering::Relaxed);
        self.allocation_count.fetch_add(1, Ordering::Relaxed);

        let new_current = self.current_bytes.fetch_add(size, Ordering::Relaxed) + size;

        // Update peak
        let mut peak = self.peak_bytes.load(Ordering::Relaxed);
        while new_current > peak {
            match self.peak_bytes.compare_exchange_weak(
                peak,
                new_current,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => break,
                Err(p) => peak = p,
            }
        }
    }

    /// Record a deallocation.
    pub fn record_deallocation(&self, size: u64) {
        self.total_deallocated.fetch_add(size, Ordering::Relaxed);
        self.deallocation_count.fetch_add(1, Ordering::Relaxed);
        self.current_bytes.fetch_sub(size, Ordering::Relaxed);
    }

    /// Get a snapshot of this region.
    pub fn snapshot(&self) -> RegionSnapshot {
        RegionSnapshot {
            name: self.name.clone(),
            current_bytes: self.current_bytes.load(Ordering::Relaxed),
            peak_bytes: self.peak_bytes.load(Ordering::Relaxed),
            total_allocated: self.total_allocated.load(Ordering::Relaxed),
            total_deallocated: self.total_deallocated.load(Ordering::Relaxed),
            allocation_count: self.allocation_count.load(Ordering::Relaxed),
            deallocation_count: self.deallocation_count.load(Ordering::Relaxed),
        }
    }
}

/// Snapshot of memory state at a point in time.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemorySnapshot {
    /// Current memory usage in bytes.
    pub current_bytes: u64,

    /// Peak memory usage in bytes.
    pub peak_bytes: u64,

    /// Total bytes allocated since start.
    pub total_allocated: u64,

    /// Total bytes deallocated since start.
    pub total_deallocated: u64,

    /// Number of allocations.
    pub allocation_count: u64,

    /// Number of deallocations.
    pub deallocation_count: u64,

    /// Per-region snapshots.
    pub regions: HashMap<String, RegionSnapshot>,
}

/// Timestamped memory snapshot.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimestampedSnapshot {
    /// When the snapshot was taken.
    pub timestamp: chrono::DateTime<chrono::Utc>,

    /// The memory snapshot.
    pub snapshot: MemorySnapshot,
}

/// Snapshot of a memory region.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegionSnapshot {
    /// Region name.
    pub name: String,

    /// Current memory usage in bytes.
    pub current_bytes: u64,

    /// Peak memory usage in bytes.
    pub peak_bytes: u64,

    /// Total bytes allocated.
    pub total_allocated: u64,

    /// Total bytes deallocated.
    pub total_deallocated: u64,

    /// Number of allocations.
    pub allocation_count: u64,

    /// Number of deallocations.
    pub deallocation_count: u64,
}

impl RegionSnapshot {
    /// Calculate fragmentation ratio (allocations / deallocations).
    pub fn fragmentation_ratio(&self) -> f64 {
        if self.deallocation_count == 0 {
            return 0.0;
        }
        self.allocation_count as f64 / self.deallocation_count as f64
    }

    /// Calculate average allocation size.
    pub fn average_allocation_size(&self) -> u64 {
        if self.allocation_count == 0 {
            return 0;
        }
        self.total_allocated / self.allocation_count
    }
}

/// Memory profiling report.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryReport {
    /// Current memory usage in bytes.
    pub current_bytes: u64,

    /// Peak memory usage in bytes.
    pub peak_bytes: u64,

    /// Total bytes allocated since profiling started.
    pub total_allocated: u64,

    /// Total bytes deallocated since profiling started.
    pub total_deallocated: u64,

    /// Number of allocations.
    pub allocation_count: u64,

    /// Number of deallocations.
    pub deallocation_count: u64,

    /// Per-region data.
    pub regions: HashMap<String, RegionSnapshot>,

    /// Memory growth rate (bytes per second), if calculable.
    pub growth_rate_bytes_per_sec: Option<f64>,

    /// Number of snapshots taken.
    pub snapshot_count: usize,
}

impl MemoryReport {
    /// Check if memory usage appears stable (no significant growth).
    pub fn is_stable(&self, threshold_bytes_per_sec: f64) -> bool {
        match self.growth_rate_bytes_per_sec {
            Some(rate) => rate.abs() < threshold_bytes_per_sec,
            None => true, // Not enough data to determine
        }
    }

    /// Get the largest region by current usage.
    pub fn largest_region(&self) -> Option<(&String, &RegionSnapshot)> {
        self.regions.iter().max_by_key(|(_, r)| r.current_bytes)
    }

    /// Calculate memory efficiency (deallocated / allocated).
    pub fn efficiency(&self) -> f64 {
        if self.total_allocated == 0 {
            return 1.0;
        }
        self.total_deallocated as f64 / self.total_allocated as f64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_memory_profiler_basic() {
        let mut profiler = MemoryProfiler::new(MemoryProfilerConfig::default());
        profiler.start();

        profiler.record_allocation("test", 1024);
        assert_eq!(profiler.snapshot().current_bytes, 1024);

        profiler.record_allocation("test", 2048);
        assert_eq!(profiler.snapshot().current_bytes, 3072);

        profiler.record_deallocation("test", 1024);
        assert_eq!(profiler.snapshot().current_bytes, 2048);
    }

    #[test]
    fn test_memory_profiler_peak() {
        let mut profiler = MemoryProfiler::new(MemoryProfilerConfig::default());
        profiler.start();

        profiler.record_allocation("test", 1000);
        profiler.record_allocation("test", 2000);
        profiler.record_deallocation("test", 1500);

        let snapshot = profiler.snapshot();
        assert_eq!(snapshot.current_bytes, 1500);
        assert_eq!(snapshot.peak_bytes, 3000);
    }

    #[test]
    fn test_memory_profiler_regions() {
        let mut profiler = MemoryProfiler::new(MemoryProfilerConfig::default());
        profiler.start();

        profiler.record_allocation("devices", 1000);
        profiler.record_allocation("registers", 2000);
        profiler.record_allocation("devices", 500);

        let regions = profiler.regions();
        assert!(regions.contains(&"devices".to_string()));
        assert!(regions.contains(&"registers".to_string()));

        let device_region = profiler.region_usage("devices").unwrap();
        assert_eq!(device_region.current_bytes, 1500);
        assert_eq!(device_region.allocation_count, 2);
    }

    #[test]
    fn test_memory_profiler_reset() {
        let mut profiler = MemoryProfiler::new(MemoryProfilerConfig::default());
        profiler.start();

        profiler.record_allocation("test", 1024);
        profiler.reset();

        let snapshot = profiler.snapshot();
        assert_eq!(snapshot.current_bytes, 0);
        assert_eq!(snapshot.allocation_count, 0);
    }

    #[test]
    fn test_memory_report() {
        let mut profiler = MemoryProfiler::new(MemoryProfilerConfig::default());
        profiler.start();

        profiler.record_allocation("a", 1000);
        profiler.record_allocation("b", 2000);
        profiler.record_deallocation("a", 500);

        let report = profiler.generate_report();
        assert_eq!(report.current_bytes, 2500);
        assert_eq!(report.allocation_count, 2);
        assert_eq!(report.deallocation_count, 1);

        let largest = report.largest_region();
        assert!(largest.is_some());
        assert_eq!(largest.unwrap().0, "b");
    }

    #[test]
    fn test_region_snapshot_metrics() {
        let region = MemoryRegion::new("test");
        region.record_allocation(100);
        region.record_allocation(200);
        region.record_deallocation(50);

        let snapshot = region.snapshot();
        assert_eq!(snapshot.average_allocation_size(), 150); // (100+200)/2
        assert_eq!(snapshot.fragmentation_ratio(), 2.0); // 2 allocs / 1 dealloc
    }
}

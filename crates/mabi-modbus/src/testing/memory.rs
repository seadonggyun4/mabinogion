//! Memory profiling utilities for detecting leaks and measuring usage.
//!
//! This module provides tools for:
//! - Tracking memory allocations
//! - Detecting memory leaks
//! - Profiling memory usage over time
//! - Generating memory reports

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::RwLock;

/// Memory snapshot at a point in time.
#[derive(Debug, Clone)]
pub struct MemorySnapshot {
    /// Timestamp when snapshot was taken.
    pub timestamp: Instant,
    /// Total allocated bytes.
    pub allocated_bytes: u64,
    /// Total number of allocations.
    pub allocation_count: u64,
    /// Total freed bytes.
    pub freed_bytes: u64,
    /// Number of deallocations.
    pub deallocation_count: u64,
    /// Current live allocations (allocated - freed).
    pub live_bytes: u64,
    /// Current live allocation count.
    pub live_count: u64,
    /// Peak memory usage.
    pub peak_bytes: u64,
    /// Resident set size (from OS).
    pub rss_bytes: Option<u64>,
    /// Heap usage (from allocator if available).
    pub heap_bytes: Option<u64>,
}

impl MemorySnapshot {
    /// Get memory usage in megabytes.
    pub fn live_mb(&self) -> f64 {
        self.live_bytes as f64 / (1024.0 * 1024.0)
    }

    /// Get peak memory in megabytes.
    pub fn peak_mb(&self) -> f64 {
        self.peak_bytes as f64 / (1024.0 * 1024.0)
    }

    /// Get RSS in megabytes.
    pub fn rss_mb(&self) -> Option<f64> {
        self.rss_bytes.map(|b| b as f64 / (1024.0 * 1024.0))
    }
}

/// Memory profiler for tracking allocations.
pub struct MemoryProfiler {
    /// Start time of profiling.
    start_time: Instant,
    /// Collected snapshots.
    snapshots: Arc<RwLock<Vec<MemorySnapshot>>>,
    /// Allocation tracker.
    tracker: Arc<AllocationTracker>,
    /// Sampling interval.
    sample_interval: Duration,
    /// Whether profiling is active.
    active: Arc<AtomicBool>,
}

use std::sync::atomic::AtomicBool;

impl MemoryProfiler {
    /// Create a new memory profiler.
    pub fn new() -> Self {
        Self {
            start_time: Instant::now(),
            snapshots: Arc::new(RwLock::new(Vec::new())),
            tracker: Arc::new(AllocationTracker::new()),
            sample_interval: Duration::from_secs(1),
            active: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Set the sampling interval.
    pub fn with_sample_interval(mut self, interval: Duration) -> Self {
        self.sample_interval = interval;
        self
    }

    /// Start profiling with background sampling.
    pub fn start(&self) -> MemoryProfilerGuard<'_> {
        self.active.store(true, Ordering::SeqCst);

        let snapshots = self.snapshots.clone();
        let tracker = self.tracker.clone();
        let interval = self.sample_interval;
        let active = self.active.clone();

        // Spawn background sampler
        tokio::spawn(async move {
            while active.load(Ordering::Relaxed) {
                let snapshot = Self::take_snapshot_internal(&tracker);
                snapshots.write().push(snapshot);
                tokio::time::sleep(interval).await;
            }
        });

        MemoryProfilerGuard { profiler: self }
    }

    /// Take a memory snapshot.
    pub fn snapshot(&self) -> MemorySnapshot {
        Self::take_snapshot_internal(&self.tracker)
    }

    fn take_snapshot_internal(tracker: &AllocationTracker) -> MemorySnapshot {
        let allocated = tracker.allocated_bytes.load(Ordering::Relaxed);
        let alloc_count = tracker.allocation_count.load(Ordering::Relaxed);
        let freed = tracker.freed_bytes.load(Ordering::Relaxed);
        let free_count = tracker.deallocation_count.load(Ordering::Relaxed);
        let peak = tracker.peak_bytes.load(Ordering::Relaxed);

        let live_bytes = allocated.saturating_sub(freed);
        let live_count = alloc_count.saturating_sub(free_count);

        MemorySnapshot {
            timestamp: Instant::now(),
            allocated_bytes: allocated,
            allocation_count: alloc_count,
            freed_bytes: freed,
            deallocation_count: free_count,
            live_bytes,
            live_count,
            peak_bytes: peak,
            rss_bytes: Self::get_rss(),
            heap_bytes: None,
        }
    }

    /// Get resident set size from OS.
    #[cfg(target_os = "linux")]
    fn get_rss() -> Option<u64> {
        std::fs::read_to_string("/proc/self/statm")
            .ok()
            .and_then(|s| {
                let parts: Vec<&str> = s.split_whitespace().collect();
                parts.get(1)?.parse::<u64>().ok().map(|pages| pages * 4096)
            })
    }

    #[cfg(target_os = "macos")]
    fn get_rss() -> Option<u64> {
        use std::process::Command;

        let output = Command::new("ps")
            .args(["-o", "rss=", "-p", &std::process::id().to_string()])
            .output()
            .ok()?;

        let rss_kb: u64 = String::from_utf8_lossy(&output.stdout)
            .trim()
            .parse()
            .ok()?;

        Some(rss_kb * 1024)
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    fn get_rss() -> Option<u64> {
        None
    }

    /// Generate a memory report.
    pub fn report(&self) -> MemoryReport {
        let snapshots = self.snapshots.read().clone();
        let current = self.snapshot();

        let duration = self.start_time.elapsed();

        // Calculate statistics
        let peak_bytes = snapshots
            .iter()
            .map(|s| s.live_bytes)
            .max()
            .unwrap_or(current.live_bytes);

        let avg_bytes = if !snapshots.is_empty() {
            snapshots.iter().map(|s| s.live_bytes).sum::<u64>() / snapshots.len() as u64
        } else {
            current.live_bytes
        };

        // Detect potential leaks (monotonically increasing memory)
        let potential_leak = if snapshots.len() >= 10 {
            let recent: Vec<_> = snapshots.iter().rev().take(10).collect();
            let increasing = recent
                .windows(2)
                .all(|w| w[0].live_bytes >= w[1].live_bytes);
            let growth = recent
                .first()
                .map(|f| f.live_bytes)
                .unwrap_or(0)
                .saturating_sub(recent.last().map(|l| l.live_bytes).unwrap_or(0));
            increasing && growth > 1024 * 1024 // More than 1MB growth
        } else {
            false
        };

        MemoryReport {
            duration,
            current_snapshot: current,
            peak_bytes,
            avg_bytes,
            snapshots,
            potential_leak,
            allocation_rate: self.tracker.allocation_count.load(Ordering::Relaxed) as f64
                / duration.as_secs_f64(),
        }
    }

    /// Get the allocation tracker for custom tracking.
    pub fn tracker(&self) -> &Arc<AllocationTracker> {
        &self.tracker
    }
}

impl Default for MemoryProfiler {
    fn default() -> Self {
        Self::new()
    }
}

/// Guard that stops profiling when dropped.
pub struct MemoryProfilerGuard<'a> {
    profiler: &'a MemoryProfiler,
}

impl<'a> Drop for MemoryProfilerGuard<'a> {
    fn drop(&mut self) {
        self.profiler.active.store(false, Ordering::SeqCst);
    }
}

/// Allocation tracker for monitoring memory usage.
pub struct AllocationTracker {
    pub allocated_bytes: AtomicU64,
    pub freed_bytes: AtomicU64,
    pub allocation_count: AtomicU64,
    pub deallocation_count: AtomicU64,
    pub peak_bytes: AtomicU64,
    current_bytes: AtomicU64,
}

impl AllocationTracker {
    /// Create a new allocation tracker.
    pub fn new() -> Self {
        Self {
            allocated_bytes: AtomicU64::new(0),
            freed_bytes: AtomicU64::new(0),
            allocation_count: AtomicU64::new(0),
            deallocation_count: AtomicU64::new(0),
            peak_bytes: AtomicU64::new(0),
            current_bytes: AtomicU64::new(0),
        }
    }

    /// Record an allocation.
    pub fn record_alloc(&self, size: usize) {
        self.allocated_bytes
            .fetch_add(size as u64, Ordering::Relaxed);
        self.allocation_count.fetch_add(1, Ordering::Relaxed);

        let current = self.current_bytes.fetch_add(size as u64, Ordering::Relaxed) + size as u64;

        // Update peak if necessary
        let mut peak = self.peak_bytes.load(Ordering::Relaxed);
        while current > peak {
            match self.peak_bytes.compare_exchange_weak(
                peak,
                current,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => break,
                Err(p) => peak = p,
            }
        }
    }

    /// Record a deallocation.
    pub fn record_dealloc(&self, size: usize) {
        self.freed_bytes.fetch_add(size as u64, Ordering::Relaxed);
        self.deallocation_count.fetch_add(1, Ordering::Relaxed);
        self.current_bytes.fetch_sub(size as u64, Ordering::Relaxed);
    }

    /// Get current live bytes.
    pub fn live_bytes(&self) -> u64 {
        self.current_bytes.load(Ordering::Relaxed)
    }

    /// Get current live allocation count.
    pub fn live_count(&self) -> u64 {
        self.allocation_count
            .load(Ordering::Relaxed)
            .saturating_sub(self.deallocation_count.load(Ordering::Relaxed))
    }

    /// Reset all counters.
    pub fn reset(&self) {
        self.allocated_bytes.store(0, Ordering::Relaxed);
        self.freed_bytes.store(0, Ordering::Relaxed);
        self.allocation_count.store(0, Ordering::Relaxed);
        self.deallocation_count.store(0, Ordering::Relaxed);
        self.peak_bytes.store(0, Ordering::Relaxed);
        self.current_bytes.store(0, Ordering::Relaxed);
    }
}

impl Default for AllocationTracker {
    fn default() -> Self {
        Self::new()
    }
}

/// Memory profiling report.
#[derive(Debug, Clone)]
pub struct MemoryReport {
    /// Total profiling duration.
    pub duration: Duration,
    /// Current memory snapshot.
    pub current_snapshot: MemorySnapshot,
    /// Peak memory usage in bytes.
    pub peak_bytes: u64,
    /// Average memory usage in bytes.
    pub avg_bytes: u64,
    /// All collected snapshots.
    pub snapshots: Vec<MemorySnapshot>,
    /// Whether a potential memory leak was detected.
    pub potential_leak: bool,
    /// Allocation rate (allocations per second).
    pub allocation_rate: f64,
}

impl MemoryReport {
    /// Get peak memory in megabytes.
    pub fn peak_mb(&self) -> f64 {
        self.peak_bytes as f64 / (1024.0 * 1024.0)
    }

    /// Get average memory in megabytes.
    pub fn avg_mb(&self) -> f64 {
        self.avg_bytes as f64 / (1024.0 * 1024.0)
    }

    /// Get current memory in megabytes.
    pub fn current_mb(&self) -> f64 {
        self.current_snapshot.live_mb()
    }

    /// Format as a human-readable string.
    pub fn format(&self) -> String {
        let mut output = String::new();

        output.push_str("=== Memory Profiling Report ===\n\n");
        output.push_str(&format!("Duration: {:?}\n", self.duration));
        output.push_str(&format!("Current Memory: {:.2} MB\n", self.current_mb()));
        output.push_str(&format!("Peak Memory: {:.2} MB\n", self.peak_mb()));
        output.push_str(&format!("Average Memory: {:.2} MB\n", self.avg_mb()));
        output.push_str(&format!(
            "Allocation Rate: {:.2}/sec\n",
            self.allocation_rate
        ));
        output.push_str(&format!(
            "Total Allocations: {}\n",
            self.current_snapshot.allocation_count
        ));
        output.push_str(&format!(
            "Live Allocations: {}\n",
            self.current_snapshot.live_count
        ));

        if let Some(rss) = self.current_snapshot.rss_mb() {
            output.push_str(&format!("RSS: {:.2} MB\n", rss));
        }

        if self.potential_leak {
            output.push_str("\n⚠️  POTENTIAL MEMORY LEAK DETECTED!\n");
            output.push_str("Memory usage has been monotonically increasing.\n");
        }

        output
    }

    /// Check if memory usage is within acceptable limits.
    pub fn check_limits(&self, max_mb: f64) -> bool {
        self.peak_mb() <= max_mb
    }
}

/// Estimate memory usage for a given number of devices and points.
pub fn estimate_memory_usage(devices: usize, points_per_device: usize) -> MemoryEstimate {
    // Estimated sizes based on actual struct layouts
    const DEVICE_OVERHEAD: usize = 512; // DeviceInfo, state, etc.
    const POINT_SIZE: usize = 128; // DataPoint definition
    const VALUE_SIZE: usize = 32; // Stored value
    const REGISTER_ENTRY: usize = 24; // HashMap/DashMap entry overhead

    let device_memory = devices * DEVICE_OVERHEAD;
    let point_memory = devices * points_per_device * POINT_SIZE;
    let value_memory = devices * points_per_device * VALUE_SIZE;
    let overhead = devices * points_per_device * REGISTER_ENTRY;

    let total = device_memory + point_memory + value_memory + overhead;

    MemoryEstimate {
        devices,
        points_per_device,
        device_memory_bytes: device_memory,
        point_memory_bytes: point_memory,
        value_memory_bytes: value_memory,
        overhead_bytes: overhead,
        total_bytes: total,
    }
}

/// Memory usage estimate.
#[derive(Debug, Clone)]
pub struct MemoryEstimate {
    pub devices: usize,
    pub points_per_device: usize,
    pub device_memory_bytes: usize,
    pub point_memory_bytes: usize,
    pub value_memory_bytes: usize,
    pub overhead_bytes: usize,
    pub total_bytes: usize,
}

impl MemoryEstimate {
    /// Get total memory in megabytes.
    pub fn total_mb(&self) -> f64 {
        self.total_bytes as f64 / (1024.0 * 1024.0)
    }

    /// Get total memory in gigabytes.
    pub fn total_gb(&self) -> f64 {
        self.total_bytes as f64 / (1024.0 * 1024.0 * 1024.0)
    }

    /// Format as human-readable string.
    pub fn format(&self) -> String {
        format!(
            "Memory Estimate for {} devices with {} points each:\n\
             - Device overhead: {:.2} MB\n\
             - Point definitions: {:.2} MB\n\
             - Stored values: {:.2} MB\n\
             - Data structure overhead: {:.2} MB\n\
             - Total: {:.2} MB ({:.2} GB)",
            self.devices,
            self.points_per_device,
            self.device_memory_bytes as f64 / (1024.0 * 1024.0),
            self.point_memory_bytes as f64 / (1024.0 * 1024.0),
            self.value_memory_bytes as f64 / (1024.0 * 1024.0),
            self.overhead_bytes as f64 / (1024.0 * 1024.0),
            self.total_mb(),
            self.total_gb(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_allocation_tracker() {
        let tracker = AllocationTracker::new();

        tracker.record_alloc(1000);
        assert_eq!(tracker.live_bytes(), 1000);
        assert_eq!(tracker.live_count(), 1);

        tracker.record_alloc(500);
        assert_eq!(tracker.live_bytes(), 1500);
        assert_eq!(tracker.live_count(), 2);

        tracker.record_dealloc(1000);
        assert_eq!(tracker.live_bytes(), 500);
        assert_eq!(tracker.live_count(), 1);
    }

    #[test]
    fn test_memory_estimate() {
        let estimate = estimate_memory_usage(10_000, 100);
        assert!(estimate.total_mb() > 0.0);
        println!("{}", estimate.format());

        let large_estimate = estimate_memory_usage(50_000, 100);
        assert!(large_estimate.total_gb() < 8.0, "Should fit in 8GB limit");
    }

    #[test]
    fn test_memory_snapshot() {
        let snapshot = MemorySnapshot {
            timestamp: Instant::now(),
            allocated_bytes: 100 * 1024 * 1024,
            allocation_count: 10000,
            freed_bytes: 50 * 1024 * 1024,
            deallocation_count: 5000,
            live_bytes: 50 * 1024 * 1024,
            live_count: 5000,
            peak_bytes: 75 * 1024 * 1024,
            rss_bytes: Some(80 * 1024 * 1024),
            heap_bytes: None,
        };

        assert!((snapshot.live_mb() - 50.0).abs() < 0.01);
        assert!((snapshot.peak_mb() - 75.0).abs() < 0.01);
    }
}

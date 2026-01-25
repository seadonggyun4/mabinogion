//! Performance profiler for monitoring and optimization.
//!
//! This module provides comprehensive performance monitoring:
//!
//! - **Latency tracking**: Histogram-based latency measurement
//! - **Throughput measurement**: Requests per second tracking
//! - **Resource monitoring**: Memory and CPU usage
//! - **Report generation**: Periodic performance reports
//!
//! ## Architecture
//!
//! ```text
//! ┌──────────────────────────────────────────────────────────────────────┐
//! │                      PerformanceProfiler                              │
//! │  ┌────────────────────────────────────────────────────────────────┐  │
//! │  │                    Latency Histogram                           │  │
//! │  │  [50µs][100µs][250µs][500µs][1ms][2.5ms][5ms][10ms]...        │  │
//! │  │    ↓      ↓      ↓      ↓     ↓     ↓     ↓    ↓              │  │
//! │  │  count  count  count  count count count count count            │  │
//! │  └────────────────────────────────────────────────────────────────┘  │
//! │                              │                                        │
//! │  ┌────────────────────────────────────────────────────────────────┐  │
//! │  │                  Throughput Counter                            │  │
//! │  │  Window: [t-59s][t-58s]...[t-1s][t-0s] → sum / window_size    │  │
//! │  └────────────────────────────────────────────────────────────────┘  │
//! │                              │                                        │
//! │  ┌────────────────────────────────────────────────────────────────┐  │
//! │  │                   Resource Monitor                             │  │
//! │  │  Memory: current/peak │ CPU: utilization │ Connections: active│  │
//! │  └────────────────────────────────────────────────────────────────┘  │
//! └──────────────────────────────────────────────────────────────────────┘
//! ```

use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use parking_lot::RwLock;

use super::config::ProfilerConfig;

/// Latency histogram for tracking response time distribution.
#[derive(Debug)]
pub struct LatencyHistogram {
    /// Bucket boundaries (in microseconds).
    buckets: Vec<u64>,
    /// Count per bucket.
    counts: Vec<AtomicU64>,
    /// Total count.
    total_count: AtomicU64,
    /// Sum of all latencies (for average calculation).
    total_sum: AtomicU64,
    /// Minimum latency seen.
    min: AtomicU64,
    /// Maximum latency seen.
    max: AtomicU64,
}

impl LatencyHistogram {
    /// Create a new histogram with the given bucket boundaries (in microseconds).
    pub fn new(buckets: Vec<u64>) -> Self {
        let counts = (0..=buckets.len())
            .map(|_| AtomicU64::new(0))
            .collect();

        Self {
            buckets,
            counts,
            total_count: AtomicU64::new(0),
            total_sum: AtomicU64::new(0),
            min: AtomicU64::new(u64::MAX),
            max: AtomicU64::new(0),
        }
    }

    /// Create with default latency buckets.
    pub fn with_defaults() -> Self {
        Self::new(vec![
            50,      // 50µs
            100,     // 100µs
            250,     // 250µs
            500,     // 500µs
            1_000,   // 1ms
            2_500,   // 2.5ms
            5_000,   // 5ms
            10_000,  // 10ms
            25_000,  // 25ms
            50_000,  // 50ms
            100_000, // 100ms
        ])
    }

    /// Record a latency value (in microseconds).
    pub fn record(&self, latency_us: u64) {
        // Find bucket
        let bucket_index = self.buckets
            .iter()
            .position(|&b| latency_us <= b)
            .unwrap_or(self.buckets.len());

        // Update bucket count
        self.counts[bucket_index].fetch_add(1, Ordering::Relaxed);

        // Update totals
        self.total_count.fetch_add(1, Ordering::Relaxed);
        self.total_sum.fetch_add(latency_us, Ordering::Relaxed);

        // Update min/max
        loop {
            let current_min = self.min.load(Ordering::Relaxed);
            if latency_us >= current_min {
                break;
            }
            if self.min.compare_exchange_weak(
                current_min,
                latency_us,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ).is_ok() {
                break;
            }
        }

        loop {
            let current_max = self.max.load(Ordering::Relaxed);
            if latency_us <= current_max {
                break;
            }
            if self.max.compare_exchange_weak(
                current_max,
                latency_us,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ).is_ok() {
                break;
            }
        }
    }

    /// Record a latency from a Duration.
    pub fn record_duration(&self, duration: Duration) {
        self.record(duration.as_micros() as u64);
    }

    /// Get total count.
    pub fn count(&self) -> u64 {
        self.total_count.load(Ordering::Relaxed)
    }

    /// Get average latency (microseconds).
    pub fn average(&self) -> f64 {
        let count = self.count();
        if count == 0 {
            return 0.0;
        }
        self.total_sum.load(Ordering::Relaxed) as f64 / count as f64
    }

    /// Get minimum latency (microseconds).
    pub fn min(&self) -> Option<u64> {
        let min = self.min.load(Ordering::Relaxed);
        if min == u64::MAX {
            None
        } else {
            Some(min)
        }
    }

    /// Get maximum latency (microseconds).
    pub fn max(&self) -> Option<u64> {
        let max = self.max.load(Ordering::Relaxed);
        if max == 0 && self.count() == 0 {
            None
        } else {
            Some(max)
        }
    }

    /// Get percentile (0-100).
    pub fn percentile(&self, p: f64) -> Option<u64> {
        let count = self.count();
        if count == 0 {
            return None;
        }

        let target = ((p / 100.0) * count as f64).ceil() as u64;
        let mut cumulative = 0u64;

        for (i, bucket_count) in self.counts.iter().enumerate() {
            cumulative += bucket_count.load(Ordering::Relaxed);
            if cumulative >= target {
                if i < self.buckets.len() {
                    return Some(self.buckets[i]);
                } else {
                    // Above highest bucket, return max
                    return self.max();
                }
            }
        }

        self.max()
    }

    /// Get p50 (median).
    pub fn p50(&self) -> Option<u64> {
        self.percentile(50.0)
    }

    /// Get p90.
    pub fn p90(&self) -> Option<u64> {
        self.percentile(90.0)
    }

    /// Get p95.
    pub fn p95(&self) -> Option<u64> {
        self.percentile(95.0)
    }

    /// Get p99.
    pub fn p99(&self) -> Option<u64> {
        self.percentile(99.0)
    }

    /// Get bucket counts for visualization.
    pub fn bucket_counts(&self) -> Vec<(String, u64)> {
        let mut result = Vec::with_capacity(self.counts.len());

        for (i, count) in self.counts.iter().enumerate() {
            let label = if i == 0 {
                format!("≤{}µs", self.buckets[0])
            } else if i < self.buckets.len() {
                format!("≤{}µs", self.buckets[i])
            } else {
                format!(">{}µs", self.buckets.last().unwrap_or(&0))
            };
            result.push((label, count.load(Ordering::Relaxed)));
        }

        result
    }

    /// Reset the histogram.
    pub fn reset(&self) {
        for count in &self.counts {
            count.store(0, Ordering::Relaxed);
        }
        self.total_count.store(0, Ordering::Relaxed);
        self.total_sum.store(0, Ordering::Relaxed);
        self.min.store(u64::MAX, Ordering::Relaxed);
        self.max.store(0, Ordering::Relaxed);
    }

    /// Get a snapshot of the histogram.
    pub fn snapshot(&self) -> HistogramSnapshot {
        HistogramSnapshot {
            count: self.count(),
            average_us: self.average(),
            min_us: self.min(),
            max_us: self.max(),
            p50_us: self.p50(),
            p90_us: self.p90(),
            p95_us: self.p95(),
            p99_us: self.p99(),
            buckets: self.bucket_counts(),
        }
    }
}

/// Snapshot of histogram data.
#[derive(Debug, Clone)]
pub struct HistogramSnapshot {
    pub count: u64,
    pub average_us: f64,
    pub min_us: Option<u64>,
    pub max_us: Option<u64>,
    pub p50_us: Option<u64>,
    pub p90_us: Option<u64>,
    pub p95_us: Option<u64>,
    pub p99_us: Option<u64>,
    pub buckets: Vec<(String, u64)>,
}

/// Throughput counter with sliding window.
pub struct ThroughputCounter {
    /// Window duration in seconds.
    window_secs: usize,
    /// Counts per second slot.
    slots: Vec<AtomicU64>,
    /// Current slot index.
    current_slot: AtomicUsize,
    /// Last slot update time.
    last_update: RwLock<Instant>,
    /// Total count ever.
    total: AtomicU64,
}

impl ThroughputCounter {
    /// Create a new throughput counter with the given window size.
    pub fn new(window_secs: usize) -> Self {
        let slots = (0..window_secs)
            .map(|_| AtomicU64::new(0))
            .collect();

        Self {
            window_secs,
            slots,
            current_slot: AtomicUsize::new(0),
            last_update: RwLock::new(Instant::now()),
            total: AtomicU64::new(0),
        }
    }

    /// Create with default 60-second window.
    pub fn with_default_window() -> Self {
        Self::new(60)
    }

    /// Increment the counter.
    pub fn increment(&self) {
        self.increment_by(1);
    }

    /// Increment by a specific amount.
    pub fn increment_by(&self, count: u64) {
        self.advance_slots();

        let slot = self.current_slot.load(Ordering::Relaxed) % self.window_secs;
        self.slots[slot].fetch_add(count, Ordering::Relaxed);
        self.total.fetch_add(count, Ordering::Relaxed);
    }

    /// Advance slots based on elapsed time.
    fn advance_slots(&self) {
        let mut last_update = self.last_update.write();
        let elapsed = last_update.elapsed();
        let slots_to_advance = elapsed.as_secs() as usize;

        if slots_to_advance > 0 {
            *last_update = Instant::now();

            let current = self.current_slot.load(Ordering::Relaxed);
            let new_slot = current + slots_to_advance;

            // Clear slots that we're advancing past
            for i in 1..=slots_to_advance.min(self.window_secs) {
                let slot_index = (current + i) % self.window_secs;
                self.slots[slot_index].store(0, Ordering::Relaxed);
            }

            self.current_slot.store(new_slot, Ordering::Relaxed);
        }
    }

    /// Get current throughput (requests per second).
    pub fn throughput(&self) -> f64 {
        self.advance_slots();

        let total: u64 = self.slots
            .iter()
            .map(|s| s.load(Ordering::Relaxed))
            .sum();

        total as f64 / self.window_secs as f64
    }

    /// Get total count ever.
    pub fn total(&self) -> u64 {
        self.total.load(Ordering::Relaxed)
    }

    /// Reset the counter.
    pub fn reset(&self) {
        for slot in &self.slots {
            slot.store(0, Ordering::Relaxed);
        }
        self.total.store(0, Ordering::Relaxed);
        *self.last_update.write() = Instant::now();
    }
}

/// Resource usage metrics.
#[derive(Debug, Clone, Default)]
pub struct ResourceUsage {
    /// Current memory usage (bytes).
    pub memory_bytes: u64,
    /// Peak memory usage (bytes).
    pub peak_memory_bytes: u64,
    /// Current connection count.
    pub connections: usize,
    /// Peak connection count.
    pub peak_connections: usize,
    /// CPU utilization (0.0-1.0), if tracked.
    pub cpu_utilization: Option<f64>,
}

/// Snapshot of profiler data at a point in time.
#[derive(Debug, Clone)]
pub struct ProfileSnapshot {
    /// Timestamp.
    pub timestamp: Instant,
    /// Latency histogram snapshot.
    pub latency: HistogramSnapshot,
    /// Current throughput (requests/second).
    pub throughput: f64,
    /// Total requests processed.
    pub total_requests: u64,
    /// Resource usage.
    pub resources: ResourceUsage,
    /// Uptime.
    pub uptime: Duration,
}

/// Performance report with analysis.
#[derive(Debug, Clone)]
pub struct ProfileReport {
    /// Report generation time.
    pub generated_at: Instant,
    /// Current snapshot.
    pub current: ProfileSnapshot,
    /// Performance summary.
    pub summary: PerformanceSummary,
    /// Recommendations (if any issues detected).
    pub recommendations: Vec<String>,
}

/// Performance summary.
#[derive(Debug, Clone)]
pub struct PerformanceSummary {
    /// Is latency within target (p99 < 10ms)?
    pub latency_ok: bool,
    /// Is throughput meeting target (100k TPS)?
    pub throughput_ok: bool,
    /// Is memory usage acceptable (< 2GB for 10K devices)?
    pub memory_ok: bool,
    /// Overall health score (0.0-1.0).
    pub health_score: f64,
}

/// Performance profiler for monitoring system performance.
pub struct PerformanceProfiler {
    /// Configuration.
    config: ProfilerConfig,

    /// Latency histogram.
    latency: LatencyHistogram,

    /// Throughput counter.
    throughput: ThroughputCounter,

    /// Resource tracking.
    memory_bytes: AtomicU64,
    peak_memory_bytes: AtomicU64,
    connections: AtomicUsize,
    peak_connections: AtomicUsize,

    /// Sample counter for sampling mode.
    sample_counter: AtomicU64,

    /// Creation time.
    created_at: Instant,

    /// Last report time.
    last_report: RwLock<Instant>,
}

impl PerformanceProfiler {
    /// Create a new profiler with the given configuration.
    pub fn new(config: ProfilerConfig) -> Self {
        let latency = LatencyHistogram::new(config.histogram_buckets.clone());
        let throughput = ThroughputCounter::with_default_window();

        Self {
            config,
            latency,
            throughput,
            memory_bytes: AtomicU64::new(0),
            peak_memory_bytes: AtomicU64::new(0),
            connections: AtomicUsize::new(0),
            peak_connections: AtomicUsize::new(0),
            sample_counter: AtomicU64::new(0),
            created_at: Instant::now(),
            last_report: RwLock::new(Instant::now()),
        }
    }

    /// Create with default configuration.
    pub fn with_defaults() -> Self {
        Self::new(ProfilerConfig::default())
    }

    /// Check if profiling is enabled.
    pub fn is_enabled(&self) -> bool {
        self.config.enabled
    }

    /// Check if this sample should be recorded (based on sample rate).
    fn should_sample(&self) -> bool {
        if !self.config.enabled {
            return false;
        }

        if self.config.sample_rate >= 1.0 {
            return true;
        }

        let counter = self.sample_counter.fetch_add(1, Ordering::Relaxed);
        let threshold = (1.0 / self.config.sample_rate) as u64;
        counter % threshold == 0
    }

    /// Record a request latency.
    pub fn record_latency(&self, duration: Duration) {
        if self.should_sample() {
            self.latency.record_duration(duration);
        }
        self.throughput.increment();
    }

    /// Record a request latency in microseconds.
    pub fn record_latency_us(&self, latency_us: u64) {
        if self.should_sample() {
            self.latency.record(latency_us);
        }
        self.throughput.increment();
    }

    /// Record multiple requests (for batch processing).
    pub fn record_batch(&self, count: u64, total_latency: Duration) {
        if self.should_sample() {
            let avg_latency = total_latency.as_micros() as u64 / count.max(1);
            self.latency.record(avg_latency);
        }
        self.throughput.increment_by(count);
    }

    /// Update memory usage.
    pub fn set_memory(&self, bytes: u64) {
        self.memory_bytes.store(bytes, Ordering::Relaxed);

        // Update peak
        loop {
            let current_peak = self.peak_memory_bytes.load(Ordering::Relaxed);
            if bytes <= current_peak {
                break;
            }
            if self.peak_memory_bytes.compare_exchange_weak(
                current_peak,
                bytes,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ).is_ok() {
                break;
            }
        }
    }

    /// Update connection count.
    pub fn set_connections(&self, count: usize) {
        self.connections.store(count, Ordering::Relaxed);

        // Update peak
        loop {
            let current_peak = self.peak_connections.load(Ordering::Relaxed);
            if count <= current_peak {
                break;
            }
            if self.peak_connections.compare_exchange_weak(
                current_peak,
                count,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ).is_ok() {
                break;
            }
        }
    }

    /// Get current latency histogram.
    pub fn latency(&self) -> &LatencyHistogram {
        &self.latency
    }

    /// Get current throughput.
    pub fn throughput(&self) -> f64 {
        self.throughput.throughput()
    }

    /// Get total requests.
    pub fn total_requests(&self) -> u64 {
        self.throughput.total()
    }

    /// Get resource usage.
    pub fn resource_usage(&self) -> ResourceUsage {
        ResourceUsage {
            memory_bytes: self.memory_bytes.load(Ordering::Relaxed),
            peak_memory_bytes: self.peak_memory_bytes.load(Ordering::Relaxed),
            connections: self.connections.load(Ordering::Relaxed),
            peak_connections: self.peak_connections.load(Ordering::Relaxed),
            cpu_utilization: None, // TODO: Implement CPU tracking if enabled
        }
    }

    /// Get a snapshot of current performance data.
    pub fn snapshot(&self) -> ProfileSnapshot {
        ProfileSnapshot {
            timestamp: Instant::now(),
            latency: self.latency.snapshot(),
            throughput: self.throughput(),
            total_requests: self.total_requests(),
            resources: self.resource_usage(),
            uptime: self.uptime(),
        }
    }

    /// Generate a performance report.
    pub fn report(&self) -> ProfileReport {
        let current = self.snapshot();
        let summary = self.analyze(&current);
        let recommendations = self.generate_recommendations(&current, &summary);

        ProfileReport {
            generated_at: Instant::now(),
            current,
            summary,
            recommendations,
        }
    }

    /// Analyze performance snapshot.
    fn analyze(&self, snapshot: &ProfileSnapshot) -> PerformanceSummary {
        // Target: p99 < 10ms
        let latency_ok = snapshot.latency.p99_us
            .map(|p99| p99 < 10_000)
            .unwrap_or(true);

        // Target: 100k TPS
        let throughput_ok = snapshot.throughput >= 100_000.0 || snapshot.total_requests < 1000;

        // Target: < 2GB for normal operation
        let memory_ok = snapshot.resources.memory_bytes < 2 * 1024 * 1024 * 1024;

        // Calculate health score
        let mut score: f64 = 1.0;

        if !latency_ok {
            score -= 0.3;
        }
        if !throughput_ok && snapshot.total_requests > 10000 {
            score -= 0.3;
        }
        if !memory_ok {
            score -= 0.4;
        }

        PerformanceSummary {
            latency_ok,
            throughput_ok,
            memory_ok,
            health_score: score.max(0.0),
        }
    }

    /// Generate recommendations based on analysis.
    fn generate_recommendations(
        &self,
        snapshot: &ProfileSnapshot,
        summary: &PerformanceSummary,
    ) -> Vec<String> {
        let mut recommendations = Vec::new();

        if !summary.latency_ok {
            if let Some(p99) = snapshot.latency.p99_us {
                recommendations.push(format!(
                    "Latency p99 is {}µs (target: <10000µs). Consider:\n\
                     - Increasing batch size\n\
                     - Reducing handler complexity\n\
                     - Adding more shards to connection pool",
                    p99
                ));
            }
        }

        if !summary.throughput_ok && snapshot.total_requests > 10000 {
            recommendations.push(format!(
                "Throughput is {:.0} req/s (target: >100000 req/s). Consider:\n\
                 - Enabling batch processing\n\
                 - Increasing worker threads\n\
                 - Optimizing handler logic",
                snapshot.throughput
            ));
        }

        if !summary.memory_ok {
            let mb = snapshot.resources.memory_bytes / (1024 * 1024);
            recommendations.push(format!(
                "Memory usage is {}MB (target: <2048MB). Consider:\n\
                 - Using sparse register store\n\
                 - Reducing connection pool size\n\
                 - Implementing memory-mapped storage",
                mb
            ));
        }

        recommendations
    }

    /// Check if it's time for a periodic report.
    pub fn should_report(&self) -> bool {
        let last = self.last_report.read().elapsed();
        last >= self.config.report_interval
    }

    /// Mark report as generated.
    pub fn mark_reported(&self) {
        *self.last_report.write() = Instant::now();
    }

    /// Get uptime.
    pub fn uptime(&self) -> Duration {
        self.created_at.elapsed()
    }

    /// Reset all metrics.
    pub fn reset(&self) {
        self.latency.reset();
        self.throughput.reset();
        self.memory_bytes.store(0, Ordering::Relaxed);
        self.peak_memory_bytes.store(0, Ordering::Relaxed);
        self.connections.store(0, Ordering::Relaxed);
        self.peak_connections.store(0, Ordering::Relaxed);
        self.sample_counter.store(0, Ordering::Relaxed);
    }

    /// Get configuration.
    pub fn config(&self) -> &ProfilerConfig {
        &self.config
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::config::ProfilerConfig;

    #[test]
    fn test_histogram_basic() {
        let histogram = LatencyHistogram::with_defaults();

        histogram.record(100);
        histogram.record(500);
        histogram.record(1000);

        assert_eq!(histogram.count(), 3);
        assert!(histogram.average() > 0.0);
        assert_eq!(histogram.min(), Some(100));
        assert_eq!(histogram.max(), Some(1000));
    }

    #[test]
    fn test_histogram_percentiles() {
        let histogram = LatencyHistogram::with_defaults();

        // Record 100 samples with known distribution
        for i in 0..100 {
            histogram.record(i * 100); // 0, 100, 200, ..., 9900
        }

        let p50 = histogram.p50();
        let p99 = histogram.p99();

        assert!(p50.is_some());
        assert!(p99.is_some());

        // p99 should be higher than p50
        assert!(p99.unwrap() >= p50.unwrap());
    }

    #[test]
    fn test_histogram_buckets() {
        let histogram = LatencyHistogram::new(vec![100, 500, 1000]);

        histogram.record(50);   // Bucket 0 (≤100)
        histogram.record(200);  // Bucket 1 (≤500)
        histogram.record(800);  // Bucket 2 (≤1000)
        histogram.record(5000); // Bucket 3 (>1000)

        let buckets = histogram.bucket_counts();
        assert_eq!(buckets.len(), 4);
        assert_eq!(buckets[0].1, 1);
        assert_eq!(buckets[1].1, 1);
        assert_eq!(buckets[2].1, 1);
        assert_eq!(buckets[3].1, 1);
    }

    #[test]
    fn test_throughput_counter() {
        let counter = ThroughputCounter::new(10);

        counter.increment_by(100);

        let throughput = counter.throughput();
        assert!(throughput > 0.0);
        assert_eq!(counter.total(), 100);
    }

    #[test]
    fn test_profiler_basic() {
        // Use a config with 100% sample rate for deterministic testing
        let config = ProfilerConfig {
            enabled: true,
            sample_rate: 1.0,
            histogram_buckets: vec![100, 500, 1000, 5000, 10000],
            report_interval: Duration::from_secs(60),
            track_memory: true,
            track_cpu: false,
        };
        let profiler = PerformanceProfiler::new(config);

        profiler.record_latency(Duration::from_micros(500));
        profiler.record_latency(Duration::from_micros(1000));
        profiler.record_latency(Duration::from_micros(2000));

        assert!(profiler.throughput() > 0.0);
        assert_eq!(profiler.total_requests(), 3);

        let snapshot = profiler.snapshot();
        assert_eq!(snapshot.latency.count, 3);
    }

    #[test]
    fn test_profiler_sampling() {
        let mut config = ProfilerConfig::default();
        config.sample_rate = 0.5; // 50% sampling

        let profiler = PerformanceProfiler::new(config);

        // Record many samples
        for _ in 0..100 {
            profiler.record_latency(Duration::from_micros(100));
        }

        // Should have sampled roughly half
        let count = profiler.latency.count();
        assert!(count > 30 && count < 70, "Expected ~50 samples, got {}", count);

        // But total requests should be all of them
        assert_eq!(profiler.total_requests(), 100);
    }

    #[test]
    fn test_profiler_resources() {
        let profiler = PerformanceProfiler::with_defaults();

        profiler.set_memory(1024 * 1024); // 1MB
        profiler.set_connections(100);

        let resources = profiler.resource_usage();
        assert_eq!(resources.memory_bytes, 1024 * 1024);
        assert_eq!(resources.connections, 100);

        // Set higher values to test peak tracking
        profiler.set_memory(2 * 1024 * 1024);
        profiler.set_connections(200);
        profiler.set_memory(1024 * 1024); // Lower again
        profiler.set_connections(100);

        let resources = profiler.resource_usage();
        assert_eq!(resources.peak_memory_bytes, 2 * 1024 * 1024);
        assert_eq!(resources.peak_connections, 200);
    }

    #[test]
    fn test_profiler_report() {
        let profiler = PerformanceProfiler::with_defaults();

        // Record some data
        for _ in 0..100 {
            profiler.record_latency(Duration::from_micros(500));
        }

        let report = profiler.report();
        assert!(report.summary.latency_ok);
        assert!(report.summary.memory_ok);
        assert!(report.summary.health_score > 0.0);
    }

    #[test]
    fn test_histogram_reset() {
        let histogram = LatencyHistogram::with_defaults();

        histogram.record(100);
        histogram.record(200);
        assert_eq!(histogram.count(), 2);

        histogram.reset();
        assert_eq!(histogram.count(), 0);
        assert!(histogram.min().is_none());
    }

    #[test]
    fn test_profiler_disabled() {
        let mut config = ProfilerConfig::default();
        config.enabled = false;

        let profiler = PerformanceProfiler::new(config);
        assert!(!profiler.is_enabled());

        // Should not record when disabled
        profiler.record_latency(Duration::from_micros(100));
        assert_eq!(profiler.latency.count(), 0);

        // But throughput should still be tracked
        assert_eq!(profiler.total_requests(), 1);
    }
}

//! Metrics collection and export.
//!
//! This module provides a comprehensive metrics collection system for Mabinogion protocol simulator.
//! It supports:
//! - Prometheus-compatible metrics export
//! - Global registry with lazy initialization
//! - Protocol-specific and device-specific metrics
//! - System metrics (CPU, memory)
//! - Request timing with automatic observation via RAII
//!
//! # Example
//!
//! ```rust,ignore
//! use mabi_core::metrics::{MetricsCollector, GLOBAL_REGISTRY, measure_request};
//!
//! let metrics = MetricsCollector::global();
//! metrics.record_read("modbus", true, Duration::from_micros(500));
//!
//! // Using the measure_request! macro
//! measure_request!(metrics, "modbus", "read", {
//!     // ... operation code
//! });
//! ```

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use once_cell::sync::Lazy;
use parking_lot::RwLock;
use prometheus::{
    Encoder, Gauge, Histogram, HistogramOpts, HistogramVec, IntCounter, IntCounterVec, IntGauge,
    IntGaugeVec, Opts, Registry, TextEncoder,
};
use serde::{Deserialize, Serialize};

// ============================================================================
// Constants
// ============================================================================

/// Metric name prefix for all Mabinogion metrics.
pub const METRIC_PREFIX: &str = "mabi";

/// Standard histogram buckets for latency measurements (in seconds).
/// Covers range from 0.1ms to 10s with exponential distribution.
pub const LATENCY_BUCKETS: &[f64] = &[
    0.0001, 0.0005, 0.001, 0.0025, 0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0,
];

/// Standard histogram buckets for tick duration measurements (in seconds).
/// Optimized for sub-millisecond to 100ms range.
pub const TICK_BUCKETS: &[f64] = &[0.0001, 0.0005, 0.001, 0.0025, 0.005, 0.01, 0.025, 0.05, 0.1];

// ============================================================================
// Global Registry
// ============================================================================

/// Global Prometheus registry for all metrics.
/// This registry is shared across all MetricsCollector instances.
pub static GLOBAL_REGISTRY: Lazy<Registry> = Lazy::new(Registry::new);

/// Global metrics collector instance (singleton).
static GLOBAL_METRICS: Lazy<MetricsCollector> =
    Lazy::new(|| MetricsCollector::with_registry(GLOBAL_REGISTRY.clone()));

// ============================================================================
// Metrics Collector
// ============================================================================

/// Metrics collector for the simulator.
///
/// Provides comprehensive metrics collection including:
/// - Request/response counters and timings
/// - Device and connection tracking
/// - System resource monitoring (CPU, memory)
/// - Protocol-specific metrics
#[derive(Clone)]
pub struct MetricsCollector {
    registry: Arc<Registry>,
    inner: Arc<MetricsInner>,
}

/// Internal metrics storage.
struct MetricsInner {
    // === Counters ===
    /// Total requests by protocol and operation type
    requests_total: IntCounterVec,
    /// Total read operations by protocol and status
    reads_total: IntCounterVec,
    /// Total write operations by protocol and status
    writes_total: IntCounterVec,
    /// Total errors by protocol and error type
    errors_total: IntCounterVec,
    /// Total engine ticks
    ticks_total: IntCounter,
    /// Total messages by protocol and direction
    messages_total: IntCounterVec,
    /// Total events by type
    events_total: IntCounterVec,

    // === Gauges ===
    /// Number of active devices
    devices_active: IntGauge,
    /// Active connections per protocol
    connections_active: IntGaugeVec,
    /// Total data points
    points_total: IntGauge,
    /// Data points per protocol and device
    points_by_device: IntGaugeVec,
    /// Memory usage in bytes
    memory_bytes: IntGauge,
    /// CPU usage percentage (0-100)
    cpu_percent: Gauge,
    /// Current tick rate (ticks per second)
    tick_rate: Gauge,

    // === Histograms ===
    /// Request duration by protocol and operation
    request_duration: HistogramVec,
    /// Message latency by protocol
    message_latency: HistogramVec,
    /// Engine tick duration
    tick_duration: Histogram,
    /// Read operation duration by protocol
    read_duration: HistogramVec,
    /// Write operation duration by protocol
    write_duration: HistogramVec,

    // === Internal tracking ===
    start_time: Instant,
    last_tick_time: RwLock<Instant>,
    tick_count_for_rate: AtomicU64,
}

impl MetricsCollector {
    /// Create a new metrics collector with a fresh registry.
    pub fn new() -> Self {
        let registry = Registry::new();
        Self::with_registry(registry)
    }

    /// Get the global metrics collector instance.
    ///
    /// This returns a shared instance that uses the global registry.
    /// Use this when you want all metrics to be collected in one place.
    pub fn global() -> &'static Self {
        &GLOBAL_METRICS
    }

    /// Create a metrics collector with a custom registry.
    pub fn with_registry(registry: Registry) -> Self {
        let inner = MetricsInner::new(&registry);

        Self {
            registry: Arc::new(registry),
            inner: Arc::new(inner),
        }
    }

    /// Get the Prometheus registry.
    pub fn registry(&self) -> &Registry {
        &self.registry
    }

    // ========================================================================
    // Request/Operation Recording
    // ========================================================================

    /// Record a request by protocol and operation.
    ///
    /// This is the primary method for tracking request counts and should be
    /// used in conjunction with `record_request_duration` or `time_request`.
    pub fn record_request(&self, protocol: &str, operation: &str) {
        self.inner
            .requests_total
            .with_label_values(&[protocol, operation])
            .inc();
    }

    /// Record a request duration.
    pub fn record_request_duration(&self, protocol: &str, operation: &str, duration: Duration) {
        self.inner
            .request_duration
            .with_label_values(&[protocol, operation])
            .observe(duration.as_secs_f64());
    }

    /// Create a request timer for automatic duration recording.
    ///
    /// The timer automatically records the duration when dropped.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let _timer = metrics.time_request("modbus", "read");
    /// // ... do work ...
    /// // duration is automatically recorded when _timer goes out of scope
    /// ```
    pub fn time_request(&self, protocol: &str, operation: &str) -> RequestTimer {
        self.record_request(protocol, operation);
        RequestTimer::new(
            self.inner
                .request_duration
                .with_label_values(&[protocol, operation]),
        )
    }

    /// Record a message (for protocol-level message tracking).
    pub fn record_message(&self, protocol: &str, direction: &str) {
        self.inner
            .messages_total
            .with_label_values(&[protocol, direction])
            .inc();
    }

    /// Record a read operation with timing and status.
    pub fn record_read(&self, protocol: &str, success: bool, duration: Duration) {
        let status = if success { "success" } else { "error" };
        self.inner
            .reads_total
            .with_label_values(&[protocol, status])
            .inc();
        self.inner
            .read_duration
            .with_label_values(&[protocol])
            .observe(duration.as_secs_f64());

        // Also record as a request
        self.inner
            .requests_total
            .with_label_values(&[protocol, "read"])
            .inc();
        self.inner
            .request_duration
            .with_label_values(&[protocol, "read"])
            .observe(duration.as_secs_f64());
    }

    /// Record a write operation with timing and status.
    pub fn record_write(&self, protocol: &str, success: bool, duration: Duration) {
        let status = if success { "success" } else { "error" };
        self.inner
            .writes_total
            .with_label_values(&[protocol, status])
            .inc();
        self.inner
            .write_duration
            .with_label_values(&[protocol])
            .observe(duration.as_secs_f64());

        // Also record as a request
        self.inner
            .requests_total
            .with_label_values(&[protocol, "write"])
            .inc();
        self.inner
            .request_duration
            .with_label_values(&[protocol, "write"])
            .observe(duration.as_secs_f64());
    }

    // ========================================================================
    // Error Recording
    // ========================================================================

    /// Record an error by protocol and error type.
    pub fn record_error(&self, protocol: &str, error_type: &str) {
        self.inner
            .errors_total
            .with_label_values(&[protocol, error_type])
            .inc();
    }

    // ========================================================================
    // Engine/Tick Recording
    // ========================================================================

    /// Record an engine tick with duration.
    pub fn record_tick(&self, duration: Duration) {
        self.inner.ticks_total.inc();
        self.inner.tick_duration.observe(duration.as_secs_f64());

        // Update tick rate calculation
        let count = self
            .inner
            .tick_count_for_rate
            .fetch_add(1, Ordering::Relaxed)
            + 1;
        let mut last_time = self.inner.last_tick_time.write();
        let elapsed = last_time.elapsed();

        // Update tick rate every second
        if elapsed >= Duration::from_secs(1) {
            let rate = count as f64 / elapsed.as_secs_f64();
            self.inner.tick_rate.set(rate);
            self.inner.tick_count_for_rate.store(0, Ordering::Relaxed);
            *last_time = Instant::now();
        }
    }

    /// Record message latency.
    pub fn record_latency(&self, protocol: &str, latency: Duration) {
        self.inner
            .message_latency
            .with_label_values(&[protocol])
            .observe(latency.as_secs_f64());
    }

    /// Record an event by type.
    pub fn record_event(&self, event_type: &str) {
        self.inner
            .events_total
            .with_label_values(&[event_type])
            .inc();
    }

    // ========================================================================
    // Gauge Setters
    // ========================================================================

    /// Set the number of active devices.
    pub fn set_devices_active(&self, count: i64) {
        self.inner.devices_active.set(count);
    }

    /// Set active connections for a protocol.
    pub fn set_connections_active(&self, protocol: &str, count: i64) {
        self.inner
            .connections_active
            .with_label_values(&[protocol])
            .set(count);
    }

    /// Increment active connections for a protocol.
    pub fn inc_connections(&self, protocol: &str) {
        self.inner
            .connections_active
            .with_label_values(&[protocol])
            .inc();
    }

    /// Decrement active connections for a protocol.
    pub fn dec_connections(&self, protocol: &str) {
        self.inner
            .connections_active
            .with_label_values(&[protocol])
            .dec();
    }

    /// Set total data points.
    pub fn set_points_total(&self, count: i64) {
        self.inner.points_total.set(count);
    }

    /// Set data points for a specific device.
    pub fn set_device_points(&self, protocol: &str, device_id: &str, count: i64) {
        self.inner
            .points_by_device
            .with_label_values(&[protocol, device_id])
            .set(count);
    }

    /// Remove device points metrics (when device is removed).
    pub fn remove_device_points(&self, protocol: &str, device_id: &str) {
        // Set to 0 to indicate removal (Prometheus doesn't support metric deletion)
        self.inner
            .points_by_device
            .with_label_values(&[protocol, device_id])
            .set(0);
    }

    // ========================================================================
    // System Metrics
    // ========================================================================

    /// Set memory usage in bytes.
    pub fn set_memory_bytes(&self, bytes: i64) {
        self.inner.memory_bytes.set(bytes);
    }

    /// Set CPU usage percentage (0-100).
    pub fn set_cpu_percent(&self, percent: f64) {
        self.inner.cpu_percent.set(percent.clamp(0.0, 100.0));
    }

    /// Update system metrics (memory and CPU).
    ///
    /// This is a convenience method that updates both memory and CPU metrics.
    pub fn update_system_metrics(&self, memory_bytes: i64, cpu_percent: f64) {
        self.set_memory_bytes(memory_bytes);
        self.set_cpu_percent(cpu_percent);
    }

    // ========================================================================
    // Getters & Export
    // ========================================================================

    /// Get uptime since the metrics collector was created.
    pub fn uptime(&self) -> Duration {
        self.inner.start_time.elapsed()
    }

    /// Get the current tick rate (ticks per second).
    pub fn tick_rate(&self) -> f64 {
        self.inner.tick_rate.get()
    }

    /// Get a snapshot of current metrics.
    pub fn snapshot(&self) -> MetricsSnapshot {
        MetricsSnapshot {
            uptime_secs: self.uptime().as_secs(),
            devices_active: self.inner.devices_active.get() as u64,
            points_total: self.inner.points_total.get() as u64,
            memory_bytes: self.inner.memory_bytes.get() as u64,
            cpu_percent: self.inner.cpu_percent.get(),
            ticks_total: self.inner.ticks_total.get(),
            tick_rate: self.inner.tick_rate.get(),
        }
    }

    /// Get a detailed snapshot with additional information.
    pub fn detailed_snapshot(&self) -> DetailedMetricsSnapshot {
        DetailedMetricsSnapshot {
            basic: self.snapshot(),
            start_time_unix: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs() - self.uptime().as_secs())
                .unwrap_or(0),
        }
    }

    /// Export metrics in Prometheus text format.
    pub fn export_prometheus(&self) -> String {
        let encoder = TextEncoder::new();
        let metric_families = self.registry.gather();
        let mut buffer = Vec::new();
        encoder.encode(&metric_families, &mut buffer).unwrap();
        String::from_utf8(buffer).unwrap()
    }

    /// Export metrics from the global registry.
    pub fn export_global_prometheus() -> String {
        let encoder = TextEncoder::new();
        let metric_families = GLOBAL_REGISTRY.gather();
        let mut buffer = Vec::new();
        encoder.encode(&metric_families, &mut buffer).unwrap();
        String::from_utf8(buffer).unwrap()
    }
}

impl Default for MetricsCollector {
    fn default() -> Self {
        Self::new()
    }
}

impl MetricsInner {
    fn new(registry: &Registry) -> Self {
        let now = Instant::now();

        // ====================================================================
        // Counters
        // ====================================================================

        let requests_total = IntCounterVec::new(
            Opts::new(
                format!("{}_requests_total", METRIC_PREFIX),
                "Total number of protocol requests",
            ),
            &["protocol", "operation"],
        )
        .unwrap();
        registry.register(Box::new(requests_total.clone())).unwrap();

        let messages_total = IntCounterVec::new(
            Opts::new(
                format!("{}_messages_total", METRIC_PREFIX),
                "Total messages processed",
            ),
            &["protocol", "direction"],
        )
        .unwrap();
        registry.register(Box::new(messages_total.clone())).unwrap();

        let reads_total = IntCounterVec::new(
            Opts::new(
                format!("{}_reads_total", METRIC_PREFIX),
                "Total read operations",
            ),
            &["protocol", "status"],
        )
        .unwrap();
        registry.register(Box::new(reads_total.clone())).unwrap();

        let writes_total = IntCounterVec::new(
            Opts::new(
                format!("{}_writes_total", METRIC_PREFIX),
                "Total write operations",
            ),
            &["protocol", "status"],
        )
        .unwrap();
        registry.register(Box::new(writes_total.clone())).unwrap();

        let errors_total = IntCounterVec::new(
            Opts::new(
                format!("{}_errors_total", METRIC_PREFIX),
                "Total errors by protocol and type",
            ),
            &["protocol", "error_type"],
        )
        .unwrap();
        registry.register(Box::new(errors_total.clone())).unwrap();

        let ticks_total = IntCounter::new(
            format!("{}_ticks_total", METRIC_PREFIX),
            "Total engine ticks processed",
        )
        .unwrap();
        registry.register(Box::new(ticks_total.clone())).unwrap();

        let events_total = IntCounterVec::new(
            Opts::new(
                format!("{}_events_total", METRIC_PREFIX),
                "Total events by type",
            ),
            &["event_type"],
        )
        .unwrap();
        registry.register(Box::new(events_total.clone())).unwrap();

        // ====================================================================
        // Gauges
        // ====================================================================

        let devices_active = IntGauge::new(
            format!("{}_devices_active", METRIC_PREFIX),
            "Number of active simulated devices",
        )
        .unwrap();
        registry.register(Box::new(devices_active.clone())).unwrap();

        let connections_active = IntGaugeVec::new(
            Opts::new(
                format!("{}_connections_active", METRIC_PREFIX),
                "Number of active connections per protocol",
            ),
            &["protocol"],
        )
        .unwrap();
        registry
            .register(Box::new(connections_active.clone()))
            .unwrap();

        let points_total = IntGauge::new(
            format!("{}_data_points_total", METRIC_PREFIX),
            "Total number of data points across all devices",
        )
        .unwrap();
        registry.register(Box::new(points_total.clone())).unwrap();

        let points_by_device = IntGaugeVec::new(
            Opts::new(
                format!("{}_data_points_by_device", METRIC_PREFIX),
                "Number of data points per device",
            ),
            &["protocol", "device_id"],
        )
        .unwrap();
        registry
            .register(Box::new(points_by_device.clone()))
            .unwrap();

        let memory_bytes = IntGauge::new(
            format!("{}_memory_usage_bytes", METRIC_PREFIX),
            "Current memory usage in bytes",
        )
        .unwrap();
        registry.register(Box::new(memory_bytes.clone())).unwrap();

        let cpu_percent = Gauge::new(
            format!("{}_cpu_usage_percent", METRIC_PREFIX),
            "Current CPU usage percentage (0-100)",
        )
        .unwrap();
        registry.register(Box::new(cpu_percent.clone())).unwrap();

        let tick_rate = Gauge::new(
            format!("{}_tick_rate", METRIC_PREFIX),
            "Current tick rate (ticks per second)",
        )
        .unwrap();
        registry.register(Box::new(tick_rate.clone())).unwrap();

        // ====================================================================
        // Histograms
        // ====================================================================

        let request_duration = HistogramVec::new(
            HistogramOpts::new(
                format!("{}_request_duration_seconds", METRIC_PREFIX),
                "Request processing duration in seconds",
            )
            .buckets(LATENCY_BUCKETS.to_vec()),
            &["protocol", "operation"],
        )
        .unwrap();
        registry
            .register(Box::new(request_duration.clone()))
            .unwrap();

        let message_latency = HistogramVec::new(
            HistogramOpts::new(
                format!("{}_message_latency_seconds", METRIC_PREFIX),
                "Message latency in seconds",
            )
            .buckets(LATENCY_BUCKETS.to_vec()),
            &["protocol"],
        )
        .unwrap();
        registry
            .register(Box::new(message_latency.clone()))
            .unwrap();

        let tick_duration = Histogram::with_opts(
            HistogramOpts::new(
                format!("{}_tick_duration_seconds", METRIC_PREFIX),
                "Engine tick processing duration in seconds",
            )
            .buckets(TICK_BUCKETS.to_vec()),
        )
        .unwrap();
        registry.register(Box::new(tick_duration.clone())).unwrap();

        let read_duration = HistogramVec::new(
            HistogramOpts::new(
                format!("{}_read_duration_seconds", METRIC_PREFIX),
                "Read operation duration in seconds",
            )
            .buckets(LATENCY_BUCKETS.to_vec()),
            &["protocol"],
        )
        .unwrap();
        registry.register(Box::new(read_duration.clone())).unwrap();

        let write_duration = HistogramVec::new(
            HistogramOpts::new(
                format!("{}_write_duration_seconds", METRIC_PREFIX),
                "Write operation duration in seconds",
            )
            .buckets(LATENCY_BUCKETS.to_vec()),
            &["protocol"],
        )
        .unwrap();
        registry.register(Box::new(write_duration.clone())).unwrap();

        Self {
            // Counters
            requests_total,
            messages_total,
            reads_total,
            writes_total,
            errors_total,
            ticks_total,
            events_total,

            // Gauges
            devices_active,
            connections_active,
            points_total,
            points_by_device,
            memory_bytes,
            cpu_percent,
            tick_rate,

            // Histograms
            request_duration,
            message_latency,
            tick_duration,
            read_duration,
            write_duration,

            // Internal tracking
            start_time: now,
            last_tick_time: RwLock::new(now),
            tick_count_for_rate: AtomicU64::new(0),
        }
    }
}

// ============================================================================
// Snapshot Types
// ============================================================================

/// Snapshot of current metrics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricsSnapshot {
    /// Uptime in seconds.
    pub uptime_secs: u64,
    /// Number of active devices.
    pub devices_active: u64,
    /// Total data points across all devices.
    pub points_total: u64,
    /// Memory usage in bytes.
    pub memory_bytes: u64,
    /// CPU usage percentage (0-100).
    pub cpu_percent: f64,
    /// Total engine ticks processed.
    pub ticks_total: u64,
    /// Current tick rate (ticks per second).
    pub tick_rate: f64,
}

/// Detailed metrics snapshot with additional information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DetailedMetricsSnapshot {
    /// Basic metrics snapshot.
    #[serde(flatten)]
    pub basic: MetricsSnapshot,
    /// Unix timestamp when the metrics collection started.
    pub start_time_unix: u64,
}

// ============================================================================
// Latency Statistics
// ============================================================================

/// Latency statistics with percentile calculations.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LatencyStats {
    /// Minimum latency in microseconds.
    pub min_us: u64,
    /// Maximum latency in microseconds.
    pub max_us: u64,
    /// Average latency in microseconds.
    pub avg_us: u64,
    /// Median (P50) latency in microseconds.
    pub p50_us: u64,
    /// P90 latency in microseconds.
    pub p90_us: u64,
    /// P95 latency in microseconds.
    pub p95_us: u64,
    /// P99 latency in microseconds.
    pub p99_us: u64,
    /// Sample count.
    pub count: u64,
    /// Standard deviation in microseconds.
    pub stddev_us: u64,
}

impl LatencyStats {
    /// Calculate statistics from duration samples.
    pub fn from_samples(samples: &[Duration]) -> Self {
        if samples.is_empty() {
            return Self::default();
        }

        let mut sorted: Vec<u64> = samples.iter().map(|d| d.as_micros() as u64).collect();
        sorted.sort_unstable();

        let count = sorted.len();
        let sum: u64 = sorted.iter().sum();
        let avg = sum / count as u64;

        // Calculate standard deviation
        let variance: u64 = sorted
            .iter()
            .map(|&x| {
                let diff = x.abs_diff(avg);
                diff * diff
            })
            .sum::<u64>()
            / count as u64;
        let stddev = (variance as f64).sqrt() as u64;

        Self {
            min_us: sorted[0],
            max_us: sorted[count - 1],
            avg_us: avg,
            p50_us: Self::percentile(&sorted, 50),
            p90_us: Self::percentile(&sorted, 90),
            p95_us: Self::percentile(&sorted, 95),
            p99_us: Self::percentile(&sorted, 99),
            count: count as u64,
            stddev_us: stddev,
        }
    }

    /// Calculate a specific percentile from sorted samples.
    fn percentile(sorted: &[u64], p: usize) -> u64 {
        if sorted.is_empty() {
            return 0;
        }
        let idx = (sorted.len() * p / 100).min(sorted.len() - 1);
        sorted[idx]
    }

    /// Check if the latency statistics indicate a performance issue.
    ///
    /// Returns true if P99 latency exceeds the threshold.
    pub fn is_latency_high(&self, threshold_us: u64) -> bool {
        self.p99_us > threshold_us
    }
}

// ============================================================================
// Timer Utilities
// ============================================================================

/// Simple timer for measuring operation duration.
///
/// Use this when you need manual control over timing.
/// For automatic recording to Prometheus, use `RequestTimer` instead.
pub struct Timer {
    start: Instant,
}

impl Timer {
    /// Start a new timer.
    pub fn start() -> Self {
        Self {
            start: Instant::now(),
        }
    }

    /// Get elapsed duration without consuming the timer.
    pub fn elapsed(&self) -> Duration {
        self.start.elapsed()
    }

    /// Stop the timer and return elapsed duration.
    pub fn stop(self) -> Duration {
        self.elapsed()
    }

    /// Reset the timer to now.
    pub fn reset(&mut self) {
        self.start = Instant::now();
    }
}

impl Default for Timer {
    fn default() -> Self {
        Self::start()
    }
}

/// Request timer that automatically records duration to Prometheus when dropped.
///
/// This implements the RAII pattern for request timing.
/// The duration is recorded when the timer goes out of scope.
///
/// # Example
///
/// ```rust,ignore
/// let _timer = metrics.time_request("modbus", "read");
/// // ... perform the operation ...
/// // duration is automatically recorded when _timer is dropped
/// ```
pub struct RequestTimer {
    histogram: Histogram,
    start: Instant,
    recorded: bool,
}

impl RequestTimer {
    /// Create a new request timer.
    pub fn new(histogram: Histogram) -> Self {
        Self {
            histogram,
            start: Instant::now(),
            recorded: false,
        }
    }

    /// Get elapsed duration without recording.
    pub fn elapsed(&self) -> Duration {
        self.start.elapsed()
    }

    /// Manually record the duration and mark as recorded.
    ///
    /// This prevents the Drop implementation from recording again.
    pub fn record(mut self) -> Duration {
        let elapsed = self.elapsed();
        self.histogram.observe(elapsed.as_secs_f64());
        self.recorded = true;
        elapsed
    }

    /// Discard the timer without recording.
    ///
    /// Use this if you want to cancel timing (e.g., on error).
    pub fn discard(mut self) {
        self.recorded = true; // Prevent recording in Drop
    }
}

impl Drop for RequestTimer {
    fn drop(&mut self) {
        if !self.recorded {
            self.histogram.observe(self.start.elapsed().as_secs_f64());
        }
    }
}

// ============================================================================
// System Metrics Collector
// ============================================================================

/// System metrics collector for CPU and memory monitoring.
///
/// This provides a way to periodically collect system metrics
/// and update the MetricsCollector.
#[cfg(feature = "system-metrics")]
pub struct SystemMetricsCollector {
    metrics: MetricsCollector,
    interval: Duration,
}

#[cfg(feature = "system-metrics")]
impl SystemMetricsCollector {
    /// Create a new system metrics collector.
    pub fn new(metrics: MetricsCollector, interval: Duration) -> Self {
        Self { metrics, interval }
    }

    /// Start collecting system metrics in a background task.
    ///
    /// Returns a handle that can be used to stop collection.
    pub fn start(self) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(self.interval);

            loop {
                interval.tick().await;
                self.collect_once();
            }
        })
    }

    /// Collect system metrics once.
    fn collect_once(&self) {
        // Memory usage (process RSS)
        #[cfg(target_os = "linux")]
        if let Ok(status) = std::fs::read_to_string("/proc/self/status") {
            if let Some(line) = status.lines().find(|l| l.starts_with("VmRSS:")) {
                if let Some(kb_str) = line.split_whitespace().nth(1) {
                    if let Ok(kb) = kb_str.parse::<i64>() {
                        self.metrics.set_memory_bytes(kb * 1024);
                    }
                }
            }
        }

        // CPU usage would require more complex tracking (comparing with previous sample)
        // For now, we skip automatic CPU collection
    }
}

// ============================================================================
// Macros
// ============================================================================

/// Measure request duration and record to metrics.
///
/// This macro times the execution of a block and records the duration.
///
/// # Example
///
/// ```rust,ignore
/// let result = measure_request!(metrics, "modbus", "read", {
///     device.read_registers(0, 10).await
/// });
/// ```
#[macro_export]
macro_rules! measure_request {
    ($metrics:expr, $protocol:expr, $operation:expr, $block:expr) => {{
        let _timer = $metrics.time_request($protocol, $operation);
        let result = $block;
        result
    }};
}

/// Record an error with context.
///
/// This macro records an error and provides a consistent error type format.
///
/// # Example
///
/// ```rust,ignore
/// record_error!(metrics, "modbus", err);
/// ```
#[macro_export]
macro_rules! record_error {
    ($metrics:expr, $protocol:expr, $error:expr) => {{
        let error_type = $crate::metrics::classify_error(&$error);
        $metrics.record_error($protocol, error_type);
    }};
}

/// Classify an error into a category for metrics.
///
/// Returns a string suitable for use as a metric label.
pub fn classify_error<E: std::fmt::Display>(error: &E) -> &'static str {
    let error_str = error.to_string().to_lowercase();

    if error_str.contains("timeout") {
        "timeout"
    } else if error_str.contains("connection") || error_str.contains("connect") {
        "connection"
    } else if error_str.contains("protocol") || error_str.contains("invalid") {
        "protocol"
    } else if error_str.contains(" io ")
        || error_str.contains("i/o")
        || error_str.starts_with("io ")
        || error_str.ends_with(" io")
    {
        "io"
    } else if error_str.contains("not found") {
        "not_found"
    } else if error_str.contains("permission") || error_str.contains("unauthorized") {
        "permission"
    } else {
        "unknown"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ========================================================================
    // MetricsCollector Tests
    // ========================================================================

    #[test]
    fn test_metrics_collector_new() {
        let metrics = MetricsCollector::new();
        assert!(metrics.uptime() >= Duration::ZERO);
    }

    #[test]
    fn test_metrics_collector_global() {
        let global1 = MetricsCollector::global();
        let global2 = MetricsCollector::global();
        // Both should point to the same instance
        assert!(std::ptr::eq(global1, global2));
    }

    #[test]
    fn test_metrics_collector_basic_operations() {
        let metrics = MetricsCollector::new();

        // Test message recording
        metrics.record_message("modbus", "rx");
        metrics.record_message("modbus", "tx");

        // Test read recording
        metrics.record_read("modbus", true, Duration::from_micros(100));
        metrics.record_read("modbus", false, Duration::from_micros(200));

        // Test write recording
        metrics.record_write("opcua", true, Duration::from_micros(50));

        // Test device count
        metrics.set_devices_active(10);

        let snapshot = metrics.snapshot();
        assert_eq!(snapshot.devices_active, 10);
    }

    #[test]
    fn test_metrics_collector_request_timing() {
        let metrics = MetricsCollector::new();

        // Test direct request recording
        metrics.record_request("modbus", "read");
        metrics.record_request_duration("modbus", "read", Duration::from_micros(150));

        // Test time_request helper
        {
            let _timer = metrics.time_request("bacnet", "subscribe");
            std::thread::sleep(Duration::from_millis(5));
        } // Timer records on drop
    }

    #[test]
    fn test_metrics_collector_error_recording() {
        let metrics = MetricsCollector::new();

        metrics.record_error("modbus", "timeout");
        metrics.record_error("modbus", "connection");
        metrics.record_error("opcua", "protocol");
    }

    #[test]
    fn test_metrics_collector_tick_recording() {
        let metrics = MetricsCollector::new();

        for i in 0..5 {
            metrics.record_tick(Duration::from_millis(10 * (i + 1)));
        }

        let snapshot = metrics.snapshot();
        assert_eq!(snapshot.ticks_total, 5);
    }

    #[test]
    fn test_metrics_collector_connections() {
        let metrics = MetricsCollector::new();

        metrics.set_connections_active("modbus", 5);
        metrics.inc_connections("modbus");
        metrics.dec_connections("modbus");
    }

    #[test]
    fn test_metrics_collector_device_points() {
        let metrics = MetricsCollector::new();

        metrics.set_points_total(1000);
        metrics.set_device_points("modbus", "device-001", 50);
        metrics.set_device_points("modbus", "device-002", 75);

        let snapshot = metrics.snapshot();
        assert_eq!(snapshot.points_total, 1000);

        // Test removal
        metrics.remove_device_points("modbus", "device-001");
    }

    #[test]
    fn test_metrics_collector_system_metrics() {
        let metrics = MetricsCollector::new();

        metrics.set_memory_bytes(1024 * 1024 * 512); // 512 MB
        metrics.set_cpu_percent(45.5);

        let snapshot = metrics.snapshot();
        assert_eq!(snapshot.memory_bytes, 1024 * 1024 * 512);
        assert!((snapshot.cpu_percent - 45.5).abs() < 0.001);

        // Test convenience method
        metrics.update_system_metrics(1024 * 1024 * 256, 30.0);
        let snapshot = metrics.snapshot();
        assert_eq!(snapshot.memory_bytes, 1024 * 1024 * 256);
        assert!((snapshot.cpu_percent - 30.0).abs() < 0.001);

        // Test clamping
        metrics.set_cpu_percent(150.0); // Should be clamped to 100
        let snapshot = metrics.snapshot();
        assert!((snapshot.cpu_percent - 100.0).abs() < 0.001);

        metrics.set_cpu_percent(-10.0); // Should be clamped to 0
        let snapshot = metrics.snapshot();
        assert!(snapshot.cpu_percent.abs() < 0.001);
    }

    #[test]
    fn test_metrics_collector_prometheus_export() {
        let metrics = MetricsCollector::new();

        metrics.record_read("modbus", true, Duration::from_micros(100));
        metrics.set_devices_active(5);

        let prometheus_output = metrics.export_prometheus();
        assert!(!prometheus_output.is_empty());
        assert!(prometheus_output.contains(METRIC_PREFIX));
        assert!(prometheus_output.contains("devices_active"));
    }

    #[test]
    fn test_metrics_collector_event_recording() {
        let metrics = MetricsCollector::new();

        metrics.record_event("device_added");
        metrics.record_event("device_removed");
        metrics.record_event("engine_started");
    }

    #[test]
    fn test_metrics_collector_snapshot() {
        let metrics = MetricsCollector::new();

        metrics.set_devices_active(10);
        metrics.set_points_total(500);
        metrics.set_memory_bytes(1024 * 1024);
        metrics.set_cpu_percent(25.0);

        let snapshot = metrics.snapshot();
        assert_eq!(snapshot.devices_active, 10);
        assert_eq!(snapshot.points_total, 500);
        assert_eq!(snapshot.memory_bytes, 1024 * 1024);
        assert!((snapshot.cpu_percent - 25.0).abs() < 0.001);

        let detailed = metrics.detailed_snapshot();
        assert_eq!(detailed.basic.devices_active, 10);
        assert!(detailed.start_time_unix > 0);
    }

    // ========================================================================
    // LatencyStats Tests
    // ========================================================================

    #[test]
    fn test_latency_stats_from_samples() {
        let samples: Vec<Duration> = vec![
            Duration::from_micros(100),
            Duration::from_micros(200),
            Duration::from_micros(300),
            Duration::from_micros(400),
            Duration::from_micros(500),
        ];

        let stats = LatencyStats::from_samples(&samples);
        assert_eq!(stats.min_us, 100);
        assert_eq!(stats.max_us, 500);
        assert_eq!(stats.avg_us, 300);
        assert_eq!(stats.count, 5);
        assert!(stats.p50_us >= 200 && stats.p50_us <= 300);
    }

    #[test]
    fn test_latency_stats_empty() {
        let samples: Vec<Duration> = vec![];
        let stats = LatencyStats::from_samples(&samples);

        assert_eq!(stats.min_us, 0);
        assert_eq!(stats.max_us, 0);
        assert_eq!(stats.avg_us, 0);
        assert_eq!(stats.count, 0);
    }

    #[test]
    fn test_latency_stats_single_sample() {
        let samples = vec![Duration::from_micros(150)];
        let stats = LatencyStats::from_samples(&samples);

        assert_eq!(stats.min_us, 150);
        assert_eq!(stats.max_us, 150);
        assert_eq!(stats.avg_us, 150);
        assert_eq!(stats.count, 1);
    }

    #[test]
    fn test_latency_stats_percentiles() {
        // Create 100 samples for accurate percentile testing
        let samples: Vec<Duration> = (1..=100).map(|i| Duration::from_micros(i)).collect();

        let stats = LatencyStats::from_samples(&samples);
        assert_eq!(stats.min_us, 1);
        assert_eq!(stats.max_us, 100);
        // Note: percentile calculation uses integer division, so exact values may vary slightly
        assert!(stats.p50_us >= 50 && stats.p50_us <= 51);
        assert!(stats.p90_us >= 90 && stats.p90_us <= 91);
        assert!(stats.p95_us >= 95 && stats.p95_us <= 96);
        assert!(stats.p99_us >= 99 && stats.p99_us <= 100);
    }

    #[test]
    fn test_latency_stats_is_latency_high() {
        let samples: Vec<Duration> = (1..=100).map(|i| Duration::from_micros(i)).collect();
        let stats = LatencyStats::from_samples(&samples);

        assert!(stats.is_latency_high(50)); // p99 (99) > 50
        assert!(!stats.is_latency_high(100)); // p99 (99) <= 100
    }

    #[test]
    fn test_latency_stats_stddev() {
        let samples: Vec<Duration> = vec![
            Duration::from_micros(100),
            Duration::from_micros(100),
            Duration::from_micros(100),
        ];
        let stats = LatencyStats::from_samples(&samples);
        assert_eq!(stats.stddev_us, 0); // All same values, no deviation
    }

    // ========================================================================
    // Timer Tests
    // ========================================================================

    #[test]
    fn test_timer_start_stop() {
        let timer = Timer::start();
        std::thread::sleep(Duration::from_millis(10));
        let elapsed = timer.stop();
        assert!(elapsed >= Duration::from_millis(10));
    }

    #[test]
    fn test_timer_elapsed() {
        let timer = Timer::start();
        std::thread::sleep(Duration::from_millis(5));
        let elapsed1 = timer.elapsed();
        std::thread::sleep(Duration::from_millis(5));
        let elapsed2 = timer.elapsed();
        assert!(elapsed2 > elapsed1);
    }

    #[test]
    fn test_timer_reset() {
        let mut timer = Timer::start();
        std::thread::sleep(Duration::from_millis(10));
        timer.reset();
        let elapsed = timer.elapsed();
        assert!(elapsed < Duration::from_millis(10));
    }

    #[test]
    fn test_timer_default() {
        let timer = Timer::default();
        assert!(timer.elapsed() >= Duration::ZERO);
    }

    // ========================================================================
    // RequestTimer Tests
    // ========================================================================

    #[test]
    fn test_request_timer_drop_records() {
        let metrics = MetricsCollector::new();
        {
            let _timer = metrics.time_request("test_proto", "test_op");
            std::thread::sleep(Duration::from_millis(5));
        }
        // Duration should be recorded on drop
        // We verify this works by checking the prometheus export contains the metric
        let output = metrics.export_prometheus();
        assert!(output.contains(&format!("{}_request_duration_seconds", METRIC_PREFIX)));
    }

    #[test]
    fn test_request_timer_manual_record() {
        let metrics = MetricsCollector::new();
        let timer = metrics.time_request("test_proto", "manual_test");
        std::thread::sleep(Duration::from_millis(5));
        let elapsed = timer.record();
        assert!(elapsed >= Duration::from_millis(5));
    }

    #[test]
    fn test_request_timer_discard() {
        let metrics = MetricsCollector::new();
        let timer = metrics.time_request("test_proto", "discard_test");
        timer.discard(); // Should not record
    }

    // ========================================================================
    // Utility Function Tests
    // ========================================================================

    #[test]
    fn test_classify_error() {
        assert_eq!(classify_error(&"Connection timeout"), "timeout");
        assert_eq!(classify_error(&"Connection refused"), "connection");
        assert_eq!(classify_error(&"I/O error occurred"), "io");
        assert_eq!(classify_error(&"Protocol violation"), "protocol");
        assert_eq!(classify_error(&"Invalid message format"), "protocol");
        assert_eq!(classify_error(&"Device not found"), "not_found");
        assert_eq!(classify_error(&"Permission denied"), "permission");
        assert_eq!(classify_error(&"Some random error"), "unknown");
    }

    // ========================================================================
    // Constants Tests
    // ========================================================================

    #[test]
    fn test_metric_prefix() {
        assert_eq!(METRIC_PREFIX, "mabi");
    }

    #[test]
    fn test_latency_buckets() {
        assert!(!LATENCY_BUCKETS.is_empty());
        // Check buckets are sorted
        for i in 1..LATENCY_BUCKETS.len() {
            assert!(LATENCY_BUCKETS[i] > LATENCY_BUCKETS[i - 1]);
        }
    }

    #[test]
    fn test_tick_buckets() {
        assert!(!TICK_BUCKETS.is_empty());
        // Check buckets are sorted
        for i in 1..TICK_BUCKETS.len() {
            assert!(TICK_BUCKETS[i] > TICK_BUCKETS[i - 1]);
        }
    }
}

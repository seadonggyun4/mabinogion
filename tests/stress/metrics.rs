//! Metrics collection for stress testing.
//!
//! Provides detailed metrics and reporting for stress tests.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Real-time metrics collector with windowed statistics.
pub struct MetricsCollector {
    /// Window size for rolling statistics.
    window_size: usize,
    /// Latency samples in the current window.
    latencies: Mutex<VecDeque<Duration>>,
    /// Request timestamps for RPS calculation.
    request_times: Mutex<VecDeque<Instant>>,
    /// Total request count.
    total_requests: AtomicU64,
    /// Successful request count.
    successful_requests: AtomicU64,
    /// Failed request count.
    failed_requests: AtomicU64,
    /// Error counts by type.
    error_counts: Mutex<std::collections::HashMap<String, u64>>,
    /// Start time.
    start_time: Instant,
}

impl MetricsCollector {
    /// Create a new metrics collector.
    pub fn new(window_size: usize) -> Self {
        Self {
            window_size,
            latencies: Mutex::new(VecDeque::with_capacity(window_size)),
            request_times: Mutex::new(VecDeque::with_capacity(window_size)),
            total_requests: AtomicU64::new(0),
            successful_requests: AtomicU64::new(0),
            failed_requests: AtomicU64::new(0),
            error_counts: Mutex::new(std::collections::HashMap::new()),
            start_time: Instant::now(),
        }
    }

    /// Record a successful request.
    pub fn record_success(&self, latency: Duration) {
        self.total_requests.fetch_add(1, Ordering::Relaxed);
        self.successful_requests.fetch_add(1, Ordering::Relaxed);
        self.record_latency(latency);
        self.record_request_time();
    }

    /// Record a failed request.
    pub fn record_failure(&self, latency: Duration, error: &str) {
        self.total_requests.fetch_add(1, Ordering::Relaxed);
        self.failed_requests.fetch_add(1, Ordering::Relaxed);
        self.record_latency(latency);
        self.record_request_time();

        let mut errors = self.error_counts.lock().unwrap();
        *errors.entry(error.to_string()).or_insert(0) += 1;
    }

    fn record_latency(&self, latency: Duration) {
        let mut latencies = self.latencies.lock().unwrap();
        if latencies.len() >= self.window_size {
            latencies.pop_front();
        }
        latencies.push_back(latency);
    }

    fn record_request_time(&self) {
        let mut times = self.request_times.lock().unwrap();
        let now = Instant::now();
        if times.len() >= self.window_size {
            times.pop_front();
        }
        times.push_back(now);
    }

    /// Get current RPS (requests per second).
    pub fn current_rps(&self) -> f64 {
        let times = self.request_times.lock().unwrap();
        if times.len() < 2 {
            return 0.0;
        }

        let first = times.front().unwrap();
        let last = times.back().unwrap();
        let duration = last.duration_since(*first);

        if duration.is_zero() {
            return 0.0;
        }

        (times.len() - 1) as f64 / duration.as_secs_f64()
    }

    /// Get latency percentiles.
    pub fn latency_percentiles(&self) -> LatencyPercentiles {
        let mut latencies: Vec<Duration> = self.latencies.lock().unwrap().iter().cloned().collect();
        latencies.sort();

        if latencies.is_empty() {
            return LatencyPercentiles::default();
        }

        let len = latencies.len();
        let p50_idx = len / 2;
        let p95_idx = len * 95 / 100;
        let p99_idx = len * 99 / 100;

        LatencyPercentiles {
            min: latencies.first().cloned().unwrap_or_default(),
            p50: latencies.get(p50_idx).cloned().unwrap_or_default(),
            p95: latencies.get(p95_idx).cloned().unwrap_or_default(),
            p99: latencies.get(p99_idx).cloned().unwrap_or_default(),
            max: latencies.last().cloned().unwrap_or_default(),
            avg: latencies.iter().sum::<Duration>() / len as u32,
        }
    }

    /// Get a summary of current metrics.
    pub fn summary(&self) -> MetricsSummary {
        let total = self.total_requests.load(Ordering::Relaxed);
        let successful = self.successful_requests.load(Ordering::Relaxed);
        let failed = self.failed_requests.load(Ordering::Relaxed);
        let elapsed = self.start_time.elapsed();

        MetricsSummary {
            duration: elapsed,
            total_requests: total,
            successful_requests: successful,
            failed_requests: failed,
            success_rate: if total > 0 {
                successful as f64 / total as f64 * 100.0
            } else {
                0.0
            },
            overall_rps: if elapsed.as_secs() > 0 {
                total as f64 / elapsed.as_secs_f64()
            } else {
                0.0
            },
            current_rps: self.current_rps(),
            latency: self.latency_percentiles(),
            errors: self.error_counts.lock().unwrap().clone(),
        }
    }
}

/// Latency percentiles.
#[derive(Debug, Clone, Default)]
pub struct LatencyPercentiles {
    pub min: Duration,
    pub p50: Duration,
    pub p95: Duration,
    pub p99: Duration,
    pub max: Duration,
    pub avg: Duration,
}

impl LatencyPercentiles {
    pub fn print(&self) {
        println!("Latency Percentiles:");
        println!("  Min: {:?}", self.min);
        println!("  P50: {:?}", self.p50);
        println!("  P95: {:?}", self.p95);
        println!("  P99: {:?}", self.p99);
        println!("  Max: {:?}", self.max);
        println!("  Avg: {:?}", self.avg);
    }
}

/// Metrics summary.
#[derive(Debug, Clone)]
pub struct MetricsSummary {
    pub duration: Duration,
    pub total_requests: u64,
    pub successful_requests: u64,
    pub failed_requests: u64,
    pub success_rate: f64,
    pub overall_rps: f64,
    pub current_rps: f64,
    pub latency: LatencyPercentiles,
    pub errors: std::collections::HashMap<String, u64>,
}

impl MetricsSummary {
    pub fn print(&self) {
        println!("\n{}", "=".repeat(60));
        println!("STRESS TEST SUMMARY");
        println!("{}", "=".repeat(60));
        println!("\nOverall:");
        println!("  Duration:         {:?}", self.duration);
        println!("  Total Requests:   {}", self.total_requests);
        println!("  Successful:       {}", self.successful_requests);
        println!("  Failed:           {}", self.failed_requests);
        println!("  Success Rate:     {:.2}%", self.success_rate);
        println!("  Overall RPS:      {:.2}", self.overall_rps);
        println!("  Current RPS:      {:.2}", self.current_rps);
        println!();
        self.latency.print();

        if !self.errors.is_empty() {
            println!("\nError Breakdown:");
            for (error, count) in &self.errors {
                println!("  {}: {}", error, count);
            }
        }
        println!("{}", "=".repeat(60));
    }

    /// Check if test passed based on thresholds.
    pub fn passed(&self, thresholds: &Thresholds) -> bool {
        self.success_rate >= thresholds.min_success_rate
            && self.latency.p95 <= thresholds.max_p95_latency
            && self.latency.p99 <= thresholds.max_p99_latency
    }
}

/// Performance thresholds for pass/fail determination.
#[derive(Debug, Clone)]
pub struct Thresholds {
    pub min_success_rate: f64,
    pub max_p95_latency: Duration,
    pub max_p99_latency: Duration,
    pub min_rps: f64,
}

impl Default for Thresholds {
    fn default() -> Self {
        Self {
            min_success_rate: 99.0,
            max_p95_latency: Duration::from_millis(100),
            max_p99_latency: Duration::from_millis(500),
            min_rps: 1000.0,
        }
    }
}

impl Thresholds {
    pub fn strict() -> Self {
        Self {
            min_success_rate: 99.9,
            max_p95_latency: Duration::from_millis(10),
            max_p99_latency: Duration::from_millis(50),
            min_rps: 10000.0,
        }
    }

    pub fn relaxed() -> Self {
        Self {
            min_success_rate: 95.0,
            max_p95_latency: Duration::from_millis(500),
            max_p99_latency: Duration::from_secs(1),
            min_rps: 100.0,
        }
    }
}

/// Live metrics reporter.
pub struct LiveReporter {
    collector: Arc<MetricsCollector>,
    interval: Duration,
}

impl LiveReporter {
    pub fn new(collector: Arc<MetricsCollector>, interval: Duration) -> Self {
        Self { collector, interval }
    }

    /// Start reporting metrics at regular intervals.
    pub async fn start(&self, duration: Duration) {
        let start = Instant::now();

        while start.elapsed() < duration {
            tokio::time::sleep(self.interval).await;

            let summary = self.collector.summary();
            println!(
                "[{:>6.1}s] RPS: {:>8.1} | Success: {:>6.2}% | P95: {:>6.1}ms | P99: {:>6.1}ms",
                summary.duration.as_secs_f64(),
                summary.current_rps,
                summary.success_rate,
                summary.latency.p95.as_millis(),
                summary.latency.p99.as_millis(),
            );
        }
    }
}

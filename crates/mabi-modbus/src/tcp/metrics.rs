//! Metrics collection for Modbus TCP server.
//!
//! This module provides Prometheus-compatible metrics for monitoring
//! server performance and behavior.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

/// Histogram bucket boundaries for latency measurements (in microseconds).
const LATENCY_BUCKETS_US: &[u64] = &[
    50,     // 50 µs
    100,    // 100 µs
    250,    // 250 µs
    500,    // 500 µs
    1000,   // 1 ms
    2500,   // 2.5 ms
    5000,   // 5 ms
    10000,  // 10 ms
    25000,  // 25 ms
    50000,  // 50 ms
    100000, // 100 ms
];
const LATENCY_BUCKET_COUNT: usize = LATENCY_BUCKETS_US.len() + 1;

/// Metrics for the Modbus TCP server.
#[derive(Debug)]
pub struct ServerMetrics {
    /// Total connections accepted.
    pub connections_total: AtomicU64,

    /// Connections currently active.
    pub connections_active: AtomicU64,

    /// Total connections rejected (due to limit).
    pub connections_rejected: AtomicU64,

    /// Total requests received.
    pub requests_total: AtomicU64,

    /// Requests by function code.
    pub requests_by_function: Box<[AtomicU64; 256]>,

    /// Total successful responses.
    pub responses_success: AtomicU64,

    /// Total exception responses.
    pub responses_exception: AtomicU64,

    /// Total errors (internal, not exceptions).
    pub errors_total: AtomicU64,

    /// Frame decode errors.
    pub frame_errors: AtomicU64,

    /// Request timeout errors.
    pub timeout_errors: AtomicU64,

    /// Total bytes received.
    pub bytes_received: AtomicU64,

    /// Total bytes sent.
    pub bytes_sent: AtomicU64,

    /// Latency histogram buckets.
    latency_buckets: Box<[AtomicU64; LATENCY_BUCKET_COUNT]>,

    /// Sum of all latencies (for calculating average).
    latency_sum_us: AtomicU64,

    /// Server start time.
    start_time: Instant,
}

impl ServerMetrics {
    /// Create new server metrics.
    pub fn new() -> Self {
        Self {
            connections_total: AtomicU64::new(0),
            connections_active: AtomicU64::new(0),
            connections_rejected: AtomicU64::new(0),
            requests_total: AtomicU64::new(0),
            requests_by_function: Box::new(std::array::from_fn(|_| AtomicU64::new(0))),
            responses_success: AtomicU64::new(0),
            responses_exception: AtomicU64::new(0),
            errors_total: AtomicU64::new(0),
            frame_errors: AtomicU64::new(0),
            timeout_errors: AtomicU64::new(0),
            bytes_received: AtomicU64::new(0),
            bytes_sent: AtomicU64::new(0),
            latency_buckets: Box::new(std::array::from_fn(|_| AtomicU64::new(0))),
            latency_sum_us: AtomicU64::new(0),
            start_time: Instant::now(),
        }
    }

    /// Record a new connection.
    pub fn record_connection(&self) {
        self.connections_total.fetch_add(1, Ordering::Relaxed);
        self.connections_active.fetch_add(1, Ordering::Relaxed);
    }

    /// Record a connection close.
    pub fn record_disconnection(&self) {
        self.connections_active.fetch_sub(1, Ordering::Relaxed);
    }

    /// Record a rejected connection.
    pub fn record_connection_rejected(&self) {
        self.connections_rejected.fetch_add(1, Ordering::Relaxed);
    }

    /// Record a request.
    pub fn record_request(&self, function_code: u8) {
        self.record_request_with_options(function_code, true);
    }

    /// Record a request with optional per-function breakdown.
    pub fn record_request_with_options(&self, function_code: u8, detailed_breakdown: bool) {
        self.requests_total.fetch_add(1, Ordering::Relaxed);

        if detailed_breakdown {
            self.requests_by_function[function_code as usize].fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Record a successful response.
    pub fn record_success(&self, latency_us: u64, bytes_in: u64, bytes_out: u64) {
        self.record_success_with_options(latency_us, bytes_in, bytes_out, true);
    }

    /// Record a successful response with optional latency sampling.
    pub fn record_success_with_options(
        &self,
        latency_us: u64,
        bytes_in: u64,
        bytes_out: u64,
        record_latency: bool,
    ) {
        self.responses_success.fetch_add(1, Ordering::Relaxed);
        self.bytes_received.fetch_add(bytes_in, Ordering::Relaxed);
        self.bytes_sent.fetch_add(bytes_out, Ordering::Relaxed);
        if record_latency {
            self.record_latency(latency_us);
        }
    }

    /// Record an exception response.
    pub fn record_exception(&self, latency_us: u64, bytes_in: u64, bytes_out: u64) {
        self.record_exception_with_options(latency_us, bytes_in, bytes_out, true);
    }

    /// Record an exception response with optional latency sampling.
    pub fn record_exception_with_options(
        &self,
        latency_us: u64,
        bytes_in: u64,
        bytes_out: u64,
        record_latency: bool,
    ) {
        self.responses_exception.fetch_add(1, Ordering::Relaxed);
        self.bytes_received.fetch_add(bytes_in, Ordering::Relaxed);
        self.bytes_sent.fetch_add(bytes_out, Ordering::Relaxed);
        if record_latency {
            self.record_latency(latency_us);
        }
    }

    /// Record an internal error.
    pub fn record_error(&self) {
        self.errors_total.fetch_add(1, Ordering::Relaxed);
    }

    /// Record a frame decode error.
    pub fn record_frame_error(&self) {
        self.frame_errors.fetch_add(1, Ordering::Relaxed);
    }

    /// Record a timeout.
    pub fn record_timeout(&self) {
        self.timeout_errors.fetch_add(1, Ordering::Relaxed);
    }

    /// Record latency in histogram.
    fn record_latency(&self, latency_us: u64) {
        self.latency_sum_us.fetch_add(latency_us, Ordering::Relaxed);

        // Find the appropriate bucket
        for (i, &boundary) in LATENCY_BUCKETS_US.iter().enumerate() {
            if latency_us <= boundary {
                self.latency_buckets[i].fetch_add(1, Ordering::Relaxed);
                return;
            }
        }

        // +Inf bucket
        self.latency_buckets[LATENCY_BUCKETS_US.len()].fetch_add(1, Ordering::Relaxed);
    }

    /// Get server uptime.
    pub fn uptime(&self) -> std::time::Duration {
        self.start_time.elapsed()
    }

    /// Get requests per second.
    pub fn requests_per_second(&self) -> f64 {
        let total = self.requests_total.load(Ordering::Relaxed) as f64;
        let uptime = self.uptime().as_secs_f64();
        if uptime > 0.0 {
            total / uptime
        } else {
            0.0
        }
    }

    /// Get average latency in microseconds.
    pub fn average_latency_us(&self) -> f64 {
        let total_responses = self.responses_success.load(Ordering::Relaxed)
            + self.responses_exception.load(Ordering::Relaxed);
        let sum = self.latency_sum_us.load(Ordering::Relaxed);

        if total_responses > 0 {
            sum as f64 / total_responses as f64
        } else {
            0.0
        }
    }

    /// Get latency percentile (approximate).
    pub fn latency_percentile(&self, percentile: f64) -> u64 {
        let total_responses = self.responses_success.load(Ordering::Relaxed)
            + self.responses_exception.load(Ordering::Relaxed);

        if total_responses == 0 {
            return 0;
        }

        let target = ((total_responses as f64) * percentile / 100.0).ceil() as u64;

        let mut cumulative = 0u64;

        for (i, bucket) in self.latency_buckets.iter().enumerate() {
            cumulative += bucket.load(Ordering::Relaxed);
            if cumulative >= target {
                if i < LATENCY_BUCKETS_US.len() {
                    return LATENCY_BUCKETS_US[i];
                } else {
                    return LATENCY_BUCKETS_US[LATENCY_BUCKETS_US.len() - 1] * 2;
                    // Beyond max
                }
            }
        }

        0
    }

    /// Get a snapshot of all metrics.
    pub fn snapshot(&self) -> MetricsSnapshot {
        MetricsSnapshot {
            connections_total: self.connections_total.load(Ordering::Relaxed),
            connections_active: self.connections_active.load(Ordering::Relaxed),
            connections_rejected: self.connections_rejected.load(Ordering::Relaxed),
            requests_total: self.requests_total.load(Ordering::Relaxed),
            responses_success: self.responses_success.load(Ordering::Relaxed),
            responses_exception: self.responses_exception.load(Ordering::Relaxed),
            errors_total: self.errors_total.load(Ordering::Relaxed),
            frame_errors: self.frame_errors.load(Ordering::Relaxed),
            timeout_errors: self.timeout_errors.load(Ordering::Relaxed),
            bytes_received: self.bytes_received.load(Ordering::Relaxed),
            bytes_sent: self.bytes_sent.load(Ordering::Relaxed),
            uptime_secs: self.uptime().as_secs_f64(),
            requests_per_second: self.requests_per_second(),
            avg_latency_us: self.average_latency_us(),
            p50_latency_us: self.latency_percentile(50.0),
            p95_latency_us: self.latency_percentile(95.0),
            p99_latency_us: self.latency_percentile(99.0),
        }
    }

    /// Export metrics in Prometheus text format.
    pub fn to_prometheus(&self) -> String {
        let snap = self.snapshot();

        let mut output = String::new();

        // Connections
        output.push_str(&format!(
            "# HELP modbus_connections_total Total connections accepted\n\
             # TYPE modbus_connections_total counter\n\
             modbus_connections_total {}\n\n",
            snap.connections_total
        ));

        output.push_str(&format!(
            "# HELP modbus_connections_active Currently active connections\n\
             # TYPE modbus_connections_active gauge\n\
             modbus_connections_active {}\n\n",
            snap.connections_active
        ));

        output.push_str(&format!(
            "# HELP modbus_connections_rejected Total rejected connections\n\
             # TYPE modbus_connections_rejected counter\n\
             modbus_connections_rejected {}\n\n",
            snap.connections_rejected
        ));

        // Requests
        output.push_str(&format!(
            "# HELP modbus_requests_total Total requests received\n\
             # TYPE modbus_requests_total counter\n\
             modbus_requests_total {}\n\n",
            snap.requests_total
        ));

        // Responses
        output.push_str(&format!(
            "# HELP modbus_responses_total Total responses sent\n\
             # TYPE modbus_responses_total counter\n\
             modbus_responses_total{{status=\"success\"}} {}\n\
             modbus_responses_total{{status=\"exception\"}} {}\n\n",
            snap.responses_success, snap.responses_exception
        ));

        // Errors
        output.push_str(&format!(
            "# HELP modbus_errors_total Total errors\n\
             # TYPE modbus_errors_total counter\n\
             modbus_errors_total{{type=\"internal\"}} {}\n\
             modbus_errors_total{{type=\"frame\"}} {}\n\
             modbus_errors_total{{type=\"timeout\"}} {}\n\n",
            snap.errors_total, snap.frame_errors, snap.timeout_errors
        ));

        // Bytes
        output.push_str(&format!(
            "# HELP modbus_bytes_total Total bytes transferred\n\
             # TYPE modbus_bytes_total counter\n\
             modbus_bytes_total{{direction=\"received\"}} {}\n\
             modbus_bytes_total{{direction=\"sent\"}} {}\n\n",
            snap.bytes_received, snap.bytes_sent
        ));

        // Latency
        output.push_str(&format!(
            "# HELP modbus_request_duration_seconds Request duration histogram\n\
             # TYPE modbus_request_duration_seconds summary\n\
             modbus_request_duration_seconds{{quantile=\"0.5\"}} {:.6}\n\
             modbus_request_duration_seconds{{quantile=\"0.95\"}} {:.6}\n\
             modbus_request_duration_seconds{{quantile=\"0.99\"}} {:.6}\n\
             modbus_request_duration_seconds_sum {:.6}\n\
             modbus_request_duration_seconds_count {}\n\n",
            snap.p50_latency_us as f64 / 1_000_000.0,
            snap.p95_latency_us as f64 / 1_000_000.0,
            snap.p99_latency_us as f64 / 1_000_000.0,
            self.latency_sum_us.load(Ordering::Relaxed) as f64 / 1_000_000.0,
            snap.responses_success + snap.responses_exception
        ));

        output
    }
}

impl Default for ServerMetrics {
    fn default() -> Self {
        Self::new()
    }
}

/// Snapshot of server metrics at a point in time.
#[derive(Debug, Clone)]
pub struct MetricsSnapshot {
    pub connections_total: u64,
    pub connections_active: u64,
    pub connections_rejected: u64,
    pub requests_total: u64,
    pub responses_success: u64,
    pub responses_exception: u64,
    pub errors_total: u64,
    pub frame_errors: u64,
    pub timeout_errors: u64,
    pub bytes_received: u64,
    pub bytes_sent: u64,
    pub uptime_secs: f64,
    pub requests_per_second: f64,
    pub avg_latency_us: f64,
    pub p50_latency_us: u64,
    pub p95_latency_us: u64,
    pub p99_latency_us: u64,
}

/// Timer for measuring request latency.
pub struct LatencyTimer {
    start: Instant,
}

impl LatencyTimer {
    /// Start a new latency timer.
    pub fn start() -> Self {
        Self {
            start: Instant::now(),
        }
    }

    /// Get elapsed time in microseconds.
    pub fn elapsed_us(&self) -> u64 {
        self.start.elapsed().as_micros() as u64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_metrics_connections() {
        let metrics = ServerMetrics::new();

        metrics.record_connection();
        metrics.record_connection();
        metrics.record_connection();
        metrics.record_disconnection();

        assert_eq!(metrics.connections_total.load(Ordering::Relaxed), 3);
        assert_eq!(metrics.connections_active.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn test_metrics_requests() {
        let metrics = ServerMetrics::new();

        metrics.record_request(0x03);
        metrics.record_request(0x03);
        metrics.record_request(0x06);

        assert_eq!(metrics.requests_total.load(Ordering::Relaxed), 3);

        assert_eq!(
            metrics.requests_by_function[0x03].load(Ordering::Relaxed),
            2
        );
        assert_eq!(
            metrics.requests_by_function[0x06].load(Ordering::Relaxed),
            1
        );
    }

    #[test]
    fn test_metrics_latency() {
        let metrics = ServerMetrics::new();

        // Record some requests with different latencies
        metrics.record_success(100, 10, 20); // 100 µs
        metrics.record_success(500, 10, 20); // 500 µs
        metrics.record_success(1000, 10, 20); // 1000 µs
        metrics.record_success(5000, 10, 20); // 5000 µs

        assert_eq!(metrics.responses_success.load(Ordering::Relaxed), 4);
        assert_eq!(metrics.average_latency_us(), 1650.0); // (100+500+1000+5000)/4
    }

    #[test]
    fn test_metrics_snapshot() {
        let metrics = ServerMetrics::new();

        metrics.record_connection();
        metrics.record_request(0x03);
        metrics.record_success(100, 10, 20);

        let snap = metrics.snapshot();

        assert_eq!(snap.connections_total, 1);
        assert_eq!(snap.connections_active, 1);
        assert_eq!(snap.requests_total, 1);
        assert_eq!(snap.responses_success, 1);
        assert_eq!(snap.bytes_received, 10);
        assert_eq!(snap.bytes_sent, 20);
    }

    #[test]
    fn test_latency_timer() {
        let timer = LatencyTimer::start();
        std::thread::sleep(std::time::Duration::from_millis(1));
        let elapsed = timer.elapsed_us();

        // Should be at least 1000 µs (1 ms)
        assert!(elapsed >= 1000);
    }

    #[test]
    fn test_prometheus_export() {
        let metrics = ServerMetrics::new();
        metrics.record_connection();
        metrics.record_request(0x03);
        metrics.record_success(100, 10, 20);

        let output = metrics.to_prometheus();

        assert!(output.contains("modbus_connections_total 1"));
        assert!(output.contains("modbus_requests_total 1"));
        assert!(output.contains("modbus_responses_total{status=\"success\"} 1"));
    }
}

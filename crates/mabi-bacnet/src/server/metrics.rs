//! Server metrics for BACnet/IP server.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

/// Server metrics for monitoring and diagnostics.
#[derive(Debug, Default)]
pub struct ServerMetrics {
    /// Total requests received.
    pub requests_received: AtomicU64,
    /// Total requests processed successfully.
    pub requests_success: AtomicU64,
    /// Total requests that resulted in errors.
    pub requests_error: AtomicU64,
    /// Total confirmed service requests.
    pub confirmed_requests: AtomicU64,
    /// Total unconfirmed service requests.
    pub unconfirmed_requests: AtomicU64,
    /// Total Who-Is requests received.
    pub who_is_requests: AtomicU64,
    /// Total I-Am responses sent.
    pub i_am_sent: AtomicU64,
    /// Total ReadProperty requests.
    pub read_property_requests: AtomicU64,
    /// Total WriteProperty requests.
    pub write_property_requests: AtomicU64,
    /// Total COV subscriptions.
    pub cov_subscriptions: AtomicU64,
    /// Total COV notifications sent.
    pub cov_notifications_sent: AtomicU64,
    /// Total bytes received.
    pub bytes_received: AtomicU64,
    /// Total bytes sent.
    pub bytes_sent: AtomicU64,
    /// Total latency in microseconds (for average calculation).
    pub total_latency_us: AtomicU64,
    /// Latency sample count.
    pub latency_samples: AtomicU64,
    /// Start time for uptime tracking.
    start_time: std::sync::OnceLock<Instant>,
}

impl ServerMetrics {
    /// Create a new metrics instance.
    pub fn new() -> Self {
        let metrics = Self::default();
        let _ = metrics.start_time.set(Instant::now());
        metrics
    }

    /// Record a received request.
    pub fn record_request(&self) {
        self.requests_received.fetch_add(1, Ordering::Relaxed);
    }

    /// Record a successful request.
    pub fn record_success(&self, latency_us: u64) {
        self.requests_success.fetch_add(1, Ordering::Relaxed);
        self.record_latency(latency_us);
    }

    /// Record an error.
    pub fn record_error(&self) {
        self.requests_error.fetch_add(1, Ordering::Relaxed);
    }

    /// Record a confirmed service request.
    pub fn record_confirmed_request(&self) {
        self.confirmed_requests.fetch_add(1, Ordering::Relaxed);
    }

    /// Record an unconfirmed service request.
    pub fn record_unconfirmed_request(&self) {
        self.unconfirmed_requests.fetch_add(1, Ordering::Relaxed);
    }

    /// Record a Who-Is request.
    pub fn record_who_is(&self) {
        self.who_is_requests.fetch_add(1, Ordering::Relaxed);
    }

    /// Record an I-Am sent.
    pub fn record_i_am_sent(&self) {
        self.i_am_sent.fetch_add(1, Ordering::Relaxed);
    }

    /// Record a ReadProperty request.
    pub fn record_read_property(&self) {
        self.read_property_requests.fetch_add(1, Ordering::Relaxed);
    }

    /// Record a WriteProperty request.
    pub fn record_write_property(&self) {
        self.write_property_requests.fetch_add(1, Ordering::Relaxed);
    }

    /// Record a COV subscription.
    pub fn record_cov_subscription(&self) {
        self.cov_subscriptions.fetch_add(1, Ordering::Relaxed);
    }

    /// Record a COV notification sent.
    pub fn record_cov_notification_sent(&self) {
        self.cov_notifications_sent.fetch_add(1, Ordering::Relaxed);
    }

    /// Record bytes received.
    pub fn record_bytes_received(&self, bytes: u64) {
        self.bytes_received.fetch_add(bytes, Ordering::Relaxed);
    }

    /// Record bytes sent.
    pub fn record_bytes_sent(&self, bytes: u64) {
        self.bytes_sent.fetch_add(bytes, Ordering::Relaxed);
    }

    /// Record latency sample.
    fn record_latency(&self, latency_us: u64) {
        self.total_latency_us.fetch_add(latency_us, Ordering::Relaxed);
        self.latency_samples.fetch_add(1, Ordering::Relaxed);
    }

    /// Get average latency in microseconds.
    pub fn average_latency_us(&self) -> u64 {
        let total = self.total_latency_us.load(Ordering::Relaxed);
        let samples = self.latency_samples.load(Ordering::Relaxed);
        if samples > 0 {
            total / samples
        } else {
            0
        }
    }

    /// Get uptime in seconds.
    pub fn uptime_secs(&self) -> u64 {
        self.start_time
            .get()
            .map(|t| t.elapsed().as_secs())
            .unwrap_or(0)
    }

    /// Get a snapshot of the metrics.
    pub fn snapshot(&self) -> ServerMetricsSnapshot {
        ServerMetricsSnapshot {
            requests_received: self.requests_received.load(Ordering::Relaxed),
            requests_success: self.requests_success.load(Ordering::Relaxed),
            requests_error: self.requests_error.load(Ordering::Relaxed),
            confirmed_requests: self.confirmed_requests.load(Ordering::Relaxed),
            unconfirmed_requests: self.unconfirmed_requests.load(Ordering::Relaxed),
            who_is_requests: self.who_is_requests.load(Ordering::Relaxed),
            i_am_sent: self.i_am_sent.load(Ordering::Relaxed),
            read_property_requests: self.read_property_requests.load(Ordering::Relaxed),
            write_property_requests: self.write_property_requests.load(Ordering::Relaxed),
            cov_subscriptions: self.cov_subscriptions.load(Ordering::Relaxed),
            cov_notifications_sent: self.cov_notifications_sent.load(Ordering::Relaxed),
            bytes_received: self.bytes_received.load(Ordering::Relaxed),
            bytes_sent: self.bytes_sent.load(Ordering::Relaxed),
            average_latency_us: self.average_latency_us(),
            uptime_secs: self.uptime_secs(),
        }
    }
}

/// Snapshot of server metrics.
#[derive(Debug, Clone, Default)]
pub struct ServerMetricsSnapshot {
    pub requests_received: u64,
    pub requests_success: u64,
    pub requests_error: u64,
    pub confirmed_requests: u64,
    pub unconfirmed_requests: u64,
    pub who_is_requests: u64,
    pub i_am_sent: u64,
    pub read_property_requests: u64,
    pub write_property_requests: u64,
    pub cov_subscriptions: u64,
    pub cov_notifications_sent: u64,
    pub bytes_received: u64,
    pub bytes_sent: u64,
    pub average_latency_us: u64,
    pub uptime_secs: u64,
}

impl ServerMetricsSnapshot {
    /// Get requests per second.
    pub fn requests_per_second(&self) -> f64 {
        if self.uptime_secs > 0 {
            self.requests_received as f64 / self.uptime_secs as f64
        } else {
            0.0
        }
    }

    /// Get success rate (0.0 - 1.0).
    pub fn success_rate(&self) -> f64 {
        let total = self.requests_success + self.requests_error;
        if total > 0 {
            self.requests_success as f64 / total as f64
        } else {
            1.0
        }
    }
}

/// Timer for measuring latency.
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
    fn test_server_metrics() {
        let metrics = ServerMetrics::new();

        metrics.record_request();
        metrics.record_request();
        metrics.record_success(100);
        metrics.record_error();

        let snapshot = metrics.snapshot();
        assert_eq!(snapshot.requests_received, 2);
        assert_eq!(snapshot.requests_success, 1);
        assert_eq!(snapshot.requests_error, 1);
    }

    #[test]
    fn test_average_latency() {
        let metrics = ServerMetrics::new();

        metrics.record_success(100);
        metrics.record_success(200);
        metrics.record_success(300);

        assert_eq!(metrics.average_latency_us(), 200);
    }

    #[test]
    fn test_success_rate() {
        let metrics = ServerMetrics::new();

        metrics.record_success(100);
        metrics.record_success(100);
        metrics.record_success(100);
        metrics.record_error();

        let snapshot = metrics.snapshot();
        assert!((snapshot.success_rate() - 0.75).abs() < 0.001);
    }
}

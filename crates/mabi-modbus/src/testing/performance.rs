//! Performance validation framework for large-scale testing.
//!
//! Provides infrastructure to validate:
//! - 10,000+ simultaneous connections
//! - 100,000+ transactions per second
//! - Sub-10ms P99 latency under load

use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::RwLock;
use tokio::net::TcpStream;
use tokio::sync::{Semaphore, broadcast};
use tokio::time::timeout;

use super::report::TestMetrics;

/// Performance validation configuration.
#[derive(Debug, Clone)]
pub struct PerformanceConfig {
    /// Target number of simultaneous connections.
    pub target_connections: usize,
    /// Target transactions per second.
    pub target_tps: u64,
    /// Test duration.
    pub duration: Duration,
    /// Connection ramp-up time.
    pub ramp_up: Duration,
    /// Server address to test.
    pub server_addr: SocketAddr,
    /// Maximum acceptable P99 latency.
    pub max_p99_latency: Duration,
    /// Maximum acceptable error rate (0.0 - 1.0).
    pub max_error_rate: f64,
    /// Number of warmup iterations.
    pub warmup_iterations: usize,
    /// Report interval for progress updates.
    pub report_interval: Duration,
}

impl Default for PerformanceConfig {
    fn default() -> Self {
        Self {
            target_connections: 1000,
            target_tps: 10_000,
            duration: Duration::from_secs(60),
            ramp_up: Duration::from_secs(10),
            server_addr: "127.0.0.1:502".parse().unwrap(),
            max_p99_latency: Duration::from_millis(10),
            max_error_rate: 0.01,
            warmup_iterations: 100,
            report_interval: Duration::from_secs(5),
        }
    }
}

impl PerformanceConfig {
    /// Create configuration for 10K connection test.
    pub fn ten_thousand_connections() -> Self {
        Self {
            target_connections: 10_000,
            target_tps: 50_000,
            duration: Duration::from_secs(120),
            ramp_up: Duration::from_secs(30),
            max_p99_latency: Duration::from_millis(50),
            ..Default::default()
        }
    }

    /// Create configuration for 100K TPS test.
    pub fn hundred_thousand_tps() -> Self {
        Self {
            target_connections: 5_000,
            target_tps: 100_000,
            duration: Duration::from_secs(60),
            ramp_up: Duration::from_secs(15),
            max_p99_latency: Duration::from_millis(10),
            ..Default::default()
        }
    }

    /// Create configuration for stress test.
    pub fn stress_test() -> Self {
        Self {
            target_connections: 50_000,
            target_tps: 500_000,
            duration: Duration::from_secs(300),
            ramp_up: Duration::from_secs(60),
            max_p99_latency: Duration::from_millis(100),
            max_error_rate: 0.05,
            ..Default::default()
        }
    }

    /// Set target connections.
    pub fn with_connections(mut self, n: usize) -> Self {
        self.target_connections = n;
        self
    }

    /// Set target TPS.
    pub fn with_tps(mut self, tps: u64) -> Self {
        self.target_tps = tps;
        self
    }

    /// Set test duration.
    pub fn with_duration(mut self, duration: Duration) -> Self {
        self.duration = duration;
        self
    }

    /// Set server address.
    pub fn with_server(mut self, addr: SocketAddr) -> Self {
        self.server_addr = addr;
        self
    }
}

/// Performance targets to validate.
#[derive(Debug, Clone)]
pub struct PerformanceTarget {
    pub name: String,
    pub metric: TargetMetric,
    pub threshold: f64,
    pub comparison: Comparison,
}

#[derive(Debug, Clone, Copy)]
pub enum TargetMetric {
    Connections,
    Tps,
    P50Latency,
    P95Latency,
    P99Latency,
    ErrorRate,
    MemoryMb,
}

#[derive(Debug, Clone, Copy)]
pub enum Comparison {
    GreaterThan,
    LessThan,
    GreaterOrEqual,
    LessOrEqual,
}

impl PerformanceTarget {
    pub fn min_connections(n: usize) -> Self {
        Self {
            name: format!("Min {} connections", n),
            metric: TargetMetric::Connections,
            threshold: n as f64,
            comparison: Comparison::GreaterOrEqual,
        }
    }

    pub fn min_tps(tps: u64) -> Self {
        Self {
            name: format!("Min {} TPS", tps),
            metric: TargetMetric::Tps,
            threshold: tps as f64,
            comparison: Comparison::GreaterOrEqual,
        }
    }

    pub fn max_p99_latency(ms: u64) -> Self {
        Self {
            name: format!("Max P99 latency {}ms", ms),
            metric: TargetMetric::P99Latency,
            threshold: ms as f64,
            comparison: Comparison::LessOrEqual,
        }
    }

    pub fn max_error_rate(rate: f64) -> Self {
        Self {
            name: format!("Max error rate {:.1}%", rate * 100.0),
            metric: TargetMetric::ErrorRate,
            threshold: rate,
            comparison: Comparison::LessOrEqual,
        }
    }

    /// Check if the target is met.
    pub fn check(&self, value: f64) -> bool {
        match self.comparison {
            Comparison::GreaterThan => value > self.threshold,
            Comparison::LessThan => value < self.threshold,
            Comparison::GreaterOrEqual => value >= self.threshold,
            Comparison::LessOrEqual => value <= self.threshold,
        }
    }
}

/// Result of a single target validation.
#[derive(Debug, Clone)]
pub struct TargetResult {
    pub target: PerformanceTarget,
    pub actual_value: f64,
    pub passed: bool,
}

/// Validation result containing all target results.
#[derive(Debug, Clone)]
pub struct ValidationResult {
    pub passed: bool,
    pub targets: Vec<TargetResult>,
    pub metrics: TestMetrics,
    pub duration: Duration,
    pub error_message: Option<String>,
}

impl ValidationResult {
    /// Get a summary of the validation.
    pub fn summary(&self) -> String {
        let passed_count = self.targets.iter().filter(|t| t.passed).count();
        let total = self.targets.len();
        format!(
            "{} ({}/{} targets passed)",
            if self.passed { "PASSED" } else { "FAILED" },
            passed_count,
            total
        )
    }
}

/// Performance validator for running large-scale tests.
pub struct PerformanceValidator {
    /// Performance configuration (public for test access).
    pub config: PerformanceConfig,
    targets: Vec<PerformanceTarget>,
    running: Arc<AtomicBool>,
    metrics: Arc<LiveMetrics>,
}

/// Live metrics collector during test execution.
struct LiveMetrics {
    requests_total: AtomicU64,
    requests_success: AtomicU64,
    requests_failed: AtomicU64,
    connections_active: AtomicU64,
    connections_total: AtomicU64,
    latencies: RwLock<Vec<Duration>>,
}

impl LiveMetrics {
    fn new() -> Self {
        Self {
            requests_total: AtomicU64::new(0),
            requests_success: AtomicU64::new(0),
            requests_failed: AtomicU64::new(0),
            connections_active: AtomicU64::new(0),
            connections_total: AtomicU64::new(0),
            latencies: RwLock::new(Vec::with_capacity(100_000)),
        }
    }

    fn record_request(&self, success: bool, latency: Duration) {
        self.requests_total.fetch_add(1, Ordering::Relaxed);
        if success {
            self.requests_success.fetch_add(1, Ordering::Relaxed);
        } else {
            self.requests_failed.fetch_add(1, Ordering::Relaxed);
        }

        // Sample latencies (keep every Nth to manage memory)
        let total = self.requests_total.load(Ordering::Relaxed);
        if total % 100 == 0 {
            self.latencies.write().push(latency);
        }
    }

    fn add_connection(&self) {
        self.connections_active.fetch_add(1, Ordering::Relaxed);
        self.connections_total.fetch_add(1, Ordering::Relaxed);
    }

    fn remove_connection(&self) {
        self.connections_active.fetch_sub(1, Ordering::Relaxed);
    }

    fn snapshot(&self) -> MetricsSnapshot {
        let mut latencies = self.latencies.read().clone();
        latencies.sort();

        let p50 = latencies.get(latencies.len() / 2).copied();
        let p95 = latencies.get(latencies.len() * 95 / 100).copied();
        let p99 = latencies.get(latencies.len() * 99 / 100).copied();

        let total = self.requests_total.load(Ordering::Relaxed);
        let success = self.requests_success.load(Ordering::Relaxed);
        let failed = self.requests_failed.load(Ordering::Relaxed);

        MetricsSnapshot {
            requests_total: total,
            requests_success: success,
            requests_failed: failed,
            connections_active: self.connections_active.load(Ordering::Relaxed),
            connections_total: self.connections_total.load(Ordering::Relaxed),
            p50_latency: p50,
            p95_latency: p95,
            p99_latency: p99,
            error_rate: if total > 0 { failed as f64 / total as f64 } else { 0.0 },
        }
    }
}

#[derive(Debug, Clone)]
struct MetricsSnapshot {
    requests_total: u64,
    requests_success: u64,
    requests_failed: u64,
    connections_active: u64,
    connections_total: u64,
    p50_latency: Option<Duration>,
    p95_latency: Option<Duration>,
    p99_latency: Option<Duration>,
    error_rate: f64,
}

impl PerformanceValidator {
    /// Create a new performance validator.
    pub fn new(config: PerformanceConfig) -> Self {
        Self {
            config,
            targets: Vec::new(),
            running: Arc::new(AtomicBool::new(false)),
            metrics: Arc::new(LiveMetrics::new()),
        }
    }

    /// Create validator for 10K connection test.
    pub fn ten_thousand_connections() -> Self {
        let config = PerformanceConfig::ten_thousand_connections();
        let mut validator = Self::new(config);
        validator.add_target(PerformanceTarget::min_connections(10_000));
        validator.add_target(PerformanceTarget::max_p99_latency(50));
        validator.add_target(PerformanceTarget::max_error_rate(0.01));
        validator
    }

    /// Create validator for 100K TPS test.
    pub fn hundred_thousand_tps() -> Self {
        let config = PerformanceConfig::hundred_thousand_tps();
        let mut validator = Self::new(config);
        validator.add_target(PerformanceTarget::min_tps(100_000));
        validator.add_target(PerformanceTarget::max_p99_latency(10));
        validator.add_target(PerformanceTarget::max_error_rate(0.01));
        validator
    }

    /// Add a performance target.
    pub fn add_target(&mut self, target: PerformanceTarget) {
        self.targets.push(target);
    }

    /// Set server address.
    pub fn server_addr(mut self, addr: SocketAddr) -> Self {
        self.config.server_addr = addr;
        self
    }

    /// Run the performance validation test.
    pub async fn run(&self) -> Result<ValidationResult, String> {
        tracing::info!(
            "Starting performance validation: {} connections, {} TPS target",
            self.config.target_connections,
            self.config.target_tps
        );

        self.running.store(true, Ordering::SeqCst);
        let start = Instant::now();

        // Spawn progress reporter
        let metrics = self.metrics.clone();
        let report_interval = self.config.report_interval;
        let running = self.running.clone();
        let reporter = tokio::spawn(async move {
            while running.load(Ordering::Relaxed) {
                tokio::time::sleep(report_interval).await;
                let snapshot = metrics.snapshot();
                tracing::info!(
                    "Progress: {} requests ({} success, {} failed), {} active connections, P99: {:?}",
                    snapshot.requests_total,
                    snapshot.requests_success,
                    snapshot.requests_failed,
                    snapshot.connections_active,
                    snapshot.p99_latency,
                );
            }
        });

        // Run the test
        let result = self.run_test().await;

        // Stop reporter
        self.running.store(false, Ordering::SeqCst);
        let _ = reporter.await;

        let duration = start.elapsed();

        // Build final metrics
        let snapshot = self.metrics.snapshot();
        let tps = if duration.as_secs() > 0 {
            snapshot.requests_total / duration.as_secs()
        } else {
            0
        };

        // Validate targets
        let mut target_results = Vec::new();
        let mut all_passed = true;

        for target in &self.targets {
            let value = match target.metric {
                TargetMetric::Connections => snapshot.connections_total as f64,
                TargetMetric::Tps => tps as f64,
                TargetMetric::P50Latency => {
                    snapshot.p50_latency.map(|d| d.as_millis() as f64).unwrap_or(0.0)
                }
                TargetMetric::P95Latency => {
                    snapshot.p95_latency.map(|d| d.as_millis() as f64).unwrap_or(0.0)
                }
                TargetMetric::P99Latency => {
                    snapshot.p99_latency.map(|d| d.as_millis() as f64).unwrap_or(0.0)
                }
                TargetMetric::ErrorRate => snapshot.error_rate,
                TargetMetric::MemoryMb => 0.0, // TODO: Integrate with memory profiler
            };

            let passed = target.check(value);
            if !passed {
                all_passed = false;
            }

            target_results.push(TargetResult {
                target: target.clone(),
                actual_value: value,
                passed,
            });
        }

        let test_metrics = TestMetrics {
            total_requests: snapshot.requests_total,
            successful_requests: snapshot.requests_success,
            failed_requests: snapshot.requests_failed,
            total_connections: snapshot.connections_total,
            peak_connections: snapshot.connections_active, // Approximation
            avg_tps: tps,
            peak_tps: tps, // Would need more tracking for accurate peak
            p50_latency_ms: snapshot.p50_latency.map(|d| d.as_millis() as f64).unwrap_or(0.0),
            p95_latency_ms: snapshot.p95_latency.map(|d| d.as_millis() as f64).unwrap_or(0.0),
            p99_latency_ms: snapshot.p99_latency.map(|d| d.as_millis() as f64).unwrap_or(0.0),
            error_rate: snapshot.error_rate,
            memory_peak_mb: 0.0, // TODO: Integrate with memory profiler
        };

        Ok(ValidationResult {
            passed: all_passed && result.is_ok(),
            targets: target_results,
            metrics: test_metrics,
            duration,
            error_message: result.err(),
        })
    }

    async fn run_test(&self) -> Result<(), String> {
        let semaphore = Arc::new(Semaphore::new(self.config.target_connections));
        let (shutdown_tx, _) = broadcast::channel::<()>(1);

        // Calculate request rate per connection
        let connections = self.config.target_connections;
        let target_tps = self.config.target_tps;
        let requests_per_connection = (target_tps as f64 / connections as f64).max(1.0);
        let request_interval = Duration::from_secs_f64(1.0 / requests_per_connection);

        // Spawn connection tasks with ramp-up
        let ramp_up_interval = self.config.ramp_up.as_millis() as usize / connections.max(1);
        let mut handles = Vec::with_capacity(connections);

        for i in 0..connections {
            if !self.running.load(Ordering::Relaxed) {
                break;
            }

            // Ramp-up delay
            if ramp_up_interval > 0 && i > 0 {
                tokio::time::sleep(Duration::from_millis(ramp_up_interval as u64)).await;
            }

            let permit = semaphore.clone().acquire_owned().await.map_err(|e| e.to_string())?;
            let metrics = self.metrics.clone();
            let server_addr = self.config.server_addr;
            let duration = self.config.duration;
            let running = self.running.clone();
            let mut shutdown_rx = shutdown_tx.subscribe();

            let handle = tokio::spawn(async move {
                let _permit = permit;

                // Connect to server
                let stream = match timeout(Duration::from_secs(10), TcpStream::connect(server_addr)).await {
                    Ok(Ok(s)) => s,
                    Ok(Err(e)) => {
                        tracing::debug!("Connection failed: {}", e);
                        return;
                    }
                    Err(_) => {
                        tracing::debug!("Connection timeout");
                        return;
                    }
                };

                metrics.add_connection();
                let start = Instant::now();

                // Send requests until duration expires or shutdown
                while running.load(Ordering::Relaxed) && start.elapsed() < duration {
                    tokio::select! {
                        _ = shutdown_rx.recv() => break,
                        _ = tokio::time::sleep(request_interval) => {
                            let req_start = Instant::now();
                            let success = Self::send_modbus_request(&stream).await;
                            let latency = req_start.elapsed();
                            metrics.record_request(success, latency);
                        }
                    }
                }

                metrics.remove_connection();
            });

            handles.push(handle);
        }

        // Wait for test duration
        tokio::time::sleep(self.config.duration).await;

        // Signal shutdown
        let _ = shutdown_tx.send(());
        self.running.store(false, Ordering::SeqCst);

        // Wait for all connections to close
        for handle in handles {
            let _ = handle.await;
        }

        Ok(())
    }

    /// Send a simple Modbus read request.
    async fn send_modbus_request(stream: &TcpStream) -> bool {
        #![allow(unused_imports)]
        use tokio::io::AsyncWriteExt;

        // Simple Modbus TCP read holding registers request
        // MBAP Header (7 bytes) + PDU (5 bytes)
        let request: [u8; 12] = [
            0x00, 0x01,             // Transaction ID
            0x00, 0x00,             // Protocol ID
            0x00, 0x06,             // Length
            0x01,                   // Unit ID
            0x03,                   // Function code: Read Holding Registers
            0x00, 0x00,             // Starting address
            0x00, 0x01,             // Quantity
        ];

        let mut response = [0u8; 256];

        // We need to use try_write/try_read since we have &TcpStream not &mut
        // This is a simplified version - in production, use proper framing
        match stream.try_write(&request) {
            Ok(_) => {
                // Wait a bit for response
                tokio::time::sleep(Duration::from_micros(100)).await;
                match stream.try_read(&mut response) {
                    Ok(n) if n > 0 => true,
                    _ => false,
                }
            }
            Err(_) => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_performance_config_default() {
        let config = PerformanceConfig::default();
        assert_eq!(config.target_connections, 1000);
        assert_eq!(config.target_tps, 10_000);
    }

    #[test]
    fn test_performance_config_presets() {
        let config = PerformanceConfig::ten_thousand_connections();
        assert_eq!(config.target_connections, 10_000);

        let config = PerformanceConfig::hundred_thousand_tps();
        assert_eq!(config.target_tps, 100_000);
    }

    #[test]
    fn test_performance_target_check() {
        let target = PerformanceTarget::min_connections(10_000);
        assert!(target.check(10_000.0));
        assert!(target.check(15_000.0));
        assert!(!target.check(9_999.0));

        let target = PerformanceTarget::max_p99_latency(10);
        assert!(target.check(5.0));
        assert!(target.check(10.0));
        assert!(!target.check(15.0));
    }

    #[test]
    fn test_live_metrics() {
        let metrics = LiveMetrics::new();

        metrics.add_connection();
        assert_eq!(metrics.connections_active.load(Ordering::Relaxed), 1);

        metrics.record_request(true, Duration::from_millis(5));
        assert_eq!(metrics.requests_total.load(Ordering::Relaxed), 1);
        assert_eq!(metrics.requests_success.load(Ordering::Relaxed), 1);

        metrics.record_request(false, Duration::from_millis(10));
        assert_eq!(metrics.requests_total.load(Ordering::Relaxed), 2);
        assert_eq!(metrics.requests_failed.load(Ordering::Relaxed), 1);

        metrics.remove_connection();
        assert_eq!(metrics.connections_active.load(Ordering::Relaxed), 0);
    }
}

//! Load generation utilities for stress testing.
//!
//! Provides configurable load generators for simulating various
//! traffic patterns and connection behaviors.

use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::RwLock;
use tokio::net::TcpStream;
use tokio::sync::Semaphore;

/// Load generation configuration.
#[derive(Debug, Clone)]
pub struct LoadConfig {
    /// Target server address.
    pub server_addr: SocketAddr,
    /// Number of concurrent connections.
    pub connections: usize,
    /// Requests per second per connection.
    pub requests_per_second: f64,
    /// Total test duration.
    pub duration: Duration,
    /// Connection ramp-up time.
    pub ramp_up: Duration,
    /// Load pattern.
    pub pattern: LoadPattern,
    /// Connection timeout.
    pub connect_timeout: Duration,
    /// Request timeout.
    pub request_timeout: Duration,
    /// Whether to keep connections alive.
    pub keep_alive: bool,
}

impl Default for LoadConfig {
    fn default() -> Self {
        Self {
            server_addr: "127.0.0.1:502".parse().unwrap(),
            connections: 100,
            requests_per_second: 100.0,
            duration: Duration::from_secs(60),
            ramp_up: Duration::from_secs(5),
            pattern: LoadPattern::Constant,
            connect_timeout: Duration::from_secs(10),
            request_timeout: Duration::from_secs(5),
            keep_alive: true,
        }
    }
}

impl LoadConfig {
    /// Create config for steady load test.
    pub fn steady(connections: usize, rps: f64) -> Self {
        Self {
            connections,
            requests_per_second: rps,
            pattern: LoadPattern::Constant,
            ..Default::default()
        }
    }

    /// Create config for spike test.
    pub fn spike(base_connections: usize, spike_connections: usize) -> Self {
        Self {
            connections: spike_connections,
            pattern: LoadPattern::Spike {
                base_load: base_connections,
                spike_load: spike_connections,
                spike_duration: Duration::from_secs(30),
                recovery_duration: Duration::from_secs(30),
            },
            ..Default::default()
        }
    }

    /// Create config for ramp test.
    pub fn ramp(start: usize, end: usize, step_duration: Duration) -> Self {
        Self {
            connections: end,
            pattern: LoadPattern::Ramp {
                start_connections: start,
                end_connections: end,
                step_duration,
            },
            ..Default::default()
        }
    }

    /// Set server address.
    pub fn with_server(mut self, addr: SocketAddr) -> Self {
        self.server_addr = addr;
        self
    }

    /// Set duration.
    pub fn with_duration(mut self, duration: Duration) -> Self {
        self.duration = duration;
        self
    }
}

/// Load pattern for traffic simulation.
#[derive(Debug, Clone)]
pub enum LoadPattern {
    /// Constant load throughout the test.
    Constant,
    /// Linearly increasing/decreasing load.
    Ramp {
        start_connections: usize,
        end_connections: usize,
        step_duration: Duration,
    },
    /// Sudden spike in load.
    Spike {
        base_load: usize,
        spike_load: usize,
        spike_duration: Duration,
        recovery_duration: Duration,
    },
    /// Wave pattern (sinusoidal).
    Wave {
        min_connections: usize,
        max_connections: usize,
        period: Duration,
    },
    /// Random fluctuations.
    Random {
        min_connections: usize,
        max_connections: usize,
    },
}

/// Load generator for creating traffic.
pub struct LoadGenerator {
    config: LoadConfig,
    running: Arc<AtomicBool>,
    stats: Arc<LoadStats>,
}

/// Statistics collected during load generation.
pub struct LoadStats {
    pub requests_sent: AtomicU64,
    pub requests_success: AtomicU64,
    pub requests_failed: AtomicU64,
    pub connections_opened: AtomicU64,
    pub connections_failed: AtomicU64,
    pub bytes_sent: AtomicU64,
    pub bytes_received: AtomicU64,
    latencies: RwLock<Vec<Duration>>,
}

impl LoadStats {
    fn new() -> Self {
        Self {
            requests_sent: AtomicU64::new(0),
            requests_success: AtomicU64::new(0),
            requests_failed: AtomicU64::new(0),
            connections_opened: AtomicU64::new(0),
            connections_failed: AtomicU64::new(0),
            bytes_sent: AtomicU64::new(0),
            bytes_received: AtomicU64::new(0),
            latencies: RwLock::new(Vec::with_capacity(10000)),
        }
    }

    fn record_latency(&self, latency: Duration) {
        let count = self.requests_sent.load(Ordering::Relaxed);
        // Sample to avoid memory issues
        if count % 10 == 0 {
            self.latencies.write().push(latency);
        }
    }

    /// Get percentile latency.
    pub fn percentile(&self, p: f64) -> Option<Duration> {
        let mut latencies = self.latencies.read().clone();
        if latencies.is_empty() {
            return None;
        }
        latencies.sort();
        let idx = ((latencies.len() as f64 * p / 100.0) as usize).min(latencies.len() - 1);
        Some(latencies[idx])
    }

    /// Get success rate.
    pub fn success_rate(&self) -> f64 {
        let total = self.requests_sent.load(Ordering::Relaxed);
        let success = self.requests_success.load(Ordering::Relaxed);
        if total > 0 {
            success as f64 / total as f64
        } else {
            0.0
        }
    }
}

impl LoadGenerator {
    /// Create a new load generator.
    pub fn new(config: LoadConfig) -> Self {
        Self {
            config,
            running: Arc::new(AtomicBool::new(false)),
            stats: Arc::new(LoadStats::new()),
        }
    }

    /// Run the load generator.
    pub async fn run(&self) -> LoadGeneratorResult {
        self.running.store(true, Ordering::SeqCst);
        let start = Instant::now();

        let semaphore = Arc::new(Semaphore::new(self.config.connections));
        let mut handles = Vec::new();

        // Calculate interval between requests per connection
        let request_interval = Duration::from_secs_f64(
            self.config.connections as f64 / self.config.requests_per_second,
        );

        // Ramp up connections gradually
        let ramp_interval = self.config.ramp_up / self.config.connections as u32;

        for i in 0..self.config.connections {
            if !self.running.load(Ordering::Relaxed) {
                break;
            }

            // Ramp-up delay
            if i > 0 {
                tokio::time::sleep(ramp_interval).await;
            }

            let permit = semaphore.clone().acquire_owned().await.unwrap();
            let config = self.config.clone();
            let stats = self.stats.clone();
            let running = self.running.clone();
            let test_duration = self.config.duration;

            let handle = tokio::spawn(async move {
                let _permit = permit;

                // Connect
                let stream = match tokio::time::timeout(
                    config.connect_timeout,
                    TcpStream::connect(config.server_addr),
                )
                .await
                {
                    Ok(Ok(s)) => {
                        stats.connections_opened.fetch_add(1, Ordering::Relaxed);
                        s
                    }
                    _ => {
                        stats.connections_failed.fetch_add(1, Ordering::Relaxed);
                        return;
                    }
                };

                let conn_start = Instant::now();

                // Send requests
                while running.load(Ordering::Relaxed) && conn_start.elapsed() < test_duration {
                    let req_start = Instant::now();

                    let result = Self::send_request(&stream, &config, &stats).await;

                    let latency = req_start.elapsed();
                    stats.record_latency(latency);
                    stats.requests_sent.fetch_add(1, Ordering::Relaxed);

                    if result {
                        stats.requests_success.fetch_add(1, Ordering::Relaxed);
                    } else {
                        stats.requests_failed.fetch_add(1, Ordering::Relaxed);
                    }

                    // Throttle to maintain target rate
                    if latency < request_interval {
                        tokio::time::sleep(request_interval - latency).await;
                    }
                }
            });

            handles.push(handle);
        }

        // Wait for test duration
        tokio::time::sleep(self.config.duration).await;
        self.running.store(false, Ordering::SeqCst);

        // Wait for all workers to finish
        for handle in handles {
            let _ = handle.await;
        }

        let duration = start.elapsed();

        LoadGeneratorResult {
            duration,
            requests_sent: self.stats.requests_sent.load(Ordering::Relaxed),
            requests_success: self.stats.requests_success.load(Ordering::Relaxed),
            requests_failed: self.stats.requests_failed.load(Ordering::Relaxed),
            connections_opened: self.stats.connections_opened.load(Ordering::Relaxed),
            connections_failed: self.stats.connections_failed.load(Ordering::Relaxed),
            bytes_sent: self.stats.bytes_sent.load(Ordering::Relaxed),
            bytes_received: self.stats.bytes_received.load(Ordering::Relaxed),
            p50_latency: self.stats.percentile(50.0),
            p95_latency: self.stats.percentile(95.0),
            p99_latency: self.stats.percentile(99.0),
            success_rate: self.stats.success_rate(),
        }
    }

    async fn send_request(stream: &TcpStream, _config: &LoadConfig, stats: &LoadStats) -> bool {
        // Modbus TCP read holding registers request
        let request: [u8; 12] = [
            0x00, 0x01, // Transaction ID
            0x00, 0x00, // Protocol ID
            0x00, 0x06, // Length
            0x01, // Unit ID
            0x03, // Function code: Read Holding Registers
            0x00, 0x00, // Starting address
            0x00, 0x0A, // Quantity (10 registers)
        ];

        // Try to send request
        if stream.try_write(&request).is_err() {
            return false;
        }
        stats
            .bytes_sent
            .fetch_add(request.len() as u64, Ordering::Relaxed);

        // Small delay for response
        tokio::time::sleep(Duration::from_micros(500)).await;

        // Try to read response
        let mut response = [0u8; 256];
        match stream.try_read(&mut response) {
            Ok(n) if n > 0 => {
                stats.bytes_received.fetch_add(n as u64, Ordering::Relaxed);
                // Check for valid Modbus response
                n >= 9 && response[7] == 0x03
            }
            _ => false,
        }
    }

    /// Stop the load generator.
    pub fn stop(&self) {
        self.running.store(false, Ordering::SeqCst);
    }

    /// Get current statistics.
    pub fn stats(&self) -> &LoadStats {
        &self.stats
    }
}

/// Result of load generation.
#[derive(Debug, Clone)]
pub struct LoadGeneratorResult {
    pub duration: Duration,
    pub requests_sent: u64,
    pub requests_success: u64,
    pub requests_failed: u64,
    pub connections_opened: u64,
    pub connections_failed: u64,
    pub bytes_sent: u64,
    pub bytes_received: u64,
    pub p50_latency: Option<Duration>,
    pub p95_latency: Option<Duration>,
    pub p99_latency: Option<Duration>,
    pub success_rate: f64,
}

impl LoadGeneratorResult {
    /// Calculate requests per second.
    pub fn rps(&self) -> f64 {
        if self.duration.as_secs_f64() > 0.0 {
            self.requests_sent as f64 / self.duration.as_secs_f64()
        } else {
            0.0
        }
    }

    /// Format as human-readable string.
    pub fn format(&self) -> String {
        format!(
            "Load Generation Results:\n\
             Duration: {:?}\n\
             Requests: {} sent, {} success, {} failed\n\
             RPS: {:.2}\n\
             Success Rate: {:.2}%\n\
             Connections: {} opened, {} failed\n\
             P50 Latency: {:?}\n\
             P95 Latency: {:?}\n\
             P99 Latency: {:?}\n\
             Bytes: {} sent, {} received",
            self.duration,
            self.requests_sent,
            self.requests_success,
            self.requests_failed,
            self.rps(),
            self.success_rate * 100.0,
            self.connections_opened,
            self.connections_failed,
            self.p50_latency,
            self.p95_latency,
            self.p99_latency,
            self.bytes_sent,
            self.bytes_received,
        )
    }
}

/// Connection simulator for testing connection handling.
pub struct ConnectionSimulator {
    server_addr: SocketAddr,
    connections: Vec<TcpStream>,
}

impl ConnectionSimulator {
    /// Create a new connection simulator.
    pub fn new(server_addr: SocketAddr) -> Self {
        Self {
            server_addr,
            connections: Vec::new(),
        }
    }

    /// Open N connections.
    pub async fn open_connections(&mut self, count: usize) -> Result<usize, String> {
        let mut opened = 0;

        for _ in 0..count {
            match tokio::time::timeout(Duration::from_secs(5), TcpStream::connect(self.server_addr))
                .await
            {
                Ok(Ok(stream)) => {
                    self.connections.push(stream);
                    opened += 1;
                }
                Ok(Err(e)) => {
                    tracing::debug!("Connection failed: {}", e);
                }
                Err(_) => {
                    tracing::debug!("Connection timeout");
                }
            }
        }

        Ok(opened)
    }

    /// Get current connection count.
    pub fn connection_count(&self) -> usize {
        self.connections.len()
    }

    /// Close all connections.
    pub fn close_all(&mut self) {
        self.connections.clear();
    }

    /// Test that all connections are still alive.
    pub async fn verify_connections(&self) -> (usize, usize) {
        let mut alive = 0;
        let mut dead = 0;

        for stream in &self.connections {
            // Try a simple peek to check if connection is alive
            let mut buf = [0u8; 1];
            let result = stream.try_read(&mut buf);
            match &result {
                Ok(_) => {
                    alive += 1;
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    // WouldBlock means connection is alive but no data
                    alive += 1;
                }
                Err(_) => {
                    dead += 1;
                }
            }
        }

        (alive, dead)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_load_config_default() {
        let config = LoadConfig::default();
        assert_eq!(config.connections, 100);
        assert!((config.requests_per_second - 100.0).abs() < 0.01);
    }

    #[test]
    fn test_load_config_presets() {
        let steady = LoadConfig::steady(500, 1000.0);
        assert_eq!(steady.connections, 500);

        let spike = LoadConfig::spike(100, 1000);
        assert_eq!(spike.connections, 1000);
    }

    #[test]
    fn test_load_stats() {
        let stats = LoadStats::new();

        stats.requests_sent.fetch_add(100, Ordering::Relaxed);
        stats.requests_success.fetch_add(95, Ordering::Relaxed);

        let rate = stats.success_rate();
        assert!((rate - 0.95).abs() < 0.01);
    }

    #[test]
    fn test_load_generator_result_rps() {
        let result = LoadGeneratorResult {
            duration: Duration::from_secs(10),
            requests_sent: 10000,
            requests_success: 9900,
            requests_failed: 100,
            connections_opened: 100,
            connections_failed: 0,
            bytes_sent: 120000,
            bytes_received: 240000,
            p50_latency: Some(Duration::from_millis(5)),
            p95_latency: Some(Duration::from_millis(10)),
            p99_latency: Some(Duration::from_millis(20)),
            success_rate: 0.99,
        };

        assert!((result.rps() - 1000.0).abs() < 0.01);
    }
}

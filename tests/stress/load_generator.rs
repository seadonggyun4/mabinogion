//! Load generator for stress testing.
//!
//! Provides configurable load generation for protocol testing.

use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{Mutex, Semaphore};

/// Load generator configuration.
#[derive(Debug, Clone)]
pub struct LoadConfig {
    /// Target requests per second.
    pub target_rps: u32,
    /// Number of concurrent connections.
    pub concurrency: usize,
    /// Test duration.
    pub duration: Duration,
    /// Ramp-up time to reach target RPS.
    pub ramp_up: Duration,
    /// Think time between requests (simulates user delay).
    pub think_time: Duration,
    /// Connection timeout.
    pub connect_timeout: Duration,
    /// Request timeout.
    pub request_timeout: Duration,
}

impl Default for LoadConfig {
    fn default() -> Self {
        Self {
            target_rps: 100,
            concurrency: 10,
            duration: Duration::from_secs(60),
            ramp_up: Duration::from_secs(10),
            think_time: Duration::ZERO,
            connect_timeout: Duration::from_secs(5),
            request_timeout: Duration::from_secs(10),
        }
    }
}

impl LoadConfig {
    pub fn with_target_rps(mut self, rps: u32) -> Self {
        self.target_rps = rps;
        self
    }

    pub fn with_concurrency(mut self, concurrency: usize) -> Self {
        self.concurrency = concurrency;
        self
    }

    pub fn with_duration(mut self, duration: Duration) -> Self {
        self.duration = duration;
        self
    }

    pub fn with_ramp_up(mut self, ramp_up: Duration) -> Self {
        self.ramp_up = ramp_up;
        self
    }

    pub fn high_load() -> Self {
        Self {
            target_rps: 10000,
            concurrency: 100,
            duration: Duration::from_secs(300),
            ramp_up: Duration::from_secs(30),
            ..Default::default()
        }
    }

    pub fn stress_test() -> Self {
        Self {
            target_rps: 50000,
            concurrency: 500,
            duration: Duration::from_secs(600),
            ramp_up: Duration::from_secs(60),
            ..Default::default()
        }
    }
}

/// Load generator statistics.
#[derive(Debug, Default)]
pub struct LoadStats {
    pub total_requests: AtomicU64,
    pub successful_requests: AtomicU64,
    pub failed_requests: AtomicU64,
    pub total_latency_ns: AtomicU64,
    pub min_latency_ns: AtomicU64,
    pub max_latency_ns: AtomicU64,
}

impl LoadStats {
    pub fn new() -> Self {
        Self {
            min_latency_ns: AtomicU64::new(u64::MAX),
            ..Default::default()
        }
    }

    pub fn record_success(&self, latency: Duration) {
        self.total_requests.fetch_add(1, Ordering::Relaxed);
        self.successful_requests.fetch_add(1, Ordering::Relaxed);
        self.record_latency(latency);
    }

    pub fn record_failure(&self, latency: Duration) {
        self.total_requests.fetch_add(1, Ordering::Relaxed);
        self.failed_requests.fetch_add(1, Ordering::Relaxed);
        self.record_latency(latency);
    }

    fn record_latency(&self, latency: Duration) {
        let latency_ns = latency.as_nanos() as u64;
        self.total_latency_ns.fetch_add(latency_ns, Ordering::Relaxed);

        // Update min (with CAS loop)
        let mut current = self.min_latency_ns.load(Ordering::Relaxed);
        while latency_ns < current {
            match self.min_latency_ns.compare_exchange_weak(
                current,
                latency_ns,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => break,
                Err(c) => current = c,
            }
        }

        // Update max (with CAS loop)
        let mut current = self.max_latency_ns.load(Ordering::Relaxed);
        while latency_ns > current {
            match self.max_latency_ns.compare_exchange_weak(
                current,
                latency_ns,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => break,
                Err(c) => current = c,
            }
        }
    }

    pub fn snapshot(&self) -> LoadStatsSnapshot {
        let total = self.total_requests.load(Ordering::Relaxed);
        let successful = self.successful_requests.load(Ordering::Relaxed);
        let failed = self.failed_requests.load(Ordering::Relaxed);
        let total_latency = self.total_latency_ns.load(Ordering::Relaxed);
        let min_latency = self.min_latency_ns.load(Ordering::Relaxed);
        let max_latency = self.max_latency_ns.load(Ordering::Relaxed);

        let avg_latency = if total > 0 {
            Duration::from_nanos(total_latency / total)
        } else {
            Duration::ZERO
        };

        LoadStatsSnapshot {
            total_requests: total,
            successful_requests: successful,
            failed_requests: failed,
            success_rate: if total > 0 {
                successful as f64 / total as f64 * 100.0
            } else {
                0.0
            },
            avg_latency,
            min_latency: Duration::from_nanos(if min_latency == u64::MAX { 0 } else { min_latency }),
            max_latency: Duration::from_nanos(max_latency),
        }
    }
}

/// Snapshot of load statistics.
#[derive(Debug, Clone)]
pub struct LoadStatsSnapshot {
    pub total_requests: u64,
    pub successful_requests: u64,
    pub failed_requests: u64,
    pub success_rate: f64,
    pub avg_latency: Duration,
    pub min_latency: Duration,
    pub max_latency: Duration,
}

impl LoadStatsSnapshot {
    pub fn print(&self) {
        println!("Load Test Results:");
        println!("  Total Requests:   {}", self.total_requests);
        println!("  Successful:       {}", self.successful_requests);
        println!("  Failed:           {}", self.failed_requests);
        println!("  Success Rate:     {:.2}%", self.success_rate);
        println!("  Avg Latency:      {:?}", self.avg_latency);
        println!("  Min Latency:      {:?}", self.min_latency);
        println!("  Max Latency:      {:?}", self.max_latency);
    }
}

/// Request generator trait.
#[async_trait::async_trait]
pub trait RequestGenerator: Send + Sync {
    /// Generate and execute a request.
    async fn execute(&self) -> Result<(), String>;
}

/// Load generator for running stress tests.
pub struct LoadGenerator<G: RequestGenerator> {
    config: LoadConfig,
    generator: Arc<G>,
    stats: Arc<LoadStats>,
}

impl<G: RequestGenerator + 'static> LoadGenerator<G> {
    /// Create a new load generator.
    pub fn new(config: LoadConfig, generator: G) -> Self {
        Self {
            config,
            generator: Arc::new(generator),
            stats: Arc::new(LoadStats::new()),
        }
    }

    /// Run the load test.
    pub async fn run(&self) -> LoadStatsSnapshot {
        let semaphore = Arc::new(Semaphore::new(self.config.concurrency));
        let start_time = Instant::now();
        let ramp_up_duration = self.config.ramp_up;
        let target_rps = self.config.target_rps;
        let test_duration = self.config.duration;
        let think_time = self.config.think_time;

        let mut handles = Vec::new();

        while start_time.elapsed() < test_duration {
            // Calculate current target RPS based on ramp-up
            let elapsed = start_time.elapsed();
            let current_rps = if elapsed < ramp_up_duration {
                let progress = elapsed.as_secs_f64() / ramp_up_duration.as_secs_f64();
                (target_rps as f64 * progress) as u32
            } else {
                target_rps
            };

            // Calculate interval between requests
            let interval = if current_rps > 0 {
                Duration::from_secs_f64(1.0 / current_rps as f64)
            } else {
                Duration::from_millis(100)
            };

            // Spawn request
            let permit = semaphore.clone().acquire_owned().await.unwrap();
            let generator = self.generator.clone();
            let stats = self.stats.clone();

            let handle = tokio::spawn(async move {
                let request_start = Instant::now();
                let result = generator.execute().await;
                let latency = request_start.elapsed();

                match result {
                    Ok(()) => stats.record_success(latency),
                    Err(_) => stats.record_failure(latency),
                }

                drop(permit);

                if !think_time.is_zero() {
                    tokio::time::sleep(think_time).await;
                }
            });

            handles.push(handle);

            // Wait for interval
            tokio::time::sleep(interval).await;
        }

        // Wait for all requests to complete
        for handle in handles {
            let _ = handle.await;
        }

        self.stats.snapshot()
    }

    /// Get current statistics.
    pub fn stats(&self) -> LoadStatsSnapshot {
        self.stats.snapshot()
    }
}

/// Simple TCP connection generator.
pub struct TcpConnectionGenerator {
    addr: SocketAddr,
    request: Vec<u8>,
    expected_response_len: usize,
}

impl TcpConnectionGenerator {
    pub fn new(addr: SocketAddr, request: Vec<u8>, expected_response_len: usize) -> Self {
        Self {
            addr,
            request,
            expected_response_len,
        }
    }
}

#[async_trait::async_trait]
impl RequestGenerator for TcpConnectionGenerator {
    async fn execute(&self) -> Result<(), String> {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let mut stream = tokio::net::TcpStream::connect(self.addr)
            .await
            .map_err(|e| e.to_string())?;

        stream
            .write_all(&self.request)
            .await
            .map_err(|e| e.to_string())?;

        let mut response = vec![0u8; self.expected_response_len];
        let n = stream
            .read(&mut response)
            .await
            .map_err(|e| e.to_string())?;

        if n == 0 {
            return Err("Empty response".into());
        }

        Ok(())
    }
}

/// Modbus load generator.
pub struct ModbusLoadGenerator {
    addr: SocketAddr,
    unit_ids: Vec<u8>,
    register_range: (u16, u16),
}

impl ModbusLoadGenerator {
    pub fn new(addr: SocketAddr, unit_ids: Vec<u8>, register_range: (u16, u16)) -> Self {
        Self {
            addr,
            unit_ids,
            register_range,
        }
    }
}

#[async_trait::async_trait]
impl RequestGenerator for ModbusLoadGenerator {
    async fn execute(&self) -> Result<(), String> {
        use rand::Rng;
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let mut rng = rand::thread_rng();

        // Select random unit ID and register
        let unit_id = self.unit_ids[rng.gen_range(0..self.unit_ids.len())];
        let start_addr = rng.gen_range(self.register_range.0..self.register_range.1);
        let count = rng.gen_range(1..=10).min(self.register_range.1 - start_addr);

        // Build Modbus TCP request
        let mut request = Vec::new();
        request.extend_from_slice(&[0x00, 0x01]); // Transaction ID
        request.extend_from_slice(&[0x00, 0x00]); // Protocol ID
        request.extend_from_slice(&[0x00, 0x06]); // Length
        request.push(unit_id);
        request.push(0x03); // Read Holding Registers
        request.extend_from_slice(&start_addr.to_be_bytes());
        request.extend_from_slice(&count.to_be_bytes());

        // Connect and send
        let mut stream = tokio::net::TcpStream::connect(self.addr)
            .await
            .map_err(|e| e.to_string())?;

        stream
            .write_all(&request)
            .await
            .map_err(|e| e.to_string())?;

        // Read response
        let mut response = vec![0u8; 256];
        let n = stream
            .read(&mut response)
            .await
            .map_err(|e| e.to_string())?;

        if n < 9 {
            return Err("Response too short".into());
        }

        // Check for exception
        if response[7] & 0x80 != 0 {
            return Err(format!("Modbus exception: 0x{:02x}", response[8]));
        }

        Ok(())
    }
}

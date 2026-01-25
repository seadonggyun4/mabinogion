//! Test helper utilities.
//!
//! Provides common utilities for integration testing.

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

// =============================================================================
// Protocol Test Helpers
// =============================================================================

/// Helper for Modbus protocol testing.
pub struct ModbusTestHelper {
    addr: SocketAddr,
}

impl ModbusTestHelper {
    pub fn new(addr: SocketAddr) -> Self {
        Self { addr }
    }

    /// Read holding registers (function code 0x03).
    pub async fn read_holding_registers(
        &self,
        unit_id: u8,
        start_addr: u16,
        count: u16,
    ) -> std::io::Result<Vec<u16>> {
        let mut stream = TcpStream::connect(self.addr).await?;

        // Build Modbus TCP ADU
        let mut request = Vec::new();
        request.extend_from_slice(&[0x00, 0x01]); // Transaction ID
        request.extend_from_slice(&[0x00, 0x00]); // Protocol ID
        request.extend_from_slice(&[0x00, 0x06]); // Length
        request.push(unit_id);
        request.push(0x03); // Function code: Read Holding Registers
        request.extend_from_slice(&start_addr.to_be_bytes());
        request.extend_from_slice(&count.to_be_bytes());

        stream.write_all(&request).await?;

        // Read response
        let mut response = vec![0u8; 256];
        let n = stream.read(&mut response).await?;

        if n < 9 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Response too short",
            ));
        }

        // Parse response
        let byte_count = response[8] as usize;
        let mut values = Vec::new();
        for i in 0..(byte_count / 2) {
            let offset = 9 + i * 2;
            let value = u16::from_be_bytes([response[offset], response[offset + 1]]);
            values.push(value);
        }

        Ok(values)
    }

    /// Write single register (function code 0x06).
    pub async fn write_single_register(
        &self,
        unit_id: u8,
        addr: u16,
        value: u16,
    ) -> std::io::Result<()> {
        let mut stream = TcpStream::connect(self.addr).await?;

        let mut request = Vec::new();
        request.extend_from_slice(&[0x00, 0x01]); // Transaction ID
        request.extend_from_slice(&[0x00, 0x00]); // Protocol ID
        request.extend_from_slice(&[0x00, 0x06]); // Length
        request.push(unit_id);
        request.push(0x06); // Function code: Write Single Register
        request.extend_from_slice(&addr.to_be_bytes());
        request.extend_from_slice(&value.to_be_bytes());

        stream.write_all(&request).await?;

        // Read response
        let mut response = vec![0u8; 12];
        let n = stream.read(&mut response).await?;

        if n < 12 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Response too short",
            ));
        }

        // Check for exception
        if response[7] & 0x80 != 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("Modbus exception: 0x{:02x}", response[8]),
            ));
        }

        Ok(())
    }

    /// Read coils (function code 0x01).
    pub async fn read_coils(
        &self,
        unit_id: u8,
        start_addr: u16,
        count: u16,
    ) -> std::io::Result<Vec<bool>> {
        let mut stream = TcpStream::connect(self.addr).await?;

        let mut request = Vec::new();
        request.extend_from_slice(&[0x00, 0x01]); // Transaction ID
        request.extend_from_slice(&[0x00, 0x00]); // Protocol ID
        request.extend_from_slice(&[0x00, 0x06]); // Length
        request.push(unit_id);
        request.push(0x01); // Function code: Read Coils
        request.extend_from_slice(&start_addr.to_be_bytes());
        request.extend_from_slice(&count.to_be_bytes());

        stream.write_all(&request).await?;

        let mut response = vec![0u8; 256];
        let n = stream.read(&mut response).await?;

        if n < 9 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Response too short",
            ));
        }

        let byte_count = response[8] as usize;
        let mut values = Vec::new();
        for i in 0..count as usize {
            let byte_idx = i / 8;
            let bit_idx = i % 8;
            if byte_idx < byte_count {
                let coil = (response[9 + byte_idx] >> bit_idx) & 0x01 != 0;
                values.push(coil);
            }
        }

        Ok(values)
    }
}

/// Helper for OPC UA protocol testing.
pub struct OpcUaTestHelper {
    endpoint: String,
}

impl OpcUaTestHelper {
    pub fn new(endpoint: impl Into<String>) -> Self {
        Self {
            endpoint: endpoint.into(),
        }
    }

    /// Get the endpoint URL.
    pub fn endpoint(&self) -> &str {
        &self.endpoint
    }

    // Add OPC UA specific test helpers as needed
}

/// Helper for BACnet protocol testing.
pub struct BacnetTestHelper {
    addr: SocketAddr,
}

impl BacnetTestHelper {
    pub fn new(addr: SocketAddr) -> Self {
        Self { addr }
    }

    /// Send a Who-Is request.
    pub async fn who_is(&self) -> std::io::Result<Vec<u8>> {
        let socket = tokio::net::UdpSocket::bind("0.0.0.0:0").await?;

        // Simple Who-Is request (BACnet/IP BVLC header + NPDU + APDU)
        let who_is = vec![
            0x81, 0x0b, // BVLC type, function
            0x00, 0x0c, // Length
            0x01, 0x20, // NPDU version, control
            0xff, 0xff, // DNET broadcast
            0x00,       // DLEN
            0xff,       // Hop count
            0x10, 0x08, // APDU type (unconfirmed), service (who-is)
        ];

        socket.send_to(&who_is, self.addr).await?;

        // Wait for I-Am response
        let mut buf = vec![0u8; 1500];
        let timeout_result = tokio::time::timeout(
            Duration::from_secs(2),
            socket.recv_from(&mut buf),
        ).await;

        match timeout_result {
            Ok(Ok((n, _))) => Ok(buf[..n].to_vec()),
            Ok(Err(e)) => Err(e),
            Err(_) => Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "No I-Am response",
            )),
        }
    }
}

/// Helper for KNX protocol testing.
pub struct KnxTestHelper {
    addr: SocketAddr,
}

impl KnxTestHelper {
    pub fn new(addr: SocketAddr) -> Self {
        Self { addr }
    }

    /// Send a search request.
    pub async fn search_request(&self) -> std::io::Result<Vec<u8>> {
        let socket = tokio::net::UdpSocket::bind("0.0.0.0:0").await?;

        // KNXnet/IP Search Request
        let local_addr = socket.local_addr()?;
        let port_bytes = local_addr.port().to_be_bytes();

        let search_request = vec![
            0x06, 0x10, // Header length, KNXnet/IP version
            0x02, 0x01, // Search Request
            0x00, 0x0e, // Total length
            0x08, 0x01, // HPAI length, protocol (UDP)
            0x00, 0x00, 0x00, 0x00, // IP (will be filled by server)
            port_bytes[0], port_bytes[1], // Port
        ];

        socket.send_to(&search_request, self.addr).await?;

        let mut buf = vec![0u8; 1500];
        let timeout_result = tokio::time::timeout(
            Duration::from_secs(2),
            socket.recv_from(&mut buf),
        ).await;

        match timeout_result {
            Ok(Ok((n, _))) => Ok(buf[..n].to_vec()),
            Ok(Err(e)) => Err(e),
            Err(_) => Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "No search response",
            )),
        }
    }
}

// =============================================================================
// File Helpers
// =============================================================================

/// Create a temporary scenario file.
pub async fn create_temp_scenario(content: &str) -> std::io::Result<PathBuf> {
    let temp_dir = std::env::temp_dir();
    let file_name = format!("test_scenario_{}.yaml", uuid::Uuid::new_v4());
    let path = temp_dir.join(file_name);

    tokio::fs::write(&path, content).await?;
    Ok(path)
}

/// Read a test fixture file.
pub async fn read_fixture(name: &str) -> std::io::Result<String> {
    let path = fixture_path(name);
    tokio::fs::read_to_string(path).await
}

/// Get the path to a test fixture.
pub fn fixture_path(name: &str) -> PathBuf {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.push("tests");
    path.push("fixtures");
    path.push(name);
    path
}

// =============================================================================
// Assertion Helpers
// =============================================================================

/// Assert that a duration is within a range.
pub fn assert_duration_in_range(actual: Duration, min: Duration, max: Duration) {
    assert!(
        actual >= min && actual <= max,
        "Duration {:?} not in range [{:?}, {:?}]",
        actual,
        min,
        max
    );
}

/// Assert that a value is approximately equal.
pub fn assert_approx_eq(actual: f64, expected: f64, tolerance: f64) {
    assert!(
        (actual - expected).abs() <= tolerance,
        "Values not approximately equal: {} vs {} (tolerance: {})",
        actual,
        expected,
        tolerance
    );
}

// =============================================================================
// Performance Helpers
// =============================================================================

/// Measure the execution time of an async function.
pub async fn measure_time<F, T>(f: F) -> (T, Duration)
where
    F: std::future::Future<Output = T>,
{
    let start = std::time::Instant::now();
    let result = f.await;
    let elapsed = start.elapsed();
    (result, elapsed)
}

/// Run a function multiple times and return statistics.
pub async fn benchmark<F, T, Fut>(iterations: usize, f: F) -> BenchmarkResult
where
    F: Fn() -> Fut,
    Fut: std::future::Future<Output = T>,
{
    let mut durations = Vec::with_capacity(iterations);

    for _ in 0..iterations {
        let (_, elapsed) = measure_time(f()).await;
        durations.push(elapsed);
    }

    durations.sort();

    let total: Duration = durations.iter().sum();
    let avg = total / iterations as u32;
    let min = durations.first().copied().unwrap_or_default();
    let max = durations.last().copied().unwrap_or_default();
    let p50 = durations.get(iterations / 2).copied().unwrap_or_default();
    let p95 = durations.get(iterations * 95 / 100).copied().unwrap_or_default();
    let p99 = durations.get(iterations * 99 / 100).copied().unwrap_or_default();

    BenchmarkResult {
        iterations,
        total,
        avg,
        min,
        max,
        p50,
        p95,
        p99,
    }
}

/// Benchmark result statistics.
#[derive(Debug, Clone)]
pub struct BenchmarkResult {
    pub iterations: usize,
    pub total: Duration,
    pub avg: Duration,
    pub min: Duration,
    pub max: Duration,
    pub p50: Duration,
    pub p95: Duration,
    pub p99: Duration,
}

impl BenchmarkResult {
    pub fn print(&self) {
        println!("Benchmark Results ({} iterations):", self.iterations);
        println!("  Total: {:?}", self.total);
        println!("  Avg:   {:?}", self.avg);
        println!("  Min:   {:?}", self.min);
        println!("  Max:   {:?}", self.max);
        println!("  P50:   {:?}", self.p50);
        println!("  P95:   {:?}", self.p95);
        println!("  P99:   {:?}", self.p99);
    }

    /// Calculate throughput (operations per second).
    pub fn throughput(&self) -> f64 {
        self.iterations as f64 / self.total.as_secs_f64()
    }
}

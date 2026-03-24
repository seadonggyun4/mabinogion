//! Virtual serial port support for Modbus RTU simulation.
//!
//! This module provides virtual serial port creation and configuration,
//! enabling RTU simulation without physical hardware.
//!
//! # Features
//!
//! - **PTY-based ports** (Unix): Creates pseudo-terminal pairs
//! - **Named pipes** (Windows): Creates named pipe pairs
//! - **Baud rate simulation**: Accurate timing delays
//! - **Serial configuration**: Parity, stop bits, data bits
//!
//! # Unix Implementation
//!
//! On Unix systems, this uses PTY (pseudo-terminal) pairs:
//! - Master: Used by the simulator
//! - Slave: Exposed as a device file for clients
//!
//! # Example
//!
//! ```rust,ignore
//! use mabi_modbus::rtu::{VirtualSerialConfig, VirtualSerial};
//!
//! fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let config = VirtualSerialConfig::default()
//!         .with_baud_rate(9600)
//!         .with_slave_path("/tmp/modbus_slave");
//!
//!     #[cfg(unix)]
//!     let serial = VirtualSerial::create(config)?;
//!
//!     // Clients can connect to /tmp/modbus_slave
//!     #[cfg(unix)]
//!     println!("Virtual serial ready at: {:?}", serial.slave_path());
//!
//!     Ok(())
//! }
//! ```

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Serial port parity configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Parity {
    /// No parity bit.
    #[default]
    None,
    /// Even parity.
    Even,
    /// Odd parity.
    Odd,
    /// Mark parity (always 1).
    Mark,
    /// Space parity (always 0).
    Space,
}

impl Parity {
    /// Get the number of bits added by this parity setting.
    pub fn bits(&self) -> u32 {
        match self {
            Parity::None => 0,
            _ => 1,
        }
    }
}

/// Serial port stop bits configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum StopBits {
    /// 1 stop bit.
    #[default]
    One,
    /// 1.5 stop bits.
    OnePointFive,
    /// 2 stop bits.
    Two,
}

impl StopBits {
    /// Get the number of stop bits as a value.
    pub fn bits(&self) -> f32 {
        match self {
            StopBits::One => 1.0,
            StopBits::OnePointFive => 1.5,
            StopBits::Two => 2.0,
        }
    }
}

/// Serial port data bits configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DataBits {
    Five = 5,
    Six = 6,
    Seven = 7,
    Eight = 8,
}

impl Default for DataBits {
    fn default() -> Self {
        Self::Eight
    }
}

impl DataBits {
    /// Get the number of data bits.
    pub fn bits(&self) -> u32 {
        *self as u32
    }
}

/// Serial port configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SerialConfig {
    /// Baud rate (bits per second).
    pub baud_rate: u32,

    /// Data bits per character.
    pub data_bits: DataBits,

    /// Parity setting.
    pub parity: Parity,

    /// Stop bits.
    pub stop_bits: StopBits,

    /// Enable hardware flow control (RTS/CTS).
    pub flow_control: bool,
}

impl Default for SerialConfig {
    fn default() -> Self {
        Self {
            baud_rate: 9600,
            data_bits: DataBits::Eight,
            parity: Parity::None,
            stop_bits: StopBits::One,
            flow_control: false,
        }
    }
}

impl SerialConfig {
    /// Create a new serial config with the given baud rate.
    pub fn new(baud_rate: u32) -> Self {
        Self {
            baud_rate,
            ..Default::default()
        }
    }

    /// Common configuration: 9600/8N1.
    pub fn baud_9600() -> Self {
        Self::new(9600)
    }

    /// Common configuration: 19200/8N1.
    pub fn baud_19200() -> Self {
        Self::new(19200)
    }

    /// Common configuration: 38400/8N1.
    pub fn baud_38400() -> Self {
        Self::new(38400)
    }

    /// Common configuration: 115200/8N1.
    pub fn baud_115200() -> Self {
        Self::new(115200)
    }

    /// Set parity.
    pub fn with_parity(mut self, parity: Parity) -> Self {
        self.parity = parity;
        self
    }

    /// Set stop bits.
    pub fn with_stop_bits(mut self, stop_bits: StopBits) -> Self {
        self.stop_bits = stop_bits;
        self
    }

    /// Set data bits.
    pub fn with_data_bits(mut self, data_bits: DataBits) -> Self {
        self.data_bits = data_bits;
        self
    }

    /// Calculate bits per character (start + data + parity + stop).
    pub fn bits_per_character(&self) -> f32 {
        1.0 // Start bit
            + self.data_bits.bits() as f32
            + self.parity.bits() as f32
            + self.stop_bits.bits()
    }

    /// Calculate character transmission time.
    pub fn char_time(&self) -> Duration {
        let bits = self.bits_per_character();
        let seconds = bits / self.baud_rate as f32;
        Duration::from_secs_f32(seconds)
    }

    /// Calculate transmission time for a given number of bytes.
    pub fn transmission_time(&self, bytes: usize) -> Duration {
        self.char_time() * bytes as u32
    }

    /// Calculate inter-frame timeout (3.5 character times).
    pub fn inter_frame_timeout(&self) -> Duration {
        // For high baud rates (> 19200), use minimum of 1.75ms
        if self.baud_rate > 19200 {
            Duration::from_micros(1750)
        } else {
            let char_time = self.char_time();
            Duration::from_secs_f32(char_time.as_secs_f32() * 3.5)
        }
    }

    /// Calculate inter-character timeout (1.5 character times).
    pub fn inter_char_timeout(&self) -> Duration {
        // For high baud rates (> 19200), use minimum of 0.75ms
        if self.baud_rate > 19200 {
            Duration::from_micros(750)
        } else {
            let char_time = self.char_time();
            Duration::from_secs_f32(char_time.as_secs_f32() * 1.5)
        }
    }
}

/// Virtual serial port configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VirtualSerialConfig {
    /// Serial port configuration.
    #[serde(flatten)]
    pub serial: SerialConfig,

    /// Path for the slave device (client connection point).
    pub slave_path: PathBuf,

    /// Optional symbolic link path for the slave device.
    pub symlink_path: Option<PathBuf>,

    /// Enable transmission delay simulation.
    pub simulate_delays: bool,

    /// Buffer size for the virtual port.
    pub buffer_size: usize,
}

impl Default for VirtualSerialConfig {
    fn default() -> Self {
        Self {
            serial: SerialConfig::default(),
            slave_path: PathBuf::from("/tmp/modbus_rtu_slave"),
            symlink_path: None,
            simulate_delays: true,
            buffer_size: 4096,
        }
    }
}

impl VirtualSerialConfig {
    /// Create a new config with the specified slave path.
    pub fn new<P: AsRef<Path>>(slave_path: P) -> Self {
        Self {
            slave_path: slave_path.as_ref().to_path_buf(),
            ..Default::default()
        }
    }

    /// Set the baud rate.
    pub fn with_baud_rate(mut self, baud_rate: u32) -> Self {
        self.serial.baud_rate = baud_rate;
        self
    }

    /// Set the slave path.
    pub fn with_slave_path<P: AsRef<Path>>(mut self, path: P) -> Self {
        self.slave_path = path.as_ref().to_path_buf();
        self
    }

    /// Set a symbolic link path for the slave device.
    pub fn with_symlink<P: AsRef<Path>>(mut self, path: P) -> Self {
        self.symlink_path = Some(path.as_ref().to_path_buf());
        self
    }

    /// Set serial configuration.
    pub fn with_serial_config(mut self, config: SerialConfig) -> Self {
        self.serial = config;
        self
    }

    /// Enable or disable transmission delay simulation.
    pub fn with_delay_simulation(mut self, enabled: bool) -> Self {
        self.simulate_delays = enabled;
        self
    }

    /// Set buffer size.
    pub fn with_buffer_size(mut self, size: usize) -> Self {
        self.buffer_size = size;
        self
    }
}

/// Virtual serial port errors.
#[derive(Debug, Error)]
pub enum VirtualSerialError {
    /// I/O error.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// PTY creation failed.
    #[error("Failed to create PTY: {0}")]
    PtyCreation(String),

    /// Symlink creation failed.
    #[error("Failed to create symlink: {source}")]
    Symlink {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    /// Port not available.
    #[error("Virtual serial port not available on this platform")]
    NotAvailable,

    /// Port already in use.
    #[error("Port already in use: {0}")]
    InUse(PathBuf),
}

/// Virtual serial port.
///
/// Provides a simulated serial port using platform-specific mechanisms.
#[derive(Debug)]
pub struct VirtualSerial {
    /// Configuration.
    config: VirtualSerialConfig,

    /// Platform-specific implementation.
    inner: VirtualSerialInner,
}

#[cfg(unix)]
#[derive(Debug)]
struct VirtualSerialInner {
    /// Master file descriptor.
    master_fd: std::os::unix::io::RawFd,

    /// Slave device path (actual PTY path).
    slave_device_path: PathBuf,

    /// Whether we created a symlink.
    has_symlink: bool,
}

#[cfg(not(unix))]
#[derive(Debug)]
struct VirtualSerialInner {
    // Placeholder for non-Unix platforms
    _marker: std::marker::PhantomData<()>,
}

impl VirtualSerial {
    /// Create a new virtual serial port.
    #[cfg(unix)]
    pub fn create(config: VirtualSerialConfig) -> Result<Self, VirtualSerialError> {
        use std::os::unix::io::AsRawFd;

        // Open PTY master
        let master =
            nix::pty::posix_openpt(nix::fcntl::OFlag::O_RDWR | nix::fcntl::OFlag::O_NOCTTY)
                .map_err(|e| VirtualSerialError::PtyCreation(e.to_string()))?;

        // Grant access to slave
        nix::pty::grantpt(&master)
            .map_err(|e| VirtualSerialError::PtyCreation(format!("grantpt failed: {}", e)))?;

        // Unlock slave
        nix::pty::unlockpt(&master)
            .map_err(|e| VirtualSerialError::PtyCreation(format!("unlockpt failed: {}", e)))?;

        // Get slave name
        let slave_name = unsafe {
            nix::pty::ptsname(&master)
                .map_err(|e| VirtualSerialError::PtyCreation(format!("ptsname failed: {}", e)))?
        };

        let slave_device_path = PathBuf::from(&slave_name);

        // Create symlink if requested
        let has_symlink = if let Some(ref symlink_path) = config.symlink_path {
            // Remove existing symlink if present
            let _ = std::fs::remove_file(symlink_path);

            std::os::unix::fs::symlink(&slave_device_path, symlink_path).map_err(|e| {
                VirtualSerialError::Symlink {
                    path: symlink_path.clone(),
                    source: e,
                }
            })?;
            true
        } else if config.slave_path != slave_device_path {
            // Create symlink at slave_path pointing to actual device
            let _ = std::fs::remove_file(&config.slave_path);

            std::os::unix::fs::symlink(&slave_device_path, &config.slave_path).map_err(|e| {
                VirtualSerialError::Symlink {
                    path: config.slave_path.clone(),
                    source: e,
                }
            })?;
            true
        } else {
            false
        };

        let inner = VirtualSerialInner {
            master_fd: master.as_raw_fd(),
            slave_device_path,
            has_symlink,
        };

        // Keep the master fd open by forgetting the PtyMaster
        std::mem::forget(master);

        Ok(Self { config, inner })
    }

    /// Create a new virtual serial port (non-Unix stub).
    #[cfg(not(unix))]
    pub fn create(_config: VirtualSerialConfig) -> Result<Self, VirtualSerialError> {
        Err(VirtualSerialError::NotAvailable)
    }

    /// Get the slave device path (where clients should connect).
    pub fn slave_path(&self) -> &Path {
        &self.config.slave_path
    }

    /// Get the actual PTY device path.
    #[cfg(unix)]
    pub fn device_path(&self) -> &Path {
        &self.inner.slave_device_path
    }

    /// Get the configuration.
    pub fn config(&self) -> &VirtualSerialConfig {
        &self.config
    }

    /// Get the serial configuration.
    pub fn serial_config(&self) -> &SerialConfig {
        &self.config.serial
    }

    /// Calculate transmission delay for a given number of bytes.
    pub fn transmission_delay(&self, bytes: usize) -> Duration {
        if self.config.simulate_delays {
            self.config.serial.transmission_time(bytes)
        } else {
            Duration::ZERO
        }
    }

    /// Get the master file descriptor (for async I/O).
    #[cfg(unix)]
    pub fn master_fd(&self) -> std::os::unix::io::RawFd {
        self.inner.master_fd
    }

    /// Create an async reader/writer for the master side.
    #[cfg(unix)]
    pub fn into_async_io(self) -> std::io::Result<tokio::fs::File> {
        use std::os::unix::io::FromRawFd;

        // Create a std::fs::File from the raw fd
        let file = unsafe { std::fs::File::from_raw_fd(self.inner.master_fd) };

        // Forget self to prevent double-close
        std::mem::forget(self);

        // Convert to tokio::fs::File
        Ok(tokio::fs::File::from_std(file))
    }
}

#[cfg(unix)]
impl Drop for VirtualSerial {
    fn drop(&mut self) {
        // Clean up symlink
        if self.inner.has_symlink {
            if let Some(ref symlink_path) = self.config.symlink_path {
                let _ = std::fs::remove_file(symlink_path);
            } else {
                let _ = std::fs::remove_file(&self.config.slave_path);
            }
        }

        // Close master fd
        unsafe {
            libc::close(self.inner.master_fd);
        }
    }
}

/// Serial port pair for testing.
///
/// Creates a connected pair of virtual serial ports for testing
/// without requiring actual hardware or PTY support.
#[derive(Debug)]
pub struct SerialPair {
    /// Server-side buffer.
    #[allow(dead_code)]
    server_tx: tokio::sync::mpsc::Sender<Vec<u8>>,
    #[allow(dead_code)]
    server_rx: tokio::sync::mpsc::Receiver<Vec<u8>>,

    /// Client-side buffer.
    #[allow(dead_code)]
    client_tx: tokio::sync::mpsc::Sender<Vec<u8>>,
    #[allow(dead_code)]
    client_rx: tokio::sync::mpsc::Receiver<Vec<u8>>,

    /// Configuration.
    #[allow(dead_code)]
    config: SerialConfig,
}

impl SerialPair {
    /// Create a new connected pair with the given configuration.
    pub fn new(config: SerialConfig) -> (SerialPairServer, SerialPairClient) {
        let (server_tx, client_rx) = tokio::sync::mpsc::channel(256);
        let (client_tx, server_rx) = tokio::sync::mpsc::channel(256);

        let server = SerialPairServer {
            tx: server_tx,
            rx: server_rx,
            config: config.clone(),
        };

        let client = SerialPairClient {
            tx: client_tx,
            rx: client_rx,
            config,
        };

        (server, client)
    }
}

/// Server side of a serial pair.
#[derive(Debug)]
pub struct SerialPairServer {
    tx: tokio::sync::mpsc::Sender<Vec<u8>>,
    rx: tokio::sync::mpsc::Receiver<Vec<u8>>,
    config: SerialConfig,
}

impl SerialPairServer {
    /// Send data to the client.
    pub async fn send(
        &self,
        data: &[u8],
    ) -> Result<(), tokio::sync::mpsc::error::SendError<Vec<u8>>> {
        // Simulate transmission delay
        if self.config.baud_rate > 0 {
            let delay = self.config.transmission_time(data.len());
            tokio::time::sleep(delay).await;
        }

        self.tx.send(data.to_vec()).await
    }

    /// Receive data from the client.
    pub async fn recv(&mut self) -> Option<Vec<u8>> {
        self.rx.recv().await
    }

    /// Try to receive without blocking.
    pub fn try_recv(&mut self) -> Result<Vec<u8>, tokio::sync::mpsc::error::TryRecvError> {
        self.rx.try_recv()
    }
}

/// Client side of a serial pair.
#[derive(Debug)]
pub struct SerialPairClient {
    tx: tokio::sync::mpsc::Sender<Vec<u8>>,
    rx: tokio::sync::mpsc::Receiver<Vec<u8>>,
    config: SerialConfig,
}

impl SerialPairClient {
    /// Send data to the server.
    pub async fn send(
        &self,
        data: &[u8],
    ) -> Result<(), tokio::sync::mpsc::error::SendError<Vec<u8>>> {
        // Simulate transmission delay
        if self.config.baud_rate > 0 {
            let delay = self.config.transmission_time(data.len());
            tokio::time::sleep(delay).await;
        }

        self.tx.send(data.to_vec()).await
    }

    /// Receive data from the server.
    pub async fn recv(&mut self) -> Option<Vec<u8>> {
        self.rx.recv().await
    }

    /// Try to receive without blocking.
    pub fn try_recv(&mut self) -> Result<Vec<u8>, tokio::sync::mpsc::error::TryRecvError> {
        self.rx.try_recv()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_serial_config_defaults() {
        let config = SerialConfig::default();
        assert_eq!(config.baud_rate, 9600);
        assert_eq!(config.data_bits, DataBits::Eight);
        assert_eq!(config.parity, Parity::None);
        assert_eq!(config.stop_bits, StopBits::One);
    }

    #[test]
    fn test_bits_per_character() {
        // 8N1: 1 + 8 + 0 + 1 = 10 bits
        let config = SerialConfig::default();
        assert_eq!(config.bits_per_character(), 10.0);

        // 8E1: 1 + 8 + 1 + 1 = 11 bits
        let config = SerialConfig::default().with_parity(Parity::Even);
        assert_eq!(config.bits_per_character(), 11.0);

        // 8N2: 1 + 8 + 0 + 2 = 11 bits
        let config = SerialConfig::default().with_stop_bits(StopBits::Two);
        assert_eq!(config.bits_per_character(), 11.0);
    }

    #[test]
    fn test_char_time() {
        let config = SerialConfig::new(9600);
        let char_time = config.char_time();

        // 10 bits at 9600 baud ≈ 1.04ms
        let ms = char_time.as_micros();
        assert!(ms > 1000 && ms < 1100);
    }

    #[test]
    fn test_inter_frame_timeout() {
        // Low baud rate: 3.5 character times
        let config = SerialConfig::new(9600);
        let timeout = config.inter_frame_timeout();
        let ms = timeout.as_micros();
        assert!(ms > 3500 && ms < 4000);

        // High baud rate: fixed 1.75ms minimum
        let config = SerialConfig::new(115200);
        let timeout = config.inter_frame_timeout();
        assert_eq!(timeout, Duration::from_micros(1750));
    }

    #[test]
    fn test_virtual_serial_config() {
        let config = VirtualSerialConfig::default()
            .with_baud_rate(19200)
            .with_slave_path("/tmp/test_serial")
            .with_delay_simulation(true);

        assert_eq!(config.serial.baud_rate, 19200);
        assert_eq!(config.slave_path, PathBuf::from("/tmp/test_serial"));
        assert!(config.simulate_delays);
    }

    #[tokio::test]
    async fn test_serial_pair() {
        let (server, mut client) = SerialPair::new(SerialConfig::new(115200));

        // Server sends to client
        let data = vec![0x01, 0x02, 0x03];
        server.send(&data).await.unwrap();

        let received = client.recv().await.unwrap();
        assert_eq!(received, data);
    }

    #[tokio::test]
    async fn test_serial_pair_bidirectional() {
        let (mut server, mut client) = SerialPair::new(SerialConfig::new(115200));

        // Client sends to server
        let client_data = vec![0x01, 0x03, 0x00, 0x00, 0x00, 0x0A];
        client.send(&client_data).await.unwrap();

        let received = server.recv().await.unwrap();
        assert_eq!(received, client_data);

        // Server sends response
        let server_data = vec![0x01, 0x03, 0x14]; // Start of response
        server.send(&server_data).await.unwrap();

        let received = client.recv().await.unwrap();
        assert_eq!(received, server_data);
    }
}

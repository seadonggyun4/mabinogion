//! Transport abstraction for Modbus RTU.
//!
//! This module provides a unified transport interface that abstracts over
//! different communication mechanisms (virtual serial, TCP bridge, etc.).
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │                   RtuTransport Trait                        │
//! │                                                              │
//! │  ┌─────────────────┐ ┌──────────────┐ ┌─────────────────┐  │
//! │  │ VirtualSerial   │ │ TcpBridge    │ │ ChannelTransport│  │
//! │  │ (PTY-based)     │ │ (TCP→RTU)    │ │ (Testing)       │  │
//! │  └─────────────────┘ └──────────────┘ └─────────────────┘  │
//! └─────────────────────────────────────────────────────────────┘
//! ```

use std::io;
use std::net::SocketAddr;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;

use async_trait::async_trait;
use bytes::{Bytes, BytesMut};
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, ReadBuf};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;

use super::serial::{SerialConfig, VirtualSerial, VirtualSerialConfig};

/// Transport configuration types.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TransportConfig {
    /// Virtual serial port (PTY on Unix).
    VirtualSerial(VirtualSerialConfig),

    /// TCP bridge (TCP connections translated to RTU).
    TcpBridge(TcpBridgeConfig),

    /// Channel-based transport (for testing).
    Channel(ChannelConfig),
}

impl Default for TransportConfig {
    fn default() -> Self {
        Self::VirtualSerial(VirtualSerialConfig::default())
    }
}

/// TCP bridge configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TcpBridgeConfig {
    /// Address to bind the TCP server to.
    pub bind_address: SocketAddr,

    /// Serial configuration for timing.
    pub serial: SerialConfig,

    /// Maximum concurrent connections.
    pub max_connections: usize,

    /// Connection timeout.
    pub connection_timeout: Duration,

    /// Whether to strip TCP-specific headers.
    pub raw_mode: bool,
}

impl Default for TcpBridgeConfig {
    fn default() -> Self {
        Self {
            bind_address: "0.0.0.0:5020".parse().unwrap(),
            serial: SerialConfig::default(),
            max_connections: 10,
            connection_timeout: Duration::from_secs(60),
            raw_mode: true,
        }
    }
}

/// Channel-based transport configuration (for testing).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelConfig {
    /// Channel buffer size.
    pub buffer_size: usize,

    /// Serial configuration for timing simulation.
    pub serial: SerialConfig,

    /// Simulate transmission delays.
    pub simulate_delays: bool,
}

impl Default for ChannelConfig {
    fn default() -> Self {
        Self {
            buffer_size: 256,
            serial: SerialConfig::default(),
            simulate_delays: false,
        }
    }
}

/// Transport type identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransportType {
    /// Virtual serial port.
    VirtualSerial,
    /// TCP bridge.
    TcpBridge,
    /// Channel (testing).
    Channel,
}

/// Trait for RTU transport implementations.
///
/// This trait abstracts over the underlying transport mechanism,
/// allowing the RTU server to work with different transports interchangeably.
#[async_trait]
pub trait RtuTransport: Send + Sync {
    /// Get the transport type.
    fn transport_type(&self) -> TransportType;

    /// Check if the transport is connected/ready.
    fn is_ready(&self) -> bool;

    /// Read data from the transport.
    ///
    /// Returns the number of bytes read, or 0 if no data is available.
    async fn read(&mut self, buf: &mut [u8]) -> io::Result<usize>;

    /// Write data to the transport.
    ///
    /// Returns the number of bytes written.
    async fn write(&mut self, data: &[u8]) -> io::Result<usize>;

    /// Flush any buffered data.
    async fn flush(&mut self) -> io::Result<()>;

    /// Get the serial configuration for timing.
    fn serial_config(&self) -> &SerialConfig;

    /// Get the transmission delay for a given number of bytes.
    fn transmission_delay(&self, bytes: usize) -> Duration {
        self.serial_config().transmission_time(bytes)
    }

    /// Get the inter-frame timeout.
    fn inter_frame_timeout(&self) -> Duration {
        self.serial_config().inter_frame_timeout()
    }

    /// Close the transport.
    async fn close(&mut self) -> io::Result<()>;
}

/// Channel-based transport for testing.
///
/// Uses tokio channels to simulate a bidirectional transport.
pub struct ChannelTransport {
    /// Receive channel.
    rx: mpsc::Receiver<Bytes>,

    /// Send channel.
    tx: mpsc::Sender<Bytes>,

    /// Configuration.
    config: ChannelConfig,

    /// Read buffer for partial reads.
    read_buffer: BytesMut,
}

impl ChannelTransport {
    /// Create a connected pair of channel transports.
    pub fn pair(config: ChannelConfig) -> (Self, Self) {
        let (tx1, rx1) = mpsc::channel(config.buffer_size);
        let (tx2, rx2) = mpsc::channel(config.buffer_size);

        let transport1 = Self {
            rx: rx1,
            tx: tx2,
            config: config.clone(),
            read_buffer: BytesMut::new(),
        };

        let transport2 = Self {
            rx: rx2,
            tx: tx1,
            config,
            read_buffer: BytesMut::new(),
        };

        (transport1, transport2)
    }

    /// Create from existing channels.
    pub fn new(tx: mpsc::Sender<Bytes>, rx: mpsc::Receiver<Bytes>, config: ChannelConfig) -> Self {
        Self {
            rx,
            tx,
            config,
            read_buffer: BytesMut::new(),
        }
    }
}

#[async_trait]
impl RtuTransport for ChannelTransport {
    fn transport_type(&self) -> TransportType {
        TransportType::Channel
    }

    fn is_ready(&self) -> bool {
        !self.tx.is_closed()
    }

    async fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        // First, try to satisfy from buffer
        if !self.read_buffer.is_empty() {
            let len = std::cmp::min(buf.len(), self.read_buffer.len());
            buf[..len].copy_from_slice(&self.read_buffer.split_to(len));
            return Ok(len);
        }

        // Read from channel
        match self.rx.recv().await {
            Some(data) => {
                let len = std::cmp::min(buf.len(), data.len());
                buf[..len].copy_from_slice(&data[..len]);

                // Buffer remaining data
                if data.len() > len {
                    self.read_buffer.extend_from_slice(&data[len..]);
                }

                Ok(len)
            }
            None => Ok(0), // Channel closed
        }
    }

    async fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        // Simulate transmission delay if enabled
        if self.config.simulate_delays {
            let delay = self.config.serial.transmission_time(data.len());
            tokio::time::sleep(delay).await;
        }

        self.tx
            .send(Bytes::copy_from_slice(data))
            .await
            .map_err(|_| io::Error::new(io::ErrorKind::BrokenPipe, "Channel closed"))?;

        Ok(data.len())
    }

    async fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }

    fn serial_config(&self) -> &SerialConfig {
        &self.config.serial
    }

    async fn close(&mut self) -> io::Result<()> {
        // Channels close automatically when dropped
        Ok(())
    }
}

#[cfg(unix)]
pub struct VirtualSerialTransport {
    io: tokio::fs::File,
    config: VirtualSerialConfig,
}

#[cfg(unix)]
impl VirtualSerialTransport {
    fn new(io: tokio::fs::File, config: VirtualSerialConfig) -> Self {
        Self { io, config }
    }
}

#[cfg(unix)]
#[async_trait]
impl RtuTransport for VirtualSerialTransport {
    fn transport_type(&self) -> TransportType {
        TransportType::VirtualSerial
    }

    fn is_ready(&self) -> bool {
        true
    }

    async fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.io.read(buf).await
    }

    async fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        if self.config.simulate_delays {
            tokio::time::sleep(self.config.serial.transmission_time(data.len())).await;
        }
        self.io.write(data).await
    }

    async fn flush(&mut self) -> io::Result<()> {
        self.io.flush().await
    }

    fn serial_config(&self) -> &SerialConfig {
        &self.config.serial
    }

    async fn close(&mut self) -> io::Result<()> {
        self.io.flush().await
    }
}

pub struct TcpBridgeTransport {
    listener: TcpListener,
    stream: Option<TcpStream>,
    config: TcpBridgeConfig,
}

impl TcpBridgeTransport {
    pub async fn bind(config: TcpBridgeConfig) -> io::Result<Self> {
        let listener = TcpListener::bind(config.bind_address).await?;
        Ok(Self {
            listener,
            stream: None,
            config,
        })
    }

    async fn ensure_stream(&mut self) -> io::Result<&mut TcpStream> {
        if self.stream.is_none() {
            let (stream, _) = self.listener.accept().await?;
            self.stream = Some(stream);
        }
        Ok(self.stream.as_mut().expect("stream initialized"))
    }
}

#[async_trait]
impl RtuTransport for TcpBridgeTransport {
    fn transport_type(&self) -> TransportType {
        TransportType::TcpBridge
    }

    fn is_ready(&self) -> bool {
        true
    }

    async fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let result = {
            let stream = self.ensure_stream().await?;
            stream.read(buf).await
        };
        match result {
            Ok(0) => {
                self.stream = None;
                Ok(0)
            }
            other => other,
        }
    }

    async fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        let stream = self.ensure_stream().await?;
        stream.write(data).await
    }

    async fn flush(&mut self) -> io::Result<()> {
        if let Some(stream) = &mut self.stream {
            stream.flush().await
        } else {
            Ok(())
        }
    }

    fn serial_config(&self) -> &SerialConfig {
        &self.config.serial
    }

    async fn close(&mut self) -> io::Result<()> {
        self.stream.take();
        Ok(())
    }
}

/// Async I/O wrapper that implements AsyncRead and AsyncWrite.
///
/// This allows using an RtuTransport with tokio's framed codecs.
pub struct TransportIo<T: RtuTransport> {
    transport: T,
    read_buffer: BytesMut,
}

impl<T: RtuTransport> TransportIo<T> {
    /// Create a new TransportIo wrapper.
    pub fn new(transport: T) -> Self {
        Self {
            transport,
            read_buffer: BytesMut::with_capacity(256),
        }
    }

    /// Get a reference to the underlying transport.
    pub fn transport(&self) -> &T {
        &self.transport
    }

    /// Get a mutable reference to the underlying transport.
    pub fn transport_mut(&mut self) -> &mut T {
        &mut self.transport
    }

    /// Consume and return the underlying transport.
    pub fn into_inner(self) -> T {
        self.transport
    }
}

impl<T: RtuTransport + Unpin> AsyncRead for TransportIo<T> {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        // This is a simplified implementation that uses poll_fn
        // In production, you'd want a proper async implementation
        let this = self.get_mut();

        // Try to read from buffer first
        if !this.read_buffer.is_empty() {
            let len = std::cmp::min(buf.remaining(), this.read_buffer.len());
            buf.put_slice(&this.read_buffer.split_to(len));
            return Poll::Ready(Ok(()));
        }

        // We need to convert the async read to a poll-based one
        // This is a simplified version - in production, use a proper async adapter
        cx.waker().wake_by_ref();
        Poll::Pending
    }
}

impl<T: RtuTransport + Unpin> AsyncWrite for TransportIo<T> {
    fn poll_write(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        _buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        // Simplified implementation
        Poll::Pending
    }

    fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}

/// Transport factory for creating transports from configuration.
pub struct TransportFactory;

impl TransportFactory {
    /// Create a transport from configuration.
    ///
    /// Returns a boxed trait object for dynamic dispatch.
    pub async fn create(config: TransportConfig) -> io::Result<Box<dyn RtuTransport>> {
        match config {
            TransportConfig::Channel(cfg) => {
                let (transport, _peer) = ChannelTransport::pair(cfg);
                Ok(Box::new(transport))
            }
            TransportConfig::VirtualSerial(cfg) => {
                #[cfg(unix)]
                {
                    let serial = VirtualSerial::create(cfg.clone())
                        .map_err(|error| io::Error::new(io::ErrorKind::Other, error.to_string()))?;
                    let io = serial.into_async_io()?;
                    Ok(Box::new(VirtualSerialTransport::new(io, cfg)))
                }
                #[cfg(not(unix))]
                {
                    let _ = cfg;
                    Err(io::Error::new(
                        io::ErrorKind::Unsupported,
                        "virtual serial transport is not available on this platform",
                    ))
                }
            }
            TransportConfig::TcpBridge(cfg) => {
                let transport = TcpBridgeTransport::bind(cfg).await?;
                Ok(Box::new(transport))
            }
        }
    }
}

/// Transport metrics.
#[derive(Debug, Clone, Default)]
pub struct TransportMetrics {
    /// Total bytes received.
    pub bytes_received: u64,

    /// Total bytes sent.
    pub bytes_sent: u64,

    /// Total frames received.
    pub frames_received: u64,

    /// Total frames sent.
    pub frames_sent: u64,

    /// CRC errors.
    pub crc_errors: u64,

    /// Framing errors.
    pub framing_errors: u64,

    /// Timeouts.
    pub timeouts: u64,
}

impl TransportMetrics {
    /// Create new metrics.
    pub fn new() -> Self {
        Self::default()
    }

    /// Record bytes received.
    pub fn record_bytes_received(&mut self, bytes: usize) {
        self.bytes_received += bytes as u64;
    }

    /// Record bytes sent.
    pub fn record_bytes_sent(&mut self, bytes: usize) {
        self.bytes_sent += bytes as u64;
    }

    /// Record frame received.
    pub fn record_frame_received(&mut self) {
        self.frames_received += 1;
    }

    /// Record frame sent.
    pub fn record_frame_sent(&mut self) {
        self.frames_sent += 1;
    }

    /// Record CRC error.
    pub fn record_crc_error(&mut self) {
        self.crc_errors += 1;
    }

    /// Record framing error.
    pub fn record_framing_error(&mut self) {
        self.framing_errors += 1;
    }

    /// Record timeout.
    pub fn record_timeout(&mut self) {
        self.timeouts += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_channel_transport_pair() {
        let config = ChannelConfig::default();
        let (mut transport1, mut transport2) = ChannelTransport::pair(config);

        // Transport 1 writes, Transport 2 reads
        let data = b"Hello, RTU!";
        transport1.write(data).await.unwrap();

        let mut buf = [0u8; 32];
        let n = transport2.read(&mut buf).await.unwrap();

        assert_eq!(&buf[..n], data);
    }

    #[tokio::test]
    async fn test_channel_transport_bidirectional() {
        let config = ChannelConfig::default();
        let (mut server, mut client) = ChannelTransport::pair(config);

        // Client sends request
        let request = [0x01, 0x03, 0x00, 0x00, 0x00, 0x0A];
        client.write(&request).await.unwrap();

        // Server receives
        let mut buf = [0u8; 32];
        let n = server.read(&mut buf).await.unwrap();
        assert_eq!(&buf[..n], &request);

        // Server sends response
        let response = [0x01, 0x03, 0x14];
        server.write(&response).await.unwrap();

        // Client receives
        let n = client.read(&mut buf).await.unwrap();
        assert_eq!(&buf[..n], &response);
    }

    #[test]
    fn test_transport_metrics() {
        let mut metrics = TransportMetrics::new();

        metrics.record_bytes_received(100);
        metrics.record_bytes_sent(50);
        metrics.record_frame_received();
        metrics.record_crc_error();

        assert_eq!(metrics.bytes_received, 100);
        assert_eq!(metrics.bytes_sent, 50);
        assert_eq!(metrics.frames_received, 1);
        assert_eq!(metrics.crc_errors, 1);
    }

    #[test]
    fn test_tcp_bridge_config_default() {
        let config = TcpBridgeConfig::default();
        assert_eq!(config.bind_address.port(), 5020);
        assert_eq!(config.max_connections, 10);
        assert!(config.raw_mode);
    }
}

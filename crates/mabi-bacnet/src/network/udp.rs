//! BACnet/IP UDP socket management.
//!
//! This module provides UDP networking for BACnet/IP communications,
//! with support for unicast and broadcast messaging.

use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;

use tokio::net::UdpSocket;
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};

use crate::error::{BacnetError, BacnetResult};

use super::bvlc::{BvlcMessage, BACNET_IP_PORT};

/// Default BACnet/IP port.
pub const DEFAULT_PORT: u16 = BACNET_IP_PORT;

/// Default buffer size for UDP packets.
pub const DEFAULT_BUFFER_SIZE: usize = 1500;

/// Maximum APDU size for BACnet/IP.
pub const MAX_APDU_SIZE: u16 = 1476;

/// Network configuration for BACnet/IP.
#[derive(Debug, Clone)]
pub struct NetworkConfig {
    /// Address to bind to.
    pub bind_addr: SocketAddr,
    /// Broadcast address for the network.
    pub broadcast_addr: SocketAddr,
    /// Receive buffer size.
    pub recv_buffer_size: usize,
    /// Send buffer size.
    pub send_buffer_size: usize,
    /// Maximum APDU size.
    pub max_apdu_size: u16,
    /// Channel buffer size for incoming packets.
    pub channel_buffer_size: usize,
}

impl Default for NetworkConfig {
    fn default() -> Self {
        Self {
            bind_addr: format!("0.0.0.0:{}", DEFAULT_PORT).parse().unwrap(),
            broadcast_addr: format!("255.255.255.255:{}", DEFAULT_PORT)
                .parse()
                .unwrap(),
            recv_buffer_size: DEFAULT_BUFFER_SIZE,
            send_buffer_size: DEFAULT_BUFFER_SIZE,
            max_apdu_size: MAX_APDU_SIZE,
            channel_buffer_size: 10_000,
        }
    }
}

impl NetworkConfig {
    /// Create config with specific bind address.
    pub fn with_bind_addr(mut self, addr: SocketAddr) -> Self {
        self.bind_addr = addr;
        self
    }

    /// Create config with specific broadcast address.
    pub fn with_broadcast_addr(mut self, addr: SocketAddr) -> Self {
        self.broadcast_addr = addr;
        self
    }

    /// Create config with specific port.
    pub fn with_port(mut self, port: u16) -> Self {
        self.bind_addr.set_port(port);
        self.broadcast_addr.set_port(port);
        self
    }
}

/// Incoming packet from the network.
#[derive(Debug, Clone)]
pub struct IncomingPacket {
    /// Raw packet data.
    pub data: Vec<u8>,
    /// Source address.
    pub source: SocketAddr,
    /// Timestamp when received.
    pub timestamp: Instant,
    /// Parsed BVLC message (if valid).
    pub bvlc: Option<BvlcMessage>,
}

impl IncomingPacket {
    /// Create a new incoming packet.
    pub fn new(data: Vec<u8>, source: SocketAddr) -> Self {
        let bvlc = BvlcMessage::decode(&data).ok();
        Self {
            data,
            source,
            timestamp: Instant::now(),
            bvlc,
        }
    }

    /// Get the BVLC message if valid.
    pub fn message(&self) -> Option<&BvlcMessage> {
        self.bvlc.as_ref()
    }

    /// Get the effective source (original source for forwarded packets).
    pub fn effective_source(&self) -> SocketAddr {
        self.bvlc
            .as_ref()
            .and_then(|m| m.effective_source())
            .unwrap_or(self.source)
    }
}

/// Network statistics.
#[derive(Debug, Default)]
pub struct NetworkStats {
    /// Total packets received.
    pub packets_received: AtomicU64,
    /// Total packets sent.
    pub packets_sent: AtomicU64,
    /// Total bytes received.
    pub bytes_received: AtomicU64,
    /// Total bytes sent.
    pub bytes_sent: AtomicU64,
    /// Receive errors.
    pub receive_errors: AtomicU64,
    /// Send errors.
    pub send_errors: AtomicU64,
    /// Dropped packets (channel full).
    pub dropped_packets: AtomicU64,
}

impl NetworkStats {
    /// Record a received packet.
    pub fn record_received(&self, bytes: u64) {
        self.packets_received.fetch_add(1, Ordering::Relaxed);
        self.bytes_received.fetch_add(bytes, Ordering::Relaxed);
    }

    /// Record a sent packet.
    pub fn record_sent(&self, bytes: u64) {
        self.packets_sent.fetch_add(1, Ordering::Relaxed);
        self.bytes_sent.fetch_add(bytes, Ordering::Relaxed);
    }

    /// Record a receive error.
    pub fn record_receive_error(&self) {
        self.receive_errors.fetch_add(1, Ordering::Relaxed);
    }

    /// Record a send error.
    pub fn record_send_error(&self) {
        self.send_errors.fetch_add(1, Ordering::Relaxed);
    }

    /// Record a dropped packet.
    pub fn record_dropped(&self) {
        self.dropped_packets.fetch_add(1, Ordering::Relaxed);
    }

    /// Get snapshot of statistics.
    pub fn snapshot(&self) -> NetworkStatsSnapshot {
        NetworkStatsSnapshot {
            packets_received: self.packets_received.load(Ordering::Relaxed),
            packets_sent: self.packets_sent.load(Ordering::Relaxed),
            bytes_received: self.bytes_received.load(Ordering::Relaxed),
            bytes_sent: self.bytes_sent.load(Ordering::Relaxed),
            receive_errors: self.receive_errors.load(Ordering::Relaxed),
            send_errors: self.send_errors.load(Ordering::Relaxed),
            dropped_packets: self.dropped_packets.load(Ordering::Relaxed),
        }
    }
}

/// Snapshot of network statistics.
#[derive(Debug, Clone, Default)]
pub struct NetworkStatsSnapshot {
    pub packets_received: u64,
    pub packets_sent: u64,
    pub bytes_received: u64,
    pub bytes_sent: u64,
    pub receive_errors: u64,
    pub send_errors: u64,
    pub dropped_packets: u64,
}

/// Handle for sending packets through the network.
#[derive(Clone)]
pub struct NetworkHandle {
    socket: Arc<UdpSocket>,
    config: NetworkConfig,
    stats: Arc<NetworkStats>,
}

impl NetworkHandle {
    /// Send a packet to a specific address.
    pub async fn send_to(&self, data: &[u8], dest: SocketAddr) -> BacnetResult<()> {
        match self.socket.send_to(data, dest).await {
            Ok(len) => {
                self.stats.record_sent(len as u64);
                debug!(dest = %dest, len, "Sent packet");
                Ok(())
            }
            Err(e) => {
                self.stats.record_send_error();
                Err(BacnetError::Io(e))
            }
        }
    }

    /// Send a BVLC message to a specific address.
    pub async fn send_message(&self, msg: &BvlcMessage, dest: SocketAddr) -> BacnetResult<()> {
        let data = msg.encode();
        self.send_to(&data, dest).await
    }

    /// Broadcast a packet.
    pub async fn broadcast(&self, data: &[u8]) -> BacnetResult<()> {
        self.send_to(data, self.config.broadcast_addr).await
    }

    /// Broadcast a BVLC message.
    pub async fn broadcast_message(&self, msg: &BvlcMessage) -> BacnetResult<()> {
        let data = msg.encode();
        self.broadcast(&data).await
    }

    /// Get network statistics.
    pub fn stats(&self) -> NetworkStatsSnapshot {
        self.stats.snapshot()
    }

    /// Get the local address.
    pub fn local_addr(&self) -> BacnetResult<SocketAddr> {
        self.socket.local_addr().map_err(BacnetError::Io)
    }
}

/// BACnet/IP network handler.
pub struct BACnetNetwork {
    config: NetworkConfig,
    socket: Arc<UdpSocket>,
    recv_tx: mpsc::Sender<IncomingPacket>,
    stats: Arc<NetworkStats>,
    shutdown: Arc<AtomicBool>,
}

impl BACnetNetwork {
    /// Bind and create a new BACnet/IP network.
    pub async fn bind(
        config: NetworkConfig,
    ) -> BacnetResult<(Self, mpsc::Receiver<IncomingPacket>)> {
        let socket = UdpSocket::bind(&config.bind_addr).await.map_err(|e| {
            BacnetError::Server(format!("Failed to bind to {}: {}", config.bind_addr, e))
        })?;

        // Enable broadcast
        socket.set_broadcast(true).map_err(|e| {
            BacnetError::Server(format!("Failed to enable broadcast: {}", e))
        })?;

        let (recv_tx, recv_rx) = mpsc::channel(config.channel_buffer_size);

        info!(addr = %config.bind_addr, "BACnet/IP network bound");

        Ok((
            Self {
                socket: Arc::new(socket),
                recv_tx,
                stats: Arc::new(NetworkStats::default()),
                shutdown: Arc::new(AtomicBool::new(false)),
                config,
            },
            recv_rx,
        ))
    }

    /// Get a handle for sending packets.
    pub fn handle(&self) -> NetworkHandle {
        NetworkHandle {
            socket: self.socket.clone(),
            config: self.config.clone(),
            stats: self.stats.clone(),
        }
    }

    /// Get network statistics.
    pub fn stats(&self) -> &Arc<NetworkStats> {
        &self.stats
    }

    /// Request shutdown of the receive loop.
    pub fn shutdown(&self) {
        self.shutdown.store(true, Ordering::SeqCst);
    }

    /// Check if shutdown has been requested.
    pub fn is_shutdown(&self) -> bool {
        self.shutdown.load(Ordering::SeqCst)
    }

    /// Run the receive loop.
    ///
    /// This method runs until shutdown is requested or an unrecoverable error occurs.
    pub async fn run_receive_loop(&self) -> BacnetResult<()> {
        let mut buf = vec![0u8; self.config.recv_buffer_size];

        info!("Starting BACnet/IP receive loop");

        loop {
            if self.shutdown.load(Ordering::SeqCst) {
                info!("Receive loop shutdown requested");
                break;
            }

            // Use a timeout to allow checking shutdown flag periodically
            let recv_result =
                tokio::time::timeout(std::time::Duration::from_millis(100), self.socket.recv_from(&mut buf))
                    .await;

            match recv_result {
                Ok(Ok((len, source))) => {
                    self.stats.record_received(len as u64);

                    let packet = IncomingPacket::new(buf[..len].to_vec(), source);

                    if packet.bvlc.is_some() {
                        debug!(source = %source, len, "Received valid BVLC packet");
                    } else {
                        debug!(source = %source, len, "Received non-BVLC packet");
                    }

                    // Try to send to channel
                    if self.recv_tx.try_send(packet).is_err() {
                        self.stats.record_dropped();
                        warn!("Receive channel full, dropping packet");
                    }
                }
                Ok(Err(e)) => {
                    self.stats.record_receive_error();
                    error!(error = %e, "UDP receive error");
                }
                Err(_) => {
                    // Timeout - just continue to check shutdown flag
                    continue;
                }
            }
        }

        info!("BACnet/IP receive loop stopped");
        Ok(())
    }

    /// Get the local address.
    pub fn local_addr(&self) -> BacnetResult<SocketAddr> {
        self.socket.local_addr().map_err(BacnetError::Io)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_network_config_default() {
        let config = NetworkConfig::default();
        assert_eq!(config.bind_addr.port(), DEFAULT_PORT);
        assert_eq!(config.max_apdu_size, MAX_APDU_SIZE);
    }

    #[test]
    fn test_network_config_with_port() {
        let config = NetworkConfig::default().with_port(12345);
        assert_eq!(config.bind_addr.port(), 12345);
        assert_eq!(config.broadcast_addr.port(), 12345);
    }

    #[test]
    fn test_network_stats() {
        let stats = NetworkStats::default();

        stats.record_received(100);
        stats.record_received(200);
        stats.record_sent(50);
        stats.record_receive_error();

        let snapshot = stats.snapshot();
        assert_eq!(snapshot.packets_received, 2);
        assert_eq!(snapshot.bytes_received, 300);
        assert_eq!(snapshot.packets_sent, 1);
        assert_eq!(snapshot.bytes_sent, 50);
        assert_eq!(snapshot.receive_errors, 1);
    }

    #[test]
    fn test_incoming_packet() {
        // Create a valid BVLC packet
        let bvlc = BvlcMessage::original_unicast(vec![0x01, 0x04]);
        let data = bvlc.encode().to_vec();
        let source: SocketAddr = "192.168.1.100:47808".parse().unwrap();

        let packet = IncomingPacket::new(data, source);

        assert!(packet.message().is_some());
        assert_eq!(packet.effective_source(), source);
    }
}

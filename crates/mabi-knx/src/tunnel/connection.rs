//! KNXnet/IP Tunnelling service.
//!
//! This module implements the KNXnet/IP tunnelling protocol for
//! point-to-point connections between client and server.

use std::net::SocketAddr;
use std::time::{Duration, Instant};

use bytes::{Buf, BufMut, BytesMut};
use tokio::sync::mpsc;

use super::ack_waiter::AckMessage;
use super::fsm::TunnelFsm;
use super::sequence::{ReceivedValidation, SequenceTracker};
use crate::address::IndividualAddress;
use crate::cemi::CemiFrame;
use crate::error::{KnxError, KnxResult};
use crate::frame::Hpai;

// ============================================================================
// Connection Types
// ============================================================================

/// Connection type for KNXnet/IP.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ConnectionType {
    /// Device Management.
    DeviceManagement = 0x03,
    /// Tunnel connection.
    Tunnel = 0x04,
    /// Remote Logging.
    RemoteLogging = 0x06,
    /// Remote Configuration.
    RemoteConfiguration = 0x07,
    /// Object Server.
    ObjectServer = 0x08,
}

impl ConnectionType {
    /// Create from raw value.
    pub fn from_u8(value: u8) -> Option<Self> {
        match value {
            0x03 => Some(Self::DeviceManagement),
            0x04 => Some(Self::Tunnel),
            0x06 => Some(Self::RemoteLogging),
            0x07 => Some(Self::RemoteConfiguration),
            0x08 => Some(Self::ObjectServer),
            _ => None,
        }
    }
}

impl From<ConnectionType> for u8 {
    fn from(ct: ConnectionType) -> Self {
        ct as u8
    }
}

/// KNX layer type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[repr(u8)]
pub enum KnxLayer {
    /// Link layer.
    #[default]
    LinkLayer = 0x02,
    /// Raw layer.
    Raw = 0x04,
    /// Bus monitor.
    BusMonitor = 0x80,
}

impl KnxLayer {
    /// Create from raw value.
    pub fn from_u8(value: u8) -> Option<Self> {
        match value {
            0x02 => Some(Self::LinkLayer),
            0x04 => Some(Self::Raw),
            0x80 => Some(Self::BusMonitor),
            _ => None,
        }
    }
}

impl From<KnxLayer> for u8 {
    fn from(kl: KnxLayer) -> Self {
        kl as u8
    }
}

// ============================================================================
// Connect Request/Response
// ============================================================================

/// Connection Request Information (CRI).
#[derive(Debug, Clone)]
pub struct ConnectionRequestInfo {
    /// Connection type.
    pub connection_type: ConnectionType,
    /// KNX layer (for tunnel connections).
    pub knx_layer: KnxLayer,
}

impl ConnectionRequestInfo {
    /// Create for tunnel connection.
    pub fn tunnel(layer: KnxLayer) -> Self {
        Self {
            connection_type: ConnectionType::Tunnel,
            knx_layer: layer,
        }
    }

    /// Encode to bytes.
    pub fn encode(&self) -> Vec<u8> {
        let mut buf = BytesMut::with_capacity(4);
        buf.put_u8(4); // Structure length
        buf.put_u8(self.connection_type.into());
        buf.put_u8(self.knx_layer.into());
        buf.put_u8(0x00); // Reserved
        buf.to_vec()
    }

    /// Decode from bytes.
    pub fn decode(data: &[u8]) -> KnxResult<Self> {
        if data.len() < 4 {
            return Err(KnxError::frame_too_short(4, data.len()));
        }

        let mut buf = data;
        let length = buf.get_u8();
        if length != 4 {
            return Err(KnxError::InvalidHeader(format!(
                "Invalid CRI length: {}",
                length
            )));
        }

        let connection_type = ConnectionType::from_u8(buf.get_u8())
            .ok_or_else(|| KnxError::InvalidHeader("Unknown connection type".to_string()))?;
        let knx_layer = KnxLayer::from_u8(buf.get_u8()).unwrap_or(KnxLayer::LinkLayer);
        let _ = buf.get_u8(); // Reserved

        Ok(Self {
            connection_type,
            knx_layer,
        })
    }
}

/// Connection Response Data (CRD).
#[derive(Debug, Clone)]
pub struct ConnectionResponseData {
    /// Assigned individual address.
    pub individual_address: IndividualAddress,
}

impl ConnectionResponseData {
    /// Create new CRD.
    pub fn new(individual_address: IndividualAddress) -> Self {
        Self { individual_address }
    }

    /// Encode to bytes.
    pub fn encode(&self) -> Vec<u8> {
        let mut buf = BytesMut::with_capacity(4);
        buf.put_u8(4); // Structure length
        buf.put_u8(ConnectionType::Tunnel.into());
        buf.put_u16(self.individual_address.encode());
        buf.to_vec()
    }

    /// Decode from bytes.
    pub fn decode(data: &[u8]) -> KnxResult<Self> {
        if data.len() < 4 {
            return Err(KnxError::frame_too_short(4, data.len()));
        }

        let mut buf = data;
        let _length = buf.get_u8();
        let _connection_type = buf.get_u8();
        let individual_address = IndividualAddress::decode(buf.get_u16());

        Ok(Self { individual_address })
    }
}

/// Connect status codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ConnectStatus {
    /// No error.
    NoError = 0x00,
    /// Unsupported connection type.
    ConnectionType = 0x22,
    /// Unsupported connection option.
    ConnectionOption = 0x23,
    /// No more connections available.
    NoMoreConnections = 0x24,
    /// No more unique connections.
    NoMoreUniqueConnections = 0x25,
    /// Data connection error.
    DataConnection = 0x26,
    /// KNX connection error.
    KnxConnection = 0x27,
    /// Tunnelling layer not supported.
    TunnellingLayer = 0x29,
}

impl ConnectStatus {
    /// Create from raw value.
    pub fn from_u8(value: u8) -> Self {
        match value {
            0x00 => Self::NoError,
            0x22 => Self::ConnectionType,
            0x23 => Self::ConnectionOption,
            0x24 => Self::NoMoreConnections,
            0x25 => Self::NoMoreUniqueConnections,
            0x26 => Self::DataConnection,
            0x27 => Self::KnxConnection,
            0x29 => Self::TunnellingLayer,
            _ => Self::NoError,
        }
    }

    /// Check if successful.
    pub fn is_success(&self) -> bool {
        matches!(self, Self::NoError)
    }
}

impl From<ConnectStatus> for u8 {
    fn from(cs: ConnectStatus) -> Self {
        cs as u8
    }
}

/// Connect Request.
#[derive(Debug, Clone)]
pub struct ConnectRequest {
    /// Control endpoint (for control messages).
    pub control_endpoint: Hpai,
    /// Data endpoint (for tunnelling data).
    pub data_endpoint: Hpai,
    /// Connection request info.
    pub cri: ConnectionRequestInfo,
}

impl ConnectRequest {
    /// Create a new connect request.
    pub fn new(control_endpoint: Hpai, data_endpoint: Hpai, cri: ConnectionRequestInfo) -> Self {
        Self {
            control_endpoint,
            data_endpoint,
            cri,
        }
    }

    /// Encode to frame body.
    pub fn encode(&self) -> Vec<u8> {
        let mut buf = BytesMut::new();
        buf.put_slice(&self.control_endpoint.encode());
        buf.put_slice(&self.data_endpoint.encode());
        buf.put_slice(&self.cri.encode());
        buf.to_vec()
    }

    /// Decode from frame body.
    pub fn decode(data: &[u8]) -> KnxResult<Self> {
        if data.len() < 20 {
            return Err(KnxError::frame_too_short(20, data.len()));
        }

        let control_endpoint = Hpai::decode(data)?;
        let data_endpoint = Hpai::decode(&data[8..])?;
        let cri = ConnectionRequestInfo::decode(&data[16..])?;

        Ok(Self {
            control_endpoint,
            data_endpoint,
            cri,
        })
    }
}

/// Connect Response.
#[derive(Debug, Clone)]
pub struct ConnectResponse {
    /// Channel ID (0 if failed).
    pub channel_id: u8,
    /// Status.
    pub status: ConnectStatus,
    /// Data endpoint (NAT resolution).
    pub data_endpoint: Hpai,
    /// Connection response data.
    pub crd: Option<ConnectionResponseData>,
}

impl ConnectResponse {
    /// Create a successful response.
    pub fn success(channel_id: u8, data_endpoint: Hpai, crd: ConnectionResponseData) -> Self {
        Self {
            channel_id,
            status: ConnectStatus::NoError,
            data_endpoint,
            crd: Some(crd),
        }
    }

    /// Create an error response.
    pub fn error(status: ConnectStatus) -> Self {
        Self {
            channel_id: 0,
            status,
            data_endpoint: Hpai::nat(),
            crd: None,
        }
    }

    /// Encode to frame body.
    pub fn encode(&self) -> Vec<u8> {
        let mut buf = BytesMut::new();
        buf.put_u8(self.channel_id);
        buf.put_u8(self.status.into());
        buf.put_slice(&self.data_endpoint.encode());

        if let Some(crd) = &self.crd {
            buf.put_slice(&crd.encode());
        }

        buf.to_vec()
    }

    /// Decode from frame body.
    pub fn decode(data: &[u8]) -> KnxResult<Self> {
        if data.len() < 2 {
            return Err(KnxError::frame_too_short(2, data.len()));
        }

        let mut buf = data;
        let channel_id = buf.get_u8();
        let status = ConnectStatus::from_u8(buf.get_u8());

        if !status.is_success() {
            return Ok(Self::error(status));
        }

        if buf.len() < 8 {
            return Err(KnxError::frame_too_short(10, data.len()));
        }

        let data_endpoint = Hpai::decode(buf)?;
        let crd = if buf.len() >= 12 {
            Some(ConnectionResponseData::decode(&buf[8..])?)
        } else {
            None
        };

        Ok(Self {
            channel_id,
            status,
            data_endpoint,
            crd,
        })
    }
}

// ============================================================================
// Tunnelling Request/Ack
// ============================================================================

/// Tunnelling Request.
#[derive(Debug, Clone)]
pub struct TunnellingRequest {
    /// Channel ID.
    pub channel_id: u8,
    /// Sequence counter.
    pub sequence_counter: u8,
    /// cEMI frame.
    pub cemi: CemiFrame,
}

impl TunnellingRequest {
    /// Create a new tunnelling request.
    pub fn new(channel_id: u8, sequence_counter: u8, cemi: CemiFrame) -> Self {
        Self {
            channel_id,
            sequence_counter,
            cemi,
        }
    }

    /// Encode to frame body.
    pub fn encode(&self) -> Vec<u8> {
        let cemi_data = self.cemi.encode();
        let mut buf = BytesMut::with_capacity(4 + cemi_data.len());

        buf.put_u8(4); // Connection header length
        buf.put_u8(self.channel_id);
        buf.put_u8(self.sequence_counter);
        buf.put_u8(0); // Reserved

        buf.put_slice(&cemi_data);
        buf.to_vec()
    }

    /// Decode from frame body.
    pub fn decode(data: &[u8]) -> KnxResult<Self> {
        if data.len() < 4 {
            return Err(KnxError::frame_too_short(4, data.len()));
        }

        let mut buf = data;
        let _header_length = buf.get_u8();
        let channel_id = buf.get_u8();
        let sequence_counter = buf.get_u8();
        let _reserved = buf.get_u8();

        let cemi = CemiFrame::decode(buf)?;

        Ok(Self {
            channel_id,
            sequence_counter,
            cemi,
        })
    }
}

/// Tunnelling ACK.
#[derive(Debug, Clone)]
pub struct TunnellingAck {
    /// Channel ID.
    pub channel_id: u8,
    /// Sequence counter.
    pub sequence_counter: u8,
    /// Status (0 = OK).
    pub status: u8,
}

impl TunnellingAck {
    /// Create an OK ACK.
    pub fn ok(channel_id: u8, sequence_counter: u8) -> Self {
        Self {
            channel_id,
            sequence_counter,
            status: 0,
        }
    }

    /// Create an error ACK.
    pub fn error(channel_id: u8, sequence_counter: u8, status: u8) -> Self {
        Self {
            channel_id,
            sequence_counter,
            status,
        }
    }

    /// Check if successful.
    pub fn is_ok(&self) -> bool {
        self.status == 0
    }

    /// Encode to frame body.
    pub fn encode(&self) -> Vec<u8> {
        vec![4, self.channel_id, self.sequence_counter, self.status]
    }

    /// Decode from frame body.
    pub fn decode(data: &[u8]) -> KnxResult<Self> {
        if data.len() < 4 {
            return Err(KnxError::frame_too_short(4, data.len()));
        }

        let mut buf = data;
        let _header_length = buf.get_u8();
        let channel_id = buf.get_u8();
        let sequence_counter = buf.get_u8();
        let status = buf.get_u8();

        Ok(Self {
            channel_id,
            sequence_counter,
            status,
        })
    }
}

// ============================================================================
// Connection State
// ============================================================================

/// Connection State Request.
#[derive(Debug, Clone)]
pub struct ConnectionStateRequest {
    /// Channel ID.
    pub channel_id: u8,
    /// Control endpoint.
    pub control_endpoint: Hpai,
}

impl ConnectionStateRequest {
    /// Create a new request.
    pub fn new(channel_id: u8, control_endpoint: Hpai) -> Self {
        Self {
            channel_id,
            control_endpoint,
        }
    }

    /// Encode to frame body.
    pub fn encode(&self) -> Vec<u8> {
        let mut buf = BytesMut::with_capacity(10);
        buf.put_u8(self.channel_id);
        buf.put_u8(0); // Reserved
        buf.put_slice(&self.control_endpoint.encode());
        buf.to_vec()
    }

    /// Decode from frame body.
    pub fn decode(data: &[u8]) -> KnxResult<Self> {
        if data.len() < 10 {
            return Err(KnxError::frame_too_short(10, data.len()));
        }

        let mut buf = data;
        let channel_id = buf.get_u8();
        let _reserved = buf.get_u8();
        let control_endpoint = Hpai::decode(buf)?;

        Ok(Self {
            channel_id,
            control_endpoint,
        })
    }
}

/// Connection State Response.
#[derive(Debug, Clone)]
pub struct ConnectionStateResponse {
    /// Channel ID.
    pub channel_id: u8,
    /// Status.
    pub status: u8,
}

impl ConnectionStateResponse {
    /// Create an OK response.
    pub fn ok(channel_id: u8) -> Self {
        Self {
            channel_id,
            status: 0,
        }
    }

    /// Encode to frame body.
    pub fn encode(&self) -> Vec<u8> {
        vec![self.channel_id, self.status]
    }

    /// Decode from frame body.
    pub fn decode(data: &[u8]) -> KnxResult<Self> {
        if data.len() < 2 {
            return Err(KnxError::frame_too_short(2, data.len()));
        }

        Ok(Self {
            channel_id: data[0],
            status: data[1],
        })
    }
}

// ============================================================================
// Disconnect Request/Response
// ============================================================================

/// Disconnect Request.
#[derive(Debug, Clone)]
pub struct DisconnectRequest {
    /// Channel ID.
    pub channel_id: u8,
    /// Control endpoint.
    pub control_endpoint: Hpai,
}

impl DisconnectRequest {
    /// Create a new request.
    pub fn new(channel_id: u8, control_endpoint: Hpai) -> Self {
        Self {
            channel_id,
            control_endpoint,
        }
    }

    /// Encode to frame body.
    pub fn encode(&self) -> Vec<u8> {
        let mut buf = BytesMut::with_capacity(10);
        buf.put_u8(self.channel_id);
        buf.put_u8(0); // Reserved
        buf.put_slice(&self.control_endpoint.encode());
        buf.to_vec()
    }

    /// Decode from frame body.
    pub fn decode(data: &[u8]) -> KnxResult<Self> {
        if data.len() < 10 {
            return Err(KnxError::frame_too_short(10, data.len()));
        }

        let mut buf = data;
        let channel_id = buf.get_u8();
        let _reserved = buf.get_u8();
        let control_endpoint = Hpai::decode(buf)?;

        Ok(Self {
            channel_id,
            control_endpoint,
        })
    }
}

/// Disconnect Response.
#[derive(Debug, Clone)]
pub struct DisconnectResponse {
    /// Channel ID.
    pub channel_id: u8,
    /// Status.
    pub status: u8,
}

impl DisconnectResponse {
    /// Create an OK response.
    pub fn ok(channel_id: u8) -> Self {
        Self {
            channel_id,
            status: 0,
        }
    }

    /// Encode to frame body.
    pub fn encode(&self) -> Vec<u8> {
        vec![self.channel_id, self.status]
    }

    /// Decode from frame body.
    pub fn decode(data: &[u8]) -> KnxResult<Self> {
        if data.len() < 2 {
            return Err(KnxError::frame_too_short(2, data.len()));
        }

        Ok(Self {
            channel_id: data[0],
            status: data[1],
        })
    }
}

// ============================================================================
// Tunnel Connection
// ============================================================================

/// Tunnel connection with production-grade sequence validation and FSM.
///
/// Each connection tracks:
/// - **SequenceTracker**: knxd-compatible rno/sno with CAS wrapping
/// - **TunnelFsm**: 7-state FSM mapped to knxd mod 0-3
/// - **ACK channel**: for server→client frame delivery tracking
#[derive(Debug)]
pub struct TunnelConnection {
    /// Channel ID.
    pub channel_id: u8,
    /// Client address (control endpoint).
    pub client_addr: SocketAddr,
    /// Data endpoint (may differ from client_addr for NAT).
    pub data_endpoint: SocketAddr,
    /// Assigned individual address.
    pub individual_address: IndividualAddress,
    /// knxd-compatible sequence tracker with CAS-based rno/sno.
    pub sequence_tracker: SequenceTracker,
    /// Per-connection tunnel FSM (7-state).
    pub fsm: TunnelFsm,
    /// Sender for incoming ACK messages (server→client delivery tracking).
    ack_tx: mpsc::Sender<AckMessage>,
    /// Receiver for incoming ACK messages — consumed by AckWaiter.
    ack_rx: parking_lot::Mutex<Option<mpsc::Receiver<AckMessage>>>,
    /// Last activity time.
    last_activity: parking_lot::RwLock<Instant>,
    /// Heartbeat timeout.
    heartbeat_timeout: Duration,
}

impl TunnelConnection {
    /// Create a new tunnel connection with full Phase 1 support.
    pub fn new(
        channel_id: u8,
        client_addr: SocketAddr,
        data_endpoint: SocketAddr,
        individual_address: IndividualAddress,
        heartbeat_timeout: Duration,
    ) -> Self {
        let (ack_tx, ack_rx) = mpsc::channel(32);

        Self {
            channel_id,
            client_addr,
            data_endpoint,
            individual_address,
            sequence_tracker: SequenceTracker::new(),
            fsm: TunnelFsm::connecting(),
            ack_tx,
            ack_rx: parking_lot::Mutex::new(Some(ack_rx)),
            last_activity: parking_lot::RwLock::new(Instant::now()),
            heartbeat_timeout,
        }
    }

    /// Create with custom desync threshold.
    pub fn with_desync_threshold(
        channel_id: u8,
        client_addr: SocketAddr,
        data_endpoint: SocketAddr,
        individual_address: IndividualAddress,
        heartbeat_timeout: Duration,
        desync_threshold: u8,
    ) -> Self {
        let (ack_tx, ack_rx) = mpsc::channel(32);

        Self {
            channel_id,
            client_addr,
            data_endpoint,
            individual_address,
            sequence_tracker: SequenceTracker::with_desync_threshold(desync_threshold),
            fsm: TunnelFsm::connecting(),
            ack_tx,
            ack_rx: parking_lot::Mutex::new(Some(ack_rx)),
            last_activity: parking_lot::RwLock::new(Instant::now()),
            heartbeat_timeout,
        }
    }

    // ========================================================================
    // Sequence management (delegates to SequenceTracker)
    // ========================================================================

    /// Get next send sequence number (CAS-safe, wraps at 255→0).
    pub fn next_send_sequence(&self) -> u8 {
        self.sequence_tracker.next_sno()
    }

    /// Get current send sequence number without incrementing.
    pub fn current_send_sequence(&self) -> u8 {
        self.sequence_tracker.current_sno()
    }

    /// Validate a received sequence number with knxd-compatible logic.
    ///
    /// Returns a `ReceivedValidation` with the appropriate action:
    /// - Valid: process frame, advance rno
    /// - Duplicate: ACK only, don't process
    /// - OutOfOrder: log warning, ACK, process
    /// - FatalDesync: tunnel restart required
    pub fn validate_recv_sequence(&self, seq: u8) -> ReceivedValidation {
        self.sequence_tracker.validate_received(seq)
    }

    /// Legacy compatibility: check and update receive sequence (returns bool).
    pub fn check_recv_sequence(&self, seq: u8) -> bool {
        matches!(
            self.sequence_tracker.validate_received(seq),
            ReceivedValidation::Valid { .. }
        )
    }

    // ========================================================================
    // ACK channel management
    // ========================================================================

    /// Feed an incoming ACK message to this connection's ACK channel.
    pub fn feed_ack(&self, msg: AckMessage) {
        // Best-effort: if receiver is dropped, this is a no-op
        let _ = self.ack_tx.try_send(msg);
    }

    /// Take the ACK receiver (can only be taken once, for AckWaiter).
    pub fn take_ack_rx(&self) -> Option<mpsc::Receiver<AckMessage>> {
        self.ack_rx.lock().take()
    }

    // ========================================================================
    // Activity tracking
    // ========================================================================

    /// Update last activity time.
    pub fn touch(&self) {
        *self.last_activity.write() = Instant::now();
    }

    /// Check if connection is timed out.
    pub fn is_timed_out(&self) -> bool {
        self.last_activity.read().elapsed() > self.heartbeat_timeout
    }

    /// Get idle duration.
    pub fn idle_duration(&self) -> Duration {
        self.last_activity.read().elapsed()
    }

    // ========================================================================
    // Reset (for reconnection scenarios)
    // ========================================================================

    /// Reset sequence tracker and FSM for reconnection.
    pub fn reset(&self) {
        self.sequence_tracker.reset();
        self.fsm.force_idle();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    #[test]
    fn test_connect_request_encode_decode() {
        let req = ConnectRequest::new(
            Hpai::udp_ipv4(Ipv4Addr::new(192, 168, 1, 100), 3671),
            Hpai::udp_ipv4(Ipv4Addr::new(192, 168, 1, 100), 3672),
            ConnectionRequestInfo::tunnel(KnxLayer::LinkLayer),
        );

        let encoded = req.encode();
        let decoded = ConnectRequest::decode(&encoded).unwrap();

        assert_eq!(decoded.control_endpoint.port, req.control_endpoint.port);
        assert_eq!(decoded.cri.connection_type, ConnectionType::Tunnel);
    }

    #[test]
    fn test_tunnelling_request_encode_decode() {
        let cemi = CemiFrame::group_value_write(
            IndividualAddress::new(1, 2, 3),
            crate::address::GroupAddress::three_level(1, 0, 1),
            vec![1],
        );

        let req = TunnellingRequest::new(1, 0, cemi);
        let encoded = req.encode();
        let decoded = TunnellingRequest::decode(&encoded).unwrap();

        assert_eq!(decoded.channel_id, 1);
        assert_eq!(decoded.sequence_counter, 0);
    }

    #[test]
    fn test_tunnelling_ack() {
        let ack = TunnellingAck::ok(1, 5);
        assert!(ack.is_ok());

        let encoded = ack.encode();
        let decoded = TunnellingAck::decode(&encoded).unwrap();

        assert_eq!(decoded.channel_id, 1);
        assert_eq!(decoded.sequence_counter, 5);
        assert!(decoded.is_ok());
    }

    #[test]
    fn test_tunnel_connection_sequence() {
        let conn = TunnelConnection::new(
            1,
            "192.168.1.100:3671".parse().unwrap(),
            "192.168.1.100:3672".parse().unwrap(),
            IndividualAddress::new(1, 1, 100),
            Duration::from_secs(60),
        );

        assert_eq!(conn.next_send_sequence(), 0);
        assert_eq!(conn.next_send_sequence(), 1);
        assert_eq!(conn.current_send_sequence(), 2);

        assert!(conn.check_recv_sequence(0));
        assert!(conn.check_recv_sequence(1));
        assert!(!conn.check_recv_sequence(10)); // Out of sequence
    }
}

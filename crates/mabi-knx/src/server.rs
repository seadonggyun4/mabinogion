//! KNXnet/IP Server implementation.
//!
//! This module provides a complete KNXnet/IP server supporting tunnelling connections.

use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::Arc;
use std::time::Duration;

use dashmap::DashMap;
use tokio::net::UdpSocket;
use tokio::sync::{broadcast, mpsc};
use tracing::{debug, error, info, trace, warn};

use crate::address::{GroupAddress, IndividualAddress};
use crate::cemi::{Apci, CemiFrame};
use crate::config::KnxServerConfig;
use crate::error::{KnxError, KnxResult};
use crate::frame::{
    DibDeviceInfo, Hpai, KnxFrame, ServiceType, SupportedServiceFamilies,
};
use crate::group::GroupObjectTable;
use crate::tunnel::{
    ConnectRequest, ConnectResponse, ConnectStatus, ConnectionResponseData,
    ConnectionStateRequest, ConnectionStateResponse, DisconnectRequest, DisconnectResponse,
    TunnelConnection, TunnellingAck, TunnellingRequest,
};

// ============================================================================
// Server Event
// ============================================================================

/// Server event.
#[derive(Debug, Clone)]
pub enum ServerEvent {
    /// Server started.
    Started { address: SocketAddr },
    /// Server stopped.
    Stopped,
    /// Client connected.
    ClientConnected {
        channel_id: u8,
        address: SocketAddr,
    },
    /// Client disconnected.
    ClientDisconnected { channel_id: u8 },
    /// Group value written.
    GroupValueWrite {
        address: GroupAddress,
        value: Vec<u8>,
        source: IndividualAddress,
    },
    /// Group value read.
    GroupValueRead {
        address: GroupAddress,
        source: IndividualAddress,
    },
    /// Error occurred.
    Error { message: String },
}

// ============================================================================
// Server State
// ============================================================================

/// Server state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ServerState {
    /// Server is stopped.
    #[default]
    Stopped,
    /// Server is starting.
    Starting,
    /// Server is running.
    Running,
    /// Server is stopping.
    Stopping,
}

// ============================================================================
// Connection Manager
// ============================================================================

/// Manages tunnel connections.
pub struct ConnectionManager {
    connections: DashMap<u8, Arc<TunnelConnection>>,
    next_channel_id: AtomicU8,
    max_connections: usize,
    heartbeat_timeout: Duration,
    individual_address_base: IndividualAddress,
}

impl ConnectionManager {
    /// Create a new connection manager.
    pub fn new(
        max_connections: usize,
        heartbeat_timeout: Duration,
        individual_address_base: IndividualAddress,
    ) -> Self {
        Self {
            connections: DashMap::new(),
            next_channel_id: AtomicU8::new(1),
            max_connections,
            heartbeat_timeout,
            individual_address_base,
        }
    }

    /// Allocate a new channel.
    pub fn allocate_channel(&self) -> Option<u8> {
        if self.connections.len() >= self.max_connections {
            return None;
        }

        // Find next available channel ID
        for _ in 0..255 {
            let channel_id = self.next_channel_id.fetch_add(1, Ordering::SeqCst);
            if channel_id != 0 && !self.connections.contains_key(&channel_id) {
                return Some(channel_id);
            }
        }

        None
    }

    /// Create a new connection.
    pub fn create_connection(
        &self,
        channel_id: u8,
        client_addr: SocketAddr,
        data_endpoint: SocketAddr,
    ) -> Arc<TunnelConnection> {
        // Assign individual address based on channel
        let individual_address = IndividualAddress::new(
            self.individual_address_base.area(),
            self.individual_address_base.line(),
            100 + channel_id,
        );

        let connection = Arc::new(TunnelConnection::new(
            channel_id,
            client_addr,
            data_endpoint,
            individual_address,
            self.heartbeat_timeout,
        ));

        self.connections.insert(channel_id, connection.clone());
        connection
    }

    /// Get a connection.
    pub fn get(&self, channel_id: u8) -> Option<Arc<TunnelConnection>> {
        self.connections.get(&channel_id).map(|c| c.clone())
    }

    /// Remove a connection.
    pub fn remove(&self, channel_id: u8) -> Option<Arc<TunnelConnection>> {
        self.connections.remove(&channel_id).map(|(_, c)| c)
    }

    /// Get all connections.
    pub fn all(&self) -> Vec<Arc<TunnelConnection>> {
        self.connections.iter().map(|r| r.value().clone()).collect()
    }

    /// Get connection count.
    pub fn len(&self) -> usize {
        self.connections.len()
    }

    /// Check if empty.
    pub fn is_empty(&self) -> bool {
        self.connections.is_empty()
    }

    /// Clean up timed out connections.
    pub fn cleanup_timed_out(&self) -> Vec<u8> {
        let timed_out: Vec<_> = self
            .connections
            .iter()
            .filter(|r| r.value().is_timed_out())
            .map(|r| *r.key())
            .collect();

        for channel_id in &timed_out {
            self.connections.remove(channel_id);
        }

        timed_out
    }
}

// ============================================================================
// KNX Server
// ============================================================================

/// KNXnet/IP Server.
pub struct KnxServer {
    config: KnxServerConfig,
    state: parking_lot::RwLock<ServerState>,
    connections: ConnectionManager,
    group_objects: Arc<GroupObjectTable>,
    event_tx: broadcast::Sender<ServerEvent>,
    shutdown_tx: parking_lot::Mutex<Option<mpsc::Sender<()>>>,
    running: AtomicBool,
}

impl KnxServer {
    /// Create a new KNX server.
    pub fn new(config: KnxServerConfig) -> Self {
        let (event_tx, _) = broadcast::channel(1000);

        Self {
            connections: ConnectionManager::new(
                config.max_connections,
                config.connection_timeout(),
                config.individual_address,
            ),
            config,
            state: parking_lot::RwLock::new(ServerState::Stopped),
            group_objects: Arc::new(GroupObjectTable::new()),
            event_tx,
            shutdown_tx: parking_lot::Mutex::new(None),
            running: AtomicBool::new(false),
        }
    }

    /// Create with custom group object table.
    pub fn with_group_objects(mut self, table: Arc<GroupObjectTable>) -> Self {
        self.group_objects = table;
        self
    }

    /// Get server state.
    pub fn state(&self) -> ServerState {
        *self.state.read()
    }

    /// Check if running.
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }

    /// Get config.
    pub fn config(&self) -> &KnxServerConfig {
        &self.config
    }

    /// Get group object table.
    pub fn group_objects(&self) -> Arc<GroupObjectTable> {
        self.group_objects.clone()
    }

    /// Subscribe to server events.
    pub fn subscribe(&self) -> broadcast::Receiver<ServerEvent> {
        self.event_tx.subscribe()
    }

    /// Get connection count.
    pub fn connection_count(&self) -> usize {
        self.connections.len()
    }

    /// Start the server.
    pub async fn start(&self) -> KnxResult<()> {
        if self.is_running() {
            return Err(KnxError::ServerAlreadyRunning);
        }

        *self.state.write() = ServerState::Starting;

        // Bind UDP socket
        let socket = UdpSocket::bind(&self.config.bind_addr).await.map_err(|e| {
            KnxError::BindError {
                address: self.config.bind_addr,
                reason: e.to_string(),
            }
        })?;

        let local_addr = socket.local_addr()?;
        info!(address = %local_addr, "KNXnet/IP server started");

        self.running.store(true, Ordering::SeqCst);
        *self.state.write() = ServerState::Running;

        let _ = self.event_tx.send(ServerEvent::Started {
            address: local_addr,
        });

        // Create shutdown channel
        let (shutdown_tx, mut shutdown_rx) = mpsc::channel::<()>(1);
        *self.shutdown_tx.lock() = Some(shutdown_tx);

        let socket = Arc::new(socket);

        // Start receive loop
        let mut buf = vec![0u8; 1024];

        loop {
            tokio::select! {
                result = socket.recv_from(&mut buf) => {
                    match result {
                        Ok((len, addr)) => {
                            if let Err(e) = self.handle_packet(&socket, &buf[..len], addr).await {
                                debug!(error = %e, "Error handling packet");
                            }
                        }
                        Err(e) => {
                            error!(error = %e, "Error receiving packet");
                        }
                    }
                }
                _ = shutdown_rx.recv() => {
                    info!("Server shutdown requested");
                    break;
                }
            }
        }

        self.running.store(false, Ordering::SeqCst);
        *self.state.write() = ServerState::Stopped;
        let _ = self.event_tx.send(ServerEvent::Stopped);

        Ok(())
    }

    /// Stop the server.
    pub async fn stop(&self) -> KnxResult<()> {
        if !self.is_running() {
            return Ok(());
        }

        *self.state.write() = ServerState::Stopping;

        if let Some(tx) = self.shutdown_tx.lock().take() {
            let _ = tx.send(()).await;
        }

        Ok(())
    }

    /// Handle incoming packet.
    async fn handle_packet(
        &self,
        socket: &UdpSocket,
        data: &[u8],
        addr: SocketAddr,
    ) -> KnxResult<()> {
        let frame = KnxFrame::decode(data)?;


        trace!(
            service_type = ?frame.service_type,
            from = %addr,
            "Received KNXnet/IP frame"
        );

        match frame.service_type {
            ServiceType::SearchRequest => {
                self.handle_search_request(socket, addr).await?;
            }
            ServiceType::DescriptionRequest => {
                self.handle_description_request(socket, addr).await?;
            }
            ServiceType::ConnectRequest => {
                self.handle_connect_request(socket, &frame.body, addr).await?;
            }
            ServiceType::ConnectionStateRequest => {
                self.handle_connection_state_request(socket, &frame.body, addr)
                    .await?;
            }
            ServiceType::DisconnectRequest => {
                self.handle_disconnect_request(socket, &frame.body, addr)
                    .await?;
            }
            ServiceType::TunnellingRequest => {
                self.handle_tunnelling_request(socket, &frame.body, addr)
                    .await?;
            }
            ServiceType::TunnellingAck => {
                // Just update connection activity
                let ack = TunnellingAck::decode(&frame.body)?;
                if let Some(conn) = self.connections.get(ack.channel_id) {
                    conn.touch();
                }
            }
            _ => {
                debug!(service_type = ?frame.service_type, "Unhandled service type");
            }
        }

        Ok(())
    }

    /// Handle search request.
    async fn handle_search_request(
        &self,
        socket: &UdpSocket,
        addr: SocketAddr,
    ) -> KnxResult<()> {
        debug!(from = %addr, "Handling SearchRequest");

        let local_addr = socket.local_addr()?;
        let local_ip = match local_addr {
            SocketAddr::V4(v4) => *v4.ip(),
            _ => std::net::Ipv4Addr::UNSPECIFIED,
        };

        let hpai = Hpai::udp_ipv4(local_ip, local_addr.port());
        let device_info = DibDeviceInfo::new(&self.config.device_name, self.config.individual_address)
            .with_serial_number(self.config.serial_number)
            .with_mac_address(self.config.mac_address);
        let families = SupportedServiceFamilies::default_families();

        let mut body = hpai.encode();
        body.extend(device_info.encode());
        body.extend(families.encode());

        let response = KnxFrame::new(ServiceType::SearchResponse, body);
        socket.send_to(&response.encode(), addr).await?;

        Ok(())
    }

    /// Handle description request.
    async fn handle_description_request(
        &self,
        socket: &UdpSocket,
        addr: SocketAddr,
    ) -> KnxResult<()> {
        debug!(from = %addr, "Handling DescriptionRequest");

        let device_info = DibDeviceInfo::new(&self.config.device_name, self.config.individual_address)
            .with_serial_number(self.config.serial_number)
            .with_mac_address(self.config.mac_address);
        let families = SupportedServiceFamilies::default_families();

        let mut body = device_info.encode();
        body.extend(families.encode());

        let response = KnxFrame::new(ServiceType::DescriptionResponse, body);
        socket.send_to(&response.encode(), addr).await?;

        Ok(())
    }

    /// Handle connect request.
    async fn handle_connect_request(
        &self,
        socket: &UdpSocket,
        data: &[u8],
        addr: SocketAddr,
    ) -> KnxResult<()> {
        let request = ConnectRequest::decode(data)?;
        debug!(from = %addr, "Handling ConnectRequest");

        // Check if we can accept more connections
        let channel_id = match self.connections.allocate_channel() {
            Some(id) => id,
            None => {
                let response = ConnectResponse::error(ConnectStatus::NoMoreConnections);
                let frame = KnxFrame::new(ServiceType::ConnectResponse, response.encode());
                socket.send_to(&frame.encode(), addr).await?;
                return Ok(());
            }
        };

        // Determine data endpoint (NAT resolution)
        let data_endpoint = if request.data_endpoint.is_nat() {
            addr
        } else {
            request.data_endpoint.to_socket_addr_v()
        };

        let connection = self.connections.create_connection(channel_id, addr, data_endpoint);

        let local_addr = socket.local_addr()?;
        let local_ip = match local_addr {
            SocketAddr::V4(v4) => *v4.ip(),
            _ => std::net::Ipv4Addr::UNSPECIFIED,
        };

        let response = ConnectResponse::success(
            channel_id,
            Hpai::udp_ipv4(local_ip, local_addr.port()),
            ConnectionResponseData::new(connection.individual_address),
        );

        let frame = KnxFrame::new(ServiceType::ConnectResponse, response.encode());
        socket.send_to(&frame.encode(), addr).await?;

        info!(
            channel_id = channel_id,
            client = %addr,
            individual_address = %connection.individual_address,
            "Client connected"
        );

        let _ = self.event_tx.send(ServerEvent::ClientConnected {
            channel_id,
            address: addr,
        });

        Ok(())
    }

    /// Handle connection state request.
    async fn handle_connection_state_request(
        &self,
        socket: &UdpSocket,
        data: &[u8],
        addr: SocketAddr,
    ) -> KnxResult<()> {
        let request = ConnectionStateRequest::decode(data)?;

        let response = if let Some(conn) = self.connections.get(request.channel_id) {
            conn.touch();
            ConnectionStateResponse::ok(request.channel_id)
        } else {
            ConnectionStateResponse {
                channel_id: request.channel_id,
                status: 0x21, // E_CONNECTION_ID
            }
        };

        let frame = KnxFrame::new(ServiceType::ConnectionStateResponse, response.encode());
        socket.send_to(&frame.encode(), addr).await?;

        Ok(())
    }

    /// Handle disconnect request.
    async fn handle_disconnect_request(
        &self,
        socket: &UdpSocket,
        data: &[u8],
        addr: SocketAddr,
    ) -> KnxResult<()> {
        let request = DisconnectRequest::decode(data)?;
        debug!(channel_id = request.channel_id, "Handling DisconnectRequest");

        self.connections.remove(request.channel_id);

        let response = DisconnectResponse::ok(request.channel_id);
        let frame = KnxFrame::new(ServiceType::DisconnectResponse, response.encode());
        socket.send_to(&frame.encode(), addr).await?;

        info!(channel_id = request.channel_id, "Client disconnected");

        let _ = self.event_tx.send(ServerEvent::ClientDisconnected {
            channel_id: request.channel_id,
        });

        Ok(())
    }

    /// Handle tunnelling request.
    async fn handle_tunnelling_request(
        &self,
        socket: &UdpSocket,
        data: &[u8],
        addr: SocketAddr,
    ) -> KnxResult<()> {
        let request = TunnellingRequest::decode(data)?;

        let connection = match self.connections.get(request.channel_id) {
            Some(conn) => {
                conn
            }
            None => {
                let ack = TunnellingAck::error(request.channel_id, request.sequence_counter, 0x21);
                let frame = KnxFrame::new(ServiceType::TunnellingAck, ack.encode());
                socket.send_to(&frame.encode(), addr).await?;
                return Ok(());
            }
        };

        // Check sequence number
        if !connection.check_recv_sequence(request.sequence_counter) {
            warn!(
                channel_id = request.channel_id,
                expected = connection.current_send_sequence(),
                got = request.sequence_counter,
                "Sequence error"
            );
        }

        connection.touch();

        // Send ACK
        let ack = TunnellingAck::ok(request.channel_id, request.sequence_counter);
        let frame = KnxFrame::new(ServiceType::TunnellingAck, ack.encode());
        socket.send_to(&frame.encode(), addr).await?;

        // Process cEMI frame
        self.process_cemi(socket, addr, &request.cemi, &connection).await?;

        Ok(())
    }

    /// Process cEMI frame.
    async fn process_cemi(
        &self,
        socket: &UdpSocket,
        client_addr: SocketAddr,
        cemi: &CemiFrame,
        connection: &TunnelConnection,
    ) -> KnxResult<()> {
        if !cemi.apci.is_group_value() {
            return Ok(());
        }

        let group_addr = match cemi.destination_group() {
            Some(addr) => {
                addr
            }
            None => {
                return Ok(());
            }
        };

        match cemi.apci {
            Apci::GroupValueWrite => {
                debug!(
                    address = %group_addr,
                    source = %cemi.source,
                    "Group Value Write"
                );

                if let Err(e) = self.group_objects.write(
                    &group_addr,
                    &cemi.data,
                    Some(cemi.source.to_string()),
                ) {
                    debug!(error = %e, "Failed to write group value");
                }

                let _ = self.event_tx.send(ServerEvent::GroupValueWrite {
                    address: group_addr,
                    value: cemi.data.clone(),
                    source: cemi.source,
                });
            }
            Apci::GroupValueRead => {
                debug!(
                    address = %group_addr,
                    source = %cemi.source,
                    "Group Value Read"
                );

                let _ = self.event_tx.send(ServerEvent::GroupValueRead {
                    address: group_addr,
                    source: cemi.source,
                });

                // Send GroupValueResponse back via tunnelling
                let response_data = match self.group_objects.read(&group_addr) {
                    Ok(data) => {
                        data
                    }
                    Err(e) => {
                        vec![0u8]
                    }
                };

                let response_cemi = CemiFrame::group_value_response(
                    self.config.individual_address,
                    group_addr,
                    response_data,
                );

                let seq = connection.next_send_sequence();
                let tunnel_req = TunnellingRequest::new(
                    connection.channel_id,
                    seq,
                    response_cemi,
                );
                let frame = KnxFrame::new(ServiceType::TunnellingRequest, tunnel_req.encode());
                let response_bytes = frame.encode();
                if let Err(e) = socket.send_to(&response_bytes, client_addr).await {
                    debug!(error = %e, "Failed to send GroupValueResponse");
                }
            }
            _ => {}
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_connection_manager() {
        let manager = ConnectionManager::new(
            10,
            Duration::from_secs(60),
            IndividualAddress::new(1, 1, 0),
        );

        let channel_id = manager.allocate_channel().unwrap();
        assert!(channel_id > 0);

        let conn = manager.create_connection(
            channel_id,
            "192.168.1.100:3671".parse().unwrap(),
            "192.168.1.100:3672".parse().unwrap(),
        );

        assert_eq!(conn.channel_id, channel_id);
        assert_eq!(manager.len(), 1);

        manager.remove(channel_id);
        assert!(manager.is_empty());
    }

    #[test]
    fn test_server_config() {
        let config = KnxServerConfig::default();
        assert!(config.validate().is_ok());
        assert!(config.tunneling_enabled);
    }
}

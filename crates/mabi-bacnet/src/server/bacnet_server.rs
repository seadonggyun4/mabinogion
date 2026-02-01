//! BACnet/IP server implementation.
//!
//! Provides a complete BACnet/IP server with service handling, COV support,
//! and device discovery.

use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::broadcast;
use tracing::{debug, error, info, warn};

use crate::apdu::types::{ApduType, ConfirmedService, ErrorClass, ErrorCode, UnconfirmedService};
use crate::error::{BacnetError, BacnetResult};
use crate::network::bvlc::BvlcMessage;
use crate::network::npdu::Npdu;
use crate::network::udp::{BACnetNetwork, IncomingPacket, NetworkConfig, NetworkHandle};
use crate::object::property::SegmentationSupport;
use crate::object::registry::ObjectRegistry;
use crate::service::cov::{CovManager, CovNotification};
use crate::service::discovery::WhoIsHandler;
use crate::service::handler::{ServiceContext, ServiceRegistry, ServiceResult};
use crate::service::property::{ReadPropertyHandler, WritePropertyHandler};
use crate::service::property_multiple::{ReadPropertyMultipleHandler, WritePropertyMultipleHandler};

use super::metrics::{LatencyTimer, ServerMetrics};

/// Server configuration.
#[derive(Debug, Clone)]
pub struct ServerConfig {
    /// Network bind address.
    pub bind_addr: SocketAddr,
    /// Broadcast address.
    pub broadcast_addr: SocketAddr,
    /// Device instance number.
    pub device_instance: u32,
    /// Device name.
    pub device_name: String,
    /// Vendor ID.
    pub vendor_id: u16,
    /// Model name.
    pub model_name: String,
    /// Maximum APDU length.
    pub max_apdu_length: u16,
    /// Maximum COV subscriptions.
    pub max_cov_subscriptions: usize,
    /// COV check interval.
    pub cov_check_interval: Duration,
    /// Shutdown timeout.
    pub shutdown_timeout: Duration,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            bind_addr: "0.0.0.0:47808".parse().unwrap(),
            broadcast_addr: "255.255.255.255:47808".parse().unwrap(),
            device_instance: 1234,
            device_name: "BACnet Simulator".to_string(),
            vendor_id: 0,
            model_name: "OTSIM".to_string(),
            max_apdu_length: 1476,
            max_cov_subscriptions: 1000,
            cov_check_interval: Duration::from_secs(1),
            shutdown_timeout: Duration::from_secs(30),
        }
    }
}

impl ServerConfig {
    /// Create a new config with the specified device instance.
    pub fn new(device_instance: u32) -> Self {
        Self {
            device_instance,
            ..Default::default()
        }
    }

    /// Set the bind address.
    pub fn with_bind_addr(mut self, addr: SocketAddr) -> Self {
        self.bind_addr = addr;
        self
    }

    /// Set the device name.
    pub fn with_device_name(mut self, name: impl Into<String>) -> Self {
        self.device_name = name.into();
        self
    }

    /// Set the vendor ID.
    pub fn with_vendor_id(mut self, vendor_id: u16) -> Self {
        self.vendor_id = vendor_id;
        self
    }
}

/// Server events.
#[derive(Debug, Clone)]
pub enum ServerEvent {
    /// Server started.
    Started { address: SocketAddr },
    /// Server stopped.
    Stopped,
    /// Device discovered (I-Am received).
    DeviceDiscovered {
        device_instance: u32,
        address: SocketAddr,
    },
    /// Error occurred.
    Error { message: String },
}

/// BACnet/IP server.
pub struct BACnetServer {
    config: ServerConfig,
    objects: Arc<ObjectRegistry>,
    services: Arc<ServiceRegistry>,
    metrics: Arc<ServerMetrics>,
    shutdown: Arc<AtomicBool>,
    shutdown_tx: broadcast::Sender<()>,
    event_tx: broadcast::Sender<ServerEvent>,
}

impl BACnetServer {
    /// Create a new BACnet/IP server.
    pub fn new(config: ServerConfig, objects: ObjectRegistry) -> Self {
        let (shutdown_tx, _) = broadcast::channel(1);
        let (event_tx, _) = broadcast::channel(64);

        // Create service registry with default handlers
        let mut services = ServiceRegistry::new();
        // Property services
        services.register_confirmed(Arc::new(ReadPropertyHandler));
        services.register_confirmed(Arc::new(WritePropertyHandler));
        // Property multiple services (batch operations)
        services.register_confirmed(Arc::new(ReadPropertyMultipleHandler::new()));
        services.register_confirmed(Arc::new(WritePropertyMultipleHandler::new()));
        // Discovery services
        services.register_unconfirmed(Arc::new(WhoIsHandler::new(
            config.device_instance,
            config.max_apdu_length,
            SegmentationSupport::None,
            config.vendor_id,
        )));

        Self {
            config,
            objects: Arc::new(objects),
            services: Arc::new(services),
            metrics: Arc::new(ServerMetrics::new()),
            shutdown: Arc::new(AtomicBool::new(false)),
            shutdown_tx,
            event_tx,
        }
    }

    /// Create with custom service registry.
    pub fn with_services(mut self, services: ServiceRegistry) -> Self {
        self.services = Arc::new(services);
        self
    }

    /// Get the object registry.
    pub fn objects(&self) -> &Arc<ObjectRegistry> {
        &self.objects
    }

    /// Get server metrics.
    pub fn metrics(&self) -> &Arc<ServerMetrics> {
        &self.metrics
    }

    /// Subscribe to server events.
    pub fn subscribe(&self) -> broadcast::Receiver<ServerEvent> {
        self.event_tx.subscribe()
    }

    /// Request server shutdown.
    pub fn shutdown(&self) {
        if !self.shutdown.swap(true, Ordering::SeqCst) {
            info!("Shutdown requested");
            let _ = self.shutdown_tx.send(());
        }
    }

    /// Check if shutdown has been requested.
    pub fn is_shutdown(&self) -> bool {
        self.shutdown.load(Ordering::SeqCst)
    }

    /// Run the server.
    pub async fn run(&self) -> BacnetResult<()> {
        // Bind network
        let network_config = NetworkConfig::default()
            .with_bind_addr(self.config.bind_addr)
            .with_broadcast_addr(self.config.broadcast_addr);

        let (network, mut recv_rx) = BACnetNetwork::bind(network_config).await?;
        let network_handle = network.handle();

        let local_addr = network.local_addr()?;
        info!(address = %local_addr, "BACnet/IP server started");

        let _ = self.event_tx.send(ServerEvent::Started {
            address: local_addr,
        });

        // Create COV manager
        let (cov_manager, mut cov_rx) =
            CovManager::new(self.config.device_instance, self.config.max_cov_subscriptions);
        let cov_manager = Arc::new(cov_manager);

        // Spawn network receive loop
        let shutdown_clone = self.shutdown.clone();
        let network_shutdown = Arc::new(AtomicBool::new(false));
        let network_shutdown_clone = network_shutdown.clone();
        let network_task = tokio::spawn(async move {
            while !shutdown_clone.load(Ordering::SeqCst) {
                if let Err(e) = network.run_receive_loop().await {
                    error!(error = %e, "Network receive loop error");
                    break;
                }
            }
            network_shutdown_clone.store(true, Ordering::SeqCst);
        });

        // Spawn COV notification sender
        let cov_network = network_handle.clone();
        let metrics_clone = self.metrics.clone();
        let shutdown_cov = self.shutdown.clone();
        let cov_task = tokio::spawn(async move {
            while !shutdown_cov.load(Ordering::SeqCst) {
                tokio::select! {
                    Some(notification) = cov_rx.recv() => {
                        if let Err(e) = send_cov_notification(&cov_network, notification).await {
                            warn!(error = %e, "Failed to send COV notification");
                        } else {
                            metrics_clone.record_cov_notification_sent();
                        }
                    }
                    _ = tokio::time::sleep(std::time::Duration::from_millis(100)) => {
                        // Check shutdown periodically
                    }
                }
            }
        });

        let mut shutdown_rx = self.shutdown_tx.subscribe();

        // Main processing loop
        loop {
            tokio::select! {
                // Process incoming packets
                Some(packet) = recv_rx.recv() => {
                    self.metrics.record_request();
                    self.metrics.record_bytes_received(packet.data.len() as u64);

                    if let Err(e) = self.process_packet(&packet, &network_handle, &cov_manager).await {
                        debug!(error = %e, "Error processing packet");
                        self.metrics.record_error();
                    }
                }

                // Handle shutdown
                _ = shutdown_rx.recv() => {
                    info!("Shutdown signal received");
                    break;
                }
            }
        }

        // Graceful shutdown - set network shutdown flag
        network_shutdown.store(true, Ordering::SeqCst);

        // Wait for tasks to finish
        let _ = tokio::time::timeout(self.config.shutdown_timeout, async {
            let _ = network_task.await;
            let _ = cov_task.await;
        })
        .await;

        let _ = self.event_tx.send(ServerEvent::Stopped);
        info!("BACnet/IP server stopped");

        Ok(())
    }

    /// Process an incoming packet.
    async fn process_packet(
        &self,
        packet: &IncomingPacket,
        network: &NetworkHandle,
        _cov_manager: &Arc<CovManager>,
    ) -> BacnetResult<()> {
        let timer = LatencyTimer::start();

        // Parse BVLC message
        let bvlc = match &packet.bvlc {
            Some(msg) => msg,
            None => {
                debug!(source = %packet.source, "Invalid BVLC message");
                return Err(BacnetError::Protocol("Invalid BVLC message".into()));
            }
        };

        // Get NPDU from BVLC
        let npdu_data = match bvlc.npdu() {
            Some(data) => data,
            None => {
                debug!(function = ?bvlc.header.function, "No NPDU in BVLC message");
                return Ok(()); // Not an error, just a BVLC-only message
            }
        };

        // Parse NPDU
        let npdu = Npdu::decode(npdu_data).map_err(|e| BacnetError::Protocol(e.to_string()))?;

        // Skip network layer messages for now
        if npdu.is_network_message() {
            debug!("Network layer message, skipping");
            return Ok(());
        }

        // Get APDU data
        let apdu = npdu.apdu();
        if apdu.is_empty() {
            return Err(BacnetError::Protocol("Empty APDU".into()));
        }

        // Parse APDU type
        let apdu_type_byte = (apdu[0] >> 4) & 0x0F;
        let apdu_type = ApduType::from_nibble(apdu_type_byte).ok_or_else(|| {
            BacnetError::Protocol(format!("Unknown APDU type: {}", apdu_type_byte))
        })?;

        // Create service context
        let ctx = ServiceContext::new(self.objects.clone(), self.config.device_instance);

        // Process based on APDU type
        let response = match apdu_type {
            ApduType::ConfirmedRequest => {
                self.metrics.record_confirmed_request();
                self.process_confirmed_request(apdu, &ctx, packet.source)?
            }
            ApduType::UnconfirmedRequest => {
                self.metrics.record_unconfirmed_request();
                self.process_unconfirmed_request(apdu, &ctx, packet.source)?
            }
            _ => {
                debug!(apdu_type = ?apdu_type, "Unsupported APDU type");
                return Ok(());
            }
        };

        // Send response if needed
        if let Some((response_apdu, dest)) = response {
            let response_npdu = if npdu.expects_reply() {
                Npdu::simple(response_apdu.clone())
            } else {
                Npdu::no_reply(response_apdu.clone())
            };

            let response_bvlc = if bvlc.is_broadcast() {
                BvlcMessage::original_broadcast(response_npdu.encode())
            } else {
                BvlcMessage::original_unicast(response_npdu.encode())
            };

            let response_bytes = response_bvlc.encode();
            self.metrics.record_bytes_sent(response_bytes.len() as u64);

            network.send_to(&response_bytes, dest).await?;
        }

        let latency = timer.elapsed_us();
        self.metrics.record_success(latency);

        Ok(())
    }

    /// Process a confirmed request.
    fn process_confirmed_request(
        &self,
        apdu: &[u8],
        ctx: &ServiceContext,
        source: SocketAddr,
    ) -> BacnetResult<Option<(Vec<u8>, SocketAddr)>> {
        if apdu.len() < 4 {
            return Err(BacnetError::Protocol("Confirmed request too short".into()));
        }

        // Parse confirmed request header
        let pdu_type = apdu[0];
        let _max_segs_response = (pdu_type >> 4) & 0x07;
        let invoke_id = apdu[2];
        let service_choice = apdu[3];

        // Get service data
        let service_data = &apdu[4..];

        // Track specific services
        if let Some(service) = ConfirmedService::from_u8(service_choice) {
            match service {
                ConfirmedService::ReadProperty | ConfirmedService::ReadPropertyMultiple => {
                    self.metrics.record_read_property();
                }
                ConfirmedService::WriteProperty | ConfirmedService::WritePropertyMultiple => {
                    self.metrics.record_write_property();
                }
                ConfirmedService::SubscribeCov | ConfirmedService::SubscribeCovProperty => {
                    self.metrics.record_cov_subscription();
                }
                _ => {}
            }
        }

        // Dispatch to handler
        let ctx_with_invoke = ctx.clone().with_invoke_id(invoke_id);
        let result = self
            .services
            .dispatch_confirmed(service_choice, service_data, &ctx_with_invoke);

        // Build response APDU
        let response_apdu = match result {
            ServiceResult::SimpleAck => {
                // Simple ACK: type (0x20) | invoke_id | service_choice
                vec![0x20, invoke_id, service_choice]
            }
            ServiceResult::ComplexAck(data) => {
                // Complex ACK: type (0x30) | invoke_id | service_choice | data
                let mut apdu = vec![0x30, invoke_id, service_choice];
                apdu.extend_from_slice(&data);
                apdu
            }
            ServiceResult::Error {
                error_class,
                error_code,
            } => {
                // Error: type (0x50) | invoke_id | service_choice | error_class | error_code
                build_error_apdu(invoke_id, service_choice, error_class, error_code)
            }
            ServiceResult::Reject(reason) => {
                // Reject: type (0x60) | invoke_id | reason
                vec![0x60, invoke_id, reason]
            }
            ServiceResult::NoResponse | ServiceResult::Broadcast(_) => {
                return Ok(None);
            }
        };

        // Return response (we don't have source address here, caller needs to provide)
        Ok(Some((response_apdu, source)))
    }

    /// Process an unconfirmed request.
    fn process_unconfirmed_request(
        &self,
        apdu: &[u8],
        ctx: &ServiceContext,
        source: SocketAddr,
    ) -> BacnetResult<Option<(Vec<u8>, SocketAddr)>> {
        if apdu.len() < 2 {
            return Err(BacnetError::Protocol(
                "Unconfirmed request too short".into(),
            ));
        }

        // Parse unconfirmed request
        let _pdu_type = apdu[0]; // Should be 0x10
        let service_choice = apdu[1];
        let service_data = &apdu[2..];

        // Track Who-Is
        if service_choice == UnconfirmedService::WhoIs as u8 {
            self.metrics.record_who_is();
        }

        // Dispatch to handler
        let result = self
            .services
            .dispatch_unconfirmed(service_choice, service_data, ctx);

        match result {
            ServiceResult::Broadcast(data) => {
                // For I-Am responses, broadcast back
                self.metrics.record_i_am_sent();

                // Build unconfirmed APDU: type (0x10) | service (I-Am = 0) | data
                let mut apdu = vec![0x10, UnconfirmedService::IAm as u8];
                apdu.extend_from_slice(&data);

                // Return with broadcast or unicast destination
                Ok(Some((apdu, source)))
            }
            ServiceResult::NoResponse => Ok(None),
            _ => Ok(None),
        }
    }
}

impl ServiceContext {
    /// Clone with invoke ID.
    fn clone(&self) -> Self {
        Self {
            objects: self.objects.clone(),
            device_instance: self.device_instance,
            invoke_id: self.invoke_id,
            max_apdu_length: self.max_apdu_length,
        }
    }
}

/// Build an error APDU.
fn build_error_apdu(invoke_id: u8, service_choice: u8, error_class: ErrorClass, error_code: ErrorCode) -> Vec<u8> {
    // Error PDU format:
    // - Type byte: 0x50
    // - Invoke ID
    // - Service choice
    // - Error class (application tag + value)
    // - Error code (application tag + value)
    let mut apdu = vec![0x50, invoke_id, service_choice];

    // Encode error class as application tag 0 (unsigned)
    let error_class_val = error_class as u32;
    if error_class_val <= 4 {
        apdu.push(0x01); // Application tag 0, length 1
        apdu.push(error_class_val as u8);
    } else {
        apdu.push(0x02); // Application tag 0, length 2
        apdu.push((error_class_val >> 8) as u8);
        apdu.push(error_class_val as u8);
    }

    // Encode error code as application tag 0 (unsigned)
    let error_code_val = error_code as u32;
    if error_code_val <= 255 {
        apdu.push(0x11); // Application tag 1, length 1
        apdu.push(error_code_val as u8);
    } else {
        apdu.push(0x12); // Application tag 1, length 2
        apdu.push((error_code_val >> 8) as u8);
        apdu.push(error_code_val as u8);
    }

    apdu
}

/// Send a COV notification.
async fn send_cov_notification(
    network: &NetworkHandle,
    notification: CovNotification,
) -> BacnetResult<()> {
    let apdu = if notification.confirmed {
        // Confirmed COV notification
        // For simplicity, we'll send unconfirmed for now
        let mut apdu = vec![0x10, UnconfirmedService::UnconfirmedCovNotification as u8];
        apdu.extend_from_slice(&notification.encode_unconfirmed());
        apdu
    } else {
        // Unconfirmed COV notification
        let mut apdu = vec![0x10, UnconfirmedService::UnconfirmedCovNotification as u8];
        apdu.extend_from_slice(&notification.encode_unconfirmed());
        apdu
    };

    let npdu = Npdu::no_reply(apdu);
    let bvlc = BvlcMessage::original_unicast(npdu.encode());

    network.send_to(&bvlc.encode(), notification.destination).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_server_config_default() {
        let config = ServerConfig::default();
        assert_eq!(config.bind_addr.port(), 47808);
        assert_eq!(config.max_apdu_length, 1476);
    }

    #[test]
    fn test_server_config_builder() {
        let config = ServerConfig::new(5678)
            .with_device_name("Test Device")
            .with_vendor_id(123);

        assert_eq!(config.device_instance, 5678);
        assert_eq!(config.device_name, "Test Device");
        assert_eq!(config.vendor_id, 123);
    }

    #[test]
    fn test_build_error_apdu() {
        let apdu = build_error_apdu(1, 12, ErrorClass::Property, ErrorCode::UnknownProperty);

        assert_eq!(apdu[0], 0x50); // Error PDU type
        assert_eq!(apdu[1], 1); // Invoke ID
        assert_eq!(apdu[2], 12); // Service choice (ReadProperty)
    }
}

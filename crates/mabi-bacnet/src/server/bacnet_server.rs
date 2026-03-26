//! BACnet/IP server implementation.
//!
//! Provides a complete BACnet/IP server with service handling, COV support,
//! and device discovery.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;
use tokio::sync::{broadcast, mpsc};
use tracing::{debug, error, info, warn};

use crate::apdu::encoding::ApduEncoder;
use crate::apdu::segmentation::{
    AssemblyResult, Segment, SegmentAssembler, SegmentTransmitter, DEFAULT_WINDOW_SIZE,
};
use crate::apdu::types::{
    AbortReason, ApduType, ConfirmedService, ErrorClass, ErrorCode, RejectReason,
    UnconfirmedService,
};
use crate::error::{BacnetError, BacnetResult};
use crate::network::bbmd::{Bbmd, BbmdConfig};
use crate::network::bvlc::{BvlcFunction, BvlcMessage};
use crate::network::npdu::Npdu;
use crate::network::udp::{BACnetNetwork, IncomingPacket, NetworkConfig, NetworkHandle};
use crate::object::device::{DeviceObject, DeviceObjectConfig};
use crate::object::property::SegmentationSupport;
use crate::object::registry::ObjectRegistry;
use crate::object::traits::CovSupport;
use crate::service::cov::{CovManager, CovNotification};
use crate::service::discovery::WhoIsHandler;
use crate::service::handler::{ServiceContext, ServiceRegistry, ServiceResult};
use crate::service::property::{ReadPropertyHandler, WritePropertyHandler};
use crate::service::property_multiple::{
    ReadPropertyMultipleHandler, WritePropertyMultipleHandler,
};
use crate::service::subscribe_cov::SubscribeCovHandler;
use crate::service::tsm::{ServerTsm, TransactionKey, TsmConfig};

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

/// Processed response from a confirmed or unconfirmed request.
///
/// May be a single unsegmented APDU, a series of segmented APDUs,
/// or no response at all.
enum ProcessedResponse {
    /// Single unsegmented APDU response.
    Single(Vec<u8>, SocketAddr),
    /// Segmented response: multiple APDU segments to send sequentially.
    Segmented {
        /// The raw service data segments (without headers — headers are built at send time).
        segments: Vec<Vec<u8>>,
        dest: SocketAddr,
        invoke_id: u8,
        service_choice: u8,
    },
    /// No response needed.
    None,
}

/// BACnet/IP server.
pub struct BACnetServer {
    config: ServerConfig,
    objects: Arc<ObjectRegistry>,
    services: Arc<ServiceRegistry>,
    metrics: Arc<ServerMetrics>,
    cov_manager: Arc<CovManager>,
    tsm: Arc<ServerTsm>,
    /// Segment assembler for incoming segmented requests (receive side).
    segment_assembler: Mutex<SegmentAssembler>,
    /// Segment transmitter for outgoing segmented responses (send side).
    segment_transmitter: SegmentTransmitter,
    /// BBMD for cross-subnet broadcast management.
    bbmd: Arc<Bbmd>,
    cov_rx: tokio::sync::Mutex<mpsc::Receiver<CovNotification>>,
    shutdown: Arc<AtomicBool>,
    shutdown_tx: broadcast::Sender<()>,
    event_tx: broadcast::Sender<ServerEvent>,
}

impl BACnetServer {
    /// Create a new BACnet/IP server.
    pub fn new(config: ServerConfig, objects: ObjectRegistry) -> Self {
        let (shutdown_tx, _) = broadcast::channel(1);
        let (event_tx, _) = broadcast::channel(64);

        let objects = Arc::new(objects);

        // Create and register the Device object (mandatory per ASHRAE 135)
        let device_config = DeviceObjectConfig {
            device_instance: config.device_instance,
            device_name: config.device_name.clone(),
            vendor_name: "OTSIM".into(),
            vendor_id: config.vendor_id,
            model_name: config.model_name.clone(),
            firmware_revision: "1.0.0".into(),
            application_software_version: "1.0.0".into(),
            description: String::new(),
            location: String::new(),
            max_apdu_length: config.max_apdu_length,
            segmentation_supported: SegmentationSupport::Both,
            apdu_timeout: 3000,
            number_of_apdu_retries: 3,
        };
        let device_object = Arc::new(DeviceObject::new(device_config, objects.clone()));
        objects.register(device_object.clone());

        // Create COV manager early so the SubscribeCOV handler can use it
        let (cov_manager, cov_rx) =
            CovManager::new(config.device_instance, config.max_cov_subscriptions);
        let cov_manager = Arc::new(cov_manager);

        // Create service registry with default handlers
        let mut services = ServiceRegistry::new();
        // Property services
        services.register_confirmed(Arc::new(ReadPropertyHandler));
        services.register_confirmed(Arc::new(WritePropertyHandler));
        // Property multiple services (batch operations)
        services.register_confirmed(Arc::new(ReadPropertyMultipleHandler::new()));
        services.register_confirmed(Arc::new(WritePropertyMultipleHandler::new()));
        // COV services
        services.register_confirmed(Arc::new(SubscribeCovHandler::new(cov_manager.clone())));
        services.register_confirmed(Arc::new(
            crate::service::subscribe_cov::SubscribeCovPropertyHandler::new(cov_manager.clone()),
        ));
        // Log services (ReadRange for TrendLog)
        services.register_confirmed(Arc::new(crate::service::read_range::ReadRangeHandler::new()));
        // File access services (AtomicReadFile/AtomicWriteFile)
        services.register_confirmed(Arc::new(
            crate::service::file_access::AtomicReadFileHandler::new(),
        ));
        services.register_confirmed(Arc::new(
            crate::service::file_access::AtomicWriteFileHandler::new(),
        ));
        // Alarm and event services
        services.register_confirmed(Arc::new(
            crate::service::alarm::AcknowledgeAlarmHandler::new(),
        ));
        services.register_confirmed(Arc::new(
            crate::service::alarm::GetAlarmSummaryHandler::new(),
        ));
        services.register_confirmed(Arc::new(
            crate::service::alarm::GetEnrollmentSummaryHandler::new(),
        ));
        services.register_confirmed(Arc::new(
            crate::service::alarm::GetEventInformationHandler::new(),
        ));
        services.register_confirmed(Arc::new(
            crate::service::alarm::ConfirmedEventNotificationHandler::new(),
        ));
        // Object management services
        services.register_confirmed(Arc::new(
            crate::service::create_delete::CreateObjectHandler::new(),
        ));
        services.register_confirmed(Arc::new(
            crate::service::create_delete::DeleteObjectHandler::new(),
        ));
        // Device control services
        services.register_confirmed(Arc::new(
            crate::service::device_control::DeviceCommunicationControlHandler::new(),
        ));
        services.register_confirmed(Arc::new(
            crate::service::device_control::ReinitializeDeviceHandler::new(),
        ));
        // Discovery services
        services.register_unconfirmed(Arc::new(WhoIsHandler::new(
            config.device_instance,
            config.max_apdu_length,
            SegmentationSupport::Both,
            config.vendor_id,
        )));
        // Time synchronization services
        services.register_unconfirmed(Arc::new(
            crate::service::device_control::TimeSynchronizationHandler::new(),
        ));
        services.register_unconfirmed(Arc::new(
            crate::service::device_control::UtcTimeSynchronizationHandler::new(),
        ));

        // Update Device object with registered services and object types
        let confirmed_choices = services.supported_confirmed_services();
        let unconfirmed_choices = services.supported_unconfirmed_services();
        device_object.update_services_supported(&confirmed_choices, &unconfirmed_choices);
        device_object.update_object_types_supported();

        // Segmentation: the max segment size for outgoing responses is the
        // max_apdu_length minus the segmented ComplexACK header overhead (5 bytes:
        // pdu_type, invoke_id, sequence_number, window_size, service_choice).
        let segment_header_overhead = 5;
        let max_segment_data =
            (config.max_apdu_length as usize).saturating_sub(segment_header_overhead);
        let segment_transmitter =
            SegmentTransmitter::new(max_segment_data).with_window_size(DEFAULT_WINDOW_SIZE);
        let segment_assembler = SegmentAssembler::default();

        Self {
            config,
            objects,
            services: Arc::new(services),
            metrics: Arc::new(ServerMetrics::new()),
            cov_manager,
            tsm: Arc::new(ServerTsm::new()),
            segment_assembler: Mutex::new(segment_assembler),
            segment_transmitter,
            bbmd: Arc::new(Bbmd::default()),
            cov_rx: tokio::sync::Mutex::new(cov_rx),
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

    /// Create with custom TSM configuration.
    pub fn with_tsm_config(mut self, config: TsmConfig) -> Self {
        self.tsm = Arc::new(ServerTsm::with_config(config));
        self
    }

    /// Create with custom BBMD configuration.
    pub fn with_bbmd_config(mut self, config: BbmdConfig) -> Self {
        self.bbmd = Arc::new(Bbmd::new(config));
        self
    }

    /// Get a reference to the TSM.
    pub fn tsm(&self) -> &Arc<ServerTsm> {
        &self.tsm
    }

    /// Get a reference to the BBMD.
    pub fn bbmd(&self) -> &Arc<Bbmd> {
        &self.bbmd
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

        // Take the COV receiver out of the server (can only run once)
        let cov_manager = self.cov_manager.clone();
        let mut cov_rx = {
            let mut guard = self.cov_rx.lock().await;
            // Replace with a dummy channel — the original receiver is consumed
            let (_dummy_tx, dummy_rx) = mpsc::channel(1);
            std::mem::replace(&mut *guard, dummy_rx)
        };

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

        // Periodic cleanup for BBMD FDT expiration and stale segment assemblies.
        // Runs inline in the main select! loop on a 30-second interval.
        let bbmd_clone = self.bbmd.clone();
        let bbmd_cleanup_enabled = self.bbmd.is_enabled();
        let cleanup_interval = Duration::from_secs(30);

        // COV change detection polling interval
        let cov_objects_registry = self.objects.clone();
        let cov_manager_poll = cov_manager.clone();
        let cov_check_interval = self.config.cov_check_interval;
        let mut cov_ticker = tokio::time::interval(cov_check_interval);
        cov_ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

        let mut shutdown_rx = self.shutdown_tx.subscribe();
        let mut cleanup_ticker = tokio::time::interval(cleanup_interval);
        cleanup_ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

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

                // Periodic cleanup: FDT expiration + stale segment assemblies
                _ = cleanup_ticker.tick() => {
                    if bbmd_cleanup_enabled {
                        let expired = bbmd_clone.cleanup();
                        if expired > 0 {
                            debug!(expired, "BBMD FDT cleanup completed");
                        }
                    }

                    // Clean up stale segment assembly entries
                    let stale = {
                        let mut assembler = self.segment_assembler.lock();
                        assembler.cleanup()
                    };
                    if stale > 0 {
                        debug!(stale, "Segment assembler cleanup completed");
                    }
                }

                // COV change detection polling
                _ = cov_ticker.tick() => {
                    // Check all objects for COV changes and send notifications
                    for obj in cov_objects_registry.iter() {
                        if let Some(cov_obj) = obj.as_any().downcast_ref::<crate::object::standard::AnalogInput>() {
                            if cov_obj.check_cov() {
                                let values = cov_obj.cov_values();
                                cov_obj.reset_cov();
                                let _ = cov_manager_poll.notify_change(obj.object_identifier(), values).await;
                            }
                        } else if let Some(cov_obj) = obj.as_any().downcast_ref::<crate::object::standard::AnalogOutput>() {
                            if cov_obj.check_cov() {
                                let values = cov_obj.cov_values();
                                cov_obj.reset_cov();
                                let _ = cov_manager_poll.notify_change(obj.object_identifier(), values).await;
                            }
                        } else if let Some(cov_obj) = obj.as_any().downcast_ref::<crate::object::standard::AnalogValue>() {
                            if cov_obj.check_cov() {
                                let values = cov_obj.cov_values();
                                cov_obj.reset_cov();
                                let _ = cov_manager_poll.notify_change(obj.object_identifier(), values).await;
                            }
                        } else if let Some(cov_obj) = obj.as_any().downcast_ref::<crate::object::standard::BinaryInput>() {
                            if cov_obj.check_cov() {
                                let values = cov_obj.cov_values();
                                cov_obj.reset_cov();
                                let _ = cov_manager_poll.notify_change(obj.object_identifier(), values).await;
                            }
                        } else if let Some(cov_obj) = obj.as_any().downcast_ref::<crate::object::standard::BinaryOutput>() {
                            if cov_obj.check_cov() {
                                let values = cov_obj.cov_values();
                                cov_obj.reset_cov();
                                let _ = cov_manager_poll.notify_change(obj.object_identifier(), values).await;
                            }
                        } else if let Some(cov_obj) = obj.as_any().downcast_ref::<crate::object::standard::BinaryValue>() {
                            if cov_obj.check_cov() {
                                let values = cov_obj.cov_values();
                                cov_obj.reset_cov();
                                let _ = cov_manager_poll.notify_change(obj.object_identifier(), values).await;
                            }
                        } else if let Some(cov_obj) = obj.as_any().downcast_ref::<crate::object::standard::MultiStateInput>() {
                            if cov_obj.check_cov() {
                                let values = cov_obj.cov_values();
                                cov_obj.reset_cov();
                                let _ = cov_manager_poll.notify_change(obj.object_identifier(), values).await;
                            }
                        } else if let Some(cov_obj) = obj.as_any().downcast_ref::<crate::object::standard::MultiStateOutput>() {
                            if cov_obj.check_cov() {
                                let values = cov_obj.cov_values();
                                cov_obj.reset_cov();
                                let _ = cov_manager_poll.notify_change(obj.object_identifier(), values).await;
                            }
                        } else if let Some(cov_obj) = obj.as_any().downcast_ref::<crate::object::standard::MultiStateValue>() {
                            if cov_obj.check_cov() {
                                let values = cov_obj.cov_values();
                                cov_obj.reset_cov();
                                let _ = cov_manager_poll.notify_change(obj.object_identifier(), values).await;
                            }
                        }
                    }

                    // Also cleanup expired subscriptions
                    cov_manager_poll.cleanup_expired();
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

        // ── BBMD BVLC-level handling ──
        // Intercept BBMD-specific BVLC functions before NPDU/APDU processing.
        // These are layer-2 management functions that operate at the BVLC level.
        if self.bbmd.is_enabled() {
            match bvlc.header.function {
                BvlcFunction::RegisterForeignDevice
                | BvlcFunction::ReadForeignDeviceTable
                | BvlcFunction::DeleteForeignDeviceTableEntry
                | BvlcFunction::ReadBroadcastDistributionTable
                | BvlcFunction::WriteBroadcastDistributionTable => {
                    // Convert source to SocketAddrV4 for BBMD handler
                    let source_v4 = match packet.source {
                        SocketAddr::V4(v4) => v4,
                        SocketAddr::V6(_) => {
                            debug!("BBMD does not support IPv6 sources");
                            return Ok(());
                        }
                    };

                    if let Some(response) = self.bbmd.handle_message(bvlc, source_v4) {
                        // Track foreign device registration metrics
                        if bvlc.header.function == BvlcFunction::RegisterForeignDevice {
                            self.metrics.record_bbmd_foreign_registration();
                        }

                        let response_bytes = response.encode();
                        self.metrics.record_bytes_sent(response_bytes.len() as u64);
                        network.send_to(&response_bytes, packet.source).await?;
                    }

                    let latency = timer.elapsed_us();
                    self.metrics.record_success(latency);
                    return Ok(());
                }
                BvlcFunction::DistributeBroadcastToNetwork => {
                    // Client requests this BBMD to distribute a broadcast.
                    // Forward to all BDT peers and registered foreign devices,
                    // then process the NPDU locally (fall through to normal processing).
                    let source_v4 = match packet.source {
                        SocketAddr::V4(v4) => v4,
                        _ => SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 0),
                    };

                    let forward_addrs = self.bbmd.get_forward_addresses(Some(&source_v4));
                    if !forward_addrs.is_empty() {
                        // Re-wrap as Forwarded-NPDU for each recipient
                        let forwarded = BvlcMessage::forwarded_npdu(bvlc.npdu.clone(), source_v4);
                        let forwarded_bytes = forwarded.encode();

                        for addr in &forward_addrs {
                            let dest = SocketAddr::V4(*addr);
                            if let Err(e) = network.send_to(&forwarded_bytes, dest).await {
                                warn!(dest = %dest, error = %e, "Failed to forward broadcast");
                            } else {
                                self.metrics.record_bbmd_forwarded();
                                self.metrics.record_bytes_sent(forwarded_bytes.len() as u64);
                            }
                        }

                        debug!(
                            source = %packet.source,
                            forwarded_to = forward_addrs.len(),
                            "Distributed broadcast to network"
                        );
                    }
                    // Fall through to process the NPDU locally
                }
                BvlcFunction::OriginalBroadcastNpdu => {
                    // Local broadcast: forward to BDT peers and foreign devices
                    let source_v4 = match packet.source {
                        SocketAddr::V4(v4) => v4,
                        _ => SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 0),
                    };

                    let forward_addrs = self.bbmd.get_forward_addresses(Some(&source_v4));
                    if !forward_addrs.is_empty() {
                        let forwarded = BvlcMessage::forwarded_npdu(bvlc.npdu.clone(), source_v4);
                        let forwarded_bytes = forwarded.encode();

                        for addr in &forward_addrs {
                            let dest = SocketAddr::V4(*addr);
                            if let Err(e) = network.send_to(&forwarded_bytes, dest).await {
                                warn!(dest = %dest, error = %e, "Failed to forward broadcast");
                            } else {
                                self.metrics.record_bbmd_forwarded();
                                self.metrics.record_bytes_sent(forwarded_bytes.len() as u64);
                            }
                        }
                    }
                    // Fall through to process locally
                }
                _ => {
                    // Other BVLC functions — continue to normal NPDU/APDU processing
                }
            }
        }

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

        // Create service context with packet source address for COV subscriptions
        let ctx = ServiceContext::new(self.objects.clone(), self.config.device_instance)
            .with_source_address(packet.source);

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
            ApduType::SegmentAck => {
                // SegmentACK received from a client acknowledging our segmented response.
                // Per ASHRAE 135 Clause 5.4.5.2, we would track outstanding segments
                // and send the next window. For a synchronous server simulator we record
                // the metric and acknowledge; the actual segment sending happens inline
                // in dispatch_and_respond (see below).
                self.metrics.record_segment_ack_received();
                debug!(source = %packet.source, "SegmentACK received");
                return Ok(());
            }
            _ => {
                debug!(apdu_type = ?apdu_type, "Unsupported APDU type");
                return Ok(());
            }
        };

        // Send response(s) — may be multiple APDUs if response is segmented
        match response {
            ProcessedResponse::Single(response_apdu, dest) => {
                self.send_response(&response_apdu, dest, &npdu, bvlc, network)
                    .await?;
            }
            ProcessedResponse::Segmented {
                segments,
                dest,
                invoke_id,
                service_choice,
            } => {
                self.send_segmented_response(
                    &segments,
                    dest,
                    invoke_id,
                    service_choice,
                    &npdu,
                    bvlc,
                    network,
                )
                .await?;
            }
            ProcessedResponse::None => {}
        }

        let latency = timer.elapsed_us();
        self.metrics.record_success(latency);

        Ok(())
    }

    /// Process a confirmed request.
    ///
    /// Implements the 3-tier BACnet error response protocol:
    /// - **Error PDU** (0x50): Service-level errors (known service, operation failed)
    /// - **Reject PDU** (0x60): Protocol violations in the request PDU
    /// - **Abort PDU** (0x70): Server-side inability to process (resources, segmentation)
    ///
    /// Supports reassembly of incoming segmented requests via `SegmentAssembler`.
    /// When a segmented request arrives, intermediate segments are acknowledged with
    /// SegmentACK and the service handler is only invoked once the full message is assembled.
    fn process_confirmed_request(
        &self,
        apdu: &[u8],
        ctx: &ServiceContext,
        source: SocketAddr,
    ) -> BacnetResult<ProcessedResponse> {
        // Too short to even contain a valid header — cannot extract invoke_id,
        // so we must silently discard per ASHRAE 135 Clause 6.1.
        if apdu.len() < 3 {
            debug!(
                "Confirmed request too short ({} bytes), discarding",
                apdu.len()
            );
            self.metrics.record_error();
            return Ok(ProcessedResponse::None);
        }

        // Parse confirmed request header per ASHRAE 135, Clause 20.1.2:
        // Byte 0: PDU type (bits 7-4) | segmented (bit 3) | more_follows (bit 2) | SA (bit 1)
        // Byte 1: max-segments-accepted (bits 6-4) | max-APDU-length-accepted (bits 3-0)
        // Byte 2: invoke-id
        // Byte 3: sequence-number (only if segmented)
        // Byte 4: proposed-window-size (only if segmented)
        // Then: service-choice | service-data
        let pdu_type_byte = apdu[0];
        let segmented = (pdu_type_byte & 0x08) != 0;
        let more_follows = (pdu_type_byte & 0x04) != 0;
        let _segmented_response_accepted = (pdu_type_byte & 0x02) != 0;

        let invoke_id = apdu[2];

        // Handle segmented requests — reassemble via SegmentAssembler
        if segmented {
            if apdu.len() < 5 {
                let abort = build_abort_apdu(invoke_id, AbortReason::Other);
                self.metrics.record_error();
                return Ok(ProcessedResponse::Single(abort, source));
            }

            let sequence_number = apdu[3];
            let _proposed_window = apdu[4];

            // Extract service choice from the first segment only
            let (service_choice_opt, segment_data) = if sequence_number == 0 {
                // First segment: service-choice is at byte 5
                if apdu.len() < 6 {
                    let reject =
                        build_reject_apdu(invoke_id, RejectReason::MissingRequiredParameter as u8);
                    self.metrics.record_error();
                    return Ok(ProcessedResponse::Single(reject, source));
                }
                (Some(apdu[5]), apdu[6..].to_vec())
            } else {
                // Subsequent segments: no service-choice header, all bytes are data
                (None, apdu[5..].to_vec())
            };

            // Build a Segment struct for the assembler
            let mut segment = Segment::new(sequence_number, more_follows, invoke_id, segment_data);
            if let Some(sc) = service_choice_opt {
                segment = segment.with_service_choice(sc);
            }

            // Hash the source address for the assembler key
            let source_hash = hash_socket_addr(&source);

            self.metrics.record_segment_received();

            let assembly_result = {
                let mut assembler = self.segment_assembler.lock();
                assembler.process_segment(source_hash, &segment)
            };

            match assembly_result {
                Ok(AssemblyResult::NeedAck(ack_seq)) => {
                    // Intermediate segment — send SegmentACK and wait for more
                    debug!(
                        invoke_id = invoke_id,
                        sequence = ack_seq,
                        "Sending SegmentACK for intermediate segment"
                    );
                    let ack = build_segment_ack_apdu(invoke_id, ack_seq, DEFAULT_WINDOW_SIZE, true);
                    self.metrics.record_segment_ack_sent();
                    return Ok(ProcessedResponse::Single(ack, source));
                }
                Ok(AssemblyResult::Complete) => {
                    // All segments received — assemble and dispatch
                    let (assembled_data, assembled_service_choice) = {
                        let mut assembler = self.segment_assembler.lock();
                        match assembler.get_complete(source_hash, invoke_id) {
                            Some(result) => result,
                            None => {
                                // Should not happen if assembler returned Complete
                                let abort = build_abort_apdu(invoke_id, AbortReason::Other);
                                self.metrics.record_error();
                                return Ok(ProcessedResponse::Single(abort, source));
                            }
                        }
                    };

                    self.metrics.record_segmented_request_reassembled();

                    let service_choice = assembled_service_choice.unwrap_or(0);
                    debug!(
                        invoke_id = invoke_id,
                        service_choice = service_choice,
                        assembled_size = assembled_data.len(),
                        "Segmented request fully reassembled"
                    );

                    return self.dispatch_and_respond(
                        invoke_id,
                        service_choice,
                        &assembled_data,
                        ctx,
                        source,
                    );
                }
                Ok(AssemblyResult::Duplicate) => {
                    debug!(invoke_id = invoke_id, "Duplicate segment ignored");
                    return Ok(ProcessedResponse::None);
                }
                Err(e) => {
                    // Assembly error — abort the transaction
                    warn!(
                        invoke_id = invoke_id,
                        error = %e,
                        "Segment assembly error"
                    );
                    let abort = build_abort_apdu(invoke_id, AbortReason::Other);
                    self.metrics.record_error();
                    return Ok(ProcessedResponse::Single(abort, source));
                }
            }
        }

        // Non-segmented: need at least 4 bytes (pdu_type, max_info, invoke_id, service_choice)
        if apdu.len() < 4 {
            let reject = build_reject_apdu(invoke_id, RejectReason::MissingRequiredParameter as u8);
            self.metrics.record_error();
            return Ok(ProcessedResponse::Single(reject, source));
        }

        let service_choice = apdu[3];
        let service_data = &apdu[4..];

        self.dispatch_and_respond(invoke_id, service_choice, service_data, ctx, source)
    }

    /// Dispatch a confirmed service and build the response APDU.
    ///
    /// Integrates with the server TSM for duplicate detection and chaos testing.
    /// Supports segmented outgoing responses when the ComplexACK exceeds max_apdu_length.
    ///
    /// Flow:
    /// 1. Check TSM for duplicate requests (return cached response if available).
    /// 2. Dispatch to the service handler.
    /// 3. Build response — segmenting if needed.
    /// 4. Cache the response in the TSM.
    /// 5. Apply chaos drop/delay if configured.
    fn dispatch_and_respond(
        &self,
        invoke_id: u8,
        service_choice: u8,
        service_data: &[u8],
        ctx: &ServiceContext,
        source: SocketAddr,
    ) -> BacnetResult<ProcessedResponse> {
        let tsm_key = TransactionKey::new(source, invoke_id);

        // Step 1: TSM duplicate detection
        match self.tsm.begin_transaction(tsm_key, service_choice) {
            Ok(Some(cached_response)) => {
                // Duplicate with cached response — return it directly
                debug!(
                    invoke_id = invoke_id,
                    source = %source,
                    "Duplicate request, returning cached response"
                );
                return Ok(ProcessedResponse::Single(cached_response, source));
            }
            Ok(None) => {
                // New transaction — proceed with handler dispatch
            }
            Err(crate::service::tsm::TsmError::DuplicateInProgress) => {
                // Duplicate while still processing — silently ignore
                debug!(
                    invoke_id = invoke_id,
                    source = %source,
                    "Duplicate request while processing, ignoring"
                );
                return Ok(ProcessedResponse::None);
            }
            Err(crate::service::tsm::TsmError::AtCapacity) => {
                // TSM at capacity — abort
                warn!("TSM at capacity, aborting request");
                let abort = build_abort_apdu(invoke_id, AbortReason::OutOfResources);
                return Ok(ProcessedResponse::Single(abort, source));
            }
        }

        // Step 2: Track specific services for metrics
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

        // Step 3: Dispatch to handler
        let ctx_with_invoke = ctx.clone().with_invoke_id(invoke_id);
        let result =
            self.services
                .dispatch_confirmed(service_choice, service_data, &ctx_with_invoke);

        // Step 4: Build response APDU using the 3-tier protocol
        let response = match result {
            ServiceResult::SimpleAck => {
                let apdu = vec![0x20, invoke_id, service_choice];
                let should_send = self.tsm.complete_transaction(&tsm_key, apdu.clone());
                if !should_send {
                    debug!(invoke_id, "Response intentionally dropped (chaos testing)");
                    return Ok(ProcessedResponse::None);
                }
                ProcessedResponse::Single(apdu, source)
            }
            ServiceResult::ComplexAck(data) => {
                // Non-segmented ComplexACK header is 3 bytes (type, invoke_id, service_choice)
                let unsegmented_len = 3 + data.len();

                if unsegmented_len <= self.config.max_apdu_length as usize {
                    // Fits in a single APDU — unsegmented ComplexACK
                    let mut apdu = vec![0x30, invoke_id, service_choice];
                    apdu.extend_from_slice(&data);

                    let should_send = self.tsm.complete_transaction(&tsm_key, apdu.clone());
                    if !should_send {
                        debug!(invoke_id, "Response intentionally dropped (chaos testing)");
                        return Ok(ProcessedResponse::None);
                    }
                    ProcessedResponse::Single(apdu, source)
                } else if self.segment_transmitter.needs_segmentation(data.len()) {
                    // Needs segmentation — split the service data into segments.
                    // Each segment gets a 5-byte segmented ComplexACK header added at send time.
                    let segments = self.segment_transmitter.segment(&data, invoke_id);
                    let segment_count = segments.len();

                    debug!(
                        invoke_id = invoke_id,
                        total_size = data.len(),
                        segment_count = segment_count,
                        "ComplexACK requires segmentation"
                    );

                    // Cache the full unsegmented response in TSM for potential retransmission
                    let mut full_response = vec![0x30, invoke_id, service_choice];
                    full_response.extend_from_slice(&data);
                    let should_send = self.tsm.complete_transaction(&tsm_key, full_response);
                    if !should_send {
                        debug!(invoke_id, "Response intentionally dropped (chaos testing)");
                        return Ok(ProcessedResponse::None);
                    }

                    self.metrics.record_segmented_response_transmitted();
                    self.metrics.record_segments_sent(segment_count as u64);

                    // Extract raw segment data
                    let segment_data: Vec<Vec<u8>> = segments.into_iter().map(|s| s.data).collect();

                    ProcessedResponse::Segmented {
                        segments: segment_data,
                        dest: source,
                        invoke_id,
                        service_choice,
                    }
                } else {
                    // Data fits in one segment but somehow exceeds unsegmented limit
                    // (edge case — shouldn't normally happen, but handle gracefully)
                    let mut apdu = vec![0x30, invoke_id, service_choice];
                    apdu.extend_from_slice(&data);
                    let should_send = self.tsm.complete_transaction(&tsm_key, apdu.clone());
                    if !should_send {
                        debug!(invoke_id, "Response intentionally dropped (chaos testing)");
                        return Ok(ProcessedResponse::None);
                    }
                    ProcessedResponse::Single(apdu, source)
                }
            }
            ServiceResult::Error {
                error_class,
                error_code,
            } => {
                let apdu = build_error_apdu(invoke_id, service_choice, error_class, error_code);
                let should_send = self.tsm.complete_transaction(&tsm_key, apdu.clone());
                if !should_send {
                    debug!(invoke_id, "Response intentionally dropped (chaos testing)");
                    return Ok(ProcessedResponse::None);
                }
                ProcessedResponse::Single(apdu, source)
            }
            ServiceResult::Reject(reason) => {
                let apdu = build_reject_apdu(invoke_id, reason);
                let should_send = self.tsm.complete_transaction(&tsm_key, apdu.clone());
                if !should_send {
                    debug!(invoke_id, "Response intentionally dropped (chaos testing)");
                    return Ok(ProcessedResponse::None);
                }
                ProcessedResponse::Single(apdu, source)
            }
            ServiceResult::Abort(reason) => {
                let apdu = build_abort_apdu(invoke_id, reason);
                let should_send = self.tsm.complete_transaction(&tsm_key, apdu.clone());
                if !should_send {
                    debug!(invoke_id, "Response intentionally dropped (chaos testing)");
                    return Ok(ProcessedResponse::None);
                }
                ProcessedResponse::Single(apdu, source)
            }
            ServiceResult::NoResponse | ServiceResult::Broadcast(_) => {
                return Ok(ProcessedResponse::None);
            }
        };

        Ok(response)
    }

    /// Process an unconfirmed request.
    fn process_unconfirmed_request(
        &self,
        apdu: &[u8],
        ctx: &ServiceContext,
        source: SocketAddr,
    ) -> BacnetResult<ProcessedResponse> {
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
                Ok(ProcessedResponse::Single(apdu, source))
            }
            ServiceResult::NoResponse => Ok(ProcessedResponse::None),
            _ => Ok(ProcessedResponse::None),
        }
    }

    /// Send a single unsegmented APDU response.
    async fn send_response(
        &self,
        response_apdu: &[u8],
        dest: SocketAddr,
        npdu: &Npdu,
        bvlc: &BvlcMessage,
        network: &NetworkHandle,
    ) -> BacnetResult<()> {
        let response_npdu = if npdu.expects_reply() {
            Npdu::simple(response_apdu.to_vec())
        } else {
            Npdu::no_reply(response_apdu.to_vec())
        };

        let response_bvlc = if bvlc.is_broadcast() {
            BvlcMessage::original_broadcast(response_npdu.encode())
        } else {
            BvlcMessage::original_unicast(response_npdu.encode())
        };

        let response_bytes = response_bvlc.encode();
        self.metrics.record_bytes_sent(response_bytes.len() as u64);

        network.send_to(&response_bytes, dest).await
    }

    /// Send a segmented ComplexACK response as multiple individual APDU packets.
    ///
    /// Per ASHRAE 135, Clause 5.4.5.2, we send segments within the proposed
    /// window size. For simplicity in this simulator, we send all segments
    /// sequentially without waiting for SegmentACK between windows (window=1).
    /// This matches common BACnet/IP behavior where the underlying transport
    /// (UDP) is reliable on a LAN.
    async fn send_segmented_response(
        &self,
        segments: &[Vec<u8>],
        dest: SocketAddr,
        invoke_id: u8,
        service_choice: u8,
        npdu: &Npdu,
        bvlc: &BvlcMessage,
        network: &NetworkHandle,
    ) -> BacnetResult<()> {
        let total = segments.len();

        for (i, segment_data) in segments.iter().enumerate() {
            let more_follows = i < total - 1;
            let sequence_number = i as u8;

            // Build the segmented ComplexACK APDU for this segment
            let mut encoder = ApduEncoder::new();
            encoder.encode_segmented_complex_ack_header(
                invoke_id,
                sequence_number,
                DEFAULT_WINDOW_SIZE,
                more_follows,
                service_choice,
            );
            encoder.put_bytes(segment_data);
            let segment_apdu = encoder.into_bytes();

            // Wrap in NPDU/BVLC and send
            let response_npdu = if npdu.expects_reply() {
                Npdu::simple(segment_apdu)
            } else {
                Npdu::no_reply(segment_apdu)
            };

            let response_bvlc = if bvlc.is_broadcast() {
                BvlcMessage::original_broadcast(response_npdu.encode())
            } else {
                BvlcMessage::original_unicast(response_npdu.encode())
            };

            let response_bytes = response_bvlc.encode();
            self.metrics.record_bytes_sent(response_bytes.len() as u64);

            network.send_to(&response_bytes, dest).await?;

            debug!(
                invoke_id = invoke_id,
                sequence = sequence_number,
                more = more_follows,
                size = segment_data.len(),
                "Sent segment {}/{}",
                i + 1,
                total
            );
        }

        debug!(
            invoke_id = invoke_id,
            total_segments = total,
            "Segmented ComplexACK fully transmitted"
        );

        Ok(())
    }

    /// Get the segment assembler (for testing / cleanup).
    pub fn segment_assembler(&self) -> &Mutex<SegmentAssembler> {
        &self.segment_assembler
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
            source_address: self.source_address,
        }
    }
}

/// Build an error APDU using the standard ApduEncoder.
///
/// Produces a BACnet-Error-PDU per ASHRAE 135, Clause 21.8, with
/// error-class and error-code encoded as ENUMERATED (Application Tag 9).
fn build_error_apdu(
    invoke_id: u8,
    service_choice: u8,
    error_class: ErrorClass,
    error_code: ErrorCode,
) -> Vec<u8> {
    let mut encoder = ApduEncoder::new();
    encoder.encode_error_pdu(
        invoke_id,
        service_choice,
        error_class as u32,
        error_code as u32,
    );
    encoder.into_bytes()
}

/// Build a Reject APDU.
///
/// Produces a BACnet-Reject-PDU per ASHRAE 135, Clause 21.6.
/// Sent when the server detects a protocol violation in the request PDU
/// (invalid tags, missing parameters, unrecognized service, etc.).
fn build_reject_apdu(invoke_id: u8, reject_reason: u8) -> Vec<u8> {
    let mut encoder = ApduEncoder::new();
    encoder.encode_reject_pdu(invoke_id, reject_reason);
    encoder.into_bytes()
}

/// Build an Abort APDU.
///
/// Produces a BACnet-Abort-PDU per ASHRAE 135, Clause 21.7.
/// Sent when the server cannot process the request due to server-side issues
/// (resource exhaustion, segmentation failure, buffer overflow, etc.).
/// The `sent_by_server` bit is always set to true since this is server-generated.
fn build_abort_apdu(invoke_id: u8, abort_reason: AbortReason) -> Vec<u8> {
    let mut encoder = ApduEncoder::new();
    encoder.encode_abort_pdu(invoke_id, abort_reason as u8, true);
    encoder.into_bytes()
}

/// Build a SegmentACK APDU.
///
/// Produces a BACnet-SegmentACK-PDU per ASHRAE 135, Clause 21.5.
/// Sent to acknowledge receipt of segment(s) during a segmented transfer.
fn build_segment_ack_apdu(
    invoke_id: u8,
    sequence_number: u8,
    actual_window_size: u8,
    sent_by_server: bool,
) -> Vec<u8> {
    let mut encoder = ApduEncoder::new();
    encoder.encode_segment_ack_pdu(
        invoke_id,
        sequence_number,
        actual_window_size,
        sent_by_server,
        false, // not a NAK
    );
    encoder.into_bytes()
}

/// Hash a SocketAddr for use as a key in the segment assembler.
///
/// The assembler uses (source_hash, invoke_id) as a key to distinguish
/// concurrent segmented transfers from different clients.
fn hash_socket_addr(addr: &SocketAddr) -> u64 {
    let mut hasher = DefaultHasher::new();
    addr.hash(&mut hasher);
    hasher.finish()
}

/// Invoke ID counter for confirmed COV notifications.
static COV_INVOKE_ID: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);

/// Send a COV notification.
async fn send_cov_notification(
    network: &NetworkHandle,
    notification: CovNotification,
) -> BacnetResult<()> {
    let apdu = if notification.confirmed {
        // Confirmed COV notification — proper ConfirmedRequest APDU
        let invoke_id = COV_INVOKE_ID.fetch_add(1, Ordering::Relaxed);
        notification.encode_confirmed(invoke_id)
    } else {
        // Unconfirmed COV notification
        let mut apdu = vec![0x10, UnconfirmedService::UnconfirmedCovNotification as u8];
        apdu.extend_from_slice(&notification.encode_unconfirmed());
        apdu
    };

    let mut npdu = Npdu::no_reply(apdu);
    if notification.confirmed {
        npdu.control.expecting_reply = true;
    }
    let bvlc = BvlcMessage::original_unicast(npdu.encode());

    network
        .send_to(&bvlc.encode(), notification.destination)
        .await
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

        // Error PDU header
        assert_eq!(apdu[0], 0x50); // Error PDU type
        assert_eq!(apdu[1], 1); // Invoke ID
        assert_eq!(apdu[2], 12); // Service choice (ReadProperty)

        // Error class = Property (2): Enumerated Tag 9, length 1 → 0x91
        assert_eq!(apdu[3], 0x91);
        assert_eq!(apdu[4], 2); // ErrorClass::Property = 2

        // Error code = UnknownProperty (32): Enumerated Tag 9, length 1 → 0x91
        assert_eq!(apdu[5], 0x91);
        assert_eq!(apdu[6], 32); // ErrorCode::UnknownProperty = 32

        assert_eq!(apdu.len(), 7);
    }

    #[test]
    fn test_build_error_apdu_unknown_object() {
        let apdu = build_error_apdu(3, 12, ErrorClass::Object, ErrorCode::UnknownObject);

        assert_eq!(apdu[0], 0x50);
        assert_eq!(apdu[1], 3);
        assert_eq!(apdu[2], 12);

        // ErrorClass::Object = 1
        assert_eq!(apdu[3], 0x91);
        assert_eq!(apdu[4], 1);

        // ErrorCode::UnknownObject = 31
        assert_eq!(apdu[5], 0x91);
        assert_eq!(apdu[6], 31);
    }

    #[test]
    fn test_build_reject_apdu() {
        let apdu = build_reject_apdu(7, RejectReason::UnrecognizedService as u8);

        assert_eq!(apdu.len(), 3);
        assert_eq!(apdu[0], 0x60); // Reject PDU type
        assert_eq!(apdu[1], 7); // Invoke ID
        assert_eq!(apdu[2], 9); // UnrecognizedService = 9
    }

    #[test]
    fn test_build_reject_apdu_missing_param() {
        let apdu = build_reject_apdu(2, RejectReason::MissingRequiredParameter as u8);

        assert_eq!(apdu[0], 0x60);
        assert_eq!(apdu[1], 2);
        assert_eq!(apdu[2], 5); // MissingRequiredParameter = 5
    }

    #[test]
    fn test_build_abort_apdu_segmentation() {
        let apdu = build_abort_apdu(10, AbortReason::SegmentationNotSupported);

        assert_eq!(apdu.len(), 3);
        assert_eq!(apdu[0], 0x71); // Abort PDU type (0x70) + server bit (0x01)
        assert_eq!(apdu[1], 10); // Invoke ID
        assert_eq!(apdu[2], 4); // SegmentationNotSupported = 4
    }

    #[test]
    fn test_build_abort_apdu_buffer_overflow() {
        let apdu = build_abort_apdu(5, AbortReason::BufferOverflow);

        assert_eq!(apdu[0], 0x71); // Server-sent abort
        assert_eq!(apdu[1], 5);
        assert_eq!(apdu[2], 1); // BufferOverflow = 1
    }

    #[test]
    fn test_build_abort_apdu_too_long() {
        let apdu = build_abort_apdu(3, AbortReason::ApduTooLong);

        assert_eq!(apdu[0], 0x71);
        assert_eq!(apdu[1], 3);
        assert_eq!(apdu[2], 11); // ApduTooLong = 11
    }

    #[test]
    fn test_build_abort_apdu_out_of_resources() {
        let apdu = build_abort_apdu(1, AbortReason::OutOfResources);

        assert_eq!(apdu[0], 0x71);
        assert_eq!(apdu[1], 1);
        assert_eq!(apdu[2], 9); // OutOfResources = 9
    }

    #[test]
    fn test_build_abort_other() {
        let apdu = build_abort_apdu(0, AbortReason::Other);

        assert_eq!(apdu[0], 0x71);
        assert_eq!(apdu[1], 0);
        assert_eq!(apdu[2], 0); // Other = 0
    }

    // ===== SegmentACK PDU tests =====

    #[test]
    fn test_build_segment_ack_apdu_server() {
        let apdu = build_segment_ack_apdu(42, 3, 1, true);

        assert_eq!(apdu.len(), 4);
        assert_eq!(apdu[0], 0x41); // SegmentACK (0x40) + server bit (0x01)
        assert_eq!(apdu[1], 42); // Invoke ID
        assert_eq!(apdu[2], 3); // Sequence number
        assert_eq!(apdu[3], 1); // Window size
    }

    #[test]
    fn test_build_segment_ack_apdu_client() {
        let apdu = build_segment_ack_apdu(10, 0, 4, false);

        assert_eq!(apdu.len(), 4);
        assert_eq!(apdu[0], 0x40); // SegmentACK (0x40), no server bit
        assert_eq!(apdu[1], 10);
        assert_eq!(apdu[2], 0);
        assert_eq!(apdu[3], 4);
    }

    // ===== hash_socket_addr tests =====

    #[test]
    fn test_hash_socket_addr_deterministic() {
        let addr1: SocketAddr = "192.168.1.100:47808".parse().unwrap();
        let h1 = hash_socket_addr(&addr1);
        let h2 = hash_socket_addr(&addr1);
        assert_eq!(h1, h2);
    }

    #[test]
    fn test_hash_socket_addr_different_addresses() {
        let addr1: SocketAddr = "192.168.1.100:47808".parse().unwrap();
        let addr2: SocketAddr = "192.168.1.101:47808".parse().unwrap();
        let h1 = hash_socket_addr(&addr1);
        let h2 = hash_socket_addr(&addr2);
        assert_ne!(h1, h2);
    }

    // ===== Segmented ComplexACK header tests =====

    #[test]
    fn test_segmented_complex_ack_header_first_segment() {
        let mut encoder = ApduEncoder::new();
        encoder.encode_segmented_complex_ack_header(
            1,    // invoke_id
            0,    // sequence 0 (first)
            1,    // window_size
            true, // more follows
            12,   // ReadProperty service
        );
        let bytes = encoder.into_bytes();

        assert_eq!(bytes.len(), 5);
        // PDU type byte: ComplexACK (0x30) | segmented (0x08) | more_follows (0x04) = 0x3C
        assert_eq!(bytes[0], 0x3C);
        assert_eq!(bytes[1], 1); // invoke_id
        assert_eq!(bytes[2], 0); // sequence_number
        assert_eq!(bytes[3], 1); // window_size
        assert_eq!(bytes[4], 12); // service_choice
    }

    #[test]
    fn test_segmented_complex_ack_header_last_segment() {
        let mut encoder = ApduEncoder::new();
        encoder.encode_segmented_complex_ack_header(
            5,     // invoke_id
            3,     // sequence 3 (last)
            1,     // window_size
            false, // no more
            14,    // ReadPropertyMultiple
        );
        let bytes = encoder.into_bytes();

        assert_eq!(bytes.len(), 5);
        // PDU type byte: ComplexACK (0x30) | segmented (0x08) = 0x38 (no more-follows)
        assert_eq!(bytes[0], 0x38);
        assert_eq!(bytes[1], 5);
        assert_eq!(bytes[2], 3);
        assert_eq!(bytes[3], 1);
        assert_eq!(bytes[4], 14);
    }

    // ===== Segment transmitter integration tests =====

    #[test]
    fn test_segment_transmitter_sizing() {
        // With max_apdu = 1476 and header overhead = 5, max segment data = 1471
        let max_apdu: u16 = 1476;
        let header_overhead = 5;
        let max_seg = (max_apdu as usize) - header_overhead;
        let transmitter = SegmentTransmitter::new(max_seg);

        // Data that fits in one segment (1471 bytes or less)
        assert!(!transmitter.needs_segmentation(1471));
        assert!(!transmitter.needs_segmentation(1000));

        // Data that needs segmentation
        assert!(transmitter.needs_segmentation(1472));
        assert!(transmitter.needs_segmentation(3000));

        // Verify segment count
        assert_eq!(transmitter.calculate_segment_count(1471), 1);
        assert_eq!(transmitter.calculate_segment_count(1472), 2);
        assert_eq!(transmitter.calculate_segment_count(2942), 2);
        assert_eq!(transmitter.calculate_segment_count(2943), 3);
    }

    #[test]
    fn test_segment_transmitter_round_trip_with_headers() {
        // Simulate the server's segmentation of a large response
        let max_apdu: u16 = 100;
        let header_overhead = 5;
        let max_seg = (max_apdu as usize) - header_overhead; // 95 bytes per segment
        let transmitter = SegmentTransmitter::new(max_seg);

        let invoke_id = 7;
        let service_choice = 14; // ReadPropertyMultiple
        let original_data: Vec<u8> = (0..300).map(|i| (i % 256) as u8).collect();

        let segments = transmitter.segment(&original_data, invoke_id);
        assert!(segments.len() > 1);

        // Build full segmented APDU packets (as the server would)
        let mut packets: Vec<Vec<u8>> = Vec::new();
        let total = segments.len();
        for (i, seg) in segments.iter().enumerate() {
            let more = i < total - 1;
            let mut enc = ApduEncoder::new();
            enc.encode_segmented_complex_ack_header(invoke_id, i as u8, 1, more, service_choice);
            enc.put_bytes(&seg.data);
            packets.push(enc.into_bytes());
        }

        // Verify each packet structure
        for (i, packet) in packets.iter().enumerate() {
            let more = i < total - 1;
            // Byte 0: type flags
            if more {
                assert_eq!(packet[0] & 0x0C, 0x0C); // segmented + more_follows
            } else {
                assert_eq!(packet[0] & 0x0C, 0x08); // segmented, no more_follows
            }
            assert_eq!(packet[1], invoke_id);
            assert_eq!(packet[2], i as u8); // sequence
            assert_eq!(packet[3], 1); // window_size
            assert_eq!(packet[4], service_choice);
        }

        // Reassemble from segments and verify data integrity
        let mut reassembled = Vec::new();
        for packet in &packets {
            reassembled.extend_from_slice(&packet[5..]); // skip 5-byte header
        }
        assert_eq!(reassembled, original_data);
    }

    // ===== Segment assembler integration tests =====

    #[test]
    fn test_segment_assembler_reassembly_flow() {
        let mut assembler = SegmentAssembler::default();
        let source_hash: u64 = 12345;
        let invoke_id = 1;

        // Simulate 3 segments arriving
        let seg1 = Segment::new(0, true, invoke_id, vec![1, 2, 3]).with_service_choice(12);
        let result = assembler.process_segment(source_hash, &seg1).unwrap();
        assert!(matches!(result, AssemblyResult::NeedAck(0)));

        let seg2 = Segment::new(1, true, invoke_id, vec![4, 5, 6]);
        let result = assembler.process_segment(source_hash, &seg2).unwrap();
        assert!(matches!(result, AssemblyResult::NeedAck(1)));

        let seg3 = Segment::new(2, false, invoke_id, vec![7, 8, 9]);
        let result = assembler.process_segment(source_hash, &seg3).unwrap();
        assert!(matches!(result, AssemblyResult::Complete));

        // Retrieve and verify
        let (data, service) = assembler.get_complete(source_hash, invoke_id).unwrap();
        assert_eq!(data, vec![1, 2, 3, 4, 5, 6, 7, 8, 9]);
        assert_eq!(service, Some(12));
    }

    // ===== Metrics tests =====

    #[test]
    fn test_segmentation_metrics() {
        let metrics = ServerMetrics::new();

        metrics.record_segmented_request_reassembled();
        metrics.record_segmented_response_transmitted();
        metrics.record_segments_sent(5);
        metrics.record_segment_received();
        metrics.record_segment_received();
        metrics.record_segment_ack_sent();
        metrics.record_segment_ack_received();

        let snapshot = metrics.snapshot();
        assert_eq!(snapshot.segmented_requests_reassembled, 1);
        assert_eq!(snapshot.segmented_responses_transmitted, 1);
        assert_eq!(snapshot.segments_sent, 5);
        assert_eq!(snapshot.segments_received, 2);
        assert_eq!(snapshot.segment_acks_sent, 1);
        assert_eq!(snapshot.segment_acks_received, 1);
    }

    // ===== SegmentACK encoder edge cases =====

    #[test]
    fn test_segment_ack_with_nak() {
        let mut encoder = ApduEncoder::new();
        encoder.encode_segment_ack_pdu(
            10, 5, 2, true, // server
            true, // NAK
        );
        let bytes = encoder.into_bytes();

        assert_eq!(bytes.len(), 4);
        // 0x40 | 0x02 (NAK) | 0x01 (server) = 0x43
        assert_eq!(bytes[0], 0x43);
        assert_eq!(bytes[1], 10);
        assert_eq!(bytes[2], 5);
        assert_eq!(bytes[3], 2);
    }

    #[test]
    fn test_segment_ack_client_no_nak() {
        let mut encoder = ApduEncoder::new();
        encoder.encode_segment_ack_pdu(
            255, 0, 1, false, // client
            false, // no NAK
        );
        let bytes = encoder.into_bytes();

        assert_eq!(bytes[0], 0x40); // Plain SegmentACK
        assert_eq!(bytes[1], 255); // Max invoke_id
        assert_eq!(bytes[2], 0); // Sequence 0
        assert_eq!(bytes[3], 1); // Window 1
    }

    // ===== BBMD server integration tests =====

    #[test]
    fn test_server_with_bbmd_config() {
        let config = ServerConfig::new(1234);
        let registry = ObjectRegistry::new();
        let server = BACnetServer::new(config, registry).with_bbmd_config(BbmdConfig::enabled());

        assert!(server.bbmd().is_enabled());
    }

    #[test]
    fn test_server_bbmd_disabled_by_default() {
        let config = ServerConfig::new(1234);
        let registry = ObjectRegistry::new();
        let server = BACnetServer::new(config, registry);

        assert!(!server.bbmd().is_enabled());
    }

    #[test]
    fn test_server_bbmd_foreign_device_registration() {
        let config = ServerConfig::new(1234);
        let registry = ObjectRegistry::new();
        let server = BACnetServer::new(config, registry).with_bbmd_config(BbmdConfig::enabled());

        let source = SocketAddrV4::new(Ipv4Addr::new(192, 168, 1, 200), 47808);
        let ttl_data = [0x00, 0x3C]; // TTL = 60 seconds

        let msg = BvlcMessage {
            header: crate::network::bvlc::BvlcHeader::new(BvlcFunction::RegisterForeignDevice, 6),
            npdu: ttl_data.to_vec(),
            original_source: None,
            result_code: None,
        };

        let response = server.bbmd().handle_message(&msg, source);
        assert!(response.is_some());

        // Verify device is registered
        assert!(server.bbmd().fdt().is_registered(&source));
    }

    #[test]
    fn test_server_bbmd_forward_addresses() {
        let config = ServerConfig::new(1234);
        let registry = ObjectRegistry::new();
        let server = BACnetServer::new(config, registry).with_bbmd_config(BbmdConfig::enabled());

        // Add a BDT peer
        let peer = SocketAddrV4::new(Ipv4Addr::new(10, 0, 0, 1), 47808);
        server
            .bbmd()
            .bdt()
            .add(crate::network::bbmd::BdtEntry::new(
                peer,
                Ipv4Addr::new(255, 0, 0, 0),
            ))
            .unwrap();

        // Register a foreign device
        let foreign = SocketAddrV4::new(Ipv4Addr::new(172, 16, 0, 50), 47808);
        server.bbmd().fdt().register(foreign, 120).unwrap();

        let addrs = server.bbmd().get_forward_addresses(None);
        assert_eq!(addrs.len(), 2);
        assert!(addrs.contains(&peer));
        assert!(addrs.contains(&foreign));
    }

    #[test]
    fn test_server_bbmd_forward_excludes_source() {
        let config = ServerConfig::new(1234);
        let registry = ObjectRegistry::new();
        let server = BACnetServer::new(config, registry).with_bbmd_config(BbmdConfig::enabled());

        let peer1 = SocketAddrV4::new(Ipv4Addr::new(10, 0, 0, 1), 47808);
        let peer2 = SocketAddrV4::new(Ipv4Addr::new(10, 0, 0, 2), 47808);
        server
            .bbmd()
            .bdt()
            .add(crate::network::bbmd::BdtEntry::new(
                peer1,
                Ipv4Addr::new(255, 0, 0, 0),
            ))
            .unwrap();
        server
            .bbmd()
            .bdt()
            .add(crate::network::bbmd::BdtEntry::new(
                peer2,
                Ipv4Addr::new(255, 0, 0, 0),
            ))
            .unwrap();

        // Exclude peer1 (the source)
        let addrs = server.bbmd().get_forward_addresses(Some(&peer1));
        assert_eq!(addrs.len(), 1);
        assert!(addrs.contains(&peer2));
    }

    #[test]
    fn test_server_bbmd_bdt_write_and_read() {
        let config = ServerConfig::new(1234);
        let registry = ObjectRegistry::new();
        let server = BACnetServer::new(config, registry).with_bbmd_config(BbmdConfig::enabled());

        // Write BDT with 2 entries (10 bytes each)
        let mut bdt_data = Vec::new();
        // Entry 1: 192.168.1.100:47808, mask 255.255.255.0
        bdt_data.extend_from_slice(&[192, 168, 1, 100]);
        bdt_data.extend_from_slice(&47808u16.to_be_bytes());
        bdt_data.extend_from_slice(&[255, 255, 255, 0]);
        // Entry 2: 10.0.0.1:47808, mask 255.0.0.0
        bdt_data.extend_from_slice(&[10, 0, 0, 1]);
        bdt_data.extend_from_slice(&47808u16.to_be_bytes());
        bdt_data.extend_from_slice(&[255, 0, 0, 0]);

        let write_msg = BvlcMessage {
            header: crate::network::bvlc::BvlcHeader::new(
                BvlcFunction::WriteBroadcastDistributionTable,
                (4 + bdt_data.len()) as u16,
            ),
            npdu: bdt_data,
            original_source: None,
            result_code: None,
        };

        let source = SocketAddrV4::new(Ipv4Addr::new(192, 168, 1, 1), 47808);
        let response = server.bbmd().handle_message(&write_msg, source);
        assert!(response.is_some());
        assert_eq!(server.bbmd().bdt().len(), 2);

        // Read BDT
        let read_msg = BvlcMessage {
            header: crate::network::bvlc::BvlcHeader::new(
                BvlcFunction::ReadBroadcastDistributionTable,
                4,
            ),
            npdu: vec![],
            original_source: None,
            result_code: None,
        };

        let response = server.bbmd().handle_message(&read_msg, source);
        assert!(response.is_some());
        let response = response.unwrap();
        assert_eq!(
            response.header.function,
            BvlcFunction::ReadBroadcastDistributionTableAck
        );
        // 2 entries * 10 bytes
        assert_eq!(response.npdu.len(), 20);
    }

    #[test]
    fn test_server_bbmd_fdt_cleanup() {
        let config = ServerConfig::new(1234);
        let registry = ObjectRegistry::new();
        let server = BACnetServer::new(config, registry).with_bbmd_config(BbmdConfig::enabled());

        // Register with TTL=0 (immediately expired)
        let foreign = SocketAddrV4::new(Ipv4Addr::new(172, 16, 0, 50), 47808);
        server.bbmd().fdt().register(foreign, 0).unwrap();
        assert_eq!(server.bbmd().fdt().len(), 1);

        // Wait a tiny bit to ensure expiry
        std::thread::sleep(std::time::Duration::from_millis(10));

        // Cleanup
        let cleaned = server.bbmd().cleanup();
        assert_eq!(cleaned, 1);
        assert_eq!(server.bbmd().fdt().len(), 0);
    }

    #[test]
    fn test_bbmd_metrics() {
        let metrics = ServerMetrics::new();

        metrics.record_bbmd_forwarded();
        metrics.record_bbmd_forwarded();
        metrics.record_bbmd_forwarded();
        metrics.record_bbmd_foreign_registration();
        metrics.record_bbmd_foreign_registration();

        let snapshot = metrics.snapshot();
        assert_eq!(snapshot.bbmd_forwarded, 3);
        assert_eq!(snapshot.bbmd_foreign_registrations, 2);
    }

    #[test]
    fn test_server_bbmd_disabled_no_response() {
        let config = ServerConfig::new(1234);
        let registry = ObjectRegistry::new();
        let server = BACnetServer::new(config, registry);
        // BBMD is disabled by default

        let source = SocketAddrV4::new(Ipv4Addr::new(192, 168, 1, 200), 47808);
        let msg = BvlcMessage {
            header: crate::network::bvlc::BvlcHeader::new(BvlcFunction::RegisterForeignDevice, 6),
            npdu: vec![0x00, 0x3C],
            original_source: None,
            result_code: None,
        };

        // When disabled, handle_message returns None
        let response = server.bbmd().handle_message(&msg, source);
        assert!(response.is_none());
    }

    #[test]
    fn test_server_bbmd_reject_foreign_devices() {
        let config = ServerConfig::new(1234);
        let registry = ObjectRegistry::new();
        let server = BACnetServer::new(config, registry)
            .with_bbmd_config(BbmdConfig::enabled().with_accept_foreign_devices(false));

        let source = SocketAddrV4::new(Ipv4Addr::new(192, 168, 1, 200), 47808);
        let msg = BvlcMessage {
            header: crate::network::bvlc::BvlcHeader::new(BvlcFunction::RegisterForeignDevice, 6),
            npdu: vec![0x00, 0x3C],
            original_source: None,
            result_code: None,
        };

        let response = server.bbmd().handle_message(&msg, source);
        assert!(response.is_some());
        let response = response.unwrap();
        // Should be NAK
        assert_eq!(
            response.result_code,
            Some(crate::network::bvlc::BvlcResultCode::RegisterForeignDeviceNak)
        );
    }

    #[test]
    fn test_server_bbmd_delete_fdt_entry() {
        let config = ServerConfig::new(1234);
        let registry = ObjectRegistry::new();
        let server = BACnetServer::new(config, registry).with_bbmd_config(BbmdConfig::enabled());

        // Register a foreign device first
        let foreign = SocketAddrV4::new(Ipv4Addr::new(192, 168, 1, 200), 47808);
        server.bbmd().fdt().register(foreign, 60).unwrap();
        assert!(server.bbmd().fdt().is_registered(&foreign));

        // Delete it via BVLC message
        let mut delete_data = Vec::new();
        delete_data.extend_from_slice(&[192, 168, 1, 200]);
        delete_data.extend_from_slice(&47808u16.to_be_bytes());

        let msg = BvlcMessage {
            header: crate::network::bvlc::BvlcHeader::new(
                BvlcFunction::DeleteForeignDeviceTableEntry,
                (4 + delete_data.len()) as u16,
            ),
            npdu: delete_data,
            original_source: None,
            result_code: None,
        };

        let source = SocketAddrV4::new(Ipv4Addr::new(192, 168, 1, 1), 47808);
        let response = server.bbmd().handle_message(&msg, source);
        assert!(response.is_some());
        let response = response.unwrap();
        assert_eq!(
            response.result_code,
            Some(crate::network::bvlc::BvlcResultCode::Success)
        );
        assert!(!server.bbmd().fdt().is_registered(&foreign));
    }

    #[test]
    fn test_segment_assembler_cleanup() {
        let config = ServerConfig::new(1234);
        let registry = ObjectRegistry::new();
        let server = BACnetServer::new(config, registry);

        // Feed a partial segment (incomplete assembly)
        {
            let mut assembler = server.segment_assembler().lock();
            let seg = Segment::new(0, true, 1, vec![1, 2, 3]).with_service_choice(12);
            let _ = assembler.process_segment(12345, &seg);
            assert_eq!(assembler.active_count(), 1);
        }

        // The cleanup won't remove it yet (it's not stale — timeout is 10s default).
        // But we can verify the cleanup interface works.
        {
            let mut assembler = server.segment_assembler().lock();
            let cleaned = assembler.cleanup();
            assert_eq!(cleaned, 0); // Not stale yet
            assert_eq!(assembler.active_count(), 1);
        }
    }
}

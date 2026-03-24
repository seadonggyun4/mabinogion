//! # mabi-knx
//!
//! KNXnet/IP simulator for the industrial protocol simulator.
//!
//! This crate provides a complete KNXnet/IP implementation including:
//!
//! - **Address System**: Individual and Group address handling
//! - **DPT System**: Extensible Datapoint Type encoding/decoding
//! - **cEMI Frames**: Common EMI frame handling
//! - **KNXnet/IP Protocol**: Full protocol stack with tunneling support
//! - **Device Abstraction**: Integration with mabi-core
//!
//! ## Quick Start
//!
//! ```rust,ignore
//! use mabi_knx::{
//!     KnxServer, KnxServerConfig, KnxDevice, KnxDeviceBuilder,
//!     GroupAddress, IndividualAddress, DptId,
//! };
//!
//! // Create a KNX device
//! let device = KnxDeviceBuilder::new("knx-1", "Living Room Controller")
//!     .individual_address(IndividualAddress::new(1, 1, 1))
//!     .group_object("1/0/1".parse()?, "Light Switch", "1.001".parse()?)
//!     .build()?;
//!
//! // Create a server
//! let config = KnxServerConfig::default();
//! let server = KnxServer::new(config).await?;
//! server.start().await?;
//! ```
//!
//! ## Module Overview
//!
//! - [`error`]: Error types and result definitions
//! - [`config`]: Configuration structures for server and devices
//! - [`address`]: Individual and Group address types
//! - [`dpt`]: Datapoint Type system with extensible codec support
//! - [`cemi`]: Common EMI frame handling
//! - [`frame`]: KNXnet/IP frame and service types
//! - [`group`]: Group object table and event system
//! - [`tunnel`]: Tunneling connection management
//! - [`server`]: KNXnet/IP server implementation
//! - [`device`]: KNX device implementing core Device trait
//! - [`factory`]: Device factory for creating KNX devices

// Core modules
pub mod address;
pub mod config;
pub mod error;

// Protocol modules
pub mod cemi;
pub mod dpt;
pub mod frame;

// Device and server modules
pub mod device;
pub mod diagnostics;
pub mod error_tracker;
pub mod factory;
pub mod filter;
pub mod group;
pub mod group_cache;
pub mod heartbeat;
pub mod metrics;
pub mod runtime;
pub mod server;
pub mod tunnel;

// Re-exports for convenience
pub use address::{AddressType, GroupAddress, GroupAddressRange, IndividualAddress};
pub use cemi::{AdditionalInfo, AdditionalInfoType, Apci, CemiFrame, MessageCode, Priority};
pub use config::TunnelBehaviorConfig;
pub use config::{
    GroupObjectConfig, GroupObjectFlagsConfig, KnxDeviceConfig, KnxServerConfig, TunnelConfig,
};
pub use device::{KnxDevice, KnxDeviceBuilder};
pub use diagnostics::{
    DiagnosticConfig, DiagnosticResult, DiagnosticRule, DiagnosticSeverity, KnxDiagnostics,
};
pub use dpt::{decode_dpt9, encode_dpt9, BoxedDptCodec, DptCodec, DptId, DptRegistry, DptValue};
pub use error::{KnxError, KnxResult};
pub use error_tracker::{
    ChannelErrorSummary, ErrorCategory, SendErrorTracker, SendErrorTrackerConfig,
    TrackerStatsSnapshot, TrackingResult,
};
pub use factory::{register_knx_factory, KnxDeviceFactory};
pub use filter::{
    CircuitBreakerState, FilterChain, FilterChainConfig, FilterChainStats,
    FilterChainStatsSnapshot, FilterDirection, FilterResult, FrameEnvelope, PaceFilter,
    PaceFilterConfig, PaceFilterStats, PaceFilterStatsSnapshot, PaceState, QueueFilter,
    QueueFilterConfig, QueueFilterStats, QueueFilterStatsSnapshot, QueuePriority, RetryFilter,
    RetryFilterConfig, RetryFilterStats, RetryFilterStatsSnapshot,
};
pub use frame::{
    DibDeviceInfo, FrameBuilder, Hpai, KnxFrame, KnxNetIpHeader, ServiceFamily, ServiceType,
    SupportedServiceFamilies,
};
pub use group::{GroupEvent, GroupObject, GroupObjectFlags, GroupObjectTable};
pub use group_cache::{
    CacheEntry, CacheEntryInfo, CacheStatsSnapshot, GroupValueCache, GroupValueCacheConfig,
    UpdateSource,
};
pub use heartbeat::{
    HeartbeatAction, HeartbeatSchedule, HeartbeatScheduler, HeartbeatSchedulerConfig,
    HeartbeatStatsSnapshot,
};
pub use metrics::{ConnectionMetricsSnapshot, KnxMetricsCollector, KnxMetricsSnapshot};
pub use runtime::{descriptor, driver};
pub use server::{ConnectionManager, KnxServer, ServerEvent, ServerState};
pub use tunnel::{
    AckMessage,
    AckResult,
    AckValidation,
    AckWaiter,
    AckWaiterStatsSnapshot,
    ConnectRequest,
    ConnectResponse,
    ConnectionStateRequest,
    ConnectionStateResponse,
    ConnectionType,
    DisconnectRequest,
    DisconnectResponse,
    FsmStatsSnapshot,
    ReceivedValidation,
    SequenceStatsSnapshot,
    // Phase 1: Sequence tracking, ACK management, FSM
    SequenceTracker,
    TunnelConnection,
    TunnelErrorReason,
    TunnelFsm,
    TunnelState,
    TunnellingAck,
    TunnellingRequest,
};

/// Canonical configuration surface for architecture-level composition.
pub type Config = KnxServerConfig;
/// Canonical server surface for architecture-level composition.
pub type Server = KnxServer;
/// Canonical device surface for architecture-level composition.
pub type Device = KnxDevice;
/// Canonical factory surface for architecture-level composition.
pub type Factory = KnxDeviceFactory;
/// Canonical stats surface for architecture-level composition.
pub type Stats = KnxMetricsSnapshot;
/// Canonical error surface for architecture-level composition.
pub type Error = KnxError;
/// Canonical result surface for architecture-level composition.
pub type Result<T> = KnxResult<T>;

/// Canonical KNX server builder.
#[derive(Clone, Default)]
pub struct Builder {
    config: Config,
    group_objects: Option<std::sync::Arc<GroupObjectTable>>,
}

impl Builder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn config(mut self, config: Config) -> Self {
        self.config = config;
        self
    }

    pub fn group_objects(mut self, table: std::sync::Arc<GroupObjectTable>) -> Self {
        self.group_objects = Some(table);
        self
    }

    pub fn build(self) -> Server {
        let server = Server::new(self.config);
        if let Some(table) = self.group_objects {
            server.with_group_objects(table)
        } else {
            server
        }
    }
}

/// Protocol version for KNXnet/IP
pub const KNXNETIP_VERSION: u8 = 0x10;

/// Default KNXnet/IP port
pub const DEFAULT_PORT: u16 = 3671;

/// Default multicast address for KNX discovery
pub const DEFAULT_MULTICAST_ADDR: &str = "224.0.23.12";

/// Prelude module for convenient imports
pub mod prelude {
    pub use crate::{
        DptCodec,
        // DPT
        DptId,
        DptRegistry,
        DptValue,
        // Addresses
        GroupAddress,
        GroupEvent,
        // Group objects
        GroupObject,
        GroupObjectConfig,
        GroupObjectTable,
        IndividualAddress,
        // Device and server
        KnxDevice,
        KnxDeviceBuilder,
        // Configuration
        KnxDeviceConfig,
        // Error handling
        KnxError,
        KnxResult,
        KnxServer,
        KnxServerConfig,
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cemi::MessageCode;

    #[test]
    fn test_group_address_parsing() {
        let addr: GroupAddress = "1/2/3".parse().unwrap();
        assert_eq!(addr.main(), 1);
        assert_eq!(addr.middle(), 2);
        assert_eq!(addr.sub(), 3);
        assert_eq!(addr.to_string(), "1/2/3");
    }

    #[test]
    fn test_individual_address_parsing() {
        let addr: IndividualAddress = "1.2.3".parse().unwrap();
        assert_eq!(addr.area(), 1);
        assert_eq!(addr.line(), 2);
        assert_eq!(addr.device(), 3);
        assert_eq!(addr.to_string(), "1.2.3");
    }

    #[test]
    fn test_dpt_id_parsing() {
        let dpt: DptId = "1.001".parse().unwrap();
        assert_eq!(dpt.main, 1);
        assert_eq!(dpt.sub, 1);
        // Display format is "DPT X.XXX"
        assert!(dpt.to_string().contains("1.001"));
    }

    #[test]
    fn test_dpt_registry_lookup() {
        let registry = DptRegistry::new();

        // Standard DPTs should be registered
        assert!(registry.get(&DptId::new(1, 1)).is_some());
        assert!(registry.get(&DptId::new(5, 1)).is_some());
        assert!(registry.get(&DptId::new(9, 1)).is_some());
    }

    #[test]
    fn test_service_type_round_trip() {
        let service = ServiceType::TunnellingRequest;
        let code: u16 = service.into();
        let parsed = ServiceType::try_from(code).unwrap();
        assert_eq!(parsed, service);
    }

    #[test]
    fn test_cemi_frame_creation() {
        let frame = CemiFrame::group_value_write(
            IndividualAddress::new(1, 1, 1),
            GroupAddress::three_level(1, 0, 1),
            vec![0x00], // Value 0
        );

        assert_eq!(frame.message_code, MessageCode::LDataInd);
    }

    #[test]
    fn test_group_address_range() {
        let range = GroupAddressRange::new(
            GroupAddress::three_level(1, 0, 0),
            GroupAddress::three_level(1, 0, 255),
        );

        assert!(range.contains(&GroupAddress::three_level(1, 0, 100)));
        assert!(!range.contains(&GroupAddress::three_level(1, 1, 0)));
    }

    #[test]
    fn test_dpt_value_encoding() {
        // Test boolean encoding
        let registry = DptRegistry::new();
        let codec = registry.get(&DptId::new(1, 1)).unwrap();

        let encoded = codec.encode(&DptValue::Bool(true)).unwrap();
        assert_eq!(encoded, vec![0x01]);

        let decoded = codec.decode(&[0x01]).unwrap();
        assert_eq!(decoded, DptValue::Bool(true));
    }

    #[test]
    fn test_knx_device_builder() {
        let device = KnxDeviceBuilder::new("test-device", "Test Device")
            .individual_address(IndividualAddress::new(1, 1, 1))
            .description("A test device")
            .build()
            .unwrap();

        assert_eq!(device.id(), "test-device");
        assert_eq!(device.individual_address().to_string(), "1.1.1");
    }

    #[test]
    fn test_factory_protocol() {
        use mabi_core::{DeviceFactory, Protocol};

        let factory = KnxDeviceFactory::new();
        assert_eq!(factory.protocol(), Protocol::KnxIp);
    }
}

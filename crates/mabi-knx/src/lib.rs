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
pub mod error;
pub mod config;
pub mod address;

// Protocol modules
pub mod dpt;
pub mod cemi;
pub mod frame;

// Device and server modules
pub mod group;
pub mod tunnel;
pub mod server;
pub mod device;
pub mod factory;

// Re-exports for convenience
pub use error::{KnxError, KnxResult};
pub use config::{
    KnxServerConfig, KnxDeviceConfig, GroupObjectConfig,
    GroupObjectFlagsConfig, TunnelConfig,
};
pub use address::{GroupAddress, IndividualAddress, AddressType, GroupAddressRange};
pub use dpt::{
    DptId, DptValue, DptCodec, DptRegistry, BoxedDptCodec,
    encode_dpt9, decode_dpt9,
};
pub use cemi::{CemiFrame, MessageCode, Priority, Apci};
pub use frame::{KnxFrame, FrameBuilder, KnxNetIpHeader, ServiceType, Hpai, DibDeviceInfo, SupportedServiceFamilies, ServiceFamily};
pub use group::{GroupObject, GroupObjectTable, GroupObjectFlags, GroupEvent};
pub use tunnel::{
    TunnelConnection,
    ConnectRequest, ConnectResponse, ConnectionType,
    TunnellingRequest, TunnellingAck,
    ConnectionStateRequest, ConnectionStateResponse,
    DisconnectRequest, DisconnectResponse,
};
pub use server::{KnxServer, ServerState, ServerEvent, ConnectionManager};
pub use device::{KnxDevice, KnxDeviceBuilder};
pub use factory::{KnxDeviceFactory, register_knx_factory};

/// Protocol version for KNXnet/IP
pub const KNXNETIP_VERSION: u8 = 0x10;

/// Default KNXnet/IP port
pub const DEFAULT_PORT: u16 = 3671;

/// Default multicast address for KNX discovery
pub const DEFAULT_MULTICAST_ADDR: &str = "224.0.23.12";

/// Prelude module for convenient imports
pub mod prelude {
    pub use crate::{
        // Error handling
        KnxError, KnxResult,
        // Addresses
        GroupAddress, IndividualAddress,
        // DPT
        DptId, DptValue, DptCodec, DptRegistry,
        // Device and server
        KnxDevice, KnxDeviceBuilder,
        KnxServer, KnxServerConfig,
        // Group objects
        GroupObject, GroupObjectTable, GroupEvent,
        // Configuration
        KnxDeviceConfig, GroupObjectConfig,
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

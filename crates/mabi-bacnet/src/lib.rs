//! # mabi-bacnet
//!
//! BACnet/IP simulator for the OTSIM protocol simulator.
//!
//! This crate provides:
//! - BACnet/IP server implementation with UDP networking
//! - BACnet object model (AI, AO, BI, BO, AV, BV, MSI, MSO, MSV, Device)
//! - Property services (ReadProperty, WriteProperty, ReadPropertyMultiple, WritePropertyMultiple)
//! - COV subscriptions and notifications
//! - Device discovery (Who-Is/I-Am)
//! - BBMD (BACnet Broadcast Management Device) for cross-subnet communication
//! - APDU segmentation for large message handling
//!
//! ## Architecture
//!
//! The crate follows a layered architecture:
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │                     BACnet Server                           │
//! │       (Server, Device Management, Event Handling)          │
//! └─────────────────────────────────────────────────────────────┘
//!                              │
//!           ┌──────────────────┼──────────────────┐
//!           ▼                  ▼                  ▼
//! ┌─────────────────┐ ┌─────────────────┐ ┌─────────────────┐
//! │  Service Layer  │ │  Object Model   │ │      BBMD       │
//! │ (Handler Reg.)  │ │ (Registry)      │ │ (Cross-subnet)  │
//! └─────────────────┘ └─────────────────┘ └─────────────────┘
//!           │                  │
//!           ▼                  ▼
//! ┌─────────────────┐ ┌─────────────────┐
//! │   APDU Layer    │ │ Property Store  │
//! │ (Segmentation)  │ │ (DashMap-based) │
//! └─────────────────┘ └─────────────────┘
//!           │
//!           ▼
//! ┌─────────────────────────────────────────────────────────────┐
//! │                    Network Layer                            │
//! │             (UDP, BVLC, NPDU handling)                     │
//! └─────────────────────────────────────────────────────────────┘
//! ```
//!
//! ## Quick Start
//!
//! ```rust,ignore
//! use mabi_bacnet::prelude::*;
//!
//! // Create object registry
//! let mut registry = ObjectRegistry::new();
//!
//! // Add analog input object
//! let ai = AnalogInput::new(1, "Zone Temperature");
//! ai.set_value(72.5);
//! registry.register(Arc::new(ai));
//!
//! // Create and run server
//! let config = ServerConfig::new(1234);
//! let server = BACnetServer::new(config, registry);
//!
//! server.run().await?;
//! ```

pub mod apdu;
pub mod config;
pub mod device;
pub mod error;
pub mod network;
pub mod object;
pub mod server;
pub mod service;

// Prelude for common imports
pub mod prelude {
    pub use crate::apdu::encoding::{ApduDecoder, ApduEncoder};
    pub use crate::apdu::types::{ApduType, ConfirmedService, UnconfirmedService};
    pub use crate::config::BacnetServerConfig;
    pub use crate::device::BacnetDevice;
    pub use crate::error::{BacnetError, BacnetResult};
    pub use crate::network::bvlc::{BvlcFunction, BvlcMessage};
    pub use crate::network::npdu::{Npdu, NpduControl, Priority};
    pub use crate::network::udp::{BACnetNetwork, NetworkConfig, NetworkHandle};
    pub use crate::object::property::{BACnetValue, PropertyId, PropertyStore, SegmentationSupport, StatusFlags};
    pub use crate::object::registry::{ObjectRegistry, RegistryError};
    pub use crate::object::standard::{
        AnalogInput, AnalogOutput, AnalogValue, BinaryInput, BinaryOutput, BinaryValue,
        MultiStateInput, MultiStateOutput, MultiStateValue,
    };
    pub use crate::object::traits::{ArcObject, BACnetObject, CovSupport, ObjectBuilder, WritableObject};
    pub use crate::object::types::{ObjectId, ObjectType};
    pub use crate::server::{BACnetServer, ServerConfig, ServerEvent, ServerMetrics};
    pub use crate::service::cov::{CovManager, CovNotification, CovSubscription};
    pub use crate::service::discovery::{DiscoveryService, IAmResponse, WhoIsRequest};
    pub use crate::service::handler::{
        ConfirmedServiceHandler, ServiceContext, ServiceRegistry, ServiceResult,
        UnconfirmedServiceHandler,
    };
    pub use crate::service::property::{PropertyService, ReadPropertyRequest, WritePropertyRequest};
    pub use crate::service::property_multiple::{
        ReadPropertyMultipleRequest, WritePropertyMultipleRequest,
        ReadPropertyMultipleHandler, WritePropertyMultipleHandler,
        PropertyReference, PropertyAccessResult,
    };
    // Network layer extensions
    pub use crate::network::bbmd::{Bbmd, BbmdConfig, BroadcastDistributionTable, ForeignDeviceTable};
    // APDU segmentation (SegmentationSupport already exported from object::property)
    pub use crate::apdu::segmentation::{
        SegmentAssembler, SegmentTransmitter,
    };
}

// Legacy re-exports for backwards compatibility
pub use config::BacnetServerConfig;
pub use device::BacnetDevice;
pub use error::{BacnetError, BacnetResult};

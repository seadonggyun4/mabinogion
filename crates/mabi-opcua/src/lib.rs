//! # trap-sim-opcua
//!
//! OPC UA server simulator for the TRAP protocol simulator.
//!
//! This crate provides a comprehensive OPC UA server simulation capability with:
//!
//! - **Server Configuration**: Flexible server setup with security policies and endpoints
//! - **Address Space Management**: Hierarchical node organization with 100,000+ node support
//! - **Node Types**: Full support for Objects, Variables, Methods, and Type nodes
//! - **Subscriptions**: Data change monitoring with 10,000+ concurrent subscription support
//! - **Historical Access**: Raw and aggregated historical data with configurable retention
//! - **High Performance**: LRU caching, concurrent access, and efficient memory management
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────┐
//! │                    OPC UA Server                         │
//! ├─────────────────────────────────────────────────────────┤
//! │  ┌─────────────┐  ┌──────────────┐  ┌───────────────┐   │
//! │  │   Session   │  │ Subscription │  │    History    │   │
//! │  │   Manager   │  │   Manager    │  │    Store      │   │
//! │  └─────────────┘  └──────────────┘  └───────────────┘   │
//! ├─────────────────────────────────────────────────────────┤
//! │  ┌─────────────────────────────────────────────────────┐│
//! │  │              Address Space (Nodes)                   ││
//! │  │  ┌─────────┐ ┌──────────┐ ┌────────┐ ┌───────────┐  ││
//! │  │  │ Objects │ │ Variables│ │ Methods│ │   Types   │  ││
//! │  │  └─────────┘ └──────────┘ └────────┘ └───────────┘  ││
//! │  └─────────────────────────────────────────────────────┘│
//! ├─────────────────────────────────────────────────────────┤
//! │  ┌───────────────┐  ┌──────────────────────────────────┐│
//! │  │  Node Cache   │  │     Variable Factory             ││
//! │  │    (LRU)      │  │  (Analog, Discrete, Batch)       ││
//! │  └───────────────┘  └──────────────────────────────────┘│
//! └─────────────────────────────────────────────────────────┘
//! ```
//!
//! ## Quick Start
//!
//! ```rust,no_run
//! use mabi_opcua::{
//!     OpcUaServerConfig, OpcUaDevice,
//!     types::{NodeId, Variant, DataValue},
//!     nodes::{AddressSpace, AddressSpaceConfig, VariableBuilder, NodeBuilder},
//!     services::{SubscriptionManager, SubscriptionManagerConfig, HistoryStore, HistoryStoreConfig},
//! };
//!
//! // Create an OPC UA device
//! let mut device = OpcUaDevice::new("opc-server-1", "My OPC UA Server");
//!
//! // Create address space with nodes
//! let address_space = AddressSpace::new(AddressSpaceConfig::default());
//!
//! // Add a variable node using the address space API
//! address_space.add_variable(
//!     NodeId::numeric(2, 1001),
//!     "Temperature",
//!     "Temperature",
//!     NodeId::numeric(0, 11), // Double data type
//!     Variant::Double(25.0),
//!     &NodeId::numeric(0, 85), // Objects folder
//! ).unwrap();
//!
//! // Create subscription manager for data changes
//! let subscriptions = SubscriptionManager::new();
//!
//! // Create history store for historical access
//! let history = HistoryStore::new(HistoryStoreConfig::default());
//! ```
//!
//! ## Module Organization
//!
//! - [`types`]: Core OPC UA types (NodeId, Variant, DataValue, etc.)
//! - [`nodes`]: Node classes and address space management
//! - [`services`]: Session, subscription, and history services
//! - [`security`]: Security policies, certificates, encryption, and authentication
//! - [`config`]: Server configuration
//! - [`device`]: Device trait implementation
//! - [`factory`]: Device factory for creating OPC UA devices

pub mod channel;
pub mod codec;
pub mod config;
pub mod device;
pub mod error;
pub mod factory;
pub mod nodes;
pub mod runtime;
pub mod security;
pub mod server;
pub mod service;
pub mod services;
pub mod transport;
pub mod types;

// Re-exports for convenience
pub use config::{
    EndpointConfig, MessageSecurityMode, OpcUaServerConfig, SecurityPolicy, UserTokenConfig,
};
pub use device::OpcUaDevice;
pub use error::{OpcUaError, OpcUaResult};
pub use factory::{OpcUaDeviceBuilder, OpcUaDeviceFactory};
pub use server::{OpcUaServer, OpcUaServerBuilder, ServerEvent, ServerState, ServerStats};

// Type re-exports
pub use types::{AccessLevel, AttributeId, DataTypeId, DataValue, NodeId, StatusCode, Variant};

// Node re-exports
pub use nodes::{
    AddToAddressSpace,
    AddressSpace,
    AddressSpaceConfig,
    AnalogVariable,
    AsyncPrefetchWorker,
    BatchConfig,
    // Batch node creation
    BatchNodeCreator,
    BatchProgress,
    BatchVariableBuilder,
    BrowseDirection,
    BrowseResult,
    CacheStats,
    DiscreteVariable,
    FolderBuilder,
    LocalizedText,
    MethodNode,
    Node,
    NodeBase,
    NodeCache,
    NodeCacheConfig,
    NodeClass,
    // Prefetching
    NodePrefetcher,
    NodeStoreStats,
    ObjectBuilder,
    ObjectNode,
    ObjectTemplate,
    PrefetchConfig,
    PrefetchStats,
    PrefetchingAddressSpace,
    ProgressCallback,
    QualifiedName,
    Reference,
    ReferenceTypeId,
    ValueGeneratorType,
    VariableBuilder,
    VariableFactory,
    VariableNode,
    VariableTemplate,
};

// Service re-exports
pub use services::{
    AggregateType, DataChangeFilter, DataChangeTrigger, DeadbandType, HistoricalDataPoint,
    HistoryStore, HistoryStoreConfig, MonitoredItem, MonitoredItemConfig,
    MonitoredItemNotification, MonitoringMode, Session, SessionInfo, SessionManager,
    SessionManagerConfig, Subscription, SubscriptionConfig, SubscriptionManager,
    SubscriptionManagerConfig,
};

// Security re-exports
pub use runtime::{descriptor, driver};
pub use security::{
    AsymmetricAlgorithm, AuthenticationResult, Certificate, CertificateManager,
    CertificateManagerConfig, CertificateStore, CertificateValidator, CryptoProvider,
    CryptoProviderConfig, DecryptionResult, EncryptionResult, HashAlgorithm, SecurityContext,
    SecurityManager, SecurityManagerConfig, SecurityPolicyConfig, SecurityPolicyProvider,
    SignatureResult, SymmetricAlgorithm, UserAuthConfig, UserAuthenticator, UserCredentials,
    UserToken, ValidationResult,
};

/// Canonical configuration surface for architecture-level composition.
pub type Config = OpcUaServerConfig;
/// Canonical builder surface for architecture-level composition.
pub type Builder = OpcUaServerBuilder;
/// Canonical server surface for architecture-level composition.
pub type Server = OpcUaServer;
/// Canonical device surface for architecture-level composition.
pub type Device = OpcUaDevice;
/// Canonical factory surface for architecture-level composition.
pub type Factory = OpcUaDeviceFactory;
/// Canonical stats surface for architecture-level composition.
pub type Stats = ServerStats;
/// Canonical error surface for architecture-level composition.
pub type Error = OpcUaError;
/// Canonical result surface for architecture-level composition.
pub type Result<T> = OpcUaResult<T>;

/// Crate version.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// OPC UA specification version supported.
pub const OPCUA_SPEC_VERSION: &str = "1.04";

/// Maximum recommended nodes for optimal performance.
pub const MAX_RECOMMENDED_NODES: usize = 100_000;

/// Maximum recommended subscriptions for optimal performance.
pub const MAX_RECOMMENDED_SUBSCRIPTIONS: usize = 10_000;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_version() {
        assert!(!VERSION.is_empty());
    }

    #[test]
    fn test_spec_version() {
        assert_eq!(OPCUA_SPEC_VERSION, "1.04");
    }
}

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

pub mod config;
pub mod device;
pub mod error;
pub mod factory;
pub mod nodes;
pub mod security;
pub mod server;
pub mod services;
pub mod types;

// Re-exports for convenience
pub use config::{
    OpcUaServerConfig, SecurityPolicy, MessageSecurityMode, EndpointConfig, UserTokenConfig,
};
pub use device::OpcUaDevice;
pub use error::{OpcUaError, OpcUaResult};
pub use factory::{OpcUaDeviceFactory, OpcUaDeviceBuilder};
pub use server::{OpcUaServer, OpcUaServerBuilder, ServerState, ServerStats, ServerEvent};

// Type re-exports
pub use types::{
    NodeId, StatusCode, Variant, DataValue, AttributeId, AccessLevel, DataTypeId,
};

// Node re-exports
pub use nodes::{
    Node, NodeClass, NodeBase, QualifiedName, LocalizedText,
    ObjectNode, VariableNode, MethodNode,
    AddressSpace, AddressSpaceConfig, NodeStoreStats,
    VariableBuilder, ObjectBuilder, FolderBuilder, BatchVariableBuilder, AddToAddressSpace,
    NodeCache, NodeCacheConfig, CacheStats,
    Reference, ReferenceTypeId, BrowseDirection, BrowseResult,
    VariableFactory, AnalogVariable, DiscreteVariable,
    // Batch node creation
    BatchNodeCreator, BatchConfig, BatchProgress, ProgressCallback,
    VariableTemplate, ObjectTemplate, ValueGeneratorType,
    // Prefetching
    NodePrefetcher, PrefetchConfig, PrefetchStats,
    AsyncPrefetchWorker, PrefetchingAddressSpace,
};

// Service re-exports
pub use services::{
    SessionManager, SessionManagerConfig, Session, SessionInfo,
    SubscriptionManager, SubscriptionManagerConfig, Subscription, SubscriptionConfig,
    MonitoredItem, MonitoredItemConfig, MonitoredItemNotification, MonitoringMode,
    DataChangeFilter, DataChangeTrigger, DeadbandType,
    HistoryStore, HistoryStoreConfig, HistoricalDataPoint, AggregateType,
};

// Security re-exports
pub use security::{
    SecurityManager, SecurityManagerConfig, SecurityContext,
    CertificateManager, CertificateManagerConfig, Certificate, CertificateStore,
    CertificateValidator, ValidationResult,
    CryptoProvider, CryptoProviderConfig, SymmetricAlgorithm, AsymmetricAlgorithm,
    HashAlgorithm, EncryptionResult, DecryptionResult, SignatureResult,
    UserAuthenticator, UserAuthConfig, AuthenticationResult, UserCredentials, UserToken,
    SecurityPolicyConfig, SecurityPolicyProvider,
};

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

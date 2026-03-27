//! # trap-sim-opcua
//!
//! OPC UA server simulator for the TRAP protocol simulator.
//!
//! This crate provides a comprehensive OPC UA server simulation capability with:
//!
//! - **Canonical Modeling Surface**: `OpcUaSimulatorConfig` and named session compilation
//! - **Server Configuration**: Flexible server setup with security policies and endpoints
//! - **Address Space Management**: Hierarchical node organization with 100,000+ node support
//! - **Node Types**: Full support for Objects, Variables, Methods, and Type nodes
//! - **Subscriptions**: Data change monitoring with 10,000+ concurrent subscription support
//! - **Historical Access**: Raw and aggregated historical data with configurable retention
//! - **High Performance**: LRU caching, concurrent access, and efficient memory management
//!
//! ## Canonical Surface
//!
//! The preferred architecture-facing surface is:
//!
//! - [`OpcUaSimulatorConfig`]
//! - [`compile_session`]
//! - [`GeneratedNodeCatalog`]
//! - [`CompiledOpcUaSession`]
//! - [`OpcUaControlSession`]
//!
//! Builder-oriented node creation and legacy numeric serve flows are still supported,
//! but they are compatibility veneer. Internally, runtime entry always converges on a
//! compiled named session.
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
//!     HistoryStore, HistoryStoreConfig, SubscriptionManager, SubscriptionManagerConfig,
//!     nodes::{AddressSpace, AddressSpaceConfig, VariableBuilder, NodeBuilder},
//!     types::{NodeId, Variant, DataValue},
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
//! - Root re-exports: Session, subscription, history, and event runtime types
//! - [`security`]: Security policies, certificates, encryption, and authentication
//! - [`config`]: Server configuration
//! - [`device`]: Device trait implementation
//! - [`factory`]: Device factory for creating OPC UA devices

pub mod channel;
pub mod codec;
pub mod config;
pub mod control;
mod core;
pub mod device;
pub mod error;
pub mod factory;
pub mod modeling;
pub mod nodes;
pub mod runtime;
mod sdk;
pub mod security;
pub mod server;
mod server_runtime;
pub mod transport;
pub mod types;

// Re-exports for convenience
pub use config::{
    EndpointConfig, MessageSecurityMode, OpcUaServerConfig, SecurityPolicy, UserTokenConfig,
};
pub use control::{
    NodeCatalogPort, NodeDescriptor, NodeTarget, NodeValueControlPort, OpcUaControlSession,
    SessionControlPort, SessionSnapshot, SessionStatus,
};
pub use device::OpcUaDevice;
pub use error::{OpcUaError, OpcUaResult};
pub use factory::{OpcUaDeviceBuilder, OpcUaDeviceFactory};
pub use modeling::{
    compile_session, inspect_summary, load_simulator_config, schema_summary, CompanionModelRef,
    CompiledDeviceDefinition, CompiledOpcUaSession, CompiledPointBinding, DeviceDefinition,
    GeneratedNodeCatalog, ModelDefinition, NamespaceCompilationPlan, NodeSetSource,
    OpcUaConfigSummary, OpcUaSchemaSummary, OpcUaSessionSummary, OpcUaSimulatorConfig,
    PresetDefinition, SessionControlConfig, SessionDefinition,
};
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

// Service/runtime re-exports
pub use sdk::event::{
    ContentFilterElement, EventData, EventFieldList, EventFilter, EventManager, EventNotification,
    FilterOperand, FilterOperator, SimpleAttributeOperand,
};
pub use sdk::history::{AggregateType, HistoricalDataPoint, HistoryStore, HistoryStoreConfig};
pub use sdk::session::{Session, SessionInfo, SessionManager, SessionManagerConfig, UserIdentity};
pub use sdk::subscription::{
    DataChangeFilter, DataChangeTrigger, DeadbandType, MonitoredItem, MonitoredItemConfig,
    MonitoredItemNotification, MonitoringMode, Subscription, SubscriptionConfig,
    SubscriptionManager, SubscriptionManagerConfig,
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

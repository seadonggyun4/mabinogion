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
//! Builder-oriented node creation and legacy numeric serve flows have been retired from the
//! public surface. Internally, runtime entry converges on compiled named sessions and the
//! canonical config/session/control path is now the only supported architecture-facing API.
//! Migration documentation remains available through the current release line and the remaining
//! legacy migration breadcrumbs are scheduled for removal in the next major release.
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
//! use std::path::Path;
//!
//! use mabi_opcua::{
//!     compile_session, AddressSpace, AddressSpaceConfig, OpcUaSimulatorConfig, Variant,
//! };
//!
//! let config = OpcUaSimulatorConfig::from_path(Path::new("simulator/opcua.yaml"))?;
//! let compiled = compile_session(&config, "default", Some(Path::new(".")))?;
//!
//! // Canonical runtime path materializes a generated node catalog into the address space.
//! let address_space = AddressSpace::new(AddressSpaceConfig::default());
//! compiled
//!     .catalog
//!     .materialize(&address_space)
//!     .expect("catalog materialization");
//!
//! let namespace_summary = compiled.catalog.namespace_summary();
//! assert!(!namespace_summary.is_empty());
//! let _ = Variant::Null;
//! # Ok::<(), mabi_opcua::OpcUaError>(())
//! ```
//!
//! ```rust,compile_fail
//! use mabi_opcua::compat::VariableBuilder;
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
//! - `compat-migration.md`: migration mapping for removed builder/factory flows

pub mod channel;
pub mod codec;
pub mod config;
pub mod control;
mod core;
pub mod device;
pub mod error;
pub mod modeling;
#[cfg(feature = "experimental-namespace-api")]
pub mod namespace;
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
    EndpointConfig, MessageSecurityMode, OpcUaServerConfig, SecurityPolicy,
    TransportConnectionMode, TransportProtocol, UserTokenConfig,
};
pub use control::{
    NodeCatalogPort, NodeDescriptor, NodeTarget, NodeValueControlPort, OpcUaControlSession,
    SecurityControlPort, SecurityControlStatus, SessionControlPort, SessionSnapshot, SessionStatus,
};
pub use device::OpcUaDevice;
pub use error::{OpcUaError, OpcUaResult};
pub use modeling::{
    compile_session, compile_session_with_report, generate_types, generate_types_with_report,
    inspect_summary, load_simulator_config, schema_summary, CompanionModelRef,
    CompanionPackDefinition, CompilationCacheReport, CompiledDeviceDefinition,
    CompiledOpcUaSession, CompiledPointBinding, CompiledSecurityProfile, DeviceDefinition,
    GeneratedNodeCatalog, GeneratedRustModule, GeneratedTypeCatalog, GeneratedTypesConfig,
    ModelDefinition, NamespaceCompilationPlan, NodeSetSource, OpcUaConfigSummary,
    OpcUaSchemaSummary, OpcUaSessionSummary, OpcUaSimulatorConfig, PresetDefinition,
    SecurityProfileDefinition, SessionControlConfig, SessionDefinition, SessionRuntimeConfig,
};
pub use server::{OpcUaServer, OpcUaServerBuilder, ServerEvent, ServerState, ServerStats};

// Type re-exports
pub use types::{AccessLevel, AttributeId, DataTypeId, DataValue, NodeId, StatusCode, Variant};

// Node re-exports
pub use nodes::{
    AddressSpace,
    AddressSpaceConfig,
    AnalogVariable,
    AsyncPrefetchWorker,
    BatchConfig,
    // Batch node creation
    BatchNodeCreator,
    BatchProgress,
    BrowseDirection,
    BrowseResult,
    CacheStats,
    DiscreteVariable,
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
    ObjectNode,
    ObjectTemplate,
    PrefetchConfig,
    PrefetchStats,
    ProgressCallback,
    QualifiedName,
    Reference,
    ReferenceTypeId,
    ValueGeneratorType,
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
    SubscriptionDurabilityConfig, SubscriptionDurabilityMode, SubscriptionManager,
    SubscriptionManagerConfig,
};

// Security re-exports
pub use runtime::{descriptor, driver};
pub use security::{
    AsymmetricAlgorithm, AuthenticationResult, Certificate, CertificateManager,
    CertificateManagerConfig, CertificateStore, CertificateValidator, CryptoProvider,
    CryptoProviderConfig, DecryptionResult, DeprecatedPolicyHandling, EncryptionResult,
    HashAlgorithm, RoleMappingRule, SecurityAuditSinkConfig, SecurityAuditSinkKind,
    SecurityContext, SecurityManager, SecurityManagerConfig, SecurityPolicyConfig,
    SecurityPolicyProvider, SignatureResult, SymmetricAlgorithm, UserAuthConfig, UserAuthenticator,
    UserCredentials, UserToken, ValidationResult,
};
#[cfg(feature = "experimental-namespace-api")]
pub use namespace::{
    NamespaceDiagnostics, NamespaceManagerPlugin, NamespaceOperation, NamespaceRegistration,
    NamespaceRuntimeSnapshot, NamespaceTypeQuery,
};

/// Canonical configuration surface for architecture-level composition.
pub type Config = OpcUaServerConfig;
/// Canonical builder surface for architecture-level composition.
pub type Builder = OpcUaServerBuilder;
/// Canonical server surface for architecture-level composition.
pub type Server = OpcUaServer;
/// Canonical device surface for architecture-level composition.
pub type Device = OpcUaDevice;
/// Canonical stats surface for architecture-level composition.
pub type Stats = ServerStats;
/// Canonical error surface for architecture-level composition.
pub type Error = OpcUaError;
/// Canonical result surface for architecture-level composition.
pub type Result<T> = OpcUaResult<T>;

/// Crate version.
pub const VERSION: &str = mabi_core::RELEASE_VERSION;

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
        assert_eq!(VERSION, mabi_core::RELEASE_VERSION);
    }

    #[test]
    fn test_spec_version() {
        assert_eq!(OPCUA_SPEC_VERSION, "1.04");
    }
}

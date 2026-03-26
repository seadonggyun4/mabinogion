//! # trap-sim-modbus
//!
//! Modbus TCP/RTU simulator for the TRAP protocol simulator.
//!
//! This crate provides:
//! - A transport-independent Modbus core and datastore context layer
//! - TCP and RTU transport adapters over a shared request execution service
//! - Profile-driven virtual device construction for simulator scenarios
//! - Protocol-aware fault injection, metrics, and runtime integration
//!
//! ## Architecture
//!
//! The crate is organized in layers:
//!
//! ```text
//! ┌──────────────┐    ┌────────────────────┐    ┌────────────────────┐
//! │ tcp / rtu    │───▶│ ModbusService      │───▶│ ServerContext      │
//! │ adapters     │    │ request execution  │    │ DeviceContext      │
//! └──────────────┘    └────────────────────┘    │ AddressSpace       │
//!                                                └────────────────────┘
//!                                                          │
//!                                                ┌────────────────────┐
//!                                                │ dense / sparse     │
//!                                                │ datastore backends │
//!                                                └────────────────────┘
//! ```
//!
//! Canonical construction flows through [`Builder`], [`Profile`], and the
//! transport adapters. Low-level modules remain available for specialized
//! integration, while the root exports provide the stable architecture surface.
//!
//! ## Builder Example
//!
//! ```rust
//! use mabi_core::types::{DataType, ModbusRegisterType};
//! use mabi_modbus::{Builder, Config, PointProfile, Profile, UnitProfile};
//!
//! let profile = Profile::new().with_unit(
//!     UnitProfile::new(1, "Pump-A").with_point(PointProfile::new(
//!         "holding_temp",
//!         "Holding Temperature",
//!         ModbusRegisterType::HoldingRegister,
//!         0,
//!         DataType::UInt16,
//!     )),
//! );
//!
//! let server = Builder::new().config(Config::default()).profile(profile).build()?;
//! assert_eq!(server.device_ids(), vec![1]);
//! # Ok::<(), mabi_modbus::Error>(())
//! ```
//!
//! ## TCP Server Example
//!
//! ```rust,no_run
//! use mabi_modbus::{Builder, Config};
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let server = Builder::new()
//!         .config(Config::default())
//!         .generated_profile(2, 8)
//!         .build()?;
//!     server.run().await?;
//!     Ok(())
//! }
//! ```
//!
//! ## Optional Layers
//!
//! - `faults` is enabled by default for protocol-aware chaos and disruption hooks
//! - `testing` enables load generation and profiling helpers
//! - `experimental-scaling` enables scalability-specific benches and utilities
//! - `performance-tests` enables release-grade performance threshold suites
//!
//! ## Custom Extensions
//!
//! Compatibility-oriented `HandlerRegistry` support remains available, but the
//! canonical extension surface is [`ExtensionRegistry`], which routes custom
//! function codes through the shared semantic core.
//!
//! You can still implement custom function handlers by implementing the
//! `FunctionHandler` trait:
//!
//! ```rust,ignore
//! use mabi_modbus::handler::{FunctionHandler, HandlerContext, ExceptionCode};
//!
//! pub struct MyCustomHandler;
//!
//! impl FunctionHandler for MyCustomHandler {
//!     fn function_code(&self) -> u8 { 0x42 }
//!
//!     fn handle(&self, pdu: &[u8], ctx: &HandlerContext) -> Result<Vec<u8>, ExceptionCode> {
//!         // Custom implementation
//!         Ok(vec![0x42, 0x00])
//!     }
//!
//!     fn name(&self) -> &'static str { "My Custom Handler" }
//! }
//! ```

pub mod config;
mod connection_core;
pub mod context;
pub mod control;
pub mod core;
pub mod device;
pub mod error;
pub mod fault_injection;
pub mod handler;
pub mod profile;
pub mod register;
pub mod registers;
pub mod rtu;
pub mod runtime;
#[cfg(feature = "experimental-scaling")]
pub mod scalability;
pub mod server;
pub mod service;
pub mod simulator;
pub mod tcp;
#[cfg(feature = "testing")]
pub mod testing;
pub mod types;
pub mod unit;

pub use context::{
    AddressSpace, BroadcastPolicy, DenseRegisterStore, DeviceContext, ServerContext,
};
pub use control::{
    FaultPresetPort, ModbusControlSession, PointCatalogPort, PointCatalogQuery, PointDescriptor,
    PointTarget, RegisterControlPort, ResponseProfilePort, SessionControlPort,
    SessionMetadataPort, SessionSnapshot, SessionStatus, TraceEntry, TraceOperation, TracePort,
    TraceStatus,
};
pub use core::{FunctionCode, RequestPdu, ResponsePdu, SemanticRequest, SemanticResponse};
// Re-exports for TCP
pub use config::{ModbusDeviceConfig, ModbusServerConfig};
pub use device::ModbusDevice;
pub use error::{ModbusError, ModbusResult};
pub use profile::{
    DatastoreKind, GeneratedProfilePreset, PointProfile, SimulatorProfile, UnitProfile,
};
pub use register::{RegisterStore, RegisterType};
pub use server::ModbusTcpServer;
pub use simulator::{
    schema_summary, ActionBindingDefinition, ActionBindingSummary, ActionDefinition,
    ActionTrigger, CompiledModbusSession, CompiledPointMetadata, CompiledTransportKind,
    DatastoreAddressRange, DatastoreDefinition, DatastoreInitialization, DatastorePolicySummary,
    DatastoreRepeatPolicy, DatastoreSelector, DatastoreTypedBlock, DeviceBundleDefinition,
    GeneratedPresetDefinition, MalformedResponseDefinition, ModbusConfigSummary,
    ModbusSchemaSummary, ModbusServiceLaunchConfig, ModbusSimulatorConfig, ModbusTransportLaunch,
    PartialResponseDefinition, PointActionBinding, ResponseProfileDefinition, SchemaSection,
    SessionControlConfig, SessionDefinition, SessionResetPolicy, SessionSummary,
    SessionTraceConfig, SimulatorDefaults, SplitResponseDefinition, TransportDefinition,
    UnitDefinition,
};
pub use tcp::ModbusTcpServerV2;
pub use service::{ExtensionContext, ExtensionHandler, ExtensionMetadata, ExtensionRegistry, ExtensionRequest, ServiceOutcome};

// Re-exports for new sparse register store (Task 2.3)
pub use registers::{
    // Config types
    AddressRange,
    // Callback system
    CallbackManager,
    CallbackPriority,
    DefaultValue,
    InitializationMode,
    ReadCallback,
    ReadCallbackFn,
    RegisterRangeConfig,
    RegisterStoreConfig,
    // Value types
    RegisterValue,
    // Store implementation
    SparseRegisterStore,
    WriteCallback,
    WriteCallbackFn,
};

// Re-exports for RTU
pub use rtu::{
    ModbusRtuServer, RtuCodec, RtuFrame, RtuServerConfig, RtuTiming, SerialConfig, VirtualSerial,
    VirtualSerialConfig,
};

// Re-exports for data types and conversion (Task 2.5)
pub use types::{RegisterConverter, RegisterDataType, TypedValue, WordOrder};

// Re-exports for multi-unit management (Task 2.5)
pub use unit::{BroadcastMode, MultiUnitManager, UnitConfig, UnitInfo, UnitManagerConfig};

// Re-exports for testing utilities
#[cfg(feature = "testing")]
pub use testing::{
    AllocationTracker,
    ConnectionSimulator,
    LoadConfig,
    // Load generation
    LoadGenerator,
    LoadPattern,
    // Memory profiling
    MemoryProfiler,
    MemoryReport,
    MemorySnapshot,
    PerformanceConfig,
    PerformanceTarget,
    // Performance validation
    PerformanceValidator,
    TestMetrics,
    // Reporting
    TestReport,
    TestSummary,
    ValidationResult,
};

// Re-exports for fault injection
#[cfg(feature = "faults")]
pub use fault_injection::{
    CrcCorruptionFault, DelayedResponseFault, ExceptionInjectionFault, ExtraDataFault, FaultAction,
    FaultConfig, FaultInjectionConfig, FaultPipeline, FaultStats, FaultStatsSnapshot, FaultTarget,
    FaultType, ModbusFault, ModbusFaultContext, NoResponseFault, PartialFrameFault, TransportKind,
    TruncatedResponseFault, WrongFunctionCodeFault, WrongTransactionIdFault, WrongUnitIdFault,
};

// Re-exports for connection disruption
#[cfg(feature = "faults")]
pub use fault_injection::connection_disruption::{
    ConnectionDisruptionConfig, ConnectionDisruptionState, DisruptionAction,
};

// Re-exports for RTU timing faults
#[cfg(feature = "faults")]
pub use fault_injection::rtu_timing::{
    BusCollisionConfig, ByteJitterConfig, CollisionMode, GapPosition, InterCharGapConfig,
    RtuTimingFaultConfig, TimingPlan, TimingSegment,
};

// Runtime integration entrypoints
pub use runtime::{descriptor, driver};

/// Canonical server configuration surface for architecture-level composition.
pub type Config = tcp::ServerConfigV2;
/// Canonical simulator profile surface for architecture-level composition.
pub type Profile = SimulatorProfile;
/// Canonical device surface for architecture-level composition.
pub type Device = ModbusDevice;
/// Canonical server surface for architecture-level composition.
pub type Server = ModbusTcpServerV2;
/// Canonical error surface for architecture-level composition.
pub type Error = ModbusError;
/// Canonical result surface for architecture-level composition.
pub type Result<T> = ModbusResult<T>;
/// Canonical stats surface for architecture-level composition.
pub type Stats = tcp::ServerMetrics;

/// Canonical server builder.
#[derive(Debug, Clone, Default)]
pub struct Builder {
    config: Config,
    profile: Option<Profile>,
}

impl Builder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn config(mut self, config: Config) -> Self {
        self.config = config;
        self
    }

    pub fn profile(mut self, profile: Profile) -> Self {
        self.profile = Some(profile);
        self
    }

    pub fn generated_profile(mut self, devices: usize, points_per_device: usize) -> Self {
        self.profile = Some(Profile::generated(devices, points_per_device));
        self
    }

    pub fn build(self) -> Result<Server> {
        let server = Server::new(self.config);
        if let Some(profile) = self.profile {
            server.set_broadcast_enabled(profile.broadcast_enabled);
            for unit in profile.units {
                server.add_device(Device::from_profile(&unit)?);
            }
        }
        Ok(server)
    }
}

/// Canonical factory helpers.
#[derive(Debug, Clone, Default)]
pub struct Factory;

impl Factory {
    pub fn server(config: Config) -> Server {
        Server::new(config)
    }

    pub fn device(config: ModbusDeviceConfig) -> Device {
        Device::new(config)
    }
}

#[cfg(test)]
mod architecture_tests {
    use mabi_core::device::Device as _;
    use mabi_core::types::{DataType, ModbusRegisterType};

    use super::{Builder, Config, DatastoreKind, PointProfile, Profile, UnitProfile};

    #[test]
    fn builder_populates_server_from_profile() {
        let profile = Profile::new()
            .with_unit(
                UnitProfile::new(7, "Pump-A")
                    .with_datastore(DatastoreKind::dense_from_counts(16, 16, 16, 16))
                    .with_point(PointProfile::new(
                        "holding_temp",
                        "Holding Temperature",
                        ModbusRegisterType::HoldingRegister,
                        0,
                        DataType::UInt16,
                    ))
                    .with_point(PointProfile::new(
                        "coil_enable",
                        "Enable",
                        ModbusRegisterType::Coil,
                        1,
                        DataType::Bool,
                    )),
            )
            .with_unit(
                UnitProfile::new(9, "Pump-B")
                    .with_datastore(DatastoreKind::dense_from_counts(16, 16, 16, 16)),
            );

        let server = Builder::new()
            .config(Config::default())
            .profile(profile)
            .build()
            .unwrap();
        let mut ids = server.device_ids();
        ids.sort_unstable();

        assert_eq!(ids, vec![7, 9]);
        assert_eq!(server.device(7).unwrap().info().point_count, 2);
        assert_eq!(server.device(7).unwrap().context().name(), "Pump-A");
    }

    #[test]
    fn generated_profile_builder_preserves_legacy_device_counts() {
        let server = Builder::new().generated_profile(2, 8).build().unwrap();
        let mut ids = server.device_ids();
        ids.sort_unstable();

        assert_eq!(ids, vec![1, 2]);
        assert_eq!(server.device(1).unwrap().info().point_count, 8);
        assert_eq!(server.device(2).unwrap().info().point_count, 8);
    }
}

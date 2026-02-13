//! # trap-sim-modbus
//!
//! Modbus TCP/RTU simulator for the TRAP protocol simulator.
//!
//! This crate provides:
//! - Modbus TCP server implementation
//! - Modbus RTU simulation (serial)
//! - Register storage (coils, discrete inputs, holding registers, input registers)
//! - Multiple device simulation
//! - Extensible handler architecture
//! - Connection pooling and metrics
//!
//! ## Architecture
//!
//! ### TCP Server Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │                    ModbusTcpServerV2                         │
//! │  ┌──────────────┐  ┌──────────────┐  ┌──────────────────┐  │
//! │  │ TcpListener  │──│ConnectionPool│──│  HandlerRegistry │  │
//! │  └──────────────┘  └──────────────┘  └──────────────────┘  │
//! │                                              │               │
//! │                        ┌─────────────────────┴───────────┐  │
//! │                        │     FunctionHandler Trait       │  │
//! │                        │  FC01  FC02  FC03  ...  Custom  │  │
//! │                        └─────────────────────────────────┘  │
//! └─────────────────────────────────────────────────────────────┘
//! ```
//!
//! ### RTU Server Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │                     ModbusRtuServer                          │
//! │  ┌──────────────────┐  ┌────────────────┐                   │
//! │  │ TransportManager │──│ HandlerRegistry│                   │
//! │  └────────┬─────────┘  └────────────────┘                   │
//! │           │                                                  │
//! │  ┌────────┴─────────────────────────────────┐               │
//! │  │           Transport Trait                 │               │
//! │  │  VirtualSerial  │  TcpBridge  │  Channel │               │
//! │  └──────────────────────────────────────────┘               │
//! │                           │                                  │
//! │           ┌───────────────┴───────────────────┐             │
//! │           │          RtuCodec                  │             │
//! │           │  Frame Detection │ CRC Validation │             │
//! │           └────────────────────────────────────┘             │
//! └─────────────────────────────────────────────────────────────┘
//! ```
//!
//! ## TCP Server Example
//!
//! ```rust,no_run
//! use mabi_modbus::{ModbusTcpServerV2, tcp::ServerConfigV2};
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let config = ServerConfigV2::default();
//!     let server = ModbusTcpServerV2::new(config);
//!     server.run().await?;
//!     Ok(())
//! }
//! ```
//!
//! ## RTU Server Example
//!
//! ```rust,no_run
//! use mabi_modbus::rtu::{ModbusRtuServer, RtuServerConfig};
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let config = RtuServerConfig::default()
//!         .with_unit_ids(vec![1, 2, 3])
//!         .with_broadcast(true);
//!
//!     let server = ModbusRtuServer::new(config);
//!     server.run().await?;
//!     Ok(())
//! }
//! ```
//!
//! ## Custom Handlers
//!
//! You can implement custom function handlers by implementing the
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
pub mod device;
pub mod error;
pub mod fault_injection;
pub mod handler;
pub mod register;
pub mod registers;
pub mod rtu;
pub mod runtime;
pub mod scalability;
pub mod server;
pub mod tcp;
pub mod testing;
pub mod types;
pub mod unit;

// Re-exports for TCP
pub use config::{ModbusDeviceConfig, ModbusServerConfig};
pub use device::ModbusDevice;
pub use error::{ModbusError, ModbusResult};
pub use register::{RegisterStore, RegisterType};
pub use server::ModbusTcpServer;
pub use tcp::ModbusTcpServerV2;

// Re-exports for new sparse register store (Task 2.3)
pub use registers::{
    // Config types
    AddressRange, DefaultValue, InitializationMode, RegisterRangeConfig, RegisterStoreConfig,
    // Store implementation
    SparseRegisterStore,
    // Callback system
    CallbackManager, CallbackPriority, ReadCallback, ReadCallbackFn, WriteCallback,
    WriteCallbackFn,
    // Value types
    RegisterValue,
};

// Re-exports for RTU
pub use rtu::{
    ModbusRtuServer, RtuCodec, RtuFrame, RtuServerConfig, RtuTiming,
    SerialConfig, VirtualSerial, VirtualSerialConfig,
};

// Re-exports for data types and conversion (Task 2.5)
pub use types::{RegisterConverter, RegisterDataType, TypedValue, WordOrder};

// Re-exports for multi-unit management (Task 2.5)
pub use unit::{
    BroadcastMode, MultiUnitManager, UnitConfig, UnitInfo, UnitManagerConfig,
};

// Re-exports for testing utilities
pub use testing::{
    // Performance validation
    PerformanceValidator, PerformanceConfig, PerformanceTarget, ValidationResult,
    // Memory profiling
    MemoryProfiler, MemorySnapshot, MemoryReport, AllocationTracker,
    // Load generation
    LoadGenerator, LoadConfig, LoadPattern, ConnectionSimulator,
    // Reporting
    TestReport, TestMetrics, TestSummary,
};

// Re-exports for fault injection
pub use fault_injection::{
    FaultAction, FaultPipeline, ModbusFault, ModbusFaultContext, TransportKind,
    FaultInjectionConfig, FaultConfig, FaultType, FaultTarget,
    FaultStats, FaultStatsSnapshot,
    CrcCorruptionFault, WrongUnitIdFault, WrongFunctionCodeFault,
    WrongTransactionIdFault, TruncatedResponseFault, ExtraDataFault,
    DelayedResponseFault, NoResponseFault, ExceptionInjectionFault,
    PartialFrameFault,
};

// Re-exports for connection disruption
pub use fault_injection::connection_disruption::{
    ConnectionDisruptionConfig, ConnectionDisruptionState, DisruptionAction,
};

// Re-exports for RTU timing faults
pub use fault_injection::rtu_timing::{
    RtuTimingFaultConfig, InterCharGapConfig, GapPosition,
    BusCollisionConfig, CollisionMode,
    ByteJitterConfig, TimingPlan, TimingSegment,
};

// Re-exports for runtime configuration
pub use runtime::{
    // Core types
    RuntimeConfigManager, RuntimeState, ConfigUpdate, ConfigEvent, UpdateCategory,
    // Config types
    ConnectionLimits, TimeoutConfig, AccessControl, AddressRange as RuntimeAddressRange,
    FeatureFlags,
    // Update utilities
    UpdateBuilder, UpdateRecord, UpdateAuditLog, StateDiff, FieldChange,
};

//! # mabi-core
//!
//! Shared domain kernel for the Mabinogion protocol resilience engine.
//!
//! This crate provides:
//! - Device trait and abstractions
//! - Data point types and value representations
//! - Engine types for managing virtual devices and protocol sessions
//! - Metrics collection and export
//! - Configuration management with hot reload support
//! - Structured logging and tracing
//! - Common utilities and helper functions
//!
//! ## Quick Start
//!
//! Use the [`prelude`] module for convenient imports:
//!
//! ```rust,ignore
//! use mabi_core::prelude::*;
//!
//! #[tokio::main]
//! async fn main() -> Result<()> {
//!     // Initialize logging
//!     init_logging(&LogConfig::development())?;
//!
//!     // Create and start the engine
//!     let config = EngineConfig::default();
//!     let engine = SimulatorEngine::new(config);
//!     engine.start().await?;
//!
//!     Ok(())
//! }
//! ```
//!
//! ## Module Overview
//!
//! - [`config`]: Configuration types and hot reload support
//! - [`device`]: Device trait and device management
//! - [`engine`]: Simulator engine for orchestrating devices
//! - [`error`]: Error types and result aliases
//! - [`factory`]: Device factory pattern for extensibility
//! - [`logging`]: Structured logging and tracing setup
//! - [`metrics`]: Prometheus metrics collection
//! - [`protocol`]: Protocol definitions for Modbus, OPC UA, BACnet, and KNX
//! - [`types`]: Data point types and addresses
//! - [`utils`]: Utility functions and helper macros
//! - [`value`]: Dynamic value types
//! - [`prelude`]: Convenient re-exports for glob imports
//!
//! ## Example
//!
//! ```rust,no_run
//! use mabi_core::{SimulatorEngine, EngineConfig, Protocol};
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let config = EngineConfig::default();
//!     let engine = SimulatorEngine::new(config);
//!
//!     engine.start().await?;
//!     Ok(())
//! }
//! ```

// =============================================================================
// Modules
// =============================================================================

pub mod capabilities;
pub mod config;
pub mod device;
pub mod device_builder;
pub mod engine;
pub mod error;
pub mod factory;
pub mod lifecycle;
pub mod logging;
pub mod metrics;
pub mod prelude;
pub mod profiling;
pub mod protocol;
pub mod simulation;
pub mod tags;
pub mod typed_point;
pub mod types;
pub mod utils;
pub mod value;
pub mod version;

// =============================================================================
// Re-exports for backwards compatibility and convenience
// =============================================================================

// Error types
pub use error::{
    Error, ErrorSeverity, Result, ResultExt, ValidationErrors, ValidationErrorsBuilder,
};

// Protocol types
pub use protocol::Protocol;

// Value types
pub use value::Value;
pub use version::{release_version, RELEASE_VERSION};

// Configuration types
pub use config::{
    ConfigDiscovery,
    // Watcher
    ConfigEvent,
    // Loader
    ConfigFormat,
    ConfigLoader,
    ConfigSource,
    ConfigWatcher,
    DeviceConfig,
    EngineConfig,
    EnvApplyResult,
    EnvConfigurable,
    // Environment
    EnvOverrides,
    FileWatcherConfig,
    // File Watcher
    FileWatcherService,
    // Hot Reload
    HotReloadManager,
    ReloadEvent,
    ReloadStrategy,
    SharedConfigWatcher,
    // Validation
    Validatable,
    ValidationContext,
    Validator,
    WatcherState,
};

// Device types
pub use device::{Device, DeviceInfo, DeviceState};

// Tags
pub use tags::{parse_tag_string, parse_tags, Taggable, Tags, TagsBuilder};

// Engine types
pub use engine::{EnginePreset, SimulatorEngine, SimulatorEngineBuilder};

// Factory types
pub use factory::{DeviceFactory, FactoryRegistry, Plugin, PluginManager};

// Metrics types
pub use metrics::MetricsCollector;

// Data point types
pub use types::{DataPoint, DataPointId, Quality};

// Logging - expanded exports for new modular structure
pub use logging::{
    // Initialization
    init_logging,
    init_test_logging,
    is_logging_initialized,
    shared_context,
    DebugModeGuard,
    DeviceContext,
    // Configuration
    LogConfig,
    LogConfigBuilder,
    LogFileStats,
    LogFormat,
    LogLevel,
    // Dynamic level control
    LogLevelController,
    LogTarget,
    ModuleTraceGuard,
    RequestContext,
    RetentionPolicy,
    // Rotation
    RotationConfig,
    RotationStrategy,
    SharedTraceContext,
    // Context propagation
    TraceContext,
};

// Capabilities
pub use capabilities::{Capability, CapabilitySet, CapabilitySetBuilder, ProtocolCapabilities};

// Lifecycle
pub use lifecycle::{DeviceLifecycle, LifecycleEvent, LifecycleStateMachine, StopReason};

// Builders
pub use device_builder::{device, point, DataPointBuilder, DeviceConfigBuilder};

// Typed points
pub use typed_point::{
    BoolPoint, DataPointType, Float32Point, Float64Point, FromDefinition, Int32Point, Int64Point,
    StringPoint, TypedDataPoint, TypedPointValue, UInt32Point, UInt64Point,
};

// Profiling
pub use profiling::{
    LeakDetector, LeakDetectorConfig, LeakSeverity, LeakWarning, MemoryProfiler,
    MemoryProfilerConfig, MemoryReport, MemoryReportBuilder, MemorySnapshot, ProfileReport,
    Profiler, ProfilerConfig, RegionAnalysis, RegionSnapshot, ReportComparison, ReportExporter,
    ReportFormat,
};

// Simulation
pub use simulation::{
    BurstPattern, CustomMemoryPattern, FailureConfig, FailureInjector, FailureSchedule,
    FailureType, LeakPattern, LoadPattern, LoadPatternBuilder, MemoryPattern, SawtoothPattern,
    ScaleConfig, Scenario, ScenarioBuilder, ScenarioSuite, ScheduledFailure, SimulationConfig,
    SimulationEvent, SimulationMetrics, SimulationPhase, SimulationResult, Simulator,
    SteppedPattern,
};

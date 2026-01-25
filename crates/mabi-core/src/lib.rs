//! # trap-sim-core
//!
//! Core abstractions and utilities for the TRAP protocol simulator.
//!
//! This crate provides:
//! - Device trait and abstractions
//! - Data point types and value representations
//! - Simulator engine for managing virtual devices
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
//! - [`protocol`]: Protocol definitions (Modbus, OPC UA, BACnet, KNX)
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
pub mod typed_point;
pub mod types;
pub mod utils;
pub mod value;

// =============================================================================
// Re-exports for backwards compatibility and convenience
// =============================================================================

// Error types
pub use error::{Error, ErrorSeverity, Result, ResultExt, ValidationErrors, ValidationErrorsBuilder};

// Protocol types
pub use protocol::Protocol;

// Value types
pub use value::Value;

// Configuration types
pub use config::{
    DeviceConfig, EngineConfig,
    // Loader
    ConfigFormat, ConfigLoader, ConfigDiscovery,
    // Environment
    EnvOverrides, EnvConfigurable, EnvApplyResult,
    // Validation
    Validatable, Validator, ValidationContext,
    // Watcher
    ConfigEvent, ConfigSource, ConfigWatcher, SharedConfigWatcher, WatcherState,
    // File Watcher
    FileWatcherService, FileWatcherConfig,
    // Hot Reload
    HotReloadManager, ReloadStrategy, ReloadEvent,
};

// Device types
pub use device::{Device, DeviceInfo, DeviceState};

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
    init_logging, init_test_logging, is_logging_initialized,
    // Configuration
    LogConfig, LogConfigBuilder, LogLevel, LogFormat, LogTarget,
    // Rotation
    RotationConfig, RotationStrategy, RetentionPolicy, LogFileStats,
    // Dynamic level control
    LogLevelController, DebugModeGuard, ModuleTraceGuard,
    // Context propagation
    TraceContext, RequestContext, DeviceContext, SharedTraceContext, shared_context,
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
    Profiler, ProfilerConfig, ProfileReport,
    MemoryProfiler, MemoryProfilerConfig, MemorySnapshot, MemoryReport, RegionSnapshot,
    LeakDetector, LeakDetectorConfig, LeakWarning, LeakSeverity, RegionAnalysis,
    ReportExporter, ReportFormat, ReportComparison, MemoryReportBuilder,
};

// Simulation
pub use simulation::{
    Simulator, SimulationConfig, SimulationPhase, SimulationEvent, SimulationMetrics, SimulationResult,
    ScaleConfig,
    MemoryPattern, CustomMemoryPattern,
    SawtoothPattern, SteppedPattern, BurstPattern, LeakPattern,
    FailureConfig, FailureType, FailureSchedule, ScheduledFailure, FailureInjector,
    LoadPattern, LoadPatternBuilder,
    Scenario, ScenarioBuilder, ScenarioSuite,
};

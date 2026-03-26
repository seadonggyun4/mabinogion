//! Prelude module for convenient imports.
//!
//! This module re-exports the most commonly used types and traits
//! for convenient glob imports.
//!
//! # Example
//!
//! ```rust,ignore
//! use mabi_core::prelude::*;
//!
//! // Now you have access to common types
//! let device_info = DeviceInfo::new("dev-001", "Test Device", Protocol::ModbusTcp);
//! ```

// =============================================================================
// Error Types
// =============================================================================
pub use crate::error::{
    Error, ErrorSeverity, Result, ResultExt, ValidationErrors, ValidationErrorsBuilder,
};

// =============================================================================
// Protocol Types
// =============================================================================
pub use crate::protocol::Protocol;

// =============================================================================
// Value Types
// =============================================================================
pub use crate::value::{Value, ValueType};

// =============================================================================
// Data Point Types
// =============================================================================
pub use crate::types::{
    AccessMode, Address, BacNetAddress, DataPoint, DataPointDef, DataPointId, DataType,
    ModbusAddress, ModbusRegisterType, Quality,
};

// =============================================================================
// Device Types
// =============================================================================
pub use crate::device::{BoxedDevice, Device, DeviceInfo, DeviceState, DeviceStatistics};

// =============================================================================
// Configuration Types
// =============================================================================
pub use crate::config::{
    BacnetIpConfig, DataPointConfig, DeviceConfig, EngineConfig, KnxIpConfig, ModbusRtuConfig,
    ModbusTcpConfig, OpcUaConfig, ProtocolConfig,
};

// =============================================================================
// Config Watcher Types
// =============================================================================
pub use crate::config::{
    create_config_watcher, ConfigEvent, ConfigEventHandler, ConfigSource, ConfigWatcher,
    SharedConfigWatcher, WatcherState,
};

// =============================================================================
// Engine Types
// =============================================================================
pub use crate::engine::{
    engine, EngineEvent, EnginePreset, EngineState, SimulatorEngine, SimulatorEngineBuilder,
};

// =============================================================================
// Factory Types
// =============================================================================
pub use crate::factory::{
    BoxedFactory, BoxedPlugin, DeviceFactory, FactoryMetadata, FactoryRegistry, Plugin, PluginInfo,
    PluginManager,
};

// =============================================================================
// Metrics Types
// =============================================================================
pub use crate::metrics::{LatencyStats, MetricsCollector, MetricsSnapshot, Timer};

// =============================================================================
// Logging Types
// =============================================================================
pub use crate::logging::{
    // Initialization
    init_logging,
    init_test_logging,
    is_logging_initialized,
    shared_context,
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
    RequestContext,
    RetentionPolicy,
    // Rotation
    RotationConfig,
    RotationStrategy,
    // Context propagation
    TraceContext,
};

// =============================================================================
// Utility Types
// =============================================================================
pub use crate::utils::{
    // Time Utilities
    current_timestamp_ms,
    current_timestamp_ns,
    current_timestamp_us,
    // String Utilities
    format_bytes,
    format_duration,
    // ID Generation
    generate_sequential_id,
    generate_short_uuid,
    generate_timestamp_id,
    generate_uuid,
    // Retry
    retry_async,
    sanitize_identifier,
    truncate_string,
    RateLimiter,
    RetryConfig,
    Stopwatch,
};

// =============================================================================
// Tracing Macros
// =============================================================================
pub use crate::{trace_device, trace_error, trace_request, trace_success};

// =============================================================================
// Builder Macros
// =============================================================================
pub use crate::{builder_setter, builder_setter_into, builder_setter_option};

// =============================================================================
// Capabilities
// =============================================================================
pub use crate::capabilities::{
    default_capabilities, Capability, CapabilitySet, CapabilitySetBuilder, ProtocolCapabilities,
};

// =============================================================================
// Lifecycle
// =============================================================================
pub use crate::lifecycle::{
    DeviceLifecycle, LifecycleEvent, LifecycleHook, LifecycleStateMachine, NoOpLifecycleHook,
    StopReason,
};

// =============================================================================
// Device Builder
// =============================================================================
pub use crate::device_builder::{device, point, DataPointBuilder, DeviceConfigBuilder};

// =============================================================================
// Typed Points
// =============================================================================
pub use crate::typed_point::{
    BoolPoint, DataPointType, Float32Point, Float64Point, FromDefinition, Int32Point, Int64Point,
    NumericPoint, StringPoint, TypedDataPoint, TypedPointValue, UInt32Point, UInt64Point,
};

// =============================================================================
// Re-exports from external crates
// =============================================================================

/// Re-export async_trait for convenience.
pub use async_trait::async_trait;

/// Re-export tracing macros for convenience.
pub use tracing::{debug, error, info, instrument, trace, warn};

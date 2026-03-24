//! Prelude module for convenient imports.
//!
//! This module provides a convenient way to import commonly used types
//! from the chaos engineering module.
//!
//! # Usage
//!
//! ```rust,ignore
//! use mabi_chaos::prelude::*;
//! ```

// =============================================================================
// Error Types
// =============================================================================

pub use crate::error::{ChaosError, ChaosResult};

// =============================================================================
// Core Traits and Types
// =============================================================================

pub use crate::fault::{
    BaseFault, Fault, FaultBehavior, FaultCategory, FaultMetadata, FaultSeverity, FaultState,
    FaultStatistics,
};

pub use crate::context::{
    FaultContext, FaultContextBuilder, OperationType, RequestData, RequestPhase, ResponseData,
    TargetInfo,
};

// =============================================================================
// Registry and Engine
// =============================================================================

pub use crate::registry::{FaultEntry, FaultFilter, FaultRegistry};

pub use crate::engine::{ChaosEngine, ChaosEngineBuilder, EngineEvent, EngineState};
pub use crate::runtime::ChaosRuntime;

// =============================================================================
// Middleware
// =============================================================================

pub use crate::middleware::{ChaosMiddleware, MiddlewareConfig, MiddlewareResult};

// =============================================================================
// Scheduler
// =============================================================================

pub use crate::scheduler::{
    ActiveChaos, ChaosEntry, ChaosEntryBuilder, ChaosEvent, ChaosSchedule, ChaosScheduleBuilder,
    ChaosScheduler, ChaosType, ScheduleState,
};

// =============================================================================
// Configuration
// =============================================================================

pub use crate::config::{
    ChaosConfig, FaultConfig, FaultTypeConfig, GlobalConfig, ScenarioInvocation, ScheduleConfig,
    ScheduleEntryConfig, ScheduleFaultConfig,
};

// =============================================================================
// Network Faults
// =============================================================================

pub use crate::network::{
    // Bandwidth
    BandwidthConfig,
    BandwidthFault,
    BandwidthFaultBuilder,
    // Packet Loss
    BurstConfig,
    // Connection
    ConnectionConfig,
    ConnectionFault,
    ConnectionFaultBuilder,
    DisconnectMode,
    // Latency
    LatencyConfig,
    LatencyDistribution,
    LatencyFaultBuilder,
    NetworkLatencyFault,
    PacketLossConfig,
    PacketLossFault,
    PacketLossFaultBuilder,
};

// =============================================================================
// Device Faults
// =============================================================================

pub use crate::device::{
    // Corrupted Data
    CorruptedDataFault,
    CorruptionConfig,
    CorruptionFaultBuilder,
    CorruptionStrategy,
    // Offline
    DeviceOfflineFault,
    OfflineConfig,
    OfflineFaultBuilder,
    OfflinePattern,
    // Slow Response
    SlowResponseConfig,
    SlowResponseFault,
    SlowResponseFaultBuilder,
    // State Transition
    StateTransitionFault,
    TransitionConfig,
    TransitionFaultBuilder,
};

// =============================================================================
// Protocol Faults
// =============================================================================

pub use crate::protocol::{
    // Checksum
    ChecksumConfig,
    ChecksumFault,
    ChecksumFaultBuilder,
    // Malformed
    MalformationType,
    MalformedConfig,
    MalformedFaultBuilder,
    MalformedPacketFault,
    // Reorder
    ReorderConfig,
    ReorderFault,
    ReorderFaultBuilder,
    // Timeout
    TimeoutConfig,
    TimeoutFault,
    TimeoutFaultBuilder,
    TimeoutPattern,
};

// =============================================================================
// BACnet Faults
// =============================================================================

pub use crate::bacnet::{
    // APDU
    ApduFault,
    ApduFaultBuilder,
    ApduFaultConfig,
    ApduFaultType,
    // COV
    CovFault,
    CovFaultBuilder,
    CovFaultConfig,
    CovFaultType,
    // Property
    PropertyFault,
    PropertyFaultBuilder,
    PropertyFaultConfig,
    PropertyFaultType,
    // Segmentation
    SegmentationFault,
    SegmentationFaultBuilder,
    SegmentationFaultConfig,
    SegmentationFaultType,
    // Service
    ServiceFault,
    ServiceFaultBuilder,
    ServiceFaultConfig,
    ServiceFaultType,
};

// =============================================================================
// Utility Re-exports
// =============================================================================

pub use mabi_core::{DataPoint, Protocol, Value};

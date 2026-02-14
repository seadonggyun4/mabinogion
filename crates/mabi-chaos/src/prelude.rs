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
    Fault, FaultBehavior, FaultCategory, FaultMetadata, FaultSeverity, FaultState, FaultStatistics,
    BaseFault,
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
    ChaosConfig, FaultConfig, FaultTypeConfig, GlobalConfig, ScheduleConfig, ScheduleEntryConfig,
    ScheduleFaultConfig,
};

// =============================================================================
// Network Faults
// =============================================================================

pub use crate::network::{
    // Latency
    LatencyConfig,
    LatencyDistribution,
    NetworkLatencyFault,
    LatencyFaultBuilder,
    // Packet Loss
    BurstConfig,
    PacketLossConfig,
    PacketLossFault,
    PacketLossFaultBuilder,
    // Connection
    ConnectionConfig,
    ConnectionFault,
    ConnectionFaultBuilder,
    DisconnectMode,
    // Bandwidth
    BandwidthConfig,
    BandwidthFault,
    BandwidthFaultBuilder,
};

// =============================================================================
// Device Faults
// =============================================================================

pub use crate::device::{
    // Offline
    DeviceOfflineFault,
    OfflineConfig,
    OfflineFaultBuilder,
    OfflinePattern,
    // Slow Response
    SlowResponseConfig,
    SlowResponseFault,
    SlowResponseFaultBuilder,
    // Corrupted Data
    CorruptedDataFault,
    CorruptionFaultBuilder,
    CorruptionConfig,
    CorruptionStrategy,
    // State Transition
    StateTransitionFault,
    TransitionConfig,
    TransitionFaultBuilder,
};

// =============================================================================
// Protocol Faults
// =============================================================================

pub use crate::protocol::{
    // Malformed
    MalformationType,
    MalformedConfig,
    MalformedPacketFault,
    MalformedFaultBuilder,
    // Checksum
    ChecksumConfig,
    ChecksumFault,
    ChecksumFaultBuilder,
    // Timeout
    TimeoutConfig,
    TimeoutFault,
    TimeoutFaultBuilder,
    TimeoutPattern,
    // Reorder
    ReorderConfig,
    ReorderFault,
    ReorderFaultBuilder,
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
    // Service
    ServiceFault,
    ServiceFaultBuilder,
    ServiceFaultConfig,
    ServiceFaultType,
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
};

// =============================================================================
// Utility Re-exports
// =============================================================================

pub use mabi_core::{DataPoint, Protocol, Value};

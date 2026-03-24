//! # mabi-chaos
//!
//! Chaos engineering module for the OT protocol simulator.
//!
//! This crate provides a **trait-based, extensible chaos engineering framework**
//! for testing the resilience of industrial protocol clients and systems.
//!
//! ## Architecture
//!
//! The chaos module is built around several key abstractions:
//!
//! - **[`Fault`]**: Core trait that all fault types implement
//! - **[`FaultContext`]**: Execution context for fault injection
//! - **[`FaultInjector`]**: Manages fault application to requests/responses
//! - **[`ChaosEngine`]**: Orchestrates multiple faults and schedules
//! - **[`Middleware`]**: Transparent fault injection layer
//!
//! ## Fault Categories
//!
//! ### Network Faults ([`network`])
//! - Latency injection (constant, uniform, normal, log-normal)
//! - Packet loss (random, burst)
//! - Connection disruption (disconnect, timeout, reset)
//! - Bandwidth throttling
//!
//! ### Device Faults ([`device`])
//! - Device offline simulation
//! - Slow response injection
//! - Corrupted data responses
//! - State transition failures
//!
//! ### Protocol Faults ([`protocol`])
//! - Malformed packets
//! - Invalid checksums
//! - Out-of-order responses
//! - Timeout simulation
//!
//! ## Usage
//!
//! ### Basic Usage
//!
//! ```rust,ignore
//! use mabi_chaos::prelude::*;
//!
//! // Create a latency fault
//! let latency = NetworkLatencyFault::builder()
//!     .base_ms(100)
//!     .jitter_ms(50)
//!     .distribution(LatencyDistribution::Normal)
//!     .build();
//!
//! // Create chaos engine
//! let engine = ChaosEngine::builder()
//!     .add_fault("latency", latency)
//!     .build();
//!
//! // Apply to a target
//! engine.enable("latency", "device-001").await;
//! ```
//!
//! ### Scheduled Chaos
//!
//! ```rust,ignore
//! use mabi_chaos::prelude::*;
//!
//! let schedule = ChaosSchedule::builder()
//!     .name("resilience-test")
//!     .add_entry(ChaosEntry::builder()
//!         .start_secs(0.0)
//!         .duration_secs(60.0)
//!         .fault(ChaosType::NetworkLatency { base_ms: 100, jitter_ms: 50 })
//!         .target("modbus-*")
//!         .build())
//!     .build();
//!
//! let scheduler = ChaosScheduler::new(schedule);
//! scheduler.start();
//! ```
//!
//! ### Middleware Pattern
//!
//! ```rust,ignore
//! use mabi_chaos::middleware::ChaosMiddleware;
//!
//! let middleware = ChaosMiddleware::new(engine);
//!
//! // Wrap device operations
//! let result = middleware.wrap_read(device, point_id).await;
//! ```

// =============================================================================
// Module Declarations
// =============================================================================

pub mod context;
pub mod engine;
pub mod error;
pub mod fault;
pub mod middleware;
pub mod registry;
pub mod runtime;
pub mod scheduler;

// Fault implementations
pub mod bacnet;
pub mod device;
pub mod network;
pub mod protocol;

// Configuration and prelude
pub mod config;
pub mod prelude;

// =============================================================================
// Re-exports
// =============================================================================

// Error types
pub use error::{ChaosError, ChaosResult};

// Core abstractions
pub use context::{FaultContext, FaultContextBuilder, RequestPhase, TargetInfo};
pub use engine::{ChaosEngine, ChaosEngineBuilder, EngineEvent, EngineState};
pub use fault::{Fault, FaultBehavior, FaultMetadata, FaultSeverity, FaultState};
pub use middleware::{ChaosMiddleware, MiddlewareConfig, MiddlewareResult};
pub use registry::{FaultEntry, FaultFilter, FaultRegistry};
pub use runtime::ChaosRuntime;
pub use scheduler::{
    ActiveChaos, ChaosEntry, ChaosEvent, ChaosSchedule, ChaosScheduler, ChaosType, ScheduleState,
};

// Network faults
pub use network::{
    BandwidthConfig, BandwidthFault, BurstConfig, ConnectionConfig, ConnectionFault,
    DisconnectMode, LatencyConfig, LatencyDistribution, NetworkLatencyFault, PacketLossConfig,
    PacketLossFault,
};

// Device faults
pub use device::{
    CorruptedDataFault, CorruptionConfig, CorruptionStrategy, DeviceOfflineFault, OfflineConfig,
    OfflinePattern, SlowResponseConfig, SlowResponseFault, StateTransitionFault, TransitionConfig,
};

// Protocol faults
pub use protocol::{
    ChecksumConfig, ChecksumFault, MalformationType, MalformedConfig, MalformedPacketFault,
    ReorderConfig, ReorderFault, TimeoutConfig, TimeoutFault, TimeoutPattern,
};

// BACnet faults
pub use bacnet::{
    ApduFault, ApduFaultConfig, ApduFaultType, CovFault, CovFaultConfig, CovFaultType,
    PropertyFault, PropertyFaultConfig, PropertyFaultType, SegmentationFault,
    SegmentationFaultConfig, SegmentationFaultType, ServiceFault, ServiceFaultConfig,
    ServiceFaultType,
};

// Configuration
pub use config::{ChaosConfig, FaultConfig, GlobalConfig, ScenarioInvocation, ScheduleConfig};

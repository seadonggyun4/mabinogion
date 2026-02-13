//! Modbus-specific fault injection framework.
//!
//! This module provides a production-grade fault injection system designed
//! specifically for Modbus protocol testing. Unlike generic chaos engineering
//! tools, these faults understand Modbus semantics: CRC positions, MBAP headers,
//! function code meanings, and PDU structure.
//!
//! # Architecture
//!
//! ```text
//! ┌────────────────────────────────────────────────────────────────────┐
//! │                        FaultPipeline                               │
//! │  ┌─────────────────────────────────────────────────────────────┐  │
//! │  │  Request PDU + Context                                      │  │
//! │  │       │                                                     │  │
//! │  │       ▼                                                     │  │
//! │  │  ┌──────────┐  ┌──────────────┐  ┌────────────────────┐    │  │
//! │  │  │NoResponse│──│ExceptionInj. │──│ WrongUnitId        │    │  │
//! │  │  │(drop?)   │  │(force exc?)  │  │ WrongFC            │    │  │
//! │  │  └──────────┘  └──────────────┘  │ WrongTID           │    │  │
//! │  │       │              │           │ TruncatedResponse   │    │  │
//! │  │       ▼              ▼           │ ExtraData           │    │  │
//! │  │  Short-circuit  Response PDU     │ CrcCorruption       │    │  │
//! │  │                      │           └────────────────────┘    │  │
//! │  │                      ▼                    │                │  │
//! │  │                 FaultAction                ▼                │  │
//! │  │            (Send/Drop/Delay/Partial/Raw)                   │  │
//! │  └─────────────────────────────────────────────────────────────┘  │
//! └────────────────────────────────────────────────────────────────────┘
//! ```
//!
//! # Usage
//!
//! ```rust,ignore
//! use mabi_modbus::fault_injection::*;
//!
//! // Build a pipeline
//! let pipeline = FaultPipeline::new()
//!     .with_fault(Arc::new(NoResponseFault::new(
//!         FaultTarget::new().with_probability(0.1),
//!     )))
//!     .with_fault(Arc::new(CrcCorruptionFault::new(
//!         CrcCorruptionMode::Invert,
//!         FaultTarget::new().with_unit_ids(vec![1]),
//!     )));
//!
//! // Apply to a response
//! let ctx = ModbusFaultContext::rtu(1, 0x03, &request_pdu, &response_pdu);
//! let action = pipeline.apply(&ctx);
//! ```

pub mod config;
pub mod stats;
pub mod targeting;

// Individual fault implementations
pub mod crc_corruption;
pub mod delayed_response;
pub mod exception_injection;
pub mod extra_data;
pub mod no_response;
pub mod partial_frame;
pub mod truncated_response;
pub mod wrong_function_code;
pub mod wrong_transaction_id;
pub mod wrong_unit_id;

// Connection and timing fault modules
pub mod connection_disruption;
pub mod rtu_timing;

use std::sync::Arc;
use std::time::Duration;

use tracing::{debug, trace};

pub use config::*;
pub use stats::{FaultStats, FaultStatsSnapshot};
pub use targeting::FaultTarget;

// Re-export individual fault types
pub use crc_corruption::CrcCorruptionFault;
pub use delayed_response::DelayedResponseFault;
pub use exception_injection::ExceptionInjectionFault;
pub use extra_data::ExtraDataFault;
pub use no_response::NoResponseFault;
pub use partial_frame::PartialFrameFault;
pub use truncated_response::TruncatedResponseFault;
pub use wrong_function_code::WrongFunctionCodeFault;
pub use wrong_transaction_id::WrongTransactionIdFault;
pub use wrong_unit_id::WrongUnitIdFault;

/// The result of applying a fault to a Modbus response.
///
/// This enum captures all possible mutations the fault pipeline can produce,
/// giving the server integration layer full control over how to handle each case.
#[derive(Debug, Clone)]
pub enum FaultAction {
    /// Send the (possibly modified) response PDU normally.
    /// The server will wrap this in the appropriate frame (MBAP or RTU+CRC).
    SendResponse(Vec<u8>),

    /// Drop the response entirely (no response sent to client).
    /// Tests timeout handling and retry logic.
    DropResponse,

    /// Delay before sending the response.
    /// The delay is applied by the server integration layer.
    DelayThenSend {
        delay: Duration,
        response: Vec<u8>,
    },

    /// Send a partial frame (RTU only).
    /// The server sends exactly these bytes then stops.
    SendPartial {
        bytes: Vec<u8>,
    },

    /// Send raw wire bytes, bypassing CRC calculation.
    /// Used for CRC corruption: the bytes include the corrupted CRC.
    SendRawBytes(Vec<u8>),

    /// Override the transaction ID in the MBAP header (TCP only).
    /// The response PDU is sent normally, but with a different TID.
    OverrideTransactionId {
        transaction_id: u16,
        response: Vec<u8>,
    },
}

/// Transport kind for the current connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransportKind {
    /// Modbus TCP (MBAP framing).
    Tcp,
    /// Modbus RTU (CRC-16, timing-based framing).
    Rtu,
}

/// Context provided to fault implementations for decision making.
///
/// Contains all information about the current request/response pair
/// that a fault might need to decide whether and how to activate.
#[derive(Debug, Clone)]
pub struct ModbusFaultContext {
    /// Transport type (TCP or RTU).
    pub transport: TransportKind,

    /// Unit ID (slave address) from the request.
    pub unit_id: u8,

    /// Function code from the request.
    pub function_code: u8,

    /// The original request PDU (starts with function code).
    pub request_pdu: Vec<u8>,

    /// The normal response PDU (starts with function code).
    /// This is what would be sent without fault injection.
    pub response_pdu: Vec<u8>,

    /// Transaction ID from the MBAP header (TCP only, 0 for RTU).
    pub transaction_id: u16,

    /// Whether this is a broadcast request (unit_id == 0).
    pub is_broadcast: bool,

    /// Sequential request number for this connection.
    pub request_number: u64,
}

impl ModbusFaultContext {
    /// Create a context for an RTU connection.
    pub fn rtu(
        unit_id: u8,
        function_code: u8,
        request_pdu: &[u8],
        response_pdu: &[u8],
        request_number: u64,
    ) -> Self {
        Self {
            transport: TransportKind::Rtu,
            unit_id,
            function_code,
            request_pdu: request_pdu.to_vec(),
            response_pdu: response_pdu.to_vec(),
            transaction_id: 0,
            is_broadcast: unit_id == 0,
            request_number,
        }
    }

    /// Create a context for a TCP connection.
    pub fn tcp(
        unit_id: u8,
        function_code: u8,
        request_pdu: &[u8],
        response_pdu: &[u8],
        transaction_id: u16,
        request_number: u64,
    ) -> Self {
        Self {
            transport: TransportKind::Tcp,
            unit_id,
            function_code,
            request_pdu: request_pdu.to_vec(),
            response_pdu: response_pdu.to_vec(),
            transaction_id,
            is_broadcast: unit_id == 0,
            request_number,
        }
    }
}

/// Trait for Modbus-specific fault implementations.
///
/// Each fault type implements this trait to define its behavior.
/// The pipeline calls faults in registration order.
///
/// # Implementation Guide
///
/// 1. Store a `FaultStats` instance for monitoring
/// 2. Store a `FaultTarget` for activation control
/// 3. In `apply()`, always call `stats.record_check()` first
/// 4. Check `should_activate()` via the target
/// 5. If activated, call `stats.record_activation()` and `stats.record_affected()`
/// 6. Return the appropriate `FaultAction`
pub trait ModbusFault: Send + Sync {
    /// Human-readable name of this fault type.
    fn fault_type(&self) -> &'static str;

    /// Whether this fault is currently enabled.
    fn is_enabled(&self) -> bool;

    /// Enable or disable this fault.
    fn set_enabled(&self, enabled: bool);

    /// Check if this fault should activate for the given context.
    /// This is called by the pipeline before `apply()`.
    fn should_activate(&self, ctx: &ModbusFaultContext) -> bool;

    /// Apply the fault, returning the action to take.
    ///
    /// This is only called if `should_activate()` returned true.
    /// The implementation should record statistics internally.
    fn apply(&self, ctx: &ModbusFaultContext) -> FaultAction;

    /// Get a snapshot of this fault's statistics.
    fn stats(&self) -> FaultStatsSnapshot;

    /// Reset all statistics counters.
    fn reset_stats(&self);

    /// Whether this fault produces short-circuit actions (Drop/Partial).
    /// Short-circuit faults are checked first in the pipeline.
    fn is_short_circuit(&self) -> bool {
        false
    }

    /// Transport compatibility.
    /// Return `None` to be compatible with all transports.
    fn compatible_transport(&self) -> Option<TransportKind> {
        None
    }
}

/// Ordered pipeline of Modbus faults.
///
/// The pipeline applies faults in a specific order:
/// 1. Short-circuit faults (NoResponse, PartialFrame) — if any activates, stop
/// 2. Response-replacing faults (ExceptionInjection) — replaces the entire PDU
/// 3. Response-modifying faults (WrongUnitId, WrongFC, Truncated, ExtraData)
/// 4. Wire-level faults (CrcCorruption) — must be last for RTU
/// 5. Timing faults (DelayedResponse) — wraps the final result
///
/// # Thread Safety
///
/// The pipeline itself is immutable after construction. Individual faults
/// handle their own thread safety via atomic operations.
pub struct FaultPipeline {
    /// All registered faults, in execution order.
    faults: Vec<Arc<dyn ModbusFault>>,
    /// Master enable flag.
    enabled: std::sync::atomic::AtomicBool,
}

impl FaultPipeline {
    /// Create an empty pipeline.
    pub fn new() -> Self {
        Self {
            faults: Vec::new(),
            enabled: std::sync::atomic::AtomicBool::new(true),
        }
    }

    /// Add a fault to the pipeline.
    pub fn with_fault(mut self, fault: Arc<dyn ModbusFault>) -> Self {
        self.faults.push(fault);
        self
    }

    /// Add multiple faults.
    pub fn with_faults(mut self, faults: Vec<Arc<dyn ModbusFault>>) -> Self {
        self.faults.extend(faults);
        self
    }

    /// Check if the pipeline is enabled.
    pub fn is_enabled(&self) -> bool {
        self.enabled.load(std::sync::atomic::Ordering::Acquire)
    }

    /// Enable or disable the entire pipeline.
    pub fn set_enabled(&self, enabled: bool) {
        self.enabled
            .store(enabled, std::sync::atomic::Ordering::Release);
    }

    /// Get the number of registered faults.
    pub fn len(&self) -> usize {
        self.faults.len()
    }

    /// Check if the pipeline has no faults.
    pub fn is_empty(&self) -> bool {
        self.faults.is_empty()
    }

    /// Apply all faults in the pipeline to a response.
    ///
    /// Returns `None` if no faults activated (use the original response).
    /// Returns `Some(FaultAction)` if any fault modified the response.
    ///
    /// Processing order:
    /// 1. Short-circuit faults checked first
    /// 2. Mutation faults applied sequentially to the response bytes
    /// 3. Timing faults wrap the final result
    pub fn apply(&self, ctx: &ModbusFaultContext) -> Option<FaultAction> {
        if !self.is_enabled() {
            return None;
        }

        // Phase 1: Check short-circuit faults (NoResponse, PartialFrame)
        for fault in &self.faults {
            if !fault.is_enabled() {
                continue;
            }
            if !fault.is_short_circuit() {
                continue;
            }
            // Check transport compatibility
            if let Some(required) = fault.compatible_transport() {
                if required != ctx.transport {
                    continue;
                }
            }
            if fault.should_activate(ctx) {
                let action = fault.apply(ctx);
                debug!(
                    fault_type = fault.fault_type(),
                    unit_id = ctx.unit_id,
                    fc = ctx.function_code,
                    "Short-circuit fault activated"
                );
                return Some(action);
            }
        }

        // Phase 2: Collect mutations from non-short-circuit faults
        let mut current_response = ctx.response_pdu.clone();
        let mut any_activated = false;
        let mut total_delay = Duration::ZERO;
        let mut override_tid: Option<u16> = None;
        let mut send_raw = false;

        for fault in &self.faults {
            if !fault.is_enabled() {
                continue;
            }
            if fault.is_short_circuit() {
                continue;
            }
            // Check transport compatibility
            if let Some(required) = fault.compatible_transport() {
                if required != ctx.transport {
                    continue;
                }
            }
            if !fault.should_activate(ctx) {
                continue;
            }

            // Create a sub-context with the current (possibly mutated) response
            let mut sub_ctx = ctx.clone();
            sub_ctx.response_pdu = current_response.clone();

            let action = fault.apply(&sub_ctx);
            any_activated = true;

            trace!(
                fault_type = fault.fault_type(),
                unit_id = ctx.unit_id,
                fc = ctx.function_code,
                "Mutation fault activated"
            );

            match action {
                FaultAction::SendResponse(pdu) => {
                    current_response = pdu;
                }
                FaultAction::DelayThenSend { delay, response } => {
                    total_delay += delay;
                    current_response = response;
                }
                FaultAction::OverrideTransactionId {
                    transaction_id,
                    response,
                } => {
                    override_tid = Some(transaction_id);
                    current_response = response;
                }
                FaultAction::SendRawBytes(bytes) => {
                    current_response = bytes;
                    send_raw = true;
                }
                // Short-circuit actions should not appear here, but handle gracefully
                FaultAction::DropResponse => {
                    return Some(FaultAction::DropResponse);
                }
                FaultAction::SendPartial { bytes } => {
                    return Some(FaultAction::SendPartial { bytes });
                }
            }
        }

        if !any_activated {
            return None;
        }

        // Build final action, composing all accumulated mutations
        let action = if send_raw {
            if total_delay > Duration::ZERO {
                // Delay + raw bytes: wrap in DelayThenSend (server must handle raw)
                FaultAction::DelayThenSend {
                    delay: total_delay,
                    response: current_response,
                }
            } else {
                FaultAction::SendRawBytes(current_response)
            }
        } else if let Some(tid) = override_tid {
            if total_delay > Duration::ZERO {
                FaultAction::DelayThenSend {
                    delay: total_delay,
                    response: current_response,
                }
            } else {
                FaultAction::OverrideTransactionId {
                    transaction_id: tid,
                    response: current_response,
                }
            }
        } else if total_delay > Duration::ZERO {
            FaultAction::DelayThenSend {
                delay: total_delay,
                response: current_response,
            }
        } else {
            FaultAction::SendResponse(current_response)
        };

        debug!(
            unit_id = ctx.unit_id,
            fc = ctx.function_code,
            "Fault pipeline produced action"
        );

        Some(action)
    }

    /// Get statistics for all faults in the pipeline.
    pub fn all_stats(&self) -> Vec<(&'static str, FaultStatsSnapshot)> {
        self.faults
            .iter()
            .map(|f| (f.fault_type(), f.stats()))
            .collect()
    }

    /// Reset statistics for all faults.
    pub fn reset_all_stats(&self) {
        for fault in &self.faults {
            fault.reset_stats();
        }
    }

    /// Build a pipeline from a `FaultInjectionConfig`.
    pub fn from_config(config: &FaultInjectionConfig) -> Self {
        let mut pipeline = Self::new();

        if !config.enabled {
            pipeline
                .enabled
                .store(false, std::sync::atomic::Ordering::Release);
            return pipeline;
        }

        for fault_cfg in &config.faults {
            let fault: Arc<dyn ModbusFault> = match fault_cfg.fault_type {
                FaultType::CrcCorruption => Arc::new(CrcCorruptionFault::from_config(
                    &fault_cfg.config,
                    fault_cfg.target.clone(),
                )),
                FaultType::WrongUnitId => Arc::new(WrongUnitIdFault::from_config(
                    &fault_cfg.config,
                    fault_cfg.target.clone(),
                )),
                FaultType::WrongFunctionCode => Arc::new(WrongFunctionCodeFault::from_config(
                    &fault_cfg.config,
                    fault_cfg.target.clone(),
                )),
                FaultType::WrongTransactionId => Arc::new(WrongTransactionIdFault::from_config(
                    &fault_cfg.config,
                    fault_cfg.target.clone(),
                )),
                FaultType::TruncatedResponse => Arc::new(TruncatedResponseFault::from_config(
                    &fault_cfg.config,
                    fault_cfg.target.clone(),
                )),
                FaultType::ExtraData => Arc::new(ExtraDataFault::from_config(
                    &fault_cfg.config,
                    fault_cfg.target.clone(),
                )),
                FaultType::DelayedResponse => Arc::new(DelayedResponseFault::from_config(
                    &fault_cfg.config,
                    fault_cfg.target.clone(),
                )),
                FaultType::NoResponse => Arc::new(NoResponseFault::new(fault_cfg.target.clone())),
                FaultType::ExceptionInjection => Arc::new(
                    ExceptionInjectionFault::from_config(&fault_cfg.config, fault_cfg.target.clone()),
                ),
                FaultType::PartialFrame => Arc::new(PartialFrameFault::from_config(
                    &fault_cfg.config,
                    fault_cfg.target.clone(),
                )),
            };

            pipeline.faults.push(fault);
        }

        pipeline
    }
}

impl Default for FaultPipeline {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for FaultPipeline {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FaultPipeline")
            .field("enabled", &self.is_enabled())
            .field("fault_count", &self.faults.len())
            .field(
                "faults",
                &self
                    .faults
                    .iter()
                    .map(|f| f.fault_type())
                    .collect::<Vec<_>>(),
            )
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Simple test fault that always drops
    struct AlwaysDropFault {
        stats: FaultStats,
    }

    impl AlwaysDropFault {
        fn new() -> Self {
            Self {
                stats: FaultStats::new(),
            }
        }
    }

    impl ModbusFault for AlwaysDropFault {
        fn fault_type(&self) -> &'static str {
            "always_drop"
        }
        fn is_enabled(&self) -> bool {
            self.stats.is_enabled()
        }
        fn set_enabled(&self, enabled: bool) {
            self.stats.set_enabled(enabled);
        }
        fn should_activate(&self, _ctx: &ModbusFaultContext) -> bool {
            self.stats.record_check();
            true
        }
        fn apply(&self, _ctx: &ModbusFaultContext) -> FaultAction {
            self.stats.record_activation();
            self.stats.record_affected();
            FaultAction::DropResponse
        }
        fn stats(&self) -> FaultStatsSnapshot {
            self.stats.snapshot()
        }
        fn reset_stats(&self) {
            self.stats.reset();
        }
        fn is_short_circuit(&self) -> bool {
            true
        }
    }

    // Simple test fault that modifies response
    struct PrependByteFault {
        byte: u8,
        stats: FaultStats,
    }

    impl PrependByteFault {
        fn new(byte: u8) -> Self {
            Self {
                byte,
                stats: FaultStats::new(),
            }
        }
    }

    impl ModbusFault for PrependByteFault {
        fn fault_type(&self) -> &'static str {
            "prepend_byte"
        }
        fn is_enabled(&self) -> bool {
            self.stats.is_enabled()
        }
        fn set_enabled(&self, enabled: bool) {
            self.stats.set_enabled(enabled);
        }
        fn should_activate(&self, _ctx: &ModbusFaultContext) -> bool {
            self.stats.record_check();
            true
        }
        fn apply(&self, ctx: &ModbusFaultContext) -> FaultAction {
            self.stats.record_activation();
            self.stats.record_affected();
            let mut response = vec![self.byte];
            response.extend_from_slice(&ctx.response_pdu);
            FaultAction::SendResponse(response)
        }
        fn stats(&self) -> FaultStatsSnapshot {
            self.stats.snapshot()
        }
        fn reset_stats(&self) {
            self.stats.reset();
        }
    }

    fn test_ctx() -> ModbusFaultContext {
        ModbusFaultContext::tcp(1, 0x03, &[0x03, 0x00, 0x00, 0x00, 0x01], &[0x03, 0x02, 0x00, 0x64], 1, 1)
    }

    #[test]
    fn test_empty_pipeline() {
        let pipeline = FaultPipeline::new();
        assert!(pipeline.is_empty());
        assert_eq!(pipeline.len(), 0);
        assert!(pipeline.apply(&test_ctx()).is_none());
    }

    #[test]
    fn test_pipeline_disabled() {
        let pipeline = FaultPipeline::new()
            .with_fault(Arc::new(AlwaysDropFault::new()));
        pipeline.set_enabled(false);
        assert!(pipeline.apply(&test_ctx()).is_none());
    }

    #[test]
    fn test_short_circuit_priority() {
        let drop_fault = Arc::new(AlwaysDropFault::new());
        let prepend_fault = Arc::new(PrependByteFault::new(0xFF));

        // Even though prepend is registered first, short-circuit faults run first
        let pipeline = FaultPipeline::new()
            .with_fault(prepend_fault.clone())
            .with_fault(drop_fault.clone());

        let action = pipeline.apply(&test_ctx());
        assert!(matches!(action, Some(FaultAction::DropResponse)));

        // Drop fault was checked and activated
        assert_eq!(drop_fault.stats().checks, 1);
        assert_eq!(drop_fault.stats().activations, 1);

        // Prepend fault was NOT checked (short-circuit happened before mutation phase)
        assert_eq!(prepend_fault.stats().checks, 0);
    }

    #[test]
    fn test_mutation_chaining() {
        let fault1 = Arc::new(PrependByteFault::new(0xAA));
        let fault2 = Arc::new(PrependByteFault::new(0xBB));

        let pipeline = FaultPipeline::new()
            .with_fault(fault1.clone())
            .with_fault(fault2.clone());

        let ctx = test_ctx();
        let action = pipeline.apply(&ctx).unwrap();

        match action {
            FaultAction::SendResponse(pdu) => {
                // fault2 prepends 0xBB to (0xAA + original)
                assert_eq!(pdu[0], 0xBB);
                assert_eq!(pdu[1], 0xAA);
                assert_eq!(&pdu[2..], &ctx.response_pdu);
            }
            _ => panic!("Expected SendResponse"),
        }
    }

    #[test]
    fn test_disabled_fault_skipped() {
        let fault = Arc::new(PrependByteFault::new(0xFF));
        fault.set_enabled(false);

        let pipeline = FaultPipeline::new().with_fault(fault.clone());
        assert!(pipeline.apply(&test_ctx()).is_none());
        assert_eq!(fault.stats().checks, 0);
    }

    #[test]
    fn test_all_stats() {
        let fault1 = Arc::new(PrependByteFault::new(0xAA));
        let fault2 = Arc::new(PrependByteFault::new(0xBB));

        let pipeline = FaultPipeline::new()
            .with_fault(fault1)
            .with_fault(fault2);

        pipeline.apply(&test_ctx());

        let stats = pipeline.all_stats();
        assert_eq!(stats.len(), 2);
        assert_eq!(stats[0].0, "prepend_byte");
        assert_eq!(stats[0].1.activations, 1);
        assert_eq!(stats[1].0, "prepend_byte");
        assert_eq!(stats[1].1.activations, 1);
    }

    #[test]
    fn test_reset_all_stats() {
        let fault = Arc::new(PrependByteFault::new(0xAA));
        let pipeline = FaultPipeline::new().with_fault(fault.clone());

        pipeline.apply(&test_ctx());
        assert_eq!(fault.stats().activations, 1);

        pipeline.reset_all_stats();
        assert_eq!(fault.stats().activations, 0);
    }

    #[test]
    fn test_rtu_context() {
        let ctx = ModbusFaultContext::rtu(1, 0x03, &[0x03], &[0x03, 0x02], 5);
        assert_eq!(ctx.transport, TransportKind::Rtu);
        assert_eq!(ctx.unit_id, 1);
        assert_eq!(ctx.transaction_id, 0);
        assert!(!ctx.is_broadcast);
        assert_eq!(ctx.request_number, 5);
    }

    #[test]
    fn test_tcp_context() {
        let ctx = ModbusFaultContext::tcp(0, 0x03, &[0x03], &[0x03, 0x02], 42, 10);
        assert_eq!(ctx.transport, TransportKind::Tcp);
        assert_eq!(ctx.unit_id, 0);
        assert!(ctx.is_broadcast);
        assert_eq!(ctx.transaction_id, 42);
        assert_eq!(ctx.request_number, 10);
    }

    #[test]
    fn test_transport_compatibility() {
        struct RtuOnlyFault {
            stats: FaultStats,
        }
        impl ModbusFault for RtuOnlyFault {
            fn fault_type(&self) -> &'static str { "rtu_only" }
            fn is_enabled(&self) -> bool { self.stats.is_enabled() }
            fn set_enabled(&self, enabled: bool) { self.stats.set_enabled(enabled); }
            fn should_activate(&self, _: &ModbusFaultContext) -> bool {
                self.stats.record_check();
                true
            }
            fn apply(&self, ctx: &ModbusFaultContext) -> FaultAction {
                self.stats.record_activation();
                FaultAction::SendResponse(ctx.response_pdu.clone())
            }
            fn stats(&self) -> FaultStatsSnapshot { self.stats.snapshot() }
            fn reset_stats(&self) { self.stats.reset(); }
            fn compatible_transport(&self) -> Option<TransportKind> {
                Some(TransportKind::Rtu)
            }
        }

        let fault = Arc::new(RtuOnlyFault { stats: FaultStats::new() });
        let pipeline = FaultPipeline::new().with_fault(fault.clone());

        // TCP context: should not activate
        let tcp_ctx = test_ctx(); // TCP context
        assert!(pipeline.apply(&tcp_ctx).is_none());

        // RTU context: should activate
        let rtu_ctx = ModbusFaultContext::rtu(1, 0x03, &[0x03], &[0x03, 0x02], 1);
        assert!(pipeline.apply(&rtu_ctx).is_some());
    }
}

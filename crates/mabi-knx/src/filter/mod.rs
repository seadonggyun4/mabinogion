//! Flow control filter chain for KNXnet/IP bus timing simulation.
//!
//! This module provides a production-grade bidirectional filter pipeline that
//! simulates real KNX bus flow control behavior. The architecture mirrors
//! trap-knx's filter chain design:
//!
//! ```text
//! Client → [QueueFilter] → [PaceFilter] → [RetryFilter] → Bus (send path)
//! Client ← [QueueFilter] ← [PaceFilter] ← [RetryFilter] ← Bus (recv path)
//! ```
//!
//! ## Components
//!
//! - [`FilterChain`]: Bidirectional pipeline orchestrator
//! - [`PaceFilter`]: 3-state FSM (P_DOWN/P_IDLE/P_BUSY) enforcing minimum
//!   inter-frame delay based on byte_delay + margin
//! - [`QueueFilter`]: 3-priority FIFO (High/Normal/Low) with WaitingForAck
//!   backpressure support
//! - [`RetryFilter`]: Retry logic with 3-state circuit breaker
//!   (Closed/Open/HalfOpen)
//!
//! ## Bus Timing Model
//!
//! KNX TP1 bus operates at 9600 baud. The PaceFilter enforces realistic
//! inter-frame timing:
//!
//! - **byte_delay**: Time to transmit one byte = 1/9600 * 11 bits ~ 1.146ms
//! - **Frame delay**: byte_delay * frame_length
//! - **Margin**: Additional 50ms safety margin (configurable)
//! - **Total**: frame_delay + margin before next frame can be sent

pub mod chain;
pub mod pace;
pub mod queue;
pub mod retry;

pub use chain::{
    FilterChain, FilterChainConfig, FilterChainStats, FilterChainStatsSnapshot, FilterDirection,
    FilterResult, FrameEnvelope,
};
pub use pace::{PaceFilter, PaceFilterConfig, PaceFilterStats, PaceFilterStatsSnapshot, PaceState};
pub use queue::{
    QueueFilter, QueueFilterConfig, QueueFilterStats, QueueFilterStatsSnapshot, QueuePriority,
};
pub use retry::{
    CircuitBreakerState, RetryFilter, RetryFilterConfig, RetryFilterStats, RetryFilterStatsSnapshot,
};

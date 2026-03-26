//! KNXnet/IP tunnelling protocol with production-grade FSM and sequence validation.
//!
//! This module provides:
//!
//! - **Connection management**: Connect/Disconnect handshake, CRI/CRD handling
//! - **Tunnelling protocol**: Request/ACK/Indication frame handling
//! - **Sequence tracking**: knxd-compatible rno/sno dual CAS tracking with
//!   Duplicate/OutOfOrder/FatalDesync detection
//! - **ACK management**: Timeout-based retry with consecutive error thresholds
//! - **7-state FSM**: Per-connection state machine mapping to knxd mod 0-3

pub mod ack_waiter;
pub mod connection;
pub mod fsm;
pub mod sequence;

// Re-export connection types (backward compatible)
pub use connection::{
    ConnectRequest, ConnectResponse, ConnectStatus, ConnectionRequestInfo, ConnectionResponseData,
    ConnectionStateRequest, ConnectionStateResponse, ConnectionType, DisconnectRequest,
    DisconnectResponse, KnxLayer, TunnelConnection, TunnellingAck, TunnellingRequest,
};

// Re-export new Phase 1 types
pub use ack_waiter::{AckMessage, AckResult, AckWaiter, AckWaiterStatsSnapshot};
pub use fsm::{FsmStatsSnapshot, TunnelErrorReason, TunnelFsm, TunnelState};
pub use sequence::{AckValidation, ReceivedValidation, SequenceStatsSnapshot, SequenceTracker};

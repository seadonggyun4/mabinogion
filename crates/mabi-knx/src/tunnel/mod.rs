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

pub mod connection;
pub mod sequence;
pub mod ack_waiter;
pub mod fsm;

// Re-export connection types (backward compatible)
pub use connection::{
    ConnectionType, KnxLayer,
    ConnectionRequestInfo, ConnectionResponseData,
    ConnectStatus, ConnectRequest, ConnectResponse,
    TunnellingRequest, TunnellingAck,
    ConnectionStateRequest, ConnectionStateResponse,
    DisconnectRequest, DisconnectResponse,
    TunnelConnection,
};

// Re-export new Phase 1 types
pub use sequence::{
    SequenceTracker, ReceivedValidation, AckValidation,
    SequenceStatsSnapshot,
};
pub use ack_waiter::{
    AckWaiter, AckResult, AckMessage,
    AckWaiterStatsSnapshot,
};
pub use fsm::{
    TunnelFsm, TunnelState, TunnelErrorReason,
    FsmStatsSnapshot,
};

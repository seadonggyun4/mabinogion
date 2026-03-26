//! Per-connection tunnel state machine (7-state FSM).
//!
//! Maps to knxd's mod 0-3 states, extended for async server operation:
//!
//! ```text
//! Disconnected ──→ Connecting ──→ Idle ↔ WaitingForAck ↔ WaitingForConfirmation
//!     ↑                ↓                    ↓                    ↓
//!     └── Error ←── Reconnecting ←─────────┴────────────────────┘
//! ```
//!
//! On the server side, this tracks the per-connection tunnel state so we know
//! whether it's safe to send frames or if the client is mid-handshake.

use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

/// 7-state tunnel FSM, mapped to knxd mod variable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TunnelState {
    /// Connection not established (knxd mod=0).
    Disconnected,

    /// Connection handshake in progress (no knxd equivalent, async extension).
    Connecting,

    /// Connected and idle, ready to send/receive (knxd mod=1).
    Idle,

    /// Waiting for TUNNELLING_ACK from client (knxd mod=2).
    WaitingForAck {
        sequence: u8,
        sent_at: Instant,
        retry_count: u8,
    },

    /// Waiting for L_Data.con from bus / confirmation processing (knxd mod=3).
    WaitingForConfirmation { sequence: u8, sent_at: Instant },

    /// Connection lost, attempting reconnection (no knxd equivalent).
    Reconnecting { attempt: u32 },

    /// Permanent error requiring manual intervention.
    Error { reason: TunnelErrorReason },
}

impl TunnelState {
    /// Get the knxd mod number equivalent.
    pub fn knxd_mod(&self) -> u8 {
        match self {
            Self::Disconnected | Self::Reconnecting { .. } | Self::Error { .. } => 0,
            Self::Connecting => 0,
            Self::Idle => 1,
            Self::WaitingForAck { .. } => 2,
            Self::WaitingForConfirmation { .. } => 3,
        }
    }

    /// Whether the tunnel can accept new outbound frames.
    pub fn can_send(&self) -> bool {
        matches!(self, Self::Idle)
    }

    /// Whether the tunnel is in any connected state.
    pub fn is_connected(&self) -> bool {
        matches!(
            self,
            Self::Idle | Self::WaitingForAck { .. } | Self::WaitingForConfirmation { .. }
        )
    }

    /// Whether the tunnel is in a terminal/error state.
    pub fn is_error(&self) -> bool {
        matches!(self, Self::Error { .. })
    }

    /// Short name for logging and display.
    pub fn name(&self) -> &'static str {
        match self {
            Self::Disconnected => "Disconnected",
            Self::Connecting => "Connecting",
            Self::Idle => "Idle",
            Self::WaitingForAck { .. } => "WaitingForAck",
            Self::WaitingForConfirmation { .. } => "WaitingForConfirmation",
            Self::Reconnecting { .. } => "Reconnecting",
            Self::Error { .. } => "Error",
        }
    }
}

impl fmt::Display for TunnelState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::WaitingForAck {
                sequence,
                retry_count,
                ..
            } => {
                write!(
                    f,
                    "WaitingForAck(seq={}, retries={})",
                    sequence, retry_count
                )
            }
            Self::WaitingForConfirmation { sequence, .. } => {
                write!(f, "WaitingForConfirmation(seq={})", sequence)
            }
            Self::Reconnecting { attempt } => {
                write!(f, "Reconnecting(attempt={})", attempt)
            }
            Self::Error { reason } => {
                write!(f, "Error({})", reason)
            }
            _ => write!(f, "{}", self.name()),
        }
    }
}

/// Reason for entering the Error state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TunnelErrorReason {
    /// CONNECT_REQUEST handshake failed.
    HandshakeFailed { message: String },
    /// Max reconnect attempts exceeded.
    MaxReconnectExceeded { attempts: u32 },
    /// Fatal sequence desync.
    SequenceDesync { expected: u8, actual: u8 },
    /// Gateway refused connection with error code.
    GatewayRefused { status: u8 },
    /// Consecutive send errors exceeded threshold.
    SendErrorThreshold { errors: u32, threshold: u32 },
}

impl fmt::Display for TunnelErrorReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::HandshakeFailed { message } => write!(f, "Handshake failed: {}", message),
            Self::MaxReconnectExceeded { attempts } => {
                write!(f, "Max reconnect attempts ({}) exceeded", attempts)
            }
            Self::SequenceDesync { expected, actual } => {
                write!(f, "Sequence desync: expected {}, got {}", expected, actual)
            }
            Self::GatewayRefused { status } => {
                write!(f, "Gateway refused: status {:#04x}", status)
            }
            Self::SendErrorThreshold { errors, threshold } => {
                write!(f, "Send errors {} >= threshold {}", errors, threshold)
            }
        }
    }
}

/// Lock-free transition statistics.
#[derive(Debug, Default)]
pub struct FsmStats {
    transitions: AtomicU64,
}

/// Snapshot of FSM statistics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FsmStatsSnapshot {
    pub transitions: u64,
}

/// Per-connection tunnel FSM with validated state transitions.
#[derive(Debug)]
pub struct TunnelFsm {
    state: parking_lot::RwLock<TunnelState>,
    stats: FsmStats,
}

impl TunnelFsm {
    /// Create in Disconnected state.
    pub fn new() -> Self {
        Self {
            state: parking_lot::RwLock::new(TunnelState::Disconnected),
            stats: FsmStats::default(),
        }
    }

    /// Create in Connecting state (for newly accepted connections).
    pub fn connecting() -> Self {
        Self {
            state: parking_lot::RwLock::new(TunnelState::Connecting),
            stats: FsmStats::default(),
        }
    }

    /// Get current state (read-only snapshot).
    pub fn state(&self) -> TunnelState {
        self.state.read().clone()
    }

    /// Whether the tunnel can accept new outbound frames.
    pub fn can_send(&self) -> bool {
        self.state.read().can_send()
    }

    /// Whether the tunnel is in any connected state.
    pub fn is_connected(&self) -> bool {
        self.state.read().is_connected()
    }

    /// Transition: Connecting → Idle (handshake completed).
    pub fn on_connected(&self) {
        let mut state = self.state.write();
        *state = TunnelState::Idle;
        self.stats.transitions.fetch_add(1, Ordering::Relaxed);
    }

    /// Transition: Idle → WaitingForAck (frame sent, awaiting ACK).
    pub fn on_frame_sent(&self, sequence: u8) {
        let mut state = self.state.write();
        *state = TunnelState::WaitingForAck {
            sequence,
            sent_at: Instant::now(),
            retry_count: 0,
        };
        self.stats.transitions.fetch_add(1, Ordering::Relaxed);
    }

    /// Transition: WaitingForAck → WaitingForAck (retry).
    pub fn on_retry(&self, sequence: u8, retry_count: u8) {
        let mut state = self.state.write();
        *state = TunnelState::WaitingForAck {
            sequence,
            sent_at: Instant::now(),
            retry_count,
        };
        self.stats.transitions.fetch_add(1, Ordering::Relaxed);
    }

    /// Transition: WaitingForAck → WaitingForConfirmation (ACK received for L_Data.req).
    pub fn on_ack_received(&self, sequence: u8) {
        let mut state = self.state.write();
        *state = TunnelState::WaitingForConfirmation {
            sequence,
            sent_at: Instant::now(),
        };
        self.stats.transitions.fetch_add(1, Ordering::Relaxed);
    }

    /// Transition: WaitingForAck → Idle (ACK received for non-confirmation frame).
    pub fn on_ack_received_simple(&self) {
        let mut state = self.state.write();
        *state = TunnelState::Idle;
        self.stats.transitions.fetch_add(1, Ordering::Relaxed);
    }

    /// Transition: WaitingForConfirmation → Idle (L_Data.con processed).
    pub fn on_confirmation_received(&self) {
        let mut state = self.state.write();
        *state = TunnelState::Idle;
        self.stats.transitions.fetch_add(1, Ordering::Relaxed);
    }

    /// Transition: any → Disconnected (client disconnected cleanly).
    pub fn on_disconnected(&self) {
        let mut state = self.state.write();
        *state = TunnelState::Disconnected;
        self.stats.transitions.fetch_add(1, Ordering::Relaxed);
    }

    /// Transition: any connected → Reconnecting.
    pub fn on_connection_lost(&self, attempt: u32) {
        let mut state = self.state.write();
        *state = TunnelState::Reconnecting { attempt };
        self.stats.transitions.fetch_add(1, Ordering::Relaxed);
    }

    /// Transition: any → Error (permanent failure).
    pub fn on_error(&self, reason: TunnelErrorReason) {
        let mut state = self.state.write();
        *state = TunnelState::Error { reason };
        self.stats.transitions.fetch_add(1, Ordering::Relaxed);
    }

    /// Force transition to Idle (used after sequence reset / reconnect success).
    pub fn force_idle(&self) {
        let mut state = self.state.write();
        *state = TunnelState::Idle;
        self.stats.transitions.fetch_add(1, Ordering::Relaxed);
    }

    /// Get statistics snapshot.
    pub fn stats_snapshot(&self) -> FsmStatsSnapshot {
        FsmStatsSnapshot {
            transitions: self.stats.transitions.load(Ordering::Relaxed),
        }
    }
}

impl Default for TunnelFsm {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_initial_state() {
        let fsm = TunnelFsm::new();
        assert!(matches!(fsm.state(), TunnelState::Disconnected));
        assert!(!fsm.can_send());
        assert!(!fsm.is_connected());
    }

    #[test]
    fn test_connecting_to_idle() {
        let fsm = TunnelFsm::connecting();
        assert!(matches!(fsm.state(), TunnelState::Connecting));

        fsm.on_connected();
        assert!(matches!(fsm.state(), TunnelState::Idle));
        assert!(fsm.can_send());
        assert!(fsm.is_connected());
    }

    #[test]
    fn test_full_send_cycle() {
        let fsm = TunnelFsm::connecting();
        fsm.on_connected();

        // Idle → WaitingForAck
        fsm.on_frame_sent(0);
        let state = fsm.state();
        assert!(matches!(
            state,
            TunnelState::WaitingForAck {
                sequence: 0,
                retry_count: 0,
                ..
            }
        ));
        assert!(!fsm.can_send());
        assert!(fsm.is_connected());

        // WaitingForAck → WaitingForConfirmation
        fsm.on_ack_received(0);
        assert!(matches!(
            fsm.state(),
            TunnelState::WaitingForConfirmation { sequence: 0, .. }
        ));

        // WaitingForConfirmation → Idle
        fsm.on_confirmation_received();
        assert!(matches!(fsm.state(), TunnelState::Idle));
        assert!(fsm.can_send());
    }

    #[test]
    fn test_simple_ack_cycle() {
        let fsm = TunnelFsm::connecting();
        fsm.on_connected();

        fsm.on_frame_sent(0);
        // For non-L_Data.req frames, go directly to Idle
        fsm.on_ack_received_simple();
        assert!(matches!(fsm.state(), TunnelState::Idle));
    }

    #[test]
    fn test_retry() {
        let fsm = TunnelFsm::connecting();
        fsm.on_connected();
        fsm.on_frame_sent(5);

        fsm.on_retry(5, 1);
        let state = fsm.state();
        assert!(matches!(
            state,
            TunnelState::WaitingForAck {
                sequence: 5,
                retry_count: 1,
                ..
            }
        ));
    }

    #[test]
    fn test_disconnection() {
        let fsm = TunnelFsm::connecting();
        fsm.on_connected();
        fsm.on_disconnected();
        assert!(matches!(fsm.state(), TunnelState::Disconnected));
    }

    #[test]
    fn test_error_state() {
        let fsm = TunnelFsm::connecting();
        fsm.on_connected();

        fsm.on_error(TunnelErrorReason::SequenceDesync {
            expected: 0,
            actual: 10,
        });
        let state = fsm.state();
        assert!(state.is_error());
        assert!(!fsm.can_send());
    }

    #[test]
    fn test_knxd_mod_mapping() {
        assert_eq!(TunnelState::Disconnected.knxd_mod(), 0);
        assert_eq!(TunnelState::Idle.knxd_mod(), 1);
        assert_eq!(
            TunnelState::WaitingForAck {
                sequence: 0,
                sent_at: Instant::now(),
                retry_count: 0
            }
            .knxd_mod(),
            2
        );
        assert_eq!(
            TunnelState::WaitingForConfirmation {
                sequence: 0,
                sent_at: Instant::now()
            }
            .knxd_mod(),
            3
        );
    }

    #[test]
    fn test_stats_tracking() {
        let fsm = TunnelFsm::connecting();
        fsm.on_connected();
        fsm.on_frame_sent(0);
        fsm.on_ack_received_simple();

        assert_eq!(fsm.stats_snapshot().transitions, 3);
    }

    #[test]
    fn test_display() {
        let state = TunnelState::WaitingForAck {
            sequence: 5,
            sent_at: Instant::now(),
            retry_count: 2,
        };
        let s = state.to_string();
        assert!(s.contains("5"));
        assert!(s.contains("2"));
    }

    #[test]
    fn test_error_reason_display() {
        let reason = TunnelErrorReason::SendErrorThreshold {
            errors: 6,
            threshold: 5,
        };
        let s = reason.to_string();
        assert!(s.contains("6"));
        assert!(s.contains("5"));
    }
}

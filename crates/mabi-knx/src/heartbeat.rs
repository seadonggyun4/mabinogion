//! Heartbeat action system for KNXnet/IP connection state simulation.
//!
//! This module provides a production-grade heartbeat response engine that
//! supports all 5 HeartbeatAction types defined by the KNXnet/IP spec:
//!
//! 1. **Continue** — Normal heartbeat response (status 0x00)
//! 2. **ImmediateReconnect** — Force client to reconnect immediately (status 0x21)
//! 3. **AbandonTunnel** — Signal unrecoverable tunnel failure (status 0x27)
//! 4. **DelayedReconnect** — Signal temporary unavailability (status 0x29)
//! 5. **NoResponse** — Simulate heartbeat timeout (no response sent)
//!
//! ## Scheduling
//!
//! The [`HeartbeatScheduler`] supports multiple scheduling strategies:
//!
//! - **Override**: Static override returning the same action for every heartbeat
//! - **Sequence**: Ordered list of actions, advancing through the list
//! - **Periodic**: Inject a fault action every N heartbeats
//! - **Probabilistic**: Random action selection with configurable probability
//! - **CountdownToFailure**: Normal for N heartbeats, then fail permanently
//!
//! ## Integration
//!
//! The heartbeat scheduler is integrated into `KnxServer::handle_connection_state_request()`.
//! When the scheduler is active, it determines the response status code instead of
//! the default behavior.

use std::fmt;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};

// ============================================================================
// HeartbeatAction — 5 action types
// ============================================================================

/// Heartbeat action determining how the server responds to connection state requests.
///
/// Maps to KNXnet/IP connection state response status codes:
/// - Continue → 0x00 (E_NO_ERROR)
/// - ImmediateReconnect → 0x21 (E_CONNECTION_ID)
/// - AbandonTunnel → 0x27 (E_KNX_CONNECTION)
/// - DelayedReconnect → 0x29 (E_TUNNELLING_LAYER)
/// - NoResponse → packet dropped (simulates timeout)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HeartbeatAction {
    /// Normal heartbeat — respond with 0x00 (no error).
    Continue,
    /// Force immediate reconnection — respond with 0x21 (E_CONNECTION_ID).
    ImmediateReconnect,
    /// Abandon the tunnel — respond with 0x27 (E_KNX_CONNECTION).
    AbandonTunnel,
    /// Delayed reconnection — respond with 0x29 (E_TUNNELLING_LAYER).
    DelayedReconnect,
    /// Suppress the heartbeat response entirely (simulates timeout).
    NoResponse,
}

impl HeartbeatAction {
    /// Get the KNXnet/IP status code for this action.
    ///
    /// Returns `None` for `NoResponse` (no packet should be sent).
    pub fn status_code(&self) -> Option<u8> {
        match self {
            Self::Continue => Some(0x00),
            Self::ImmediateReconnect => Some(0x21),
            Self::AbandonTunnel => Some(0x27),
            Self::DelayedReconnect => Some(0x29),
            Self::NoResponse => None,
        }
    }

    /// Parse a status code into a HeartbeatAction.
    pub fn from_status_code(code: u8) -> Self {
        match code {
            0x00 => Self::Continue,
            0x21 => Self::ImmediateReconnect,
            0x27 => Self::AbandonTunnel,
            0x29 => Self::DelayedReconnect,
            _ => Self::Continue, // Unknown codes default to Continue
        }
    }

    /// Whether this action results in a response being sent.
    pub fn sends_response(&self) -> bool {
        !matches!(self, Self::NoResponse)
    }

    /// Whether this action indicates the connection should be terminated.
    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::AbandonTunnel)
    }

    /// Whether this action indicates the client should reconnect.
    pub fn requires_reconnect(&self) -> bool {
        matches!(self, Self::ImmediateReconnect | Self::DelayedReconnect)
    }

    /// Whether this action indicates normal operation.
    pub fn is_normal(&self) -> bool {
        matches!(self, Self::Continue)
    }

    /// All possible actions.
    pub fn all() -> &'static [HeartbeatAction] {
        &[
            Self::Continue,
            Self::ImmediateReconnect,
            Self::AbandonTunnel,
            Self::DelayedReconnect,
            Self::NoResponse,
        ]
    }

    /// Human-readable name.
    pub fn name(&self) -> &'static str {
        match self {
            Self::Continue => "Continue",
            Self::ImmediateReconnect => "ImmediateReconnect",
            Self::AbandonTunnel => "AbandonTunnel",
            Self::DelayedReconnect => "DelayedReconnect",
            Self::NoResponse => "NoResponse",
        }
    }
}

impl fmt::Display for HeartbeatAction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.name())
    }
}

// ============================================================================
// HeartbeatSchedule — scheduling strategies
// ============================================================================

/// Scheduling strategy for heartbeat actions.
///
/// Controls how the server determines the heartbeat response for each
/// incoming connection state request.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum HeartbeatSchedule {
    /// Always return the default action (Continue).
    Normal,

    /// Static override — always return the specified action.
    Override {
        action: HeartbeatAction,
    },

    /// Ordered sequence of actions — advances through the list, repeating
    /// the last action when exhausted.
    Sequence {
        actions: Vec<HeartbeatAction>,
    },

    /// Inject a fault action every N heartbeats.
    /// Normal heartbeats return Continue, every `interval`th returns the fault action.
    Periodic {
        /// Fault action to inject.
        action: HeartbeatAction,
        /// Inject fault every N heartbeats (1-based).
        interval: u32,
    },

    /// Probabilistic fault injection.
    /// Each heartbeat has `probability` chance of returning the fault action.
    Probabilistic {
        /// Fault action to inject.
        action: HeartbeatAction,
        /// Probability of fault (0.0 - 1.0).
        probability: f64,
    },

    /// Normal for the first N heartbeats, then fail permanently.
    CountdownToFailure {
        /// Failure action.
        action: HeartbeatAction,
        /// Number of normal heartbeats before failure.
        countdown: u32,
    },
}

impl Default for HeartbeatSchedule {
    fn default() -> Self {
        Self::Normal
    }
}

impl HeartbeatSchedule {
    /// Validate the schedule configuration.
    pub fn validate(&self) -> Result<(), String> {
        match self {
            Self::Normal | Self::Override { .. } => Ok(()),
            Self::Sequence { actions } => {
                if actions.is_empty() {
                    Err("HeartbeatSchedule::Sequence requires at least one action".to_string())
                } else {
                    Ok(())
                }
            }
            Self::Periodic { interval, .. } => {
                if *interval == 0 {
                    Err("HeartbeatSchedule::Periodic interval must be > 0".to_string())
                } else {
                    Ok(())
                }
            }
            Self::Probabilistic { probability, .. } => {
                if *probability < 0.0 || *probability > 1.0 {
                    Err(format!(
                        "HeartbeatSchedule::Probabilistic probability must be 0.0-1.0, got {}",
                        probability
                    ))
                } else {
                    Ok(())
                }
            }
            Self::CountdownToFailure { countdown, .. } => {
                if *countdown == 0 {
                    Err("HeartbeatSchedule::CountdownToFailure countdown must be > 0".to_string())
                } else {
                    Ok(())
                }
            }
        }
    }
}

// ============================================================================
// HeartbeatScheduler — runtime scheduler
// ============================================================================

/// Runtime heartbeat scheduler that determines the response action for each
/// incoming connection state request.
///
/// Thread-safe — uses atomic counters and a read-write lock for schedule updates.
pub struct HeartbeatScheduler {
    /// Current schedule.
    schedule: RwLock<HeartbeatSchedule>,
    /// Heartbeat counter (total requests processed).
    counter: AtomicU32,
    /// Per-channel heartbeat counter for sequence advancement.
    /// Packed as: (channel_id << 24) | counter
    /// For simplicity, we use a global sequence index.
    sequence_index: AtomicU32,
    /// Statistics.
    stats: HeartbeatStats,
}

/// Heartbeat scheduler statistics.
pub struct HeartbeatStats {
    /// Total heartbeat requests processed.
    pub total_requests: AtomicU64,
    /// Normal (Continue) responses.
    pub continue_count: AtomicU64,
    /// ImmediateReconnect responses.
    pub immediate_reconnect_count: AtomicU64,
    /// AbandonTunnel responses.
    pub abandon_tunnel_count: AtomicU64,
    /// DelayedReconnect responses.
    pub delayed_reconnect_count: AtomicU64,
    /// NoResponse (timeout) count.
    pub no_response_count: AtomicU64,
}

impl HeartbeatStats {
    fn new() -> Self {
        Self {
            total_requests: AtomicU64::new(0),
            continue_count: AtomicU64::new(0),
            immediate_reconnect_count: AtomicU64::new(0),
            abandon_tunnel_count: AtomicU64::new(0),
            delayed_reconnect_count: AtomicU64::new(0),
            no_response_count: AtomicU64::new(0),
        }
    }

    fn record(&self, action: HeartbeatAction) {
        self.total_requests.fetch_add(1, Ordering::Relaxed);
        match action {
            HeartbeatAction::Continue => {
                self.continue_count.fetch_add(1, Ordering::Relaxed);
            }
            HeartbeatAction::ImmediateReconnect => {
                self.immediate_reconnect_count.fetch_add(1, Ordering::Relaxed);
            }
            HeartbeatAction::AbandonTunnel => {
                self.abandon_tunnel_count.fetch_add(1, Ordering::Relaxed);
            }
            HeartbeatAction::DelayedReconnect => {
                self.delayed_reconnect_count.fetch_add(1, Ordering::Relaxed);
            }
            HeartbeatAction::NoResponse => {
                self.no_response_count.fetch_add(1, Ordering::Relaxed);
            }
        }
    }

    /// Take a snapshot of the statistics.
    pub fn snapshot(&self) -> HeartbeatStatsSnapshot {
        HeartbeatStatsSnapshot {
            total_requests: self.total_requests.load(Ordering::Relaxed),
            continue_count: self.continue_count.load(Ordering::Relaxed),
            immediate_reconnect_count: self.immediate_reconnect_count.load(Ordering::Relaxed),
            abandon_tunnel_count: self.abandon_tunnel_count.load(Ordering::Relaxed),
            delayed_reconnect_count: self.delayed_reconnect_count.load(Ordering::Relaxed),
            no_response_count: self.no_response_count.load(Ordering::Relaxed),
        }
    }
}

/// Immutable snapshot of heartbeat statistics.
#[derive(Debug, Clone)]
pub struct HeartbeatStatsSnapshot {
    pub total_requests: u64,
    pub continue_count: u64,
    pub immediate_reconnect_count: u64,
    pub abandon_tunnel_count: u64,
    pub delayed_reconnect_count: u64,
    pub no_response_count: u64,
}

impl HeartbeatStatsSnapshot {
    /// Fault rate (non-Continue / total).
    pub fn fault_rate(&self) -> f64 {
        if self.total_requests == 0 {
            return 0.0;
        }
        let faults = self.total_requests - self.continue_count;
        faults as f64 / self.total_requests as f64
    }
}

impl HeartbeatScheduler {
    /// Create a new scheduler with the given schedule.
    pub fn new(schedule: HeartbeatSchedule) -> Self {
        Self {
            schedule: RwLock::new(schedule),
            counter: AtomicU32::new(0),
            sequence_index: AtomicU32::new(0),
            stats: HeartbeatStats::new(),
        }
    }

    /// Create a scheduler with default (Normal) schedule.
    pub fn normal() -> Self {
        Self::new(HeartbeatSchedule::Normal)
    }

    /// Update the schedule at runtime.
    pub fn set_schedule(&self, schedule: HeartbeatSchedule) {
        *self.schedule.write() = schedule;
        self.counter.store(0, Ordering::Relaxed);
        self.sequence_index.store(0, Ordering::Relaxed);
    }

    /// Get the current schedule (clone).
    pub fn schedule(&self) -> HeartbeatSchedule {
        self.schedule.read().clone()
    }

    /// Get statistics snapshot.
    pub fn stats_snapshot(&self) -> HeartbeatStatsSnapshot {
        self.stats.snapshot()
    }

    /// Get the total heartbeat count.
    pub fn heartbeat_count(&self) -> u32 {
        self.counter.load(Ordering::Relaxed)
    }

    /// Reset the scheduler state (counter, sequence index).
    pub fn reset(&self) {
        self.counter.store(0, Ordering::Relaxed);
        self.sequence_index.store(0, Ordering::Relaxed);
    }

    /// Determine the next heartbeat action.
    ///
    /// This is the core scheduling method. It reads the current schedule,
    /// advances internal counters, and returns the appropriate action.
    ///
    /// # Arguments
    /// * `channel_id` — The channel for which the heartbeat was received
    pub fn next_action(&self, _channel_id: u8) -> HeartbeatAction {
        let count = self.counter.fetch_add(1, Ordering::Relaxed);
        let schedule = self.schedule.read();

        let action = match &*schedule {
            HeartbeatSchedule::Normal => HeartbeatAction::Continue,

            HeartbeatSchedule::Override { action } => *action,

            HeartbeatSchedule::Sequence { actions } => {
                let idx = self.sequence_index.fetch_add(1, Ordering::Relaxed) as usize;
                if idx < actions.len() {
                    actions[idx]
                } else {
                    // Repeat last action when sequence is exhausted
                    *actions.last().unwrap_or(&HeartbeatAction::Continue)
                }
            }

            HeartbeatSchedule::Periodic { action, interval } => {
                // 1-based: first heartbeat is 1, inject at multiples of interval
                let one_based = count + 1;
                if one_based % *interval == 0 {
                    *action
                } else {
                    HeartbeatAction::Continue
                }
            }

            HeartbeatSchedule::Probabilistic { action, probability } => {
                let random = heartbeat_rand();
                if random < *probability {
                    *action
                } else {
                    HeartbeatAction::Continue
                }
            }

            HeartbeatSchedule::CountdownToFailure { action, countdown } => {
                if count >= *countdown {
                    *action
                } else {
                    HeartbeatAction::Continue
                }
            }
        };

        self.stats.record(action);
        action
    }
}

impl fmt::Debug for HeartbeatScheduler {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HeartbeatScheduler")
            .field("schedule", &*self.schedule.read())
            .field("counter", &self.counter.load(Ordering::Relaxed))
            .field("sequence_index", &self.sequence_index.load(Ordering::Relaxed))
            .finish()
    }
}

// ============================================================================
// HeartbeatSchedulerConfig — serde config
// ============================================================================

/// Configuration for the heartbeat scheduler.
///
/// This is used in `TunnelBehaviorConfig` and supports YAML/JSON deserialization.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeartbeatSchedulerConfig {
    /// Whether the scheduler is enabled.
    /// When disabled, normal heartbeat behavior is used.
    #[serde(default = "default_false")]
    pub enabled: bool,

    /// The scheduling strategy.
    #[serde(default)]
    pub schedule: HeartbeatSchedule,
}

fn default_false() -> bool {
    false
}

impl Default for HeartbeatSchedulerConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            schedule: HeartbeatSchedule::default(),
        }
    }
}

impl HeartbeatSchedulerConfig {
    /// Validate the configuration.
    pub fn validate(&self) -> Result<(), String> {
        if self.enabled {
            self.schedule.validate()
        } else {
            Ok(())
        }
    }
}

// ============================================================================
// Simple PRNG for probabilistic scheduling
// ============================================================================

/// Simple thread-local PRNG for heartbeat scheduling.
/// No cryptographic strength needed — just reasonable distribution.
fn heartbeat_rand() -> f64 {
    use std::cell::Cell;
    thread_local! {
        static STATE: Cell<u64> = Cell::new(0xDEAD_BEEF_CAFE_BABEu64);
    }
    STATE.with(|s| {
        let mut x = s.get();
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        s.set(x);
        (x as f64) / (u64::MAX as f64)
    })
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_heartbeat_action_status_codes() {
        assert_eq!(HeartbeatAction::Continue.status_code(), Some(0x00));
        assert_eq!(HeartbeatAction::ImmediateReconnect.status_code(), Some(0x21));
        assert_eq!(HeartbeatAction::AbandonTunnel.status_code(), Some(0x27));
        assert_eq!(HeartbeatAction::DelayedReconnect.status_code(), Some(0x29));
        assert_eq!(HeartbeatAction::NoResponse.status_code(), None);
    }

    #[test]
    fn test_heartbeat_action_from_status_code() {
        assert_eq!(HeartbeatAction::from_status_code(0x00), HeartbeatAction::Continue);
        assert_eq!(HeartbeatAction::from_status_code(0x21), HeartbeatAction::ImmediateReconnect);
        assert_eq!(HeartbeatAction::from_status_code(0x27), HeartbeatAction::AbandonTunnel);
        assert_eq!(HeartbeatAction::from_status_code(0x29), HeartbeatAction::DelayedReconnect);
        assert_eq!(HeartbeatAction::from_status_code(0xFF), HeartbeatAction::Continue);
    }

    #[test]
    fn test_heartbeat_action_properties() {
        assert!(HeartbeatAction::Continue.is_normal());
        assert!(!HeartbeatAction::Continue.is_terminal());
        assert!(!HeartbeatAction::Continue.requires_reconnect());
        assert!(HeartbeatAction::Continue.sends_response());

        assert!(HeartbeatAction::AbandonTunnel.is_terminal());
        assert!(!HeartbeatAction::AbandonTunnel.is_normal());

        assert!(HeartbeatAction::ImmediateReconnect.requires_reconnect());
        assert!(HeartbeatAction::DelayedReconnect.requires_reconnect());
        assert!(!HeartbeatAction::AbandonTunnel.requires_reconnect());

        assert!(!HeartbeatAction::NoResponse.sends_response());
    }

    #[test]
    fn test_heartbeat_action_all() {
        let all = HeartbeatAction::all();
        assert_eq!(all.len(), 5);
    }

    #[test]
    fn test_heartbeat_action_display() {
        assert_eq!(format!("{}", HeartbeatAction::Continue), "Continue");
        assert_eq!(format!("{}", HeartbeatAction::ImmediateReconnect), "ImmediateReconnect");
        assert_eq!(format!("{}", HeartbeatAction::AbandonTunnel), "AbandonTunnel");
        assert_eq!(format!("{}", HeartbeatAction::DelayedReconnect), "DelayedReconnect");
        assert_eq!(format!("{}", HeartbeatAction::NoResponse), "NoResponse");
    }

    #[test]
    fn test_schedule_normal() {
        let scheduler = HeartbeatScheduler::normal();
        assert_eq!(scheduler.next_action(1), HeartbeatAction::Continue);
        assert_eq!(scheduler.next_action(1), HeartbeatAction::Continue);
        assert_eq!(scheduler.heartbeat_count(), 2);
    }

    #[test]
    fn test_schedule_override() {
        let scheduler = HeartbeatScheduler::new(HeartbeatSchedule::Override {
            action: HeartbeatAction::ImmediateReconnect,
        });

        assert_eq!(scheduler.next_action(1), HeartbeatAction::ImmediateReconnect);
        assert_eq!(scheduler.next_action(1), HeartbeatAction::ImmediateReconnect);
        assert_eq!(scheduler.next_action(2), HeartbeatAction::ImmediateReconnect);
    }

    #[test]
    fn test_schedule_sequence() {
        let scheduler = HeartbeatScheduler::new(HeartbeatSchedule::Sequence {
            actions: vec![
                HeartbeatAction::Continue,
                HeartbeatAction::ImmediateReconnect,
                HeartbeatAction::AbandonTunnel,
            ],
        });

        assert_eq!(scheduler.next_action(1), HeartbeatAction::Continue);
        assert_eq!(scheduler.next_action(1), HeartbeatAction::ImmediateReconnect);
        assert_eq!(scheduler.next_action(1), HeartbeatAction::AbandonTunnel);
        // Beyond sequence — repeats last
        assert_eq!(scheduler.next_action(1), HeartbeatAction::AbandonTunnel);
        assert_eq!(scheduler.next_action(1), HeartbeatAction::AbandonTunnel);
    }

    #[test]
    fn test_schedule_periodic() {
        let scheduler = HeartbeatScheduler::new(HeartbeatSchedule::Periodic {
            action: HeartbeatAction::AbandonTunnel,
            interval: 3,
        });

        // Heartbeats: 1=Continue, 2=Continue, 3=Fault, 4=Continue, 5=Continue, 6=Fault
        assert_eq!(scheduler.next_action(1), HeartbeatAction::Continue);     // count=0, 1-based=1
        assert_eq!(scheduler.next_action(1), HeartbeatAction::Continue);     // count=1, 1-based=2
        assert_eq!(scheduler.next_action(1), HeartbeatAction::AbandonTunnel); // count=2, 1-based=3
        assert_eq!(scheduler.next_action(1), HeartbeatAction::Continue);     // count=3, 1-based=4
        assert_eq!(scheduler.next_action(1), HeartbeatAction::Continue);     // count=4, 1-based=5
        assert_eq!(scheduler.next_action(1), HeartbeatAction::AbandonTunnel); // count=5, 1-based=6
    }

    #[test]
    fn test_schedule_countdown_to_failure() {
        let scheduler = HeartbeatScheduler::new(HeartbeatSchedule::CountdownToFailure {
            action: HeartbeatAction::ImmediateReconnect,
            countdown: 3,
        });

        assert_eq!(scheduler.next_action(1), HeartbeatAction::Continue);           // 0 < 3
        assert_eq!(scheduler.next_action(1), HeartbeatAction::Continue);           // 1 < 3
        assert_eq!(scheduler.next_action(1), HeartbeatAction::Continue);           // 2 < 3
        assert_eq!(scheduler.next_action(1), HeartbeatAction::ImmediateReconnect); // 3 >= 3
        assert_eq!(scheduler.next_action(1), HeartbeatAction::ImmediateReconnect); // 4 >= 3
    }

    #[test]
    fn test_schedule_probabilistic() {
        let scheduler = HeartbeatScheduler::new(HeartbeatSchedule::Probabilistic {
            action: HeartbeatAction::NoResponse,
            probability: 0.0,
        });

        // probability=0.0 → always Continue
        for _ in 0..20 {
            assert_eq!(scheduler.next_action(1), HeartbeatAction::Continue);
        }

        let scheduler = HeartbeatScheduler::new(HeartbeatSchedule::Probabilistic {
            action: HeartbeatAction::NoResponse,
            probability: 1.0,
        });

        // probability=1.0 → always NoResponse
        for _ in 0..20 {
            assert_eq!(scheduler.next_action(1), HeartbeatAction::NoResponse);
        }
    }

    #[test]
    fn test_scheduler_set_schedule() {
        let scheduler = HeartbeatScheduler::normal();
        assert_eq!(scheduler.next_action(1), HeartbeatAction::Continue);

        scheduler.set_schedule(HeartbeatSchedule::Override {
            action: HeartbeatAction::AbandonTunnel,
        });
        assert_eq!(scheduler.next_action(1), HeartbeatAction::AbandonTunnel);
        assert_eq!(scheduler.heartbeat_count(), 1); // Counter was reset
    }

    #[test]
    fn test_scheduler_reset() {
        let scheduler = HeartbeatScheduler::new(HeartbeatSchedule::Sequence {
            actions: vec![HeartbeatAction::Continue, HeartbeatAction::ImmediateReconnect],
        });

        assert_eq!(scheduler.next_action(1), HeartbeatAction::Continue);
        assert_eq!(scheduler.next_action(1), HeartbeatAction::ImmediateReconnect);

        scheduler.reset();

        // After reset, sequence starts over
        assert_eq!(scheduler.next_action(1), HeartbeatAction::Continue);
        assert_eq!(scheduler.next_action(1), HeartbeatAction::ImmediateReconnect);
    }

    #[test]
    fn test_scheduler_stats() {
        let scheduler = HeartbeatScheduler::new(HeartbeatSchedule::Sequence {
            actions: vec![
                HeartbeatAction::Continue,
                HeartbeatAction::ImmediateReconnect,
                HeartbeatAction::AbandonTunnel,
                HeartbeatAction::DelayedReconnect,
                HeartbeatAction::NoResponse,
            ],
        });

        for _ in 0..5 {
            scheduler.next_action(1);
        }

        let stats = scheduler.stats_snapshot();
        assert_eq!(stats.total_requests, 5);
        assert_eq!(stats.continue_count, 1);
        assert_eq!(stats.immediate_reconnect_count, 1);
        assert_eq!(stats.abandon_tunnel_count, 1);
        assert_eq!(stats.delayed_reconnect_count, 1);
        assert_eq!(stats.no_response_count, 1);
    }

    #[test]
    fn test_stats_fault_rate() {
        let scheduler = HeartbeatScheduler::new(HeartbeatSchedule::Periodic {
            action: HeartbeatAction::ImmediateReconnect,
            interval: 2,
        });

        // 1=Continue, 2=Fault, 3=Continue, 4=Fault
        for _ in 0..4 {
            scheduler.next_action(1);
        }

        let stats = scheduler.stats_snapshot();
        assert_eq!(stats.total_requests, 4);
        assert_eq!(stats.continue_count, 2);
        assert_eq!(stats.immediate_reconnect_count, 2);
        assert!((stats.fault_rate() - 0.5).abs() < f64::EPSILON);
    }

    #[test]
    fn test_stats_fault_rate_zero() {
        let stats = HeartbeatStatsSnapshot {
            total_requests: 0,
            continue_count: 0,
            immediate_reconnect_count: 0,
            abandon_tunnel_count: 0,
            delayed_reconnect_count: 0,
            no_response_count: 0,
        };
        assert_eq!(stats.fault_rate(), 0.0);
    }

    #[test]
    fn test_schedule_validate() {
        assert!(HeartbeatSchedule::Normal.validate().is_ok());
        assert!(HeartbeatSchedule::Override {
            action: HeartbeatAction::Continue,
        }.validate().is_ok());

        assert!(HeartbeatSchedule::Sequence {
            actions: vec![HeartbeatAction::Continue],
        }.validate().is_ok());
        assert!(HeartbeatSchedule::Sequence {
            actions: vec![],
        }.validate().is_err());

        assert!(HeartbeatSchedule::Periodic {
            action: HeartbeatAction::Continue,
            interval: 1,
        }.validate().is_ok());
        assert!(HeartbeatSchedule::Periodic {
            action: HeartbeatAction::Continue,
            interval: 0,
        }.validate().is_err());

        assert!(HeartbeatSchedule::Probabilistic {
            action: HeartbeatAction::Continue,
            probability: 0.5,
        }.validate().is_ok());
        assert!(HeartbeatSchedule::Probabilistic {
            action: HeartbeatAction::Continue,
            probability: -0.1,
        }.validate().is_err());
        assert!(HeartbeatSchedule::Probabilistic {
            action: HeartbeatAction::Continue,
            probability: 1.1,
        }.validate().is_err());

        assert!(HeartbeatSchedule::CountdownToFailure {
            action: HeartbeatAction::Continue,
            countdown: 5,
        }.validate().is_ok());
        assert!(HeartbeatSchedule::CountdownToFailure {
            action: HeartbeatAction::Continue,
            countdown: 0,
        }.validate().is_err());
    }

    #[test]
    fn test_scheduler_config_default() {
        let config = HeartbeatSchedulerConfig::default();
        assert!(!config.enabled);
        assert!(matches!(config.schedule, HeartbeatSchedule::Normal));
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_scheduler_debug() {
        let scheduler = HeartbeatScheduler::normal();
        let debug_str = format!("{:?}", scheduler);
        assert!(debug_str.contains("HeartbeatScheduler"));
        assert!(debug_str.contains("Normal"));
    }
}

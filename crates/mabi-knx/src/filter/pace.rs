//! PaceFilter — 3-state FSM for KNX bus timing simulation.
//!
//! The PaceFilter enforces realistic inter-frame delays that match the
//! physical KNX TP1 bus timing. This is critical for testing trap-knx's
//! `PaceFilter` which guarantees minimum spacing between frames.
//!
//! ## State Machine
//!
//! ```text
//! ┌─────────┐  frame_start()  ┌─────────┐  frame_complete()  ┌─────────┐
//! │ P_DOWN  │ ──────────────→ │ P_BUSY  │ ─────────────────→ │ P_IDLE  │
//! │ (off)   │                 │ (tx)    │                    │ (wait)  │
//! └─────────┘                 └─────────┘                    └────┬────┘
//!      ↑                                                         │
//!      │                           cooldown expired              │
//!      └─────────────────────────────────────────────────────────┘
//! ```
//!
//! - **P_DOWN**: Filter is not active (initial state or after disable).
//!   Frames pass through with no delay.
//! - **P_BUSY**: A frame is currently being transmitted on the bus.
//!   New frames receive a delay equal to the remaining transmission time.
//! - **P_IDLE**: The bus has completed a transmission and is in the
//!   cooldown period (margin). Once the cooldown expires, transitions
//!   back to P_DOWN (or stays in P_IDLE if continuous operation).
//!
//! ## Timing Model
//!
//! KNX TP1 operates at 9600 baud with 11 bits per byte (1 start, 8 data,
//! 1 parity, 1 stop):
//!
//! - **byte_time** = 1/9600 * 11 = 1.146ms per byte
//! - **frame_time** = byte_time * frame_length
//! - **margin** = configurable (default 50ms)
//! - **total_delay** = frame_time + margin

use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use tracing::trace;

use super::chain::{FilterResult, FrameEnvelope};

// ============================================================================
// KNX TP1 Bus Constants
// ============================================================================

/// KNX TP1 baud rate.
const KNX_TP1_BAUD_RATE: u32 = 9600;

/// Bits per byte on KNX TP1 (1 start + 8 data + 1 parity + 1 stop).
const KNX_TP1_BITS_PER_BYTE: u32 = 11;

/// Time to transmit one byte on KNX TP1 in microseconds.
/// = (1_000_000 * 11) / 9600 = 1145.83... ≈ 1146 us
const KNX_TP1_BYTE_TIME_US: u64 =
    1_000_000 * KNX_TP1_BITS_PER_BYTE as u64 / KNX_TP1_BAUD_RATE as u64;

// ============================================================================
// PaceFilter State
// ============================================================================

/// PaceFilter FSM state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaceState {
    /// Filter is inactive. Frames pass through without delay.
    Down,
    /// A frame is currently being transmitted on the bus.
    /// `busy_until` is the estimated completion time.
    Busy {
        /// When the current transmission will be complete.
        busy_until: Instant,
    },
    /// Bus is in cooldown after a transmission.
    /// No new frame should start until `idle_until` expires.
    Idle {
        /// When the cooldown period ends and new frames can begin.
        idle_until: Instant,
    },
}

impl PaceState {
    /// Short name for logging.
    pub fn name(&self) -> &'static str {
        match self {
            Self::Down => "P_DOWN",
            Self::Busy { .. } => "P_BUSY",
            Self::Idle { .. } => "P_IDLE",
        }
    }

    /// Whether the bus is currently available for immediate transmission.
    pub fn is_available(&self) -> bool {
        match self {
            Self::Down => true,
            Self::Busy { busy_until } => Instant::now() >= *busy_until,
            Self::Idle { idle_until } => Instant::now() >= *idle_until,
        }
    }

    /// Get the remaining delay before the bus becomes available.
    pub fn remaining_delay(&self) -> Duration {
        match self {
            Self::Down => Duration::ZERO,
            Self::Busy { busy_until } => {
                let now = Instant::now();
                if now >= *busy_until {
                    Duration::ZERO
                } else {
                    *busy_until - now
                }
            }
            Self::Idle { idle_until } => {
                let now = Instant::now();
                if now >= *idle_until {
                    Duration::ZERO
                } else {
                    *idle_until - now
                }
            }
        }
    }
}

impl fmt::Display for PaceState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Down => write!(f, "P_DOWN"),
            Self::Busy { busy_until } => {
                let remaining = busy_until.saturating_duration_since(Instant::now());
                write!(f, "P_BUSY({}ms remaining)", remaining.as_millis())
            }
            Self::Idle { idle_until } => {
                let remaining = idle_until.saturating_duration_since(Instant::now());
                write!(f, "P_IDLE({}ms cooldown)", remaining.as_millis())
            }
        }
    }
}

// ============================================================================
// PaceFilter Configuration
// ============================================================================

/// PaceFilter configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaceFilterConfig {
    /// Whether the PaceFilter is enabled.
    #[serde(default = "default_true")]
    pub enabled: bool,

    /// Additional margin after frame transmission in milliseconds.
    /// This simulates the bus quiet time between frames.
    /// KNX specification recommends 50ms minimum.
    #[serde(default = "default_margin_ms")]
    pub margin_ms: u64,

    /// Override byte time in microseconds (0 = use KNX TP1 default 1146us).
    /// Allows simulating different bus speeds for testing.
    #[serde(default)]
    pub byte_time_override_us: u64,

    /// Maximum accumulated delay in milliseconds.
    /// If the computed delay exceeds this, the frame is dropped.
    /// 0 = no limit.
    #[serde(default = "default_max_delay_ms")]
    pub max_delay_ms: u64,

    /// Whether to automatically transition from Idle → Down when
    /// the cooldown expires, or stay in Idle until explicitly reset.
    #[serde(default = "default_true")]
    pub auto_transition: bool,
}

fn default_true() -> bool {
    true
}

fn default_margin_ms() -> u64 {
    50
}

fn default_max_delay_ms() -> u64 {
    5000
}

impl Default for PaceFilterConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            margin_ms: default_margin_ms(),
            byte_time_override_us: 0,
            max_delay_ms: default_max_delay_ms(),
            auto_transition: true,
        }
    }
}

impl PaceFilterConfig {
    /// Get the byte time to use for calculations.
    pub fn byte_time_us(&self) -> u64 {
        if self.byte_time_override_us > 0 {
            self.byte_time_override_us
        } else {
            KNX_TP1_BYTE_TIME_US
        }
    }

    /// Get the margin as a Duration.
    pub fn margin(&self) -> Duration {
        Duration::from_millis(self.margin_ms)
    }

    /// Get the maximum delay as a Duration.
    pub fn max_delay(&self) -> Duration {
        Duration::from_millis(self.max_delay_ms)
    }

    /// Calculate the transmission time for a frame of the given size.
    pub fn frame_time(&self, frame_size_bytes: usize) -> Duration {
        let us = self.byte_time_us() * frame_size_bytes as u64;
        Duration::from_micros(us)
    }

    /// Calculate the total delay for a frame (frame_time + margin).
    pub fn total_frame_delay(&self, frame_size_bytes: usize) -> Duration {
        self.frame_time(frame_size_bytes) + self.margin()
    }

    /// Validate the configuration.
    pub fn validate(&self) -> Result<(), String> {
        if self.margin_ms > 10000 {
            return Err(format!(
                "PaceFilter margin_ms {} exceeds maximum 10000ms",
                self.margin_ms
            ));
        }
        Ok(())
    }
}

// ============================================================================
// PaceFilter Statistics
// ============================================================================

/// Lock-free PaceFilter statistics.
#[derive(Debug, Default)]
pub struct PaceFilterStats {
    /// Frames that passed with zero delay (bus was available).
    pub immediate_pass: AtomicU64,
    /// Frames that were delayed due to bus busy.
    pub delayed_frames: AtomicU64,
    /// Frames dropped because delay exceeded max_delay_ms.
    pub dropped_frames: AtomicU64,
    /// Total delay applied in microseconds.
    pub total_delay_us: AtomicU64,
    /// Number of Busy → Idle transitions.
    pub busy_to_idle: AtomicU64,
    /// Number of Idle → Down transitions.
    pub idle_to_down: AtomicU64,
}

/// Snapshot of PaceFilter statistics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PaceFilterStatsSnapshot {
    pub immediate_pass: u64,
    pub delayed_frames: u64,
    pub dropped_frames: u64,
    pub total_delay_us: u64,
    pub busy_to_idle: u64,
    pub idle_to_down: u64,
}

// ============================================================================
// PaceFilter
// ============================================================================

/// 3-state FSM filter enforcing KNX TP1 bus timing.
///
/// The PaceFilter ensures that frames are not sent faster than the
/// physical KNX bus can handle. It calculates inter-frame delay based on:
///
/// 1. Frame size (bytes) × byte_time (1.146ms for KNX TP1)
/// 2. Safety margin (default 50ms)
///
/// When the bus is busy, subsequent frames are delayed or dropped.
pub struct PaceFilter {
    config: PaceFilterConfig,
    state: RwLock<PaceState>,
    stats: PaceFilterStats,
}

impl PaceFilter {
    /// Create a new PaceFilter with the given configuration.
    pub fn new(config: PaceFilterConfig) -> Self {
        let initial_state = if config.enabled {
            PaceState::Down
        } else {
            PaceState::Down
        };

        Self {
            config,
            state: RwLock::new(initial_state),
            stats: PaceFilterStats::default(),
        }
    }

    /// Get the current state.
    pub fn state(&self) -> PaceState {
        let state = *self.state.read();
        // Auto-transition check
        if self.config.auto_transition {
            match state {
                PaceState::Busy { busy_until } if Instant::now() >= busy_until => {
                    // Transition to Idle
                    let idle_until = busy_until + self.config.margin();
                    let _ = state; // PaceState is Copy, no need to drop
                    let mut w = self.state.write();
                    *w = PaceState::Idle { idle_until };
                    self.stats.busy_to_idle.fetch_add(1, Ordering::Relaxed);
                    return *w;
                }
                PaceState::Idle { idle_until } if Instant::now() >= idle_until => {
                    let _ = state; // PaceState is Copy, no need to drop
                    let mut w = self.state.write();
                    *w = PaceState::Down;
                    self.stats.idle_to_down.fetch_add(1, Ordering::Relaxed);
                    return *w;
                }
                _ => {}
            }
        }
        state
    }

    /// Process a frame in the send direction.
    ///
    /// Calculates the necessary delay based on current bus state and
    /// frame size, then transitions the state machine.
    pub fn process_send(&self, envelope: &FrameEnvelope) -> FilterResult {
        if !self.config.enabled {
            return FilterResult::pass();
        }

        let frame_time = self.config.frame_time(envelope.frame_size_bytes);
        let now = Instant::now();

        let mut state = self.state.write();

        // Perform auto-transitions while holding the lock
        match *state {
            PaceState::Busy { busy_until } if now >= busy_until => {
                let idle_until = busy_until + self.config.margin();
                if now >= idle_until {
                    *state = PaceState::Down;
                    self.stats.idle_to_down.fetch_add(1, Ordering::Relaxed);
                } else {
                    *state = PaceState::Idle { idle_until };
                    self.stats.busy_to_idle.fetch_add(1, Ordering::Relaxed);
                }
            }
            PaceState::Idle { idle_until } if now >= idle_until => {
                *state = PaceState::Down;
                self.stats.idle_to_down.fetch_add(1, Ordering::Relaxed);
            }
            _ => {}
        }

        let delay = match *state {
            PaceState::Down => {
                // Bus is free — start transmission immediately
                let busy_until = now + frame_time;
                *state = PaceState::Busy { busy_until };
                self.stats.immediate_pass.fetch_add(1, Ordering::Relaxed);
                Duration::ZERO
            }
            PaceState::Busy { busy_until } => {
                // Bus is busy — delay until current transmission completes + margin
                let delay = busy_until.saturating_duration_since(now) + self.config.margin();

                // Check max delay
                if self.config.max_delay_ms > 0 && delay > self.config.max_delay() {
                    self.stats.dropped_frames.fetch_add(1, Ordering::Relaxed);
                    return FilterResult::Dropped {
                        reason: format!(
                            "PaceFilter: delay {}ms exceeds max_delay_ms {}",
                            delay.as_millis(),
                            self.config.max_delay_ms
                        ),
                    };
                }

                // Schedule this frame after the delay
                let new_busy_until = busy_until + self.config.margin() + frame_time;
                *state = PaceState::Busy {
                    busy_until: new_busy_until,
                };
                self.stats.delayed_frames.fetch_add(1, Ordering::Relaxed);
                delay
            }
            PaceState::Idle { idle_until } => {
                // Bus is in cooldown — delay until cooldown expires
                let delay = idle_until.saturating_duration_since(now);

                // Check max delay
                if self.config.max_delay_ms > 0 && delay > self.config.max_delay() {
                    self.stats.dropped_frames.fetch_add(1, Ordering::Relaxed);
                    return FilterResult::Dropped {
                        reason: format!(
                            "PaceFilter: idle delay {}ms exceeds max_delay_ms {}",
                            delay.as_millis(),
                            self.config.max_delay_ms
                        ),
                    };
                }

                // Start new busy period after cooldown
                let busy_until = idle_until + frame_time;
                *state = PaceState::Busy { busy_until };
                self.stats.delayed_frames.fetch_add(1, Ordering::Relaxed);
                delay
            }
        };

        if delay > Duration::ZERO {
            self.stats
                .total_delay_us
                .fetch_add(delay.as_micros() as u64, Ordering::Relaxed);

            trace!(
                delay_ms = delay.as_millis(),
                frame_size = envelope.frame_size_bytes,
                state = %*state,
                "PaceFilter: frame delayed"
            );
        }

        FilterResult::pass_with_delay(delay)
    }

    /// Process a frame in the recv direction.
    ///
    /// For received frames, the PaceFilter simply records timing
    /// information without imposing delays.
    pub fn process_recv(&self, _envelope: &FrameEnvelope) -> FilterResult {
        // Receive path: no delay applied, just pass through.
        // The recv path timing is handled by the sender's PaceFilter.
        FilterResult::pass()
    }

    /// Notify that a frame transmission has completed on the bus.
    ///
    /// This triggers the transition from Busy → Idle if the state
    /// machine hasn't already auto-transitioned.
    pub fn on_frame_completed(&self) {
        if !self.config.enabled {
            return;
        }

        let mut state = self.state.write();
        if let PaceState::Busy { busy_until } = *state {
            let now = Instant::now();
            if now >= busy_until {
                let idle_until = now + self.config.margin();
                *state = PaceState::Idle { idle_until };
                self.stats.busy_to_idle.fetch_add(1, Ordering::Relaxed);
            }
            // If still busy, let the auto-transition handle it
        }
    }

    /// Force reset to P_DOWN state.
    pub fn reset(&self) {
        let mut state = self.state.write();
        *state = PaceState::Down;
    }

    /// Get a snapshot of the statistics.
    pub fn stats_snapshot(&self) -> PaceFilterStatsSnapshot {
        PaceFilterStatsSnapshot {
            immediate_pass: self.stats.immediate_pass.load(Ordering::Relaxed),
            delayed_frames: self.stats.delayed_frames.load(Ordering::Relaxed),
            dropped_frames: self.stats.dropped_frames.load(Ordering::Relaxed),
            total_delay_us: self.stats.total_delay_us.load(Ordering::Relaxed),
            busy_to_idle: self.stats.busy_to_idle.load(Ordering::Relaxed),
            idle_to_down: self.stats.idle_to_down.load(Ordering::Relaxed),
        }
    }
}

impl fmt::Debug for PaceFilter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PaceFilter")
            .field("enabled", &self.config.enabled)
            .field("state", &*self.state.read())
            .field("margin_ms", &self.config.margin_ms)
            .finish()
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::address::{GroupAddress, IndividualAddress};
    use crate::cemi::CemiFrame;

    fn make_envelope() -> FrameEnvelope {
        let cemi = CemiFrame::group_value_write(
            IndividualAddress::new(1, 1, 1),
            GroupAddress::three_level(1, 0, 1),
            vec![0x01],
        );
        FrameEnvelope::new(cemi, 1, "192.168.1.100:3671".parse().unwrap())
    }

    #[test]
    fn test_pace_state_names() {
        assert_eq!(PaceState::Down.name(), "P_DOWN");
        assert_eq!(
            PaceState::Busy {
                busy_until: Instant::now()
            }
            .name(),
            "P_BUSY"
        );
        assert_eq!(
            PaceState::Idle {
                idle_until: Instant::now()
            }
            .name(),
            "P_IDLE"
        );
    }

    #[test]
    fn test_pace_state_availability() {
        assert!(PaceState::Down.is_available());

        // Busy state in the past → available
        let past = Instant::now() - Duration::from_secs(1);
        assert!(PaceState::Busy { busy_until: past }.is_available());

        // Busy state in the future → not available
        let future = Instant::now() + Duration::from_secs(1);
        assert!(!PaceState::Busy { busy_until: future }.is_available());

        // Idle state in the past → available
        assert!(PaceState::Idle { idle_until: past }.is_available());

        // Idle state in the future → not available
        assert!(!PaceState::Idle { idle_until: future }.is_available());
    }

    #[test]
    fn test_pace_state_remaining_delay() {
        assert_eq!(PaceState::Down.remaining_delay(), Duration::ZERO);

        let past = Instant::now() - Duration::from_secs(1);
        assert_eq!(
            PaceState::Busy { busy_until: past }.remaining_delay(),
            Duration::ZERO
        );
    }

    #[test]
    fn test_config_defaults() {
        let config = PaceFilterConfig::default();
        assert!(config.enabled);
        assert_eq!(config.margin_ms, 50);
        assert_eq!(config.byte_time_override_us, 0);
        assert_eq!(config.max_delay_ms, 5000);
        assert!(config.auto_transition);
    }

    #[test]
    fn test_config_byte_time() {
        let config = PaceFilterConfig::default();
        assert_eq!(config.byte_time_us(), KNX_TP1_BYTE_TIME_US);

        let mut config = PaceFilterConfig::default();
        config.byte_time_override_us = 2000;
        assert_eq!(config.byte_time_us(), 2000);
    }

    #[test]
    fn test_config_frame_time() {
        let config = PaceFilterConfig::default();
        // 10 bytes * 1146us = 11460us = 11.46ms
        let frame_time = config.frame_time(10);
        assert_eq!(frame_time.as_micros(), 10 * KNX_TP1_BYTE_TIME_US as u128);
    }

    #[test]
    fn test_config_total_frame_delay() {
        let config = PaceFilterConfig::default();
        let total = config.total_frame_delay(10);
        let expected = config.frame_time(10) + config.margin();
        assert_eq!(total, expected);
    }

    #[test]
    fn test_config_validate() {
        let config = PaceFilterConfig::default();
        assert!(config.validate().is_ok());

        let mut bad_config = PaceFilterConfig::default();
        bad_config.margin_ms = 20000;
        assert!(bad_config.validate().is_err());
    }

    #[test]
    fn test_pace_filter_disabled() {
        let mut config = PaceFilterConfig::default();
        config.enabled = false;
        let filter = PaceFilter::new(config);

        let envelope = make_envelope();
        let result = filter.process_send(&envelope);
        assert!(matches!(result, FilterResult::Pass { delay } if delay == Duration::ZERO));
    }

    #[test]
    fn test_pace_filter_first_frame_no_delay() {
        let config = PaceFilterConfig::default();
        let filter = PaceFilter::new(config);

        let envelope = make_envelope();
        let result = filter.process_send(&envelope);

        // First frame should pass with zero delay
        match result {
            FilterResult::Pass { delay } => {
                assert_eq!(delay, Duration::ZERO);
            }
            _ => panic!("Expected Pass result"),
        }

        // State should now be Busy
        let state = *filter.state.read();
        assert!(matches!(state, PaceState::Busy { .. }));

        let stats = filter.stats_snapshot();
        assert_eq!(stats.immediate_pass, 1);
    }

    #[test]
    fn test_pace_filter_second_frame_delayed() {
        let config = PaceFilterConfig::default();
        let filter = PaceFilter::new(config.clone());

        // First frame — no delay
        let env1 = make_envelope();
        let result1 = filter.process_send(&env1);
        assert!(matches!(result1, FilterResult::Pass { delay } if delay == Duration::ZERO));

        // Second frame — should be delayed
        let env2 = make_envelope();
        let result2 = filter.process_send(&env2);
        match result2 {
            FilterResult::Pass { delay } => {
                // Delay should be > 0 (at least margin)
                assert!(delay > Duration::ZERO);
            }
            _ => panic!("Expected Pass result with delay"),
        }

        let stats = filter.stats_snapshot();
        assert_eq!(stats.immediate_pass, 1);
        assert_eq!(stats.delayed_frames, 1);
    }

    #[test]
    fn test_pace_filter_recv_passthrough() {
        let config = PaceFilterConfig::default();
        let filter = PaceFilter::new(config);

        let envelope = make_envelope();
        let result = filter.process_recv(&envelope);
        assert!(matches!(result, FilterResult::Pass { delay } if delay == Duration::ZERO));
    }

    #[test]
    fn test_pace_filter_on_frame_completed() {
        let config = PaceFilterConfig::default();
        let filter = PaceFilter::new(config);

        // Put into busy state with expired time
        {
            let mut state = filter.state.write();
            *state = PaceState::Busy {
                busy_until: Instant::now() - Duration::from_millis(1),
            };
        }

        filter.on_frame_completed();

        let state = *filter.state.read();
        assert!(matches!(state, PaceState::Idle { .. }));
    }

    #[test]
    fn test_pace_filter_reset() {
        let config = PaceFilterConfig::default();
        let filter = PaceFilter::new(config);

        // Send a frame to move to Busy
        let envelope = make_envelope();
        filter.process_send(&envelope);

        // Reset should go back to Down
        filter.reset();
        assert!(matches!(*filter.state.read(), PaceState::Down));
    }

    #[test]
    fn test_pace_filter_max_delay_drop() {
        let mut config = PaceFilterConfig::default();
        config.max_delay_ms = 1; // Very low max delay
        let filter = PaceFilter::new(config);

        // First frame — no delay
        let env1 = make_envelope();
        filter.process_send(&env1);

        // Force a very long busy period
        {
            let mut state = filter.state.write();
            *state = PaceState::Busy {
                busy_until: Instant::now() + Duration::from_secs(10),
            };
        }

        // Second frame should be dropped
        let env2 = make_envelope();
        let result = filter.process_send(&env2);
        assert!(matches!(result, FilterResult::Dropped { .. }));

        let stats = filter.stats_snapshot();
        assert_eq!(stats.dropped_frames, 1);
    }

    #[test]
    fn test_pace_filter_debug() {
        let config = PaceFilterConfig::default();
        let filter = PaceFilter::new(config);
        let debug_str = format!("{:?}", filter);
        assert!(debug_str.contains("PaceFilter"));
        assert!(debug_str.contains("Down"));
    }

    #[test]
    fn test_pace_state_display() {
        let state = PaceState::Down;
        assert_eq!(state.to_string(), "P_DOWN");

        let busy = PaceState::Busy {
            busy_until: Instant::now() + Duration::from_millis(100),
        };
        let s = busy.to_string();
        assert!(s.starts_with("P_BUSY("));

        let idle = PaceState::Idle {
            idle_until: Instant::now() + Duration::from_millis(50),
        };
        let s = idle.to_string();
        assert!(s.starts_with("P_IDLE("));
    }

    #[test]
    fn test_knx_tp1_byte_time_constant() {
        // Verify: 1_000_000 * 11 / 9600 = 1145.83... → 1145 (integer division)
        assert_eq!(KNX_TP1_BYTE_TIME_US, 1145);
    }
}

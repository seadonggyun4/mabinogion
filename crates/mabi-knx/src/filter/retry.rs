//! RetryFilter with 3-state CircuitBreaker.
//!
//! The RetryFilter provides retry logic with circuit breaker protection
//! for frame transmission. This prevents the server from overwhelming
//! a failing connection with repeated attempts.
//!
//! ## Circuit Breaker States
//!
//! ```text
//! ┌────────┐  failure_count >= threshold  ┌────────┐  recovery_timeout  ┌──────────┐
//! │ Closed │ ──────────────────────────→  │  Open  │ ─────────────────→ │ HalfOpen │
//! │ (pass) │                              │ (drop) │                    │ (probe)  │
//! └────────┘                              └────────┘                    └────┬─────┘
//!      ↑                                       ↑                            │
//!      │         success                       │       failure              │
//!      └───────────────────────────────────────┘←──────────────────────────┘
//!      │                                        (back to Open)
//!      └── success in HalfOpen ──→ reset to Closed
//! ```
//!
//! - **Closed**: Normal operation. Frames pass through. Failures are counted.
//! - **Open**: Circuit tripped. All frames are dropped immediately.
//!   Transitions to HalfOpen after `recovery_timeout`.
//! - **HalfOpen**: Probe state. One frame is allowed through. If it succeeds,
//!   the circuit closes. If it fails, the circuit opens again.
//!
//! ## Retry Logic
//!
//! The RetryFilter tracks retry counts per frame. When a frame fails:
//! 1. If retry_count < max_retries, the frame can be re-sent
//! 2. Each failure increments the circuit breaker's failure counter
//! 3. When failure_count >= failure_threshold, the circuit opens

use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use tracing::{debug, info, trace, warn};

use super::chain::{FilterResult, FrameEnvelope};

// ============================================================================
// Circuit Breaker State
// ============================================================================

/// 3-state circuit breaker state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CircuitBreakerState {
    /// Normal operation. Frames pass through.
    Closed,
    /// Circuit tripped. All frames are dropped.
    /// Contains the timestamp when the circuit opened.
    Open {
        /// When the circuit was tripped.
        opened_at: Instant,
    },
    /// Probe state. One frame is allowed through to test recovery.
    HalfOpen,
}

impl CircuitBreakerState {
    /// Short name for logging.
    pub fn name(&self) -> &'static str {
        match self {
            Self::Closed => "Closed",
            Self::Open { .. } => "Open",
            Self::HalfOpen => "HalfOpen",
        }
    }

    /// Whether the circuit allows frames to pass.
    pub fn allows_traffic(&self) -> bool {
        matches!(self, Self::Closed | Self::HalfOpen)
    }
}

impl fmt::Display for CircuitBreakerState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Closed => write!(f, "Closed"),
            Self::Open { opened_at } => {
                let elapsed = opened_at.elapsed();
                write!(f, "Open({}ms ago)", elapsed.as_millis())
            }
            Self::HalfOpen => write!(f, "HalfOpen"),
        }
    }
}

// ============================================================================
// RetryFilter Configuration
// ============================================================================

/// RetryFilter configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetryFilterConfig {
    /// Whether the RetryFilter is enabled.
    #[serde(default = "default_true")]
    pub enabled: bool,

    /// Maximum retry attempts per frame before giving up.
    #[serde(default = "default_max_retries")]
    pub max_retries: u8,

    /// Delay between retry attempts in milliseconds.
    /// Uses exponential backoff: retry_delay * 2^attempt.
    #[serde(default = "default_retry_delay_ms")]
    pub retry_delay_ms: u64,

    /// Whether to use exponential backoff for retries.
    #[serde(default = "default_true")]
    pub exponential_backoff: bool,

    /// Maximum retry delay (cap for exponential backoff) in milliseconds.
    #[serde(default = "default_max_retry_delay_ms")]
    pub max_retry_delay_ms: u64,

    /// Number of consecutive failures before the circuit opens.
    #[serde(default = "default_failure_threshold")]
    pub failure_threshold: u32,

    /// Time to wait before transitioning from Open → HalfOpen in milliseconds.
    #[serde(default = "default_recovery_timeout_ms")]
    pub recovery_timeout_ms: u64,

    /// Number of successful probes in HalfOpen before closing the circuit.
    #[serde(default = "default_success_threshold")]
    pub success_threshold: u32,
}

fn default_true() -> bool {
    true
}

fn default_max_retries() -> u8 {
    3
}

fn default_retry_delay_ms() -> u64 {
    100
}

fn default_max_retry_delay_ms() -> u64 {
    5000
}

fn default_failure_threshold() -> u32 {
    5
}

fn default_recovery_timeout_ms() -> u64 {
    10000
}

fn default_success_threshold() -> u32 {
    1
}

impl Default for RetryFilterConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            max_retries: default_max_retries(),
            retry_delay_ms: default_retry_delay_ms(),
            exponential_backoff: true,
            max_retry_delay_ms: default_max_retry_delay_ms(),
            failure_threshold: default_failure_threshold(),
            recovery_timeout_ms: default_recovery_timeout_ms(),
            success_threshold: default_success_threshold(),
        }
    }
}

impl RetryFilterConfig {
    /// Get the retry delay for a given attempt number.
    pub fn retry_delay(&self, attempt: u8) -> Duration {
        let base = self.retry_delay_ms;
        let delay = if self.exponential_backoff {
            base.saturating_mul(1u64 << attempt.min(10))
        } else {
            base
        };
        let capped = delay.min(self.max_retry_delay_ms);
        Duration::from_millis(capped)
    }

    /// Get the recovery timeout as Duration.
    pub fn recovery_timeout(&self) -> Duration {
        Duration::from_millis(self.recovery_timeout_ms)
    }

    /// Validate the configuration.
    pub fn validate(&self) -> Result<(), String> {
        if self.failure_threshold == 0 {
            return Err("RetryFilter failure_threshold must be > 0".to_string());
        }
        if self.recovery_timeout_ms == 0 {
            return Err("RetryFilter recovery_timeout_ms must be > 0".to_string());
        }
        if self.success_threshold == 0 {
            return Err("RetryFilter success_threshold must be > 0".to_string());
        }
        Ok(())
    }
}

// ============================================================================
// RetryFilter Statistics
// ============================================================================

/// Lock-free RetryFilter statistics.
#[derive(Debug, Default)]
pub struct RetryFilterStats {
    /// Frames that passed through without circuit breaker interference.
    pub direct_pass: AtomicU64,
    /// Frames dropped because the circuit is open.
    pub circuit_open_drops: AtomicU64,
    /// Frames allowed through in HalfOpen state (probes).
    pub probe_frames: AtomicU64,
    /// Total retry attempts.
    pub retry_attempts: AtomicU64,
    /// Successful deliveries.
    pub successes: AtomicU64,
    /// Failed deliveries.
    pub failures: AtomicU64,
    /// Circuit state transitions (Closed→Open, Open→HalfOpen, etc.)
    pub state_transitions: AtomicU64,
    /// Number of times circuit was tripped (Closed → Open).
    pub circuit_trips: AtomicU64,
    /// Number of times circuit was reset (HalfOpen → Closed).
    pub circuit_resets: AtomicU64,
}

/// Snapshot of RetryFilter statistics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetryFilterStatsSnapshot {
    pub direct_pass: u64,
    pub circuit_open_drops: u64,
    pub probe_frames: u64,
    pub retry_attempts: u64,
    pub successes: u64,
    pub failures: u64,
    pub state_transitions: u64,
    pub circuit_trips: u64,
    pub circuit_resets: u64,
}

// ============================================================================
// RetryFilter
// ============================================================================

/// Retry filter with 3-state circuit breaker protection.
///
/// The RetryFilter manages retry logic for failed frame transmissions
/// and protects the system with a circuit breaker that trips after
/// too many consecutive failures.
pub struct RetryFilter {
    config: RetryFilterConfig,
    /// Current circuit breaker state.
    circuit_state: RwLock<CircuitBreakerState>,
    /// Consecutive failure count.
    failure_count: RwLock<u32>,
    /// Consecutive success count (used in HalfOpen).
    success_count: RwLock<u32>,
    /// Statistics.
    stats: RetryFilterStats,
}

impl RetryFilter {
    /// Create a new RetryFilter with the given configuration.
    pub fn new(config: RetryFilterConfig) -> Self {
        Self {
            config,
            circuit_state: RwLock::new(CircuitBreakerState::Closed),
            failure_count: RwLock::new(0),
            success_count: RwLock::new(0),
            stats: RetryFilterStats::default(),
        }
    }

    /// Get the current circuit breaker state.
    ///
    /// This also handles the auto-transition from Open → HalfOpen
    /// when the recovery timeout has elapsed.
    pub fn circuit_state(&self) -> CircuitBreakerState {
        let state = *self.circuit_state.read();

        // Auto-transition: Open → HalfOpen after recovery_timeout
        if let CircuitBreakerState::Open { opened_at } = state {
            if opened_at.elapsed() >= self.config.recovery_timeout() {
                let mut state_w = self.circuit_state.write();
                if let CircuitBreakerState::Open { opened_at: oa } = *state_w {
                    if oa.elapsed() >= self.config.recovery_timeout() {
                        *state_w = CircuitBreakerState::HalfOpen;
                        *self.success_count.write() = 0;
                        self.stats.state_transitions.fetch_add(1, Ordering::Relaxed);

                        info!(
                            recovery_timeout_ms = self.config.recovery_timeout_ms,
                            "CircuitBreaker: Open → HalfOpen (recovery timeout elapsed)"
                        );

                        return CircuitBreakerState::HalfOpen;
                    }
                }
            }
        }

        state
    }

    /// Process a frame in the send direction.
    ///
    /// Checks the circuit breaker state and either:
    /// - Passes the frame (Closed/HalfOpen)
    /// - Drops the frame (Open)
    /// - Adds retry delay if this is a retry attempt
    pub fn process_send(&self, envelope: &FrameEnvelope) -> FilterResult {
        if !self.config.enabled {
            return FilterResult::pass();
        }

        let state = self.circuit_state();

        match state {
            CircuitBreakerState::Closed => {
                // Normal operation — pass through
                let delay = if envelope.retry_count > 0 {
                    let retry_delay = self.config.retry_delay(envelope.retry_count - 1);
                    self.stats.retry_attempts.fetch_add(1, Ordering::Relaxed);

                    trace!(
                        retry_count = envelope.retry_count,
                        delay_ms = retry_delay.as_millis(),
                        "RetryFilter: retry with delay"
                    );

                    retry_delay
                } else {
                    Duration::ZERO
                };

                self.stats.direct_pass.fetch_add(1, Ordering::Relaxed);
                FilterResult::pass_with_delay(delay)
            }
            CircuitBreakerState::Open { .. } => {
                // Circuit is open — drop frame
                self.stats
                    .circuit_open_drops
                    .fetch_add(1, Ordering::Relaxed);

                debug!(
                    channel_id = envelope.channel_id,
                    "RetryFilter: circuit breaker OPEN, dropping frame"
                );

                FilterResult::Dropped {
                    reason: format!(
                        "CircuitBreaker: circuit is Open (failures >= {})",
                        self.config.failure_threshold
                    ),
                }
            }
            CircuitBreakerState::HalfOpen => {
                // Probe mode — allow one frame through
                self.stats.probe_frames.fetch_add(1, Ordering::Relaxed);

                debug!(
                    channel_id = envelope.channel_id,
                    "RetryFilter: circuit breaker HalfOpen, allowing probe frame"
                );

                FilterResult::pass()
            }
        }
    }

    /// Process a frame in the recv direction.
    ///
    /// No filtering on receive — just pass through.
    pub fn process_recv(&self, _envelope: &FrameEnvelope) -> FilterResult {
        FilterResult::pass()
    }

    /// Record a successful frame transmission.
    ///
    /// In Closed state: resets the failure counter.
    /// In HalfOpen state: increments success counter and may close the circuit.
    pub fn on_success(&self) {
        if !self.config.enabled {
            return;
        }

        self.stats.successes.fetch_add(1, Ordering::Relaxed);

        let mut state = self.circuit_state.write();

        match *state {
            CircuitBreakerState::Closed => {
                // Reset failure count on success
                *self.failure_count.write() = 0;
            }
            CircuitBreakerState::HalfOpen => {
                let mut sc = self.success_count.write();
                *sc += 1;

                if *sc >= self.config.success_threshold {
                    // Enough successes — close the circuit
                    *state = CircuitBreakerState::Closed;
                    *self.failure_count.write() = 0;
                    *sc = 0;
                    self.stats.state_transitions.fetch_add(1, Ordering::Relaxed);
                    self.stats.circuit_resets.fetch_add(1, Ordering::Relaxed);

                    info!(
                        success_threshold = self.config.success_threshold,
                        "CircuitBreaker: HalfOpen → Closed (recovery successful)"
                    );
                }
            }
            CircuitBreakerState::Open { .. } => {
                // Shouldn't happen (frames are dropped when Open), but handle gracefully
            }
        }
    }

    /// Record a failed frame transmission.
    ///
    /// In Closed state: increments failure counter and may trip the circuit.
    /// In HalfOpen state: immediately opens the circuit again.
    pub fn on_failure(&self) {
        if !self.config.enabled {
            return;
        }

        self.stats.failures.fetch_add(1, Ordering::Relaxed);

        let mut state = self.circuit_state.write();

        match *state {
            CircuitBreakerState::Closed => {
                let mut fc = self.failure_count.write();
                *fc += 1;

                if *fc >= self.config.failure_threshold {
                    // Trip the circuit
                    *state = CircuitBreakerState::Open {
                        opened_at: Instant::now(),
                    };
                    self.stats.state_transitions.fetch_add(1, Ordering::Relaxed);
                    self.stats.circuit_trips.fetch_add(1, Ordering::Relaxed);

                    warn!(
                        failure_count = *fc,
                        threshold = self.config.failure_threshold,
                        recovery_timeout_ms = self.config.recovery_timeout_ms,
                        "CircuitBreaker: Closed → Open (failure threshold reached)"
                    );
                }
            }
            CircuitBreakerState::HalfOpen => {
                // Probe failed — back to Open
                *state = CircuitBreakerState::Open {
                    opened_at: Instant::now(),
                };
                *self.success_count.write() = 0;
                self.stats.state_transitions.fetch_add(1, Ordering::Relaxed);
                self.stats.circuit_trips.fetch_add(1, Ordering::Relaxed);

                warn!("CircuitBreaker: HalfOpen → Open (probe failed)");
            }
            CircuitBreakerState::Open { .. } => {
                // Already open — just update failure count for stats
                *self.failure_count.write() += 1;
            }
        }
    }

    /// Check if a frame can be retried.
    pub fn can_retry(&self, envelope: &FrameEnvelope) -> bool {
        if !self.config.enabled {
            return false;
        }
        envelope.retry_count < self.config.max_retries && self.circuit_state().allows_traffic()
    }

    /// Get the retry delay for the current retry attempt.
    pub fn retry_delay(&self, attempt: u8) -> Duration {
        self.config.retry_delay(attempt)
    }

    /// Get the current failure count.
    pub fn failure_count(&self) -> u32 {
        *self.failure_count.read()
    }

    /// Get the current success count (relevant in HalfOpen).
    pub fn success_count(&self) -> u32 {
        *self.success_count.read()
    }

    /// Force the circuit to a specific state (for testing).
    pub fn force_state(&self, state: CircuitBreakerState) {
        *self.circuit_state.write() = state;
    }

    /// Reset the circuit breaker to Closed with zeroed counters.
    pub fn reset(&self) {
        *self.circuit_state.write() = CircuitBreakerState::Closed;
        *self.failure_count.write() = 0;
        *self.success_count.write() = 0;
    }

    /// Get a snapshot of the statistics.
    pub fn stats_snapshot(&self) -> RetryFilterStatsSnapshot {
        RetryFilterStatsSnapshot {
            direct_pass: self.stats.direct_pass.load(Ordering::Relaxed),
            circuit_open_drops: self.stats.circuit_open_drops.load(Ordering::Relaxed),
            probe_frames: self.stats.probe_frames.load(Ordering::Relaxed),
            retry_attempts: self.stats.retry_attempts.load(Ordering::Relaxed),
            successes: self.stats.successes.load(Ordering::Relaxed),
            failures: self.stats.failures.load(Ordering::Relaxed),
            state_transitions: self.stats.state_transitions.load(Ordering::Relaxed),
            circuit_trips: self.stats.circuit_trips.load(Ordering::Relaxed),
            circuit_resets: self.stats.circuit_resets.load(Ordering::Relaxed),
        }
    }
}

impl fmt::Debug for RetryFilter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RetryFilter")
            .field("enabled", &self.config.enabled)
            .field("circuit_state", &*self.circuit_state.read())
            .field("failure_count", &*self.failure_count.read())
            .field("success_count", &*self.success_count.read())
            .field("max_retries", &self.config.max_retries)
            .field("failure_threshold", &self.config.failure_threshold)
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

    fn make_retry_envelope(retry_count: u8) -> FrameEnvelope {
        let mut env = make_envelope();
        env.retry_count = retry_count;
        env
    }

    #[test]
    fn test_circuit_breaker_state_names() {
        assert_eq!(CircuitBreakerState::Closed.name(), "Closed");
        assert_eq!(
            CircuitBreakerState::Open {
                opened_at: Instant::now()
            }
            .name(),
            "Open"
        );
        assert_eq!(CircuitBreakerState::HalfOpen.name(), "HalfOpen");
    }

    #[test]
    fn test_circuit_breaker_allows_traffic() {
        assert!(CircuitBreakerState::Closed.allows_traffic());
        assert!(CircuitBreakerState::HalfOpen.allows_traffic());
        assert!(!CircuitBreakerState::Open {
            opened_at: Instant::now()
        }
        .allows_traffic());
    }

    #[test]
    fn test_circuit_breaker_display() {
        let s = CircuitBreakerState::Closed.to_string();
        assert_eq!(s, "Closed");

        let s = CircuitBreakerState::HalfOpen.to_string();
        assert_eq!(s, "HalfOpen");

        let s = CircuitBreakerState::Open {
            opened_at: Instant::now(),
        }
        .to_string();
        assert!(s.starts_with("Open("));
    }

    #[test]
    fn test_config_defaults() {
        let config = RetryFilterConfig::default();
        assert!(config.enabled);
        assert_eq!(config.max_retries, 3);
        assert_eq!(config.retry_delay_ms, 100);
        assert!(config.exponential_backoff);
        assert_eq!(config.max_retry_delay_ms, 5000);
        assert_eq!(config.failure_threshold, 5);
        assert_eq!(config.recovery_timeout_ms, 10000);
        assert_eq!(config.success_threshold, 1);
    }

    #[test]
    fn test_config_retry_delay_constant() {
        let mut config = RetryFilterConfig::default();
        config.exponential_backoff = false;
        config.retry_delay_ms = 100;

        assert_eq!(config.retry_delay(0), Duration::from_millis(100));
        assert_eq!(config.retry_delay(1), Duration::from_millis(100));
        assert_eq!(config.retry_delay(5), Duration::from_millis(100));
    }

    #[test]
    fn test_config_retry_delay_exponential() {
        let config = RetryFilterConfig::default();
        // retry_delay_ms = 100, exponential
        assert_eq!(config.retry_delay(0), Duration::from_millis(100)); // 100 * 2^0
        assert_eq!(config.retry_delay(1), Duration::from_millis(200)); // 100 * 2^1
        assert_eq!(config.retry_delay(2), Duration::from_millis(400)); // 100 * 2^2
        assert_eq!(config.retry_delay(3), Duration::from_millis(800)); // 100 * 2^3
    }

    #[test]
    fn test_config_retry_delay_capped() {
        let mut config = RetryFilterConfig::default();
        config.max_retry_delay_ms = 500;

        assert_eq!(config.retry_delay(0), Duration::from_millis(100));
        assert_eq!(config.retry_delay(1), Duration::from_millis(200));
        assert_eq!(config.retry_delay(2), Duration::from_millis(400));
        assert_eq!(config.retry_delay(3), Duration::from_millis(500)); // Capped
        assert_eq!(config.retry_delay(10), Duration::from_millis(500)); // Capped
    }

    #[test]
    fn test_config_validate() {
        let config = RetryFilterConfig::default();
        assert!(config.validate().is_ok());

        let mut bad = RetryFilterConfig::default();
        bad.failure_threshold = 0;
        assert!(bad.validate().is_err());

        let mut bad = RetryFilterConfig::default();
        bad.recovery_timeout_ms = 0;
        assert!(bad.validate().is_err());

        let mut bad = RetryFilterConfig::default();
        bad.success_threshold = 0;
        assert!(bad.validate().is_err());
    }

    #[test]
    fn test_retry_filter_disabled() {
        let mut config = RetryFilterConfig::default();
        config.enabled = false;
        let filter = RetryFilter::new(config);

        let envelope = make_envelope();
        let result = filter.process_send(&envelope);
        assert!(result.should_continue());
    }

    #[test]
    fn test_retry_filter_closed_passthrough() {
        let config = RetryFilterConfig::default();
        let filter = RetryFilter::new(config);

        let envelope = make_envelope();
        let result = filter.process_send(&envelope);
        assert!(matches!(
            result,
            FilterResult::Pass {
                delay
            } if delay == Duration::ZERO
        ));

        let stats = filter.stats_snapshot();
        assert_eq!(stats.direct_pass, 1);
    }

    #[test]
    fn test_retry_filter_retry_with_delay() {
        let config = RetryFilterConfig::default();
        let filter = RetryFilter::new(config);

        let envelope = make_retry_envelope(2);
        let result = filter.process_send(&envelope);

        match result {
            FilterResult::Pass { delay } => {
                // retry_count=2, so delay = retry_delay(1) = 200ms
                assert_eq!(delay, Duration::from_millis(200));
            }
            _ => panic!("Expected Pass with delay"),
        }

        let stats = filter.stats_snapshot();
        assert_eq!(stats.retry_attempts, 1);
    }

    #[test]
    fn test_circuit_breaker_trip() {
        let mut config = RetryFilterConfig::default();
        config.failure_threshold = 3;
        let filter = RetryFilter::new(config);

        // Record failures
        filter.on_failure();
        assert!(matches!(
            filter.circuit_state(),
            CircuitBreakerState::Closed
        ));
        filter.on_failure();
        assert!(matches!(
            filter.circuit_state(),
            CircuitBreakerState::Closed
        ));
        filter.on_failure();
        // 3 failures = threshold — circuit should be Open
        assert!(matches!(
            filter.circuit_state(),
            CircuitBreakerState::Open { .. }
        ));

        let stats = filter.stats_snapshot();
        assert_eq!(stats.failures, 3);
        assert_eq!(stats.circuit_trips, 1);
    }

    #[test]
    fn test_circuit_breaker_open_drops_frames() {
        let mut config = RetryFilterConfig::default();
        config.failure_threshold = 1;
        let filter = RetryFilter::new(config);

        filter.on_failure(); // Trip immediately
        assert!(matches!(
            filter.circuit_state(),
            CircuitBreakerState::Open { .. }
        ));

        let envelope = make_envelope();
        let result = filter.process_send(&envelope);
        assert!(matches!(result, FilterResult::Dropped { .. }));

        let stats = filter.stats_snapshot();
        assert_eq!(stats.circuit_open_drops, 1);
    }

    #[test]
    fn test_circuit_breaker_halfopen_recovery() {
        let mut config = RetryFilterConfig::default();
        config.failure_threshold = 1;
        config.recovery_timeout_ms = 1; // Very short for testing
        config.success_threshold = 1;
        let filter = RetryFilter::new(config);

        // Trip the circuit
        filter.on_failure();
        assert!(matches!(
            filter.circuit_state(),
            CircuitBreakerState::Open { .. }
        ));

        // Wait for recovery timeout
        std::thread::sleep(Duration::from_millis(5));

        // Should be HalfOpen now
        let state = filter.circuit_state();
        assert!(matches!(state, CircuitBreakerState::HalfOpen));

        // Probe should pass
        let envelope = make_envelope();
        let result = filter.process_send(&envelope);
        assert!(result.should_continue());

        let stats = filter.stats_snapshot();
        assert_eq!(stats.probe_frames, 1);

        // Success should close the circuit
        filter.on_success();
        assert!(matches!(
            filter.circuit_state(),
            CircuitBreakerState::Closed
        ));
        assert_eq!(filter.stats_snapshot().circuit_resets, 1);
    }

    #[test]
    fn test_circuit_breaker_halfopen_failure() {
        let mut config = RetryFilterConfig::default();
        config.failure_threshold = 1;
        config.recovery_timeout_ms = 1;
        let filter = RetryFilter::new(config);

        // Trip and wait for HalfOpen
        filter.on_failure();
        std::thread::sleep(Duration::from_millis(5));
        assert!(matches!(
            filter.circuit_state(),
            CircuitBreakerState::HalfOpen
        ));

        // Probe failure → back to Open
        filter.on_failure();
        assert!(matches!(
            filter.circuit_state(),
            CircuitBreakerState::Open { .. }
        ));
    }

    #[test]
    fn test_success_resets_failure_count() {
        let mut config = RetryFilterConfig::default();
        config.failure_threshold = 5;
        let filter = RetryFilter::new(config);

        filter.on_failure();
        filter.on_failure();
        assert_eq!(filter.failure_count(), 2);

        filter.on_success();
        assert_eq!(filter.failure_count(), 0);
    }

    #[test]
    fn test_can_retry() {
        let mut config = RetryFilterConfig::default();
        config.max_retries = 3;
        let filter = RetryFilter::new(config);

        let mut env = make_envelope();
        env.retry_count = 0;
        assert!(filter.can_retry(&env));

        env.retry_count = 2;
        assert!(filter.can_retry(&env));

        env.retry_count = 3;
        assert!(!filter.can_retry(&env));
    }

    #[test]
    fn test_can_retry_circuit_open() {
        let mut config = RetryFilterConfig::default();
        config.failure_threshold = 1;
        let filter = RetryFilter::new(config);

        filter.on_failure(); // Trip

        let env = make_envelope();
        assert!(!filter.can_retry(&env));
    }

    #[test]
    fn test_force_state() {
        let config = RetryFilterConfig::default();
        let filter = RetryFilter::new(config);

        filter.force_state(CircuitBreakerState::HalfOpen);
        assert!(matches!(
            filter.circuit_state(),
            CircuitBreakerState::HalfOpen
        ));

        filter.force_state(CircuitBreakerState::Closed);
        assert!(matches!(
            filter.circuit_state(),
            CircuitBreakerState::Closed
        ));
    }

    #[test]
    fn test_reset() {
        let mut config = RetryFilterConfig::default();
        config.failure_threshold = 1;
        let filter = RetryFilter::new(config);

        filter.on_failure(); // Trip
        assert!(matches!(
            filter.circuit_state(),
            CircuitBreakerState::Open { .. }
        ));

        filter.reset();
        assert!(matches!(
            filter.circuit_state(),
            CircuitBreakerState::Closed
        ));
        assert_eq!(filter.failure_count(), 0);
        assert_eq!(filter.success_count(), 0);
    }

    #[test]
    fn test_retry_filter_recv_passthrough() {
        let config = RetryFilterConfig::default();
        let filter = RetryFilter::new(config);

        let envelope = make_envelope();
        let result = filter.process_recv(&envelope);
        assert!(result.should_continue());
    }

    #[test]
    fn test_retry_filter_debug() {
        let config = RetryFilterConfig::default();
        let filter = RetryFilter::new(config);
        let debug_str = format!("{:?}", filter);
        assert!(debug_str.contains("RetryFilter"));
        assert!(debug_str.contains("Closed"));
    }

    #[test]
    fn test_retry_filter_stats_snapshot() {
        let config = RetryFilterConfig::default();
        let filter = RetryFilter::new(config);

        filter.on_success();
        filter.on_failure();

        let stats = filter.stats_snapshot();
        assert_eq!(stats.successes, 1);
        assert_eq!(stats.failures, 1);
    }
}

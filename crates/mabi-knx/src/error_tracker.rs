//! Send Error Tracker with consecutive and sliding window error rate monitoring.
//!
//! This module provides production-grade error tracking for KNXnet/IP frame
//! transmission, supporting two complementary monitoring strategies:
//!
//! ## Consecutive Error Tracking
//!
//! Tracks consecutive send failures. When the count reaches a configurable
//! threshold, a tunnel restart is triggered (matching knxd behavior where
//! 5 consecutive errors trigger reconnection).
//!
//! ## Sliding Window Error Rate
//!
//! Monitors the error rate over a configurable time window (default: 60 seconds).
//! This catches intermittent but persistent error patterns that wouldn't trigger
//! the consecutive threshold.
//!
//! ## Per-Channel Tracking
//!
//! Each tunnel connection (channel) has independent error tracking. This allows
//! the simulator to inject channel-specific failure patterns for testing.
//!
//! ## Integration
//!
//! The tracker is integrated into the server's send pipeline:
//! - `on_send_success(channel_id)` — resets consecutive counter
//! - `on_send_failure(channel_id, error)` — increments counters, checks thresholds
//! - `on_ack_error(channel_id, status)` — tracks ACK-level errors
//! - `on_confirmation_nack(channel_id)` — tracks L_Data.con failures

use std::collections::VecDeque;
use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use dashmap::DashMap;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};

// ============================================================================
// Error Event
// ============================================================================

/// A recorded error event with timestamp and classification.
#[derive(Debug, Clone)]
struct ErrorEvent {
    /// When the error occurred.
    timestamp: Instant,
    /// Error category — stored for per-category window analysis.
    #[allow(dead_code)]
    category: ErrorCategory,
}

/// Error category for classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ErrorCategory {
    /// UDP send failure (I/O error).
    SendFailure,
    /// ACK error (non-zero status in TUNNELLING_ACK).
    AckError,
    /// ACK timeout (no ACK received within timeout).
    AckTimeout,
    /// L_Data.con NACK (confirmation failure from bus).
    ConfirmationNack,
    /// L_Data.con timeout (no confirmation received).
    ConfirmationTimeout,
    /// Flow control drop (filter chain dropped the frame).
    FlowControlDrop,
}

impl ErrorCategory {
    /// Human-readable name.
    pub fn name(&self) -> &'static str {
        match self {
            Self::SendFailure => "SendFailure",
            Self::AckError => "AckError",
            Self::AckTimeout => "AckTimeout",
            Self::ConfirmationNack => "ConfirmationNack",
            Self::ConfirmationTimeout => "ConfirmationTimeout",
            Self::FlowControlDrop => "FlowControlDrop",
        }
    }

    /// All categories.
    pub fn all() -> &'static [ErrorCategory] {
        &[
            Self::SendFailure,
            Self::AckError,
            Self::AckTimeout,
            Self::ConfirmationNack,
            Self::ConfirmationTimeout,
            Self::FlowControlDrop,
        ]
    }
}

impl fmt::Display for ErrorCategory {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.name())
    }
}

// ============================================================================
// Channel Error State
// ============================================================================

/// Per-channel error tracking state.
struct ChannelErrorState {
    /// Consecutive error counter — resets on success.
    consecutive_errors: u32,
    /// Sliding window of recent error events.
    recent_errors: VecDeque<ErrorEvent>,
    /// Total successes.
    total_successes: u64,
    /// Total failures.
    total_failures: u64,
    /// Per-category failure counts.
    category_counts: [u64; 6],
    /// Timestamp of last successful send.
    last_success_at: Option<Instant>,
    /// Timestamp of last error.
    last_error_at: Option<Instant>,
    /// Whether this channel has been flagged for restart.
    restart_triggered: bool,
}

impl ChannelErrorState {
    fn new() -> Self {
        Self {
            consecutive_errors: 0,
            recent_errors: VecDeque::with_capacity(128),
            total_successes: 0,
            total_failures: 0,
            category_counts: [0; 6],
            last_success_at: None,
            last_error_at: None,
            restart_triggered: false,
        }
    }

    /// Record a successful send.
    fn record_success(&mut self) {
        self.consecutive_errors = 0;
        self.total_successes += 1;
        self.last_success_at = Some(Instant::now());
        self.restart_triggered = false;
    }

    /// Record a failed send.
    fn record_failure(&mut self, category: ErrorCategory) {
        self.consecutive_errors += 1;
        self.total_failures += 1;
        self.last_error_at = Some(Instant::now());
        self.category_counts[category as usize] += 1;
        self.recent_errors.push_back(ErrorEvent {
            timestamp: Instant::now(),
            category,
        });
    }

    /// Prune events older than the window duration.
    fn prune_old_events(&mut self, window: Duration) {
        let cutoff = Instant::now() - window;
        while let Some(front) = self.recent_errors.front() {
            if front.timestamp < cutoff {
                self.recent_errors.pop_front();
            } else {
                break;
            }
        }
    }

    /// Get the error count within the sliding window.
    fn window_error_count(&self, window: Duration) -> usize {
        let cutoff = Instant::now() - window;
        self.recent_errors
            .iter()
            .filter(|e| e.timestamp >= cutoff)
            .count()
    }

    /// Calculate error rate within the sliding window.
    /// Returns (errors_in_window, total_in_window, rate).
    fn window_error_rate(&self, window: Duration, total_sends_in_window: u64) -> f64 {
        if total_sends_in_window == 0 {
            return 0.0;
        }
        let errors = self.window_error_count(window) as f64;
        errors / total_sends_in_window as f64
    }

    /// Reset all state for this channel.
    fn reset(&mut self) {
        self.consecutive_errors = 0;
        self.recent_errors.clear();
        self.restart_triggered = false;
    }
}

// ============================================================================
// Tracker Configuration
// ============================================================================

/// SendErrorTracker configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SendErrorTrackerConfig {
    /// Whether the error tracker is enabled.
    #[serde(default = "default_true")]
    pub enabled: bool,

    /// Consecutive error threshold for tunnel restart.
    /// When consecutive errors reach this count, the tracker signals a restart.
    /// Default: 5 (matches knxd behavior).
    #[serde(default = "default_consecutive_threshold")]
    pub consecutive_threshold: u32,

    /// Sliding window duration in milliseconds.
    /// Errors older than this are pruned from the window.
    /// Default: 60000 (60 seconds).
    #[serde(default = "default_window_ms")]
    pub window_ms: u64,

    /// Error rate threshold within the sliding window (0.0 - 1.0).
    /// When the error rate exceeds this within the window, a warning is triggered.
    /// Default: 0.5 (50% error rate).
    #[serde(default = "default_rate_threshold")]
    pub rate_threshold: f64,

    /// Minimum number of events in the window before rate threshold applies.
    /// Prevents false positives when only a few events have occurred.
    /// Default: 10.
    #[serde(default = "default_min_events_for_rate")]
    pub min_events_for_rate: u64,
}

fn default_true() -> bool {
    true
}

fn default_consecutive_threshold() -> u32 {
    5
}

fn default_window_ms() -> u64 {
    60_000
}

fn default_rate_threshold() -> f64 {
    0.5
}

fn default_min_events_for_rate() -> u64 {
    10
}

impl Default for SendErrorTrackerConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            consecutive_threshold: default_consecutive_threshold(),
            window_ms: default_window_ms(),
            rate_threshold: default_rate_threshold(),
            min_events_for_rate: default_min_events_for_rate(),
        }
    }
}

impl SendErrorTrackerConfig {
    /// Get the sliding window duration.
    pub fn window(&self) -> Duration {
        Duration::from_millis(self.window_ms)
    }

    /// Validate the configuration.
    pub fn validate(&self) -> Result<(), String> {
        if self.consecutive_threshold == 0 {
            return Err("SendErrorTracker consecutive_threshold must be > 0".to_string());
        }
        if self.window_ms == 0 {
            return Err("SendErrorTracker window_ms must be > 0".to_string());
        }
        if self.rate_threshold < 0.0 || self.rate_threshold > 1.0 {
            return Err(format!(
                "SendErrorTracker rate_threshold must be 0.0-1.0, got {}",
                self.rate_threshold
            ));
        }
        Ok(())
    }
}

// ============================================================================
// Tracker Result
// ============================================================================

/// Result of error tracking evaluation.
///
/// Returned by `on_send_failure()` to indicate what action should be taken.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TrackingResult {
    /// Error recorded, no threshold exceeded.
    Recorded,
    /// Consecutive error threshold exceeded — tunnel restart recommended.
    ConsecutiveThresholdExceeded {
        consecutive_errors: u32,
        threshold: u32,
    },
    /// Sliding window error rate exceeded — warning state.
    RateThresholdExceeded {
        error_count: usize,
        window_ms: u64,
        rate: u32, // Rate as percentage (0-100)
    },
}

impl TrackingResult {
    /// Whether a tunnel restart is recommended.
    pub fn requires_restart(&self) -> bool {
        matches!(self, Self::ConsecutiveThresholdExceeded { .. })
    }

    /// Whether this is a warning state.
    pub fn is_warning(&self) -> bool {
        matches!(self, Self::RateThresholdExceeded { .. })
    }
}

// ============================================================================
// SendErrorTracker
// ============================================================================

/// Production-grade send error tracker with per-channel state.
///
/// Thread-safe — uses DashMap for per-channel state and atomics for global counters.
pub struct SendErrorTracker {
    /// Configuration.
    config: SendErrorTrackerConfig,
    /// Per-channel error state.
    channels: DashMap<u8, RwLock<ChannelErrorState>>,
    /// Global statistics.
    stats: TrackerStats,
}

/// Global tracker statistics.
pub struct TrackerStats {
    /// Total successes across all channels.
    pub total_successes: AtomicU64,
    /// Total failures across all channels.
    pub total_failures: AtomicU64,
    /// Number of times consecutive threshold was exceeded.
    pub consecutive_triggers: AtomicU64,
    /// Number of times rate threshold was exceeded.
    pub rate_triggers: AtomicU64,
}

impl TrackerStats {
    fn new() -> Self {
        Self {
            total_successes: AtomicU64::new(0),
            total_failures: AtomicU64::new(0),
            consecutive_triggers: AtomicU64::new(0),
            rate_triggers: AtomicU64::new(0),
        }
    }

    /// Take a snapshot.
    pub fn snapshot(&self) -> TrackerStatsSnapshot {
        TrackerStatsSnapshot {
            total_successes: self.total_successes.load(Ordering::Relaxed),
            total_failures: self.total_failures.load(Ordering::Relaxed),
            consecutive_triggers: self.consecutive_triggers.load(Ordering::Relaxed),
            rate_triggers: self.rate_triggers.load(Ordering::Relaxed),
        }
    }
}

/// Immutable snapshot of tracker statistics.
#[derive(Debug, Clone)]
pub struct TrackerStatsSnapshot {
    pub total_successes: u64,
    pub total_failures: u64,
    pub consecutive_triggers: u64,
    pub rate_triggers: u64,
}

impl TrackerStatsSnapshot {
    /// Overall error rate.
    pub fn error_rate(&self) -> f64 {
        let total = self.total_successes + self.total_failures;
        if total == 0 {
            return 0.0;
        }
        self.total_failures as f64 / total as f64
    }

    /// Total events.
    pub fn total_events(&self) -> u64 {
        self.total_successes + self.total_failures
    }
}

/// Per-channel error summary.
#[derive(Debug, Clone)]
pub struct ChannelErrorSummary {
    pub channel_id: u8,
    pub consecutive_errors: u32,
    pub total_successes: u64,
    pub total_failures: u64,
    pub window_error_count: usize,
    pub restart_triggered: bool,
    pub last_success_elapsed_ms: Option<u64>,
    pub last_error_elapsed_ms: Option<u64>,
    pub category_counts: [(ErrorCategory, u64); 6],
}

impl SendErrorTracker {
    /// Create a new tracker with the given configuration.
    pub fn new(config: SendErrorTrackerConfig) -> Self {
        Self {
            channels: DashMap::new(),
            config,
            stats: TrackerStats::new(),
        }
    }

    /// Create a tracker with default configuration.
    pub fn with_defaults() -> Self {
        Self::new(SendErrorTrackerConfig::default())
    }

    /// Check if the tracker is enabled.
    pub fn is_enabled(&self) -> bool {
        self.config.enabled
    }

    /// Get the configuration.
    pub fn config(&self) -> &SendErrorTrackerConfig {
        &self.config
    }

    /// Record a successful send for a channel.
    pub fn on_send_success(&self, channel_id: u8) {
        if !self.config.enabled {
            return;
        }

        self.ensure_channel(channel_id);

        if let Some(state_lock) = self.channels.get(&channel_id) {
            let mut state = state_lock.write();
            state.record_success();
        }

        self.stats.total_successes.fetch_add(1, Ordering::Relaxed);
    }

    /// Record a send failure for a channel.
    ///
    /// Returns a `TrackingResult` indicating whether any threshold was exceeded.
    pub fn on_send_failure(
        &self,
        channel_id: u8,
        category: ErrorCategory,
    ) -> TrackingResult {
        if !self.config.enabled {
            return TrackingResult::Recorded;
        }

        self.ensure_channel(channel_id);
        self.stats.total_failures.fetch_add(1, Ordering::Relaxed);

        let result = if let Some(state_lock) = self.channels.get(&channel_id) {
            let mut state = state_lock.write();
            state.record_failure(category);

            // Prune old events
            let window = self.config.window();
            state.prune_old_events(window);

            // Check consecutive threshold
            if state.consecutive_errors >= self.config.consecutive_threshold
                && !state.restart_triggered
            {
                state.restart_triggered = true;
                self.stats.consecutive_triggers.fetch_add(1, Ordering::Relaxed);
                return TrackingResult::ConsecutiveThresholdExceeded {
                    consecutive_errors: state.consecutive_errors,
                    threshold: self.config.consecutive_threshold,
                };
            }

            // Check sliding window rate threshold
            let total_in_window = state.total_successes + state.total_failures;
            if total_in_window >= self.config.min_events_for_rate {
                let error_count = state.window_error_count(window);
                let rate = state.window_error_rate(window, total_in_window);
                if rate > self.config.rate_threshold {
                    self.stats.rate_triggers.fetch_add(1, Ordering::Relaxed);
                    return TrackingResult::RateThresholdExceeded {
                        error_count,
                        window_ms: self.config.window_ms,
                        rate: (rate * 100.0) as u32,
                    };
                }
            }

            TrackingResult::Recorded
        } else {
            TrackingResult::Recorded
        };

        result
    }

    /// Record an ACK error.
    pub fn on_ack_error(&self, channel_id: u8, _status: u8) -> TrackingResult {
        self.on_send_failure(channel_id, ErrorCategory::AckError)
    }

    /// Record an ACK timeout.
    pub fn on_ack_timeout(&self, channel_id: u8) -> TrackingResult {
        self.on_send_failure(channel_id, ErrorCategory::AckTimeout)
    }

    /// Record a confirmation NACK.
    pub fn on_confirmation_nack(&self, channel_id: u8) -> TrackingResult {
        self.on_send_failure(channel_id, ErrorCategory::ConfirmationNack)
    }

    /// Record a confirmation timeout.
    pub fn on_confirmation_timeout(&self, channel_id: u8) -> TrackingResult {
        self.on_send_failure(channel_id, ErrorCategory::ConfirmationTimeout)
    }

    /// Record a flow control drop.
    pub fn on_flow_control_drop(&self, channel_id: u8) -> TrackingResult {
        self.on_send_failure(channel_id, ErrorCategory::FlowControlDrop)
    }

    /// Get the consecutive error count for a channel.
    pub fn consecutive_errors(&self, channel_id: u8) -> u32 {
        self.channels
            .get(&channel_id)
            .map(|s| s.read().consecutive_errors)
            .unwrap_or(0)
    }

    /// Get the error count in the current sliding window for a channel.
    pub fn window_error_count(&self, channel_id: u8) -> usize {
        let window = self.config.window();
        self.channels
            .get(&channel_id)
            .map(|s| s.read().window_error_count(window))
            .unwrap_or(0)
    }

    /// Check if a channel has triggered a restart.
    pub fn is_restart_triggered(&self, channel_id: u8) -> bool {
        self.channels
            .get(&channel_id)
            .map(|s| s.read().restart_triggered)
            .unwrap_or(false)
    }

    /// Reset error state for a channel.
    pub fn reset_channel(&self, channel_id: u8) {
        if let Some(state_lock) = self.channels.get(&channel_id) {
            state_lock.write().reset();
        }
    }

    /// Remove a channel's tracking state entirely.
    pub fn remove_channel(&self, channel_id: u8) {
        self.channels.remove(&channel_id);
    }

    /// Get a summary for a specific channel.
    pub fn channel_summary(&self, channel_id: u8) -> Option<ChannelErrorSummary> {
        let window = self.config.window();
        self.channels.get(&channel_id).map(|state_lock| {
            let state = state_lock.read();
            let categories = ErrorCategory::all();
            let mut category_counts = [(ErrorCategory::SendFailure, 0u64); 6];
            for (i, cat) in categories.iter().enumerate() {
                category_counts[i] = (*cat, state.category_counts[i]);
            }

            ChannelErrorSummary {
                channel_id,
                consecutive_errors: state.consecutive_errors,
                total_successes: state.total_successes,
                total_failures: state.total_failures,
                window_error_count: state.window_error_count(window),
                restart_triggered: state.restart_triggered,
                last_success_elapsed_ms: state.last_success_at.map(|t| t.elapsed().as_millis() as u64),
                last_error_elapsed_ms: state.last_error_at.map(|t| t.elapsed().as_millis() as u64),
                category_counts,
            }
        })
    }

    /// Get global statistics.
    pub fn stats_snapshot(&self) -> TrackerStatsSnapshot {
        self.stats.snapshot()
    }

    /// Ensure a channel state entry exists.
    fn ensure_channel(&self, channel_id: u8) {
        self.channels
            .entry(channel_id)
            .or_insert_with(|| RwLock::new(ChannelErrorState::new()));
    }
}

impl fmt::Debug for SendErrorTracker {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SendErrorTracker")
            .field("enabled", &self.config.enabled)
            .field("channels", &self.channels.len())
            .field("consecutive_threshold", &self.config.consecutive_threshold)
            .field("window_ms", &self.config.window_ms)
            .field("rate_threshold", &self.config.rate_threshold)
            .finish()
    }
}

impl Default for SendErrorTracker {
    fn default() -> Self {
        Self::with_defaults()
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_category_names() {
        assert_eq!(ErrorCategory::SendFailure.name(), "SendFailure");
        assert_eq!(ErrorCategory::AckError.name(), "AckError");
        assert_eq!(ErrorCategory::AckTimeout.name(), "AckTimeout");
        assert_eq!(ErrorCategory::ConfirmationNack.name(), "ConfirmationNack");
        assert_eq!(ErrorCategory::ConfirmationTimeout.name(), "ConfirmationTimeout");
        assert_eq!(ErrorCategory::FlowControlDrop.name(), "FlowControlDrop");
    }

    #[test]
    fn test_error_category_display() {
        assert_eq!(format!("{}", ErrorCategory::SendFailure), "SendFailure");
    }

    #[test]
    fn test_error_category_all() {
        assert_eq!(ErrorCategory::all().len(), 6);
    }

    #[test]
    fn test_tracker_success_resets_consecutive() {
        let tracker = SendErrorTracker::with_defaults();

        tracker.on_send_failure(1, ErrorCategory::SendFailure);
        tracker.on_send_failure(1, ErrorCategory::SendFailure);
        assert_eq!(tracker.consecutive_errors(1), 2);

        tracker.on_send_success(1);
        assert_eq!(tracker.consecutive_errors(1), 0);
    }

    #[test]
    fn test_tracker_consecutive_threshold() {
        let config = SendErrorTrackerConfig {
            consecutive_threshold: 3,
            ..Default::default()
        };
        let tracker = SendErrorTracker::new(config);

        let r1 = tracker.on_send_failure(1, ErrorCategory::SendFailure);
        assert_eq!(r1, TrackingResult::Recorded);

        let r2 = tracker.on_send_failure(1, ErrorCategory::AckError);
        assert_eq!(r2, TrackingResult::Recorded);

        let r3 = tracker.on_send_failure(1, ErrorCategory::AckTimeout);
        assert!(matches!(r3, TrackingResult::ConsecutiveThresholdExceeded {
            consecutive_errors: 3,
            threshold: 3,
        }));

        assert!(r3.requires_restart());
        assert!(!r3.is_warning());
    }

    #[test]
    fn test_tracker_restart_only_triggers_once() {
        let config = SendErrorTrackerConfig {
            consecutive_threshold: 2,
            ..Default::default()
        };
        let tracker = SendErrorTracker::new(config);

        let r1 = tracker.on_send_failure(1, ErrorCategory::SendFailure);
        assert_eq!(r1, TrackingResult::Recorded);

        let r2 = tracker.on_send_failure(1, ErrorCategory::SendFailure);
        assert!(matches!(r2, TrackingResult::ConsecutiveThresholdExceeded { .. }));

        // Subsequent failures should NOT re-trigger (restart_triggered=true)
        let r3 = tracker.on_send_failure(1, ErrorCategory::SendFailure);
        // This may still be Recorded or RateThresholdExceeded, but not ConsecutiveThresholdExceeded
        assert!(!matches!(r3, TrackingResult::ConsecutiveThresholdExceeded { .. }));
    }

    #[test]
    fn test_tracker_success_clears_restart_flag() {
        let config = SendErrorTrackerConfig {
            consecutive_threshold: 2,
            ..Default::default()
        };
        let tracker = SendErrorTracker::new(config);

        tracker.on_send_failure(1, ErrorCategory::SendFailure);
        tracker.on_send_failure(1, ErrorCategory::SendFailure);
        assert!(tracker.is_restart_triggered(1));

        tracker.on_send_success(1);
        assert!(!tracker.is_restart_triggered(1));

        // Can trigger again
        tracker.on_send_failure(1, ErrorCategory::SendFailure);
        let r = tracker.on_send_failure(1, ErrorCategory::SendFailure);
        assert!(matches!(r, TrackingResult::ConsecutiveThresholdExceeded { .. }));
    }

    #[test]
    fn test_tracker_multi_channel() {
        let tracker = SendErrorTracker::with_defaults();

        tracker.on_send_failure(1, ErrorCategory::SendFailure);
        tracker.on_send_failure(1, ErrorCategory::SendFailure);
        tracker.on_send_failure(2, ErrorCategory::AckError);

        assert_eq!(tracker.consecutive_errors(1), 2);
        assert_eq!(tracker.consecutive_errors(2), 1);
        assert_eq!(tracker.consecutive_errors(3), 0); // Non-existent channel
    }

    #[test]
    fn test_tracker_window_error_count() {
        let config = SendErrorTrackerConfig {
            window_ms: 60_000, // 60 seconds
            ..Default::default()
        };
        let tracker = SendErrorTracker::new(config);

        tracker.on_send_failure(1, ErrorCategory::SendFailure);
        tracker.on_send_failure(1, ErrorCategory::AckError);
        tracker.on_send_failure(1, ErrorCategory::AckTimeout);

        assert_eq!(tracker.window_error_count(1), 3);
    }

    #[test]
    fn test_tracker_convenience_methods() {
        let config = SendErrorTrackerConfig {
            consecutive_threshold: 100, // High threshold so we don't trigger
            ..Default::default()
        };
        let tracker = SendErrorTracker::new(config);

        let r1 = tracker.on_ack_error(1, 0x21);
        assert_eq!(r1, TrackingResult::Recorded);

        let r2 = tracker.on_ack_timeout(1);
        assert_eq!(r2, TrackingResult::Recorded);

        let r3 = tracker.on_confirmation_nack(1);
        assert_eq!(r3, TrackingResult::Recorded);

        let r4 = tracker.on_confirmation_timeout(1);
        assert_eq!(r4, TrackingResult::Recorded);

        let r5 = tracker.on_flow_control_drop(1);
        assert_eq!(r5, TrackingResult::Recorded);

        assert_eq!(tracker.consecutive_errors(1), 5);
    }

    #[test]
    fn test_tracker_reset_channel() {
        let tracker = SendErrorTracker::with_defaults();

        tracker.on_send_failure(1, ErrorCategory::SendFailure);
        tracker.on_send_failure(1, ErrorCategory::SendFailure);
        assert_eq!(tracker.consecutive_errors(1), 2);

        tracker.reset_channel(1);
        assert_eq!(tracker.consecutive_errors(1), 0);
        assert!(!tracker.is_restart_triggered(1));
    }

    #[test]
    fn test_tracker_remove_channel() {
        let tracker = SendErrorTracker::with_defaults();

        tracker.on_send_failure(1, ErrorCategory::SendFailure);
        assert_eq!(tracker.consecutive_errors(1), 1);

        tracker.remove_channel(1);
        assert_eq!(tracker.consecutive_errors(1), 0);
    }

    #[test]
    fn test_tracker_channel_summary() {
        let tracker = SendErrorTracker::with_defaults();

        tracker.on_send_success(1);
        tracker.on_send_failure(1, ErrorCategory::SendFailure);
        tracker.on_send_failure(1, ErrorCategory::AckError);

        let summary = tracker.channel_summary(1).unwrap();
        assert_eq!(summary.channel_id, 1);
        assert_eq!(summary.consecutive_errors, 2);
        assert_eq!(summary.total_successes, 1);
        assert_eq!(summary.total_failures, 2);
        assert!(!summary.restart_triggered);
        assert!(summary.last_success_elapsed_ms.is_some());
        assert!(summary.last_error_elapsed_ms.is_some());
    }

    #[test]
    fn test_tracker_global_stats() {
        let tracker = SendErrorTracker::with_defaults();

        tracker.on_send_success(1);
        tracker.on_send_success(2);
        tracker.on_send_failure(1, ErrorCategory::SendFailure);

        let stats = tracker.stats_snapshot();
        assert_eq!(stats.total_successes, 2);
        assert_eq!(stats.total_failures, 1);
        assert_eq!(stats.total_events(), 3);

        let rate = stats.error_rate();
        assert!((rate - 1.0 / 3.0).abs() < 0.01);
    }

    #[test]
    fn test_tracker_stats_empty() {
        let stats = TrackerStatsSnapshot {
            total_successes: 0,
            total_failures: 0,
            consecutive_triggers: 0,
            rate_triggers: 0,
        };
        assert_eq!(stats.error_rate(), 0.0);
        assert_eq!(stats.total_events(), 0);
    }

    #[test]
    fn test_tracker_disabled() {
        let config = SendErrorTrackerConfig {
            enabled: false,
            ..Default::default()
        };
        let tracker = SendErrorTracker::new(config);

        tracker.on_send_success(1);
        let result = tracker.on_send_failure(1, ErrorCategory::SendFailure);
        assert_eq!(result, TrackingResult::Recorded);
        assert_eq!(tracker.consecutive_errors(1), 0);
    }

    #[test]
    fn test_config_validate() {
        assert!(SendErrorTrackerConfig::default().validate().is_ok());

        assert!(SendErrorTrackerConfig {
            consecutive_threshold: 0,
            ..Default::default()
        }.validate().is_err());

        assert!(SendErrorTrackerConfig {
            window_ms: 0,
            ..Default::default()
        }.validate().is_err());

        assert!(SendErrorTrackerConfig {
            rate_threshold: 1.5,
            ..Default::default()
        }.validate().is_err());

        assert!(SendErrorTrackerConfig {
            rate_threshold: -0.1,
            ..Default::default()
        }.validate().is_err());
    }

    #[test]
    fn test_config_defaults() {
        let config = SendErrorTrackerConfig::default();
        assert!(config.enabled);
        assert_eq!(config.consecutive_threshold, 5);
        assert_eq!(config.window_ms, 60_000);
        assert_eq!(config.rate_threshold, 0.5);
        assert_eq!(config.min_events_for_rate, 10);
    }

    #[test]
    fn test_tracker_debug() {
        let tracker = SendErrorTracker::with_defaults();
        let debug_str = format!("{:?}", tracker);
        assert!(debug_str.contains("SendErrorTracker"));
        assert!(debug_str.contains("enabled"));
    }

    #[test]
    fn test_tracking_result_properties() {
        let recorded = TrackingResult::Recorded;
        assert!(!recorded.requires_restart());
        assert!(!recorded.is_warning());

        let consecutive = TrackingResult::ConsecutiveThresholdExceeded {
            consecutive_errors: 5,
            threshold: 5,
        };
        assert!(consecutive.requires_restart());
        assert!(!consecutive.is_warning());

        let rate = TrackingResult::RateThresholdExceeded {
            error_count: 10,
            window_ms: 60_000,
            rate: 75,
        };
        assert!(!rate.requires_restart());
        assert!(rate.is_warning());
    }

    #[test]
    fn test_category_counts_in_summary() {
        let tracker = SendErrorTracker::with_defaults();

        tracker.on_send_failure(1, ErrorCategory::SendFailure);
        tracker.on_send_failure(1, ErrorCategory::SendFailure);
        tracker.on_send_failure(1, ErrorCategory::AckError);
        tracker.on_send_failure(1, ErrorCategory::ConfirmationNack);

        let summary = tracker.channel_summary(1).unwrap();

        // Find SendFailure count
        let send_failures = summary.category_counts
            .iter()
            .find(|(cat, _)| *cat == ErrorCategory::SendFailure)
            .map(|(_, count)| *count)
            .unwrap();
        assert_eq!(send_failures, 2);

        let ack_errors = summary.category_counts
            .iter()
            .find(|(cat, _)| *cat == ErrorCategory::AckError)
            .map(|(_, count)| *count)
            .unwrap();
        assert_eq!(ack_errors, 1);
    }
}

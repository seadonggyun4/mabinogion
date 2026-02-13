//! TCP connection disruption fault injection.
//!
//! Simulates various TCP connection failure modes to test the
//! trap-modbus 7-state `ConnectionTransportState` FSM:
//! Disconnected → Connecting → Connected → LinkRecovering →
//! ProtocolRecovering → Reconnecting → Error

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;

use serde::{Deserialize, Serialize};

/// Configuration for TCP connection disruption behaviors.
///
/// These disruptions operate at the TCP connection level, not at the
/// Modbus protocol level. They simulate network failures, connection
/// drops, and unreliable links.
///
/// # Testing Targets
///
/// - `drop_after_requests` → Tests `LinkRecovery` detection and reconnection
/// - `periodic_drop` → Tests `Reconnecting` state cycling
/// - `hold_open_timeout` → Tests timeout detection without RST
/// - `rst_after_partial` → Tests mid-frame disconnection handling
/// - `drop_mid_frame` → Tests incomplete write detection
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionDisruptionConfig {
    /// Drop the connection after N successful request-response cycles.
    /// The server will close (or RST) the TCP connection after the Nth response.
    /// This triggers the `LinkRecovery` → `Reconnecting` path in trap-modbus.
    #[serde(default)]
    pub drop_after_requests: Option<u64>,

    /// Drop the connection in the middle of sending a response frame.
    /// This simulates a network failure during data transfer.
    #[serde(default)]
    pub drop_mid_frame: bool,

    /// Send a partial response, then RST the connection.
    /// This combines partial data with connection loss.
    #[serde(default)]
    pub rst_after_partial: bool,

    /// Number of bytes to send before RST (for rst_after_partial).
    #[serde(default = "default_partial_bytes")]
    pub partial_byte_count: usize,

    /// Keep the connection open but stop sending responses after N requests.
    /// The connection stays alive but becomes unresponsive, testing
    /// the client's response_timeout handling.
    #[serde(default)]
    pub hold_open_timeout: Option<Duration>,

    /// Drop the connection every N requests (periodic).
    /// After each drop, the client must reconnect. This tests
    /// repeated `Reconnecting` state transitions.
    #[serde(default)]
    pub periodic_drop: Option<u64>,

    /// Delay before closing the connection (simulates slow disconnect).
    #[serde(default)]
    pub close_delay: Option<Duration>,

    /// Whether to send TCP RST instead of FIN.
    /// RST is more abrupt and may cause different error paths.
    #[serde(default)]
    pub use_rst: bool,
}

fn default_partial_bytes() -> usize {
    4
}

impl Default for ConnectionDisruptionConfig {
    fn default() -> Self {
        Self {
            drop_after_requests: None,
            drop_mid_frame: false,
            rst_after_partial: false,
            partial_byte_count: default_partial_bytes(),
            hold_open_timeout: None,
            periodic_drop: None,
            close_delay: None,
            use_rst: false,
        }
    }
}

impl ConnectionDisruptionConfig {
    /// Create a new default configuration (all disruptions disabled).
    pub fn new() -> Self {
        Self::default()
    }

    /// Drop connection after N requests.
    pub fn with_drop_after_requests(mut self, n: u64) -> Self {
        self.drop_after_requests = Some(n);
        self
    }

    /// Enable mid-frame drop.
    pub fn with_drop_mid_frame(mut self) -> Self {
        self.drop_mid_frame = true;
        self
    }

    /// Enable RST after partial response.
    pub fn with_rst_after_partial(mut self, byte_count: usize) -> Self {
        self.rst_after_partial = true;
        self.partial_byte_count = byte_count;
        self
    }

    /// Hold the connection open without responding.
    pub fn with_hold_open_timeout(mut self, duration: Duration) -> Self {
        self.hold_open_timeout = Some(duration);
        self
    }

    /// Drop every N requests.
    pub fn with_periodic_drop(mut self, n: u64) -> Self {
        self.periodic_drop = Some(n);
        self
    }

    /// Send TCP RST instead of FIN.
    pub fn with_rst(mut self) -> Self {
        self.use_rst = true;
        self
    }

    /// Add delay before disconnection.
    pub fn with_close_delay(mut self, delay: Duration) -> Self {
        self.close_delay = Some(delay);
        self
    }
}

/// Runtime state tracker for connection disruption.
///
/// Tracks the current request count and determines when to trigger
/// disruption actions. This is used internally by the server
/// integration layer.
pub struct ConnectionDisruptionState {
    config: ConnectionDisruptionConfig,
    /// Total request-response cycles completed on this connection.
    request_count: AtomicU64,
    /// Whether a hold-open timeout is currently active.
    holding_open: AtomicBool,
}

impl ConnectionDisruptionState {
    /// Create a new disruption state tracker.
    pub fn new(config: ConnectionDisruptionConfig) -> Self {
        Self {
            config,
            request_count: AtomicU64::new(0),
            holding_open: AtomicBool::new(false),
        }
    }

    /// Record a completed request-response cycle.
    /// Returns the action to take, if any.
    pub fn record_request(&self) -> DisruptionAction {
        let count = self.request_count.fetch_add(1, Ordering::Relaxed) + 1;

        // Check drop_after_requests
        if let Some(threshold) = self.config.drop_after_requests {
            if count >= threshold {
                return self.build_disconnect_action();
            }
        }

        // Check periodic_drop
        if let Some(period) = self.config.periodic_drop {
            if period > 0 && count % period == 0 {
                return self.build_disconnect_action();
            }
        }

        // Check hold_open_timeout
        if let Some(hold_duration) = self.config.hold_open_timeout {
            if count >= self.config.drop_after_requests.unwrap_or(u64::MAX) {
                return DisruptionAction::HoldOpen { duration: hold_duration };
            }
        }

        DisruptionAction::None
    }

    /// Build the appropriate disconnect action based on config.
    fn build_disconnect_action(&self) -> DisruptionAction {
        if self.config.rst_after_partial {
            DisruptionAction::RstAfterPartial {
                byte_count: self.config.partial_byte_count,
                close_delay: self.config.close_delay,
                use_rst: self.config.use_rst,
            }
        } else if self.config.drop_mid_frame {
            DisruptionAction::DropMidFrame {
                close_delay: self.config.close_delay,
                use_rst: self.config.use_rst,
            }
        } else {
            DisruptionAction::Disconnect {
                close_delay: self.config.close_delay,
                use_rst: self.config.use_rst,
            }
        }
    }

    /// Get the current request count.
    pub fn request_count(&self) -> u64 {
        self.request_count.load(Ordering::Acquire)
    }

    /// Check if hold-open mode is active.
    pub fn is_holding_open(&self) -> bool {
        self.holding_open.load(Ordering::Acquire)
    }

    /// Set hold-open state.
    pub fn set_holding_open(&self, holding: bool) {
        self.holding_open.store(holding, Ordering::Release);
    }

    /// Reset the state (for reconnections).
    pub fn reset(&self) {
        self.request_count.store(0, Ordering::Release);
        self.holding_open.store(false, Ordering::Release);
    }
}

/// Action to take on the TCP connection.
#[derive(Debug, Clone)]
pub enum DisruptionAction {
    /// No disruption, proceed normally.
    None,

    /// Close the connection (FIN or RST).
    Disconnect {
        /// Delay before closing.
        close_delay: Option<Duration>,
        /// Use RST instead of FIN.
        use_rst: bool,
    },

    /// Drop the connection while sending a response.
    DropMidFrame {
        /// Delay before closing.
        close_delay: Option<Duration>,
        /// Use RST instead of FIN.
        use_rst: bool,
    },

    /// Send partial bytes, then RST.
    RstAfterPartial {
        /// Number of bytes to send before RST.
        byte_count: usize,
        /// Delay before RST.
        close_delay: Option<Duration>,
        /// Use RST instead of FIN.
        use_rst: bool,
    },

    /// Keep the connection open but don't send responses.
    HoldOpen {
        /// How long to hold the connection open.
        duration: Duration,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = ConnectionDisruptionConfig::default();
        assert!(config.drop_after_requests.is_none());
        assert!(!config.drop_mid_frame);
        assert!(!config.rst_after_partial);
        assert!(config.periodic_drop.is_none());
    }

    #[test]
    fn test_drop_after_requests() {
        let config = ConnectionDisruptionConfig::new().with_drop_after_requests(3);
        let state = ConnectionDisruptionState::new(config);

        assert!(matches!(state.record_request(), DisruptionAction::None));
        assert!(matches!(state.record_request(), DisruptionAction::None));
        assert!(matches!(
            state.record_request(),
            DisruptionAction::Disconnect { .. }
        ));
    }

    #[test]
    fn test_periodic_drop() {
        let config = ConnectionDisruptionConfig::new().with_periodic_drop(2);
        let state = ConnectionDisruptionState::new(config);

        assert!(matches!(state.record_request(), DisruptionAction::None)); // 1
        assert!(matches!(
            state.record_request(),
            DisruptionAction::Disconnect { .. }
        )); // 2
        assert!(matches!(state.record_request(), DisruptionAction::None)); // 3
        assert!(matches!(
            state.record_request(),
            DisruptionAction::Disconnect { .. }
        )); // 4
    }

    #[test]
    fn test_rst_after_partial() {
        let config = ConnectionDisruptionConfig::new()
            .with_drop_after_requests(1)
            .with_rst_after_partial(6);
        let state = ConnectionDisruptionState::new(config);

        match state.record_request() {
            DisruptionAction::RstAfterPartial { byte_count, .. } => {
                assert_eq!(byte_count, 6);
            }
            other => panic!("Expected RstAfterPartial, got {:?}", other),
        }
    }

    #[test]
    fn test_drop_mid_frame() {
        let config = ConnectionDisruptionConfig::new()
            .with_drop_after_requests(1)
            .with_drop_mid_frame();
        let state = ConnectionDisruptionState::new(config);

        assert!(matches!(
            state.record_request(),
            DisruptionAction::DropMidFrame { .. }
        ));
    }

    #[test]
    fn test_config_builder() {
        let config = ConnectionDisruptionConfig::new()
            .with_drop_after_requests(5)
            .with_rst()
            .with_close_delay(Duration::from_millis(100));

        assert_eq!(config.drop_after_requests, Some(5));
        assert!(config.use_rst);
        assert_eq!(config.close_delay, Some(Duration::from_millis(100)));
    }

    #[test]
    fn test_state_reset() {
        let config = ConnectionDisruptionConfig::new().with_drop_after_requests(2);
        let state = ConnectionDisruptionState::new(config);

        state.record_request(); // 1
        assert_eq!(state.request_count(), 1);

        state.reset();
        assert_eq!(state.request_count(), 0);

        // After reset, should count from 0 again
        assert!(matches!(state.record_request(), DisruptionAction::None)); // 1
        assert!(matches!(
            state.record_request(),
            DisruptionAction::Disconnect { .. }
        )); // 2
    }

    #[test]
    fn test_no_disruption_by_default() {
        let config = ConnectionDisruptionConfig::default();
        let state = ConnectionDisruptionState::new(config);

        for _ in 0..100 {
            assert!(matches!(state.record_request(), DisruptionAction::None));
        }
    }

    #[test]
    fn test_use_rst_flag() {
        let config = ConnectionDisruptionConfig::new()
            .with_drop_after_requests(1)
            .with_rst();
        let state = ConnectionDisruptionState::new(config);

        match state.record_request() {
            DisruptionAction::Disconnect { use_rst, .. } => {
                assert!(use_rst);
            }
            other => panic!("Expected Disconnect, got {:?}", other),
        }
    }
}

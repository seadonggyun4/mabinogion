//! Device lifecycle management traits and types.
//!
//! This module provides a clean separation of lifecycle concerns from
//! device operations, improving testability and maintainability.
//!
//! # Lifecycle States
//!
//! ```text
//! ┌──────────────┐
//! │ Uninitialized│
//! └──────┬───────┘
//!        │ initialize()
//!        ▼
//! ┌──────────────┐
//! │ Initializing │──── Error ────┐
//! └──────┬───────┘               │
//!        │                       ▼
//!        ▼               ┌───────────┐
//! ┌──────────────┐       │   Error   │
//! │   Offline    │◄──────┤           │
//! └──────┬───────┘       └───────────┘
//!        │ start()              ▲
//!        ▼                      │
//! ┌──────────────┐              │
//! │    Online    │─── error ────┘
//! └──────┬───────┘
//!        │ stop()
//!        ▼
//! ┌──────────────┐
//! │ ShuttingDown │
//! └──────┬───────┘
//!        │
//!        ▼
//! ┌──────────────┐
//! │   Offline    │
//! └──────────────┘
//! ```

use std::time::{Duration, Instant};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::device::DeviceState;
use crate::error::Result;

/// Lifecycle management trait for devices.
///
/// This trait separates lifecycle concerns from regular device operations,
/// making it easier to implement custom lifecycle behavior and testing.
#[async_trait]
pub trait DeviceLifecycle: Send + Sync {
    /// Get the current lifecycle state.
    fn state(&self) -> DeviceState;

    /// Check if the device is in an operational state.
    fn is_operational(&self) -> bool {
        self.state().is_operational()
    }

    /// Check if the device can accept requests.
    fn can_accept_requests(&self) -> bool {
        self.state().can_accept_requests()
    }

    /// Initialize the device.
    ///
    /// This should set up any necessary resources and move the device
    /// from `Uninitialized` to `Offline` state.
    async fn initialize(&mut self) -> Result<()>;

    /// Start the device.
    ///
    /// This should make the device operational and move it to `Online` state.
    async fn start(&mut self) -> Result<()>;

    /// Stop the device.
    ///
    /// This should gracefully stop the device and move it to `Offline` state.
    async fn stop(&mut self) -> Result<()>;

    /// Handle errors during operation.
    ///
    /// This is called when an error occurs during device operation.
    /// The default implementation transitions to `Error` state.
    async fn on_error(&mut self, _error: &crate::error::Error) -> Result<()> {
        Ok(())
    }

    /// Attempt to recover from an error state.
    ///
    /// Returns true if recovery was successful.
    async fn recover(&mut self) -> Result<bool> {
        Ok(false)
    }
}

/// Lifecycle event types.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum LifecycleEvent {
    /// Device initialized.
    Initialized {
        device_id: String,
        timestamp: chrono::DateTime<chrono::Utc>,
    },
    /// Device started.
    Started {
        device_id: String,
        timestamp: chrono::DateTime<chrono::Utc>,
    },
    /// Device stopped.
    Stopped {
        device_id: String,
        timestamp: chrono::DateTime<chrono::Utc>,
        reason: StopReason,
    },
    /// State changed.
    StateChanged {
        device_id: String,
        old_state: DeviceState,
        new_state: DeviceState,
        timestamp: chrono::DateTime<chrono::Utc>,
    },
    /// Error occurred.
    Error {
        device_id: String,
        error: String,
        timestamp: chrono::DateTime<chrono::Utc>,
    },
    /// Recovery attempted.
    RecoveryAttempted {
        device_id: String,
        success: bool,
        timestamp: chrono::DateTime<chrono::Utc>,
    },
}

/// Reason for device stop.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StopReason {
    /// Normal shutdown requested by user.
    UserRequested,
    /// Shutdown due to error.
    Error,
    /// Shutdown for maintenance.
    Maintenance,
    /// System shutdown.
    SystemShutdown,
    /// Timeout.
    Timeout,
}

/// Lifecycle state machine that manages state transitions.
#[derive(Debug, Clone)]
pub struct LifecycleStateMachine {
    current_state: DeviceState,
    last_transition: Option<Instant>,
    error_count: u32,
    max_retries: u32,
    retry_delay: Duration,
}

impl LifecycleStateMachine {
    /// Create a new state machine.
    pub fn new() -> Self {
        Self {
            current_state: DeviceState::Uninitialized,
            last_transition: None,
            error_count: 0,
            max_retries: 3,
            retry_delay: Duration::from_secs(1),
        }
    }

    /// Create with custom retry settings.
    pub fn with_retries(mut self, max_retries: u32, retry_delay: Duration) -> Self {
        self.max_retries = max_retries;
        self.retry_delay = retry_delay;
        self
    }

    /// Get the current state.
    pub fn state(&self) -> DeviceState {
        self.current_state
    }

    /// Check if a transition is valid.
    pub fn can_transition_to(&self, target: DeviceState) -> bool {
        match (self.current_state, target) {
            // From Uninitialized
            (DeviceState::Uninitialized, DeviceState::Initializing) => true,
            (DeviceState::Uninitialized, DeviceState::Error) => true,

            // From Initializing
            (DeviceState::Initializing, DeviceState::Offline) => true,
            (DeviceState::Initializing, DeviceState::Online) => true,
            (DeviceState::Initializing, DeviceState::Error) => true,

            // From Offline
            (DeviceState::Offline, DeviceState::Online) => true,
            (DeviceState::Offline, DeviceState::Error) => true,

            // From Online
            (DeviceState::Online, DeviceState::Offline) => true,
            (DeviceState::Online, DeviceState::ShuttingDown) => true,
            (DeviceState::Online, DeviceState::Error) => true,

            // From ShuttingDown
            (DeviceState::ShuttingDown, DeviceState::Offline) => true,
            (DeviceState::ShuttingDown, DeviceState::Error) => true,

            // From Error
            (DeviceState::Error, DeviceState::Offline) => true,
            (DeviceState::Error, DeviceState::Initializing) => true,
            (DeviceState::Error, DeviceState::Uninitialized) => true,

            _ => false,
        }
    }

    /// Attempt to transition to a new state.
    pub fn transition_to(&mut self, target: DeviceState) -> Result<DeviceState> {
        if !self.can_transition_to(target) {
            return Err(crate::error::Error::Engine(format!(
                "Invalid state transition: {:?} -> {:?}",
                self.current_state, target
            )));
        }

        let old_state = self.current_state;
        self.current_state = target;
        self.last_transition = Some(Instant::now());

        // Reset error count on successful non-error transition
        if target != DeviceState::Error {
            self.error_count = 0;
        }

        Ok(old_state)
    }

    /// Record an error and check if retries are exhausted.
    pub fn record_error(&mut self) -> bool {
        self.error_count += 1;
        self.error_count > self.max_retries
    }

    /// Get the current error count.
    pub fn error_count(&self) -> u32 {
        self.error_count
    }

    /// Reset the error count.
    pub fn reset_errors(&mut self) {
        self.error_count = 0;
    }

    /// Get time since last transition.
    pub fn time_in_state(&self) -> Option<Duration> {
        self.last_transition.map(|t| t.elapsed())
    }

    /// Check if retry delay has passed.
    pub fn can_retry(&self) -> bool {
        self.last_transition
            .map(|t| t.elapsed() >= self.retry_delay)
            .unwrap_or(true)
    }
}

impl Default for LifecycleStateMachine {
    fn default() -> Self {
        Self::new()
    }
}

/// Lifecycle hook trait for extending lifecycle behavior.
///
/// Implement this trait to add custom behavior at lifecycle transitions.
#[async_trait]
pub trait LifecycleHook: Send + Sync {
    /// Called before initialization.
    async fn before_init(&self) -> Result<()> {
        Ok(())
    }

    /// Called after successful initialization.
    async fn after_init(&self) -> Result<()> {
        Ok(())
    }

    /// Called before starting.
    async fn before_start(&self) -> Result<()> {
        Ok(())
    }

    /// Called after successful start.
    async fn after_start(&self) -> Result<()> {
        Ok(())
    }

    /// Called before stopping.
    async fn before_stop(&self) -> Result<()> {
        Ok(())
    }

    /// Called after successful stop.
    async fn after_stop(&self) -> Result<()> {
        Ok(())
    }

    /// Called when entering error state.
    async fn on_error(&self, _error: &crate::error::Error) -> Result<()> {
        Ok(())
    }
}

/// A no-op lifecycle hook for use as a default.
pub struct NoOpLifecycleHook;

#[async_trait]
impl LifecycleHook for NoOpLifecycleHook {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lifecycle_state_machine_transitions() {
        let mut sm = LifecycleStateMachine::new();

        assert_eq!(sm.state(), DeviceState::Uninitialized);

        // Valid transition
        sm.transition_to(DeviceState::Initializing).unwrap();
        assert_eq!(sm.state(), DeviceState::Initializing);

        sm.transition_to(DeviceState::Offline).unwrap();
        assert_eq!(sm.state(), DeviceState::Offline);

        sm.transition_to(DeviceState::Online).unwrap();
        assert_eq!(sm.state(), DeviceState::Online);
    }

    #[test]
    fn test_lifecycle_state_machine_invalid_transition() {
        let mut sm = LifecycleStateMachine::new();

        // Invalid transition: Uninitialized -> Online
        let result = sm.transition_to(DeviceState::Online);
        assert!(result.is_err());
    }

    #[test]
    fn test_lifecycle_state_machine_error_handling() {
        let mut sm = LifecycleStateMachine::new().with_retries(3, Duration::from_millis(100));

        // Record errors
        assert!(!sm.record_error()); // 1st error
        assert!(!sm.record_error()); // 2nd error
        assert!(!sm.record_error()); // 3rd error
        assert!(sm.record_error()); // 4th error - exhausted

        assert_eq!(sm.error_count(), 4);

        sm.reset_errors();
        assert_eq!(sm.error_count(), 0);
    }

    #[test]
    fn test_lifecycle_valid_transitions() {
        let sm = LifecycleStateMachine::new();

        assert!(sm.can_transition_to(DeviceState::Initializing));
        assert!(sm.can_transition_to(DeviceState::Error));
        assert!(!sm.can_transition_to(DeviceState::Online));
        assert!(!sm.can_transition_to(DeviceState::ShuttingDown));
    }
}

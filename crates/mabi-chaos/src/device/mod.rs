//! Device fault implementations.
//!
//! This module provides device-related faults:
//! - [`DeviceOfflineFault`]: Simulate device going offline
//! - [`SlowResponseFault`]: Simulate slow device responses
//! - [`CorruptedDataFault`]: Return corrupted or invalid data
//! - [`StateTransitionFault`]: Fail state transitions

mod offline;
mod slow_response;
mod corrupted_data;
mod state_transition;

pub use offline::{DeviceOfflineFault, OfflineConfig, OfflinePattern, OfflineFaultBuilder};
pub use slow_response::{SlowResponseFault, SlowResponseConfig, SlowResponseFaultBuilder};
pub use corrupted_data::{CorruptedDataFault, CorruptionConfig, CorruptionStrategy, CorruptionFaultBuilder};
pub use state_transition::{StateTransitionFault, TransitionConfig, TransitionFaultBuilder};

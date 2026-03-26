//! Device fault implementations.
//!
//! This module provides device-related faults:
//! - [`DeviceOfflineFault`]: Simulate device going offline
//! - [`SlowResponseFault`]: Simulate slow device responses
//! - [`CorruptedDataFault`]: Return corrupted or invalid data
//! - [`StateTransitionFault`]: Fail state transitions

mod corrupted_data;
mod offline;
mod slow_response;
mod state_transition;

pub use corrupted_data::{
    CorruptedDataFault, CorruptionConfig, CorruptionFaultBuilder, CorruptionStrategy,
};
pub use offline::{DeviceOfflineFault, OfflineConfig, OfflineFaultBuilder, OfflinePattern};
pub use slow_response::{SlowResponseConfig, SlowResponseFault, SlowResponseFaultBuilder};
pub use state_transition::{StateTransitionFault, TransitionConfig, TransitionFaultBuilder};

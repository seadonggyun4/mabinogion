//! Protocol fault implementations.
//!
//! This module provides protocol-related faults:
//! - [`MalformedPacketFault`]: Generate malformed protocol packets
//! - [`ChecksumFault`]: Invalid checksums in packets
//! - [`TimeoutFault`]: Protocol-level timeouts
//! - [`ReorderFault`]: Out-of-order responses

mod malformed;
mod checksum;
mod timeout;
mod reorder;

pub use malformed::{MalformedPacketFault, MalformedConfig, MalformationType, MalformedFaultBuilder};
pub use checksum::{ChecksumFault, ChecksumConfig, ChecksumFaultBuilder};
pub use timeout::{TimeoutFault, TimeoutConfig, TimeoutPattern, TimeoutFaultBuilder};
pub use reorder::{ReorderFault, ReorderConfig, ReorderFaultBuilder};

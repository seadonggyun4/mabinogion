//! Protocol fault implementations.
//!
//! This module provides protocol-related faults:
//! - [`MalformedPacketFault`]: Generate malformed protocol packets
//! - [`ChecksumFault`]: Invalid checksums in packets
//! - [`TimeoutFault`]: Protocol-level timeouts
//! - [`ReorderFault`]: Out-of-order responses

mod checksum;
mod malformed;
mod reorder;
mod timeout;

pub use checksum::{ChecksumConfig, ChecksumFault, ChecksumFaultBuilder};
pub use malformed::{
    MalformationType, MalformedConfig, MalformedFaultBuilder, MalformedPacketFault,
};
pub use reorder::{ReorderConfig, ReorderFault, ReorderFaultBuilder};
pub use timeout::{TimeoutConfig, TimeoutFault, TimeoutFaultBuilder, TimeoutPattern};

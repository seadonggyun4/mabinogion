//! BACnet/IP protocol-specific fault implementations.
//!
//! This module provides BACnet-aware chaos engineering faults that simulate
//! real-world protocol failures at the BACnet application layer:
//!
//! - [`ApduFault`]: APDU-level corruption (bad headers, invalid service codes, invoke ID reuse)
//! - [`ServiceFault`]: Service-level failures (reject/abort specific services, return BACnet errors)
//! - [`CovFault`]: COV notification failures (drop/delay/corrupt notifications, stale data)
//! - [`PropertyFault`]: Property access corruption (wrong types, out-of-range, missing required)
//! - [`SegmentationFault`]: Segmentation failures (missing segments, out-of-order, window errors)

mod apdu_fault;
mod cov_fault;
mod property_fault;
mod segmentation_fault;
mod service_fault;

pub use apdu_fault::{ApduFault, ApduFaultBuilder, ApduFaultConfig, ApduFaultType};
pub use cov_fault::{CovFault, CovFaultBuilder, CovFaultConfig, CovFaultType};
pub use property_fault::{
    PropertyFault, PropertyFaultBuilder, PropertyFaultConfig, PropertyFaultType,
};
pub use segmentation_fault::{
    SegmentationFault, SegmentationFaultBuilder, SegmentationFaultConfig, SegmentationFaultType,
};
pub use service_fault::{ServiceFault, ServiceFaultBuilder, ServiceFaultConfig, ServiceFaultType};

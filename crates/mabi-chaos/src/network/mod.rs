//! Network fault implementations.
//!
//! This module provides network-related faults:
//! - [`NetworkLatencyFault`]: Inject latency into operations
//! - [`PacketLossFault`]: Simulate packet loss
//! - [`ConnectionFault`]: Simulate connection disruptions
//! - [`BandwidthFault`]: Simulate bandwidth throttling

mod latency;
mod packet_loss;
mod connection;
mod bandwidth;

pub use latency::{NetworkLatencyFault, LatencyConfig, LatencyDistribution, LatencyFaultBuilder};
pub use packet_loss::{PacketLossFault, PacketLossConfig, BurstConfig, PacketLossFaultBuilder};
pub use connection::{ConnectionFault, ConnectionConfig, DisconnectMode, ConnectionFaultBuilder};
pub use bandwidth::{BandwidthFault, BandwidthConfig, BandwidthFaultBuilder};

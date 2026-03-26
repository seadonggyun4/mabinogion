//! Network fault implementations.
//!
//! This module provides network-related faults:
//! - [`NetworkLatencyFault`]: Inject latency into operations
//! - [`PacketLossFault`]: Simulate packet loss
//! - [`ConnectionFault`]: Simulate connection disruptions
//! - [`BandwidthFault`]: Simulate bandwidth throttling

mod bandwidth;
mod connection;
mod latency;
mod packet_loss;

pub use bandwidth::{BandwidthConfig, BandwidthFault, BandwidthFaultBuilder};
pub use connection::{ConnectionConfig, ConnectionFault, ConnectionFaultBuilder, DisconnectMode};
pub use latency::{LatencyConfig, LatencyDistribution, LatencyFaultBuilder, NetworkLatencyFault};
pub use packet_loss::{BurstConfig, PacketLossConfig, PacketLossFault, PacketLossFaultBuilder};

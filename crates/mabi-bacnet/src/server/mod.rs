//! BACnet/IP server implementation.
//!
//! This module provides a high-performance BACnet/IP server with:
//!
//! - Service handler registry pattern
//! - UDP network handling with broadcast support
//! - COV subscription management
//! - Device discovery (Who-Is/I-Am)
//! - Metrics collection
//! - Graceful shutdown support

mod bacnet_server;
mod metrics;

pub use bacnet_server::{BACnetServer, ServerConfig, ServerEvent};
pub use metrics::{ServerMetrics, ServerMetricsSnapshot};

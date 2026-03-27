//! OPC UA TCP Transport Layer.
//!
//! Handles TCP connections, message framing, and the Hello/Acknowledge handshake.

pub mod codec;
pub mod connection;
pub(crate) mod hooks;
pub mod messages;
pub mod metrics;
pub(crate) mod runtime;
pub(crate) mod secure_channel_runtime;
pub mod tcp_listener;

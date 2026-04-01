//! OPC UA transport adapters and shared runtime.
//!
//! The crate currently ships UA-TCP listener and reverse-connect adapters plus an HTTPS
//! request/response adapter, all converging on the same typed transport runtime.

pub(crate) mod adapter;
pub mod codec;
pub mod connection;
pub(crate) mod hooks;
pub(crate) mod https_listener;
pub mod messages;
pub mod metrics;
pub(crate) mod runtime;
pub(crate) mod secure_channel_runtime;
pub mod tcp_listener;
pub(crate) mod tcp_reverse_connector;

//! OPC UA error types.

use mabi_core::Error as CoreError;
use thiserror::Error;

/// OPC UA result type.
pub type OpcUaResult<T> = Result<T, OpcUaError>;

/// OPC UA error types.
#[derive(Debug, Error)]
pub enum OpcUaError {
    /// Invalid canonical simulator config.
    #[error("Configuration error: {0}")]
    Config(String),

    /// Node not found.
    #[error("Node not found: {node_id}")]
    NodeNotFound { node_id: String },

    /// Invalid node ID.
    #[error("Invalid node ID: {0}")]
    InvalidNodeId(String),

    /// Server error.
    #[error("Server error: {0}")]
    Server(String),

    /// Connection error.
    #[error("Connection error: {0}")]
    Connection(String),

    /// Subscription error.
    #[error("Subscription error: {0}")]
    Subscription(String),

    /// Security error.
    #[error("Security error: {0}")]
    Security(String),

    /// Invalid state error.
    #[error("Invalid state: {0}")]
    InvalidState(String),

    /// Write error.
    #[error("Write error: {0}")]
    WriteError(String),

    /// Binary codec encoding/decoding error.
    #[error("Codec error: {0}")]
    Codec(String),

    /// Protocol-level error.
    #[error("Protocol error: {0}")]
    ProtocolError(String),

    /// Requested service is not supported.
    #[error("Service not supported: {service_id}")]
    ServiceNotSupported { service_id: String },

    /// Invalid secure channel ID.
    #[error("Bad secure channel ID: {0}")]
    BadSecureChannelId(u32),

    /// Invalid sequence number.
    #[error("Bad sequence number: expected {expected}, got {actual}")]
    BadSequenceNumber { expected: u32, actual: u32 },

    /// Message exceeds maximum allowed size.
    #[error("Message too large: {size} bytes (max: {max})")]
    MessageTooLarge { size: usize, max: usize },

    /// Bind error.
    #[error("Failed to bind {address}: {reason}")]
    Bind {
        address: std::net::SocketAddr,
        reason: String,
    },

    /// I/O error.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// Core error.
    #[error("Core error: {0}")]
    Core(#[from] CoreError),
}

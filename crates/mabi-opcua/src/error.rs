//! OPC UA error types.

use thiserror::Error;
use mabi_core::Error as CoreError;

/// OPC UA result type.
pub type OpcUaResult<T> = Result<T, OpcUaError>;

/// OPC UA error types.
#[derive(Debug, Error)]
pub enum OpcUaError {
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

    /// I/O error.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// Core error.
    #[error("Core error: {0}")]
    Core(#[from] CoreError),
}

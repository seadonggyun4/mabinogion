//! BACnet error types.

use mabi_core::Error as CoreError;
use thiserror::Error;

/// BACnet result type.
pub type BacnetResult<T> = Result<T, BacnetError>;

/// BACnet error types.
#[derive(Debug, Error)]
pub enum BacnetError {
    /// Object not found.
    #[error("Object not found: {object_type}:{instance}")]
    ObjectNotFound { object_type: String, instance: u32 },

    /// Property not found.
    #[error("Property not found: {property}")]
    PropertyNotFound { property: String },

    /// Invalid object type.
    #[error("Invalid object type: {0}")]
    InvalidObjectType(u16),

    /// Server error.
    #[error("Server error: {0}")]
    Server(String),

    /// Protocol error.
    #[error("Protocol error: {0}")]
    Protocol(String),

    /// I/O error.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// Core error.
    #[error("Core error: {0}")]
    Core(#[from] CoreError),
}

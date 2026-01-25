//! KNX error types.
//!
//! This module provides comprehensive error handling for the KNX simulator,
//! with structured error types for different failure scenarios.

use std::net::SocketAddr;
use thiserror::Error;

use mabi_core::Error as CoreError;

/// KNX result type.
pub type KnxResult<T> = Result<T, KnxError>;

/// KNX error types.
#[derive(Debug, Error)]
pub enum KnxError {
    // ========================================================================
    // Address Errors
    // ========================================================================
    /// Invalid group address format.
    #[error("Invalid group address: {0}")]
    InvalidGroupAddress(String),

    /// Invalid individual address format.
    #[error("Invalid individual address: {0}")]
    InvalidIndividualAddress(String),

    /// Address out of range.
    #[error("Address out of range: {address} (valid: {valid_range})")]
    AddressOutOfRange { address: String, valid_range: String },

    // ========================================================================
    // DPT (Datapoint Type) Errors
    // ========================================================================
    /// Invalid datapoint type.
    #[error("Invalid datapoint type: {0}")]
    InvalidDpt(String),

    /// DPT encoding error.
    #[error("DPT encoding error for {dpt}: {reason}")]
    DptEncoding { dpt: String, reason: String },

    /// DPT decoding error.
    #[error("DPT decoding error for {dpt}: {reason}")]
    DptDecoding { dpt: String, reason: String },

    /// DPT value out of range.
    #[error("DPT value out of range: {value} (valid: {valid_range})")]
    DptValueOutOfRange { value: String, valid_range: String },

    // ========================================================================
    // Frame Errors
    // ========================================================================
    /// Frame too short.
    #[error("Frame too short: expected at least {expected} bytes, got {actual}")]
    FrameTooShort { expected: usize, actual: usize },

    /// Invalid frame header.
    #[error("Invalid frame header: {0}")]
    InvalidHeader(String),

    /// Invalid protocol version.
    #[error("Invalid protocol version: expected {expected:#04x}, got {actual:#04x}")]
    InvalidProtocolVersion { expected: u8, actual: u8 },

    /// Unknown service type.
    #[error("Unknown service type: {0:#06x}")]
    UnknownServiceType(u16),

    /// Frame length mismatch.
    #[error("Frame length mismatch: header says {header_length}, actual is {actual_length}")]
    FrameLengthMismatch {
        header_length: usize,
        actual_length: usize,
    },

    /// Invalid HPAI (Host Protocol Address Information).
    #[error("Invalid HPAI: {0}")]
    InvalidHpai(String),

    // ========================================================================
    // cEMI Errors
    // ========================================================================
    /// Unknown cEMI message code.
    #[error("Unknown cEMI message code: {0:#04x}")]
    UnknownMessageCode(u8),

    /// Invalid cEMI frame.
    #[error("Invalid cEMI frame: {0}")]
    InvalidCemi(String),

    /// Unknown APCI.
    #[error("Unknown APCI: {0:#06x}")]
    UnknownApci(u16),

    // ========================================================================
    // Connection Errors
    // ========================================================================
    /// Connection failed.
    #[error("Connection failed to {address}: {reason}")]
    ConnectionFailed { address: SocketAddr, reason: String },

    /// Connection timeout.
    #[error("Connection timeout after {timeout_ms}ms")]
    ConnectionTimeout { timeout_ms: u64 },

    /// Connection closed.
    #[error("Connection closed: {0}")]
    ConnectionClosed(String),

    /// No more connections available.
    #[error("No more connections available: maximum {max} reached")]
    NoMoreConnections { max: usize },

    /// Invalid channel ID.
    #[error("Invalid channel ID: {0}")]
    InvalidChannel(u8),

    /// Sequence error.
    #[error("Sequence error: expected {expected}, got {actual}")]
    SequenceError { expected: u8, actual: u8 },

    // ========================================================================
    // Tunnel Errors
    // ========================================================================
    /// Tunnel connection error.
    #[error("Tunnel connection error: {0}")]
    TunnelError(String),

    /// Tunnel request timeout.
    #[error("Tunnel request timeout for channel {channel_id}")]
    TunnelTimeout { channel_id: u8 },

    /// Tunnel ACK error.
    #[error("Tunnel ACK error: status {status:#04x}")]
    TunnelAckError { status: u8 },

    // ========================================================================
    // Group Object Errors
    // ========================================================================
    /// Group object not found.
    #[error("Group object not found: {0}")]
    GroupObjectNotFound(String),

    /// Group object write not allowed.
    #[error("Write not allowed for group object: {0}")]
    GroupObjectWriteNotAllowed(String),

    /// Group object read not allowed.
    #[error("Read not allowed for group object: {0}")]
    GroupObjectReadNotAllowed(String),

    // ========================================================================
    // Server Errors
    // ========================================================================
    /// Server error.
    #[error("Server error: {0}")]
    Server(String),

    /// Server not running.
    #[error("Server not running")]
    ServerNotRunning,

    /// Server already running.
    #[error("Server already running")]
    ServerAlreadyRunning,

    /// Bind error.
    #[error("Failed to bind to {address}: {reason}")]
    BindError { address: SocketAddr, reason: String },

    // ========================================================================
    // Configuration Errors
    // ========================================================================
    /// Configuration error.
    #[error("Configuration error: {0}")]
    Config(String),

    /// Invalid configuration value.
    #[error("Invalid configuration value for '{field}': {reason}")]
    InvalidConfigValue { field: String, reason: String },

    // ========================================================================
    // Generic Errors
    // ========================================================================
    /// I/O error.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// Core error.
    #[error("Core error: {0}")]
    Core(#[from] CoreError),

    /// Internal error.
    #[error("Internal error: {0}")]
    Internal(String),
}

impl KnxError {
    // ========================================================================
    // Convenience constructors
    // ========================================================================

    /// Create a frame too short error.
    pub fn frame_too_short(expected: usize, actual: usize) -> Self {
        Self::FrameTooShort { expected, actual }
    }

    /// Create a connection failed error.
    pub fn connection_failed(address: SocketAddr, reason: impl Into<String>) -> Self {
        Self::ConnectionFailed {
            address,
            reason: reason.into(),
        }
    }

    /// Create a DPT encoding error.
    pub fn dpt_encoding(dpt: impl Into<String>, reason: impl Into<String>) -> Self {
        Self::DptEncoding {
            dpt: dpt.into(),
            reason: reason.into(),
        }
    }

    /// Create a DPT decoding error.
    pub fn dpt_decoding(dpt: impl Into<String>, reason: impl Into<String>) -> Self {
        Self::DptDecoding {
            dpt: dpt.into(),
            reason: reason.into(),
        }
    }

    /// Create a sequence error.
    pub fn sequence_error(expected: u8, actual: u8) -> Self {
        Self::SequenceError { expected, actual }
    }

    // ========================================================================
    // Error categorization
    // ========================================================================

    /// Check if this is a recoverable error.
    pub fn is_recoverable(&self) -> bool {
        matches!(
            self,
            Self::ConnectionTimeout { .. }
                | Self::TunnelTimeout { .. }
                | Self::SequenceError { .. }
                | Self::ConnectionClosed(_)
        )
    }

    /// Check if this is a protocol error.
    pub fn is_protocol_error(&self) -> bool {
        matches!(
            self,
            Self::FrameTooShort { .. }
                | Self::InvalidHeader(_)
                | Self::InvalidProtocolVersion { .. }
                | Self::UnknownServiceType(_)
                | Self::FrameLengthMismatch { .. }
                | Self::UnknownMessageCode(_)
                | Self::InvalidCemi(_)
                | Self::UnknownApci(_)
        )
    }

    /// Check if this is a configuration error.
    pub fn is_config_error(&self) -> bool {
        matches!(self, Self::Config(_) | Self::InvalidConfigValue { .. })
    }
}

impl From<KnxError> for CoreError {
    fn from(err: KnxError) -> Self {
        CoreError::Protocol(err.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_display() {
        let err = KnxError::InvalidGroupAddress("invalid".to_string());
        assert!(err.to_string().contains("Invalid group address"));
    }

    #[test]
    fn test_frame_too_short() {
        let err = KnxError::frame_too_short(10, 5);
        assert!(err.to_string().contains("10"));
        assert!(err.to_string().contains("5"));
    }

    #[test]
    fn test_is_recoverable() {
        assert!(KnxError::ConnectionTimeout { timeout_ms: 1000 }.is_recoverable());
        assert!(!KnxError::InvalidGroupAddress("x".into()).is_recoverable());
    }

    #[test]
    fn test_is_protocol_error() {
        assert!(KnxError::UnknownServiceType(0x1234).is_protocol_error());
        assert!(!KnxError::Server("test".into()).is_protocol_error());
    }
}

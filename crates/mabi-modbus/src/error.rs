//! Modbus error types.

use mabi_core::Error as CoreError;
use thiserror::Error;

/// Modbus result type.
pub type ModbusResult<T> = Result<T, ModbusError>;

/// Modbus error types.
#[derive(Debug, Error)]
pub enum ModbusError {
    /// Invalid function code.
    #[error("Invalid function code: {0}")]
    InvalidFunction(u8),

    /// Invalid address.
    #[error("Invalid address: {address} (range: 0-{max})")]
    InvalidAddress { address: u16, max: u16 },

    /// Invalid quantity.
    #[error("Invalid quantity: {quantity} (range: 1-{max})")]
    InvalidQuantity { quantity: u16, max: u16 },

    /// Invalid data.
    #[error("Invalid data: {0}")]
    InvalidData(String),

    /// Device not found.
    #[error("Device not found: unit {unit_id}")]
    DeviceNotFound { unit_id: u8 },

    /// Server error.
    #[error("Server error: {0}")]
    Server(String),

    /// Connection error.
    #[error("Connection error: {0}")]
    Connection(String),

    /// I/O error.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// Core error.
    #[error("Core error: {0}")]
    Core(#[from] CoreError),

    /// Internal error (for server implementation details).
    #[error("Internal error: {0}")]
    Internal(String),

    /// Invalid unit ID.
    #[error("Invalid unit ID {unit_id}: {reason}")]
    InvalidUnitId { unit_id: u8, reason: String },

    /// Unit not found.
    #[error("Unit not found: {unit_id}")]
    UnitNotFound { unit_id: u8 },

    /// Unit already exists.
    #[error("Unit already exists: {unit_id}")]
    UnitAlreadyExists { unit_id: u8 },

    /// Unit limit reached.
    #[error("Unit limit reached: maximum {max} units allowed")]
    UnitLimitReached { max: usize },

    /// Unit disabled.
    #[error("Unit {unit_id} is disabled")]
    UnitDisabled { unit_id: u8 },

    /// Configuration error.
    #[error("Configuration error: {0}")]
    Config(String),
}

impl ModbusError {
    /// Convert to Modbus exception code.
    pub fn to_exception_code(&self) -> u8 {
        match self {
            Self::InvalidFunction(_) => 0x01,       // Illegal Function
            Self::InvalidAddress { .. } => 0x02,    // Illegal Data Address
            Self::InvalidQuantity { .. } => 0x03,   // Illegal Data Value
            Self::InvalidData(_) => 0x03,           // Illegal Data Value
            Self::DeviceNotFound { .. } => 0x0B,    // Gateway Target Device Failed to Respond
            Self::Server(_) => 0x04,                // Server Device Failure
            Self::Connection(_) => 0x0A,            // Gateway Path Unavailable
            Self::Io(_) => 0x04,                    // Server Device Failure
            Self::Core(_) => 0x04,                  // Server Device Failure
            Self::Internal(_) => 0x04,              // Server Device Failure
            Self::InvalidUnitId { .. } => 0x0B,     // Gateway Target Device Failed to Respond
            Self::UnitNotFound { .. } => 0x0B,      // Gateway Target Device Failed to Respond
            Self::UnitAlreadyExists { .. } => 0x04, // Server Device Failure
            Self::UnitLimitReached { .. } => 0x04,  // Server Device Failure
            Self::UnitDisabled { .. } => 0x0B,      // Gateway Target Device Failed to Respond
            Self::Config(_) => 0x04,                // Server Device Failure
        }
    }

    /// Create an invalid address error.
    pub fn invalid_address(address: u16, max: u16) -> Self {
        Self::InvalidAddress { address, max }
    }

    /// Create an invalid quantity error.
    pub fn invalid_quantity(quantity: u16, max: u16) -> Self {
        Self::InvalidQuantity { quantity, max }
    }
}

impl From<ModbusError> for CoreError {
    fn from(err: ModbusError) -> Self {
        CoreError::Protocol(err.to_string())
    }
}

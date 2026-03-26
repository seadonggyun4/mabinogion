//! Data type definitions for Modbus register values.
//!
//! This module defines the `RegisterDataType` enum which represents all
//! supported data types that can be stored in Modbus registers.

use serde::{Deserialize, Serialize};
use std::fmt;

/// Supported data types for Modbus register values.
///
/// Each type specifies how bytes in registers should be interpreted.
/// Multi-register types (32-bit, 64-bit, String) span multiple consecutive
/// registers.
///
/// # Register Counts
///
/// | Type     | Registers | Bits | Range/Precision            |
/// |----------|-----------|------|----------------------------|
/// | Bool     | 1         | 1    | true/false (0x0000/0xFF00) |
/// | Int16    | 1         | 16   | -32,768 to 32,767          |
/// | UInt16   | 1         | 16   | 0 to 65,535                |
/// | Int32    | 2         | 32   | -2^31 to 2^31-1            |
/// | UInt32   | 2         | 32   | 0 to 2^32-1                |
/// | Float32  | 2         | 32   | IEEE 754 single precision  |
/// | Int64    | 4         | 64   | -2^63 to 2^63-1            |
/// | UInt64   | 4         | 64   | 0 to 2^64-1                |
/// | Float64  | 4         | 64   | IEEE 754 double precision  |
/// | String(n)| n         | 16*n | ASCII/UTF-8 encoded        |
///
/// # Example
///
/// ```rust
/// use mabi_modbus::types::RegisterDataType;
///
/// let data_type = RegisterDataType::Float32;
/// assert_eq!(data_type.register_count(), 2);
/// assert_eq!(data_type.byte_count(), 4);
/// assert!(data_type.is_numeric());
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RegisterDataType {
    /// Boolean value (1 register).
    ///
    /// In Modbus, typically 0x0000 = false, 0xFF00 = true
    Bool,

    /// 16-bit signed integer (1 register).
    Int16,

    /// 16-bit unsigned integer (1 register).
    UInt16,

    /// 32-bit signed integer (2 registers).
    Int32,

    /// 32-bit unsigned integer (2 registers).
    UInt32,

    /// 32-bit IEEE 754 floating point (2 registers).
    Float32,

    /// 64-bit signed integer (4 registers).
    Int64,

    /// 64-bit unsigned integer (4 registers).
    UInt64,

    /// 64-bit IEEE 754 floating point (4 registers).
    Float64,

    /// Fixed-length ASCII/UTF-8 string (N registers).
    ///
    /// Each register holds 2 bytes (characters).
    /// The parameter specifies the number of registers.
    #[serde(rename = "string")]
    String(u16),

    /// Raw bytes (N registers).
    ///
    /// Each register holds 2 bytes.
    /// The parameter specifies the number of registers.
    #[serde(rename = "bytes")]
    Bytes(u16),
}

impl Default for RegisterDataType {
    /// Returns `UInt16` as the default type (single register).
    fn default() -> Self {
        Self::UInt16
    }
}

impl RegisterDataType {
    /// Returns the number of 16-bit registers required for this type.
    ///
    /// # Example
    ///
    /// ```rust
    /// use mabi_modbus::types::RegisterDataType;
    ///
    /// assert_eq!(RegisterDataType::Bool.register_count(), 1);
    /// assert_eq!(RegisterDataType::Float32.register_count(), 2);
    /// assert_eq!(RegisterDataType::Float64.register_count(), 4);
    /// assert_eq!(RegisterDataType::String(10).register_count(), 10);
    /// ```
    pub fn register_count(&self) -> u16 {
        match self {
            Self::Bool | Self::Int16 | Self::UInt16 => 1,
            Self::Int32 | Self::UInt32 | Self::Float32 => 2,
            Self::Int64 | Self::UInt64 | Self::Float64 => 4,
            Self::String(n) | Self::Bytes(n) => *n,
        }
    }

    /// Returns the number of bytes required for this type.
    pub fn byte_count(&self) -> usize {
        (self.register_count() * 2) as usize
    }

    /// Returns true if this is a single-register type.
    pub fn is_single_register(&self) -> bool {
        self.register_count() == 1
    }

    /// Returns true if this is a multi-register type.
    pub fn is_multi_register(&self) -> bool {
        self.register_count() > 1
    }

    /// Returns true if this is a numeric type.
    pub fn is_numeric(&self) -> bool {
        !matches!(self, Self::String(_) | Self::Bytes(_) | Self::Bool)
    }

    /// Returns true if this is an integer type.
    pub fn is_integer(&self) -> bool {
        matches!(
            self,
            Self::Int16 | Self::UInt16 | Self::Int32 | Self::UInt32 | Self::Int64 | Self::UInt64
        )
    }

    /// Returns true if this is a floating-point type.
    pub fn is_float(&self) -> bool {
        matches!(self, Self::Float32 | Self::Float64)
    }

    /// Returns true if this is a signed type.
    pub fn is_signed(&self) -> bool {
        matches!(
            self,
            Self::Int16 | Self::Int32 | Self::Int64 | Self::Float32 | Self::Float64
        )
    }

    /// Returns the minimum value for integer types.
    ///
    /// Returns `None` for non-integer types.
    pub fn min_value(&self) -> Option<i128> {
        match self {
            Self::Bool => Some(0),
            Self::Int16 => Some(i16::MIN as i128),
            Self::UInt16 => Some(0),
            Self::Int32 => Some(i32::MIN as i128),
            Self::UInt32 => Some(0),
            Self::Int64 => Some(i64::MIN as i128),
            Self::UInt64 => Some(0),
            _ => None,
        }
    }

    /// Returns the maximum value for integer types.
    ///
    /// Returns `None` for non-integer types.
    pub fn max_value(&self) -> Option<i128> {
        match self {
            Self::Bool => Some(1),
            Self::Int16 => Some(i16::MAX as i128),
            Self::UInt16 => Some(u16::MAX as i128),
            Self::Int32 => Some(i32::MAX as i128),
            Self::UInt32 => Some(u32::MAX as i128),
            Self::Int64 => Some(i64::MAX as i128),
            Self::UInt64 => Some(u64::MAX as i128),
            _ => None,
        }
    }

    /// Create a String type with the given character capacity.
    ///
    /// # Arguments
    ///
    /// * `chars` - Maximum number of characters (2 per register)
    ///
    /// # Example
    ///
    /// ```rust
    /// use mabi_modbus::types::RegisterDataType;
    ///
    /// // 20-character string = 10 registers
    /// let str_type = RegisterDataType::string_with_chars(20);
    /// assert_eq!(str_type.register_count(), 10);
    /// ```
    pub fn string_with_chars(chars: u16) -> Self {
        Self::String((chars + 1) / 2) // Round up to account for 2 bytes per register
    }

    /// Create a Bytes type with the given byte capacity.
    pub fn bytes_with_capacity(bytes: u16) -> Self {
        Self::Bytes((bytes + 1) / 2)
    }

    /// Get a human-readable name for this type.
    pub fn name(&self) -> &'static str {
        match self {
            Self::Bool => "Bool",
            Self::Int16 => "Int16",
            Self::UInt16 => "UInt16",
            Self::Int32 => "Int32",
            Self::UInt32 => "UInt32",
            Self::Float32 => "Float32",
            Self::Int64 => "Int64",
            Self::UInt64 => "UInt64",
            Self::Float64 => "Float64",
            Self::String(_) => "String",
            Self::Bytes(_) => "Bytes",
        }
    }

    /// All basic (non-variable-length) data types.
    pub fn basic_types() -> &'static [RegisterDataType] {
        &[
            Self::Bool,
            Self::Int16,
            Self::UInt16,
            Self::Int32,
            Self::UInt32,
            Self::Float32,
            Self::Int64,
            Self::UInt64,
            Self::Float64,
        ]
    }
}

impl fmt::Display for RegisterDataType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Bool => write!(f, "Bool"),
            Self::Int16 => write!(f, "Int16"),
            Self::UInt16 => write!(f, "UInt16"),
            Self::Int32 => write!(f, "Int32"),
            Self::UInt32 => write!(f, "UInt32"),
            Self::Float32 => write!(f, "Float32"),
            Self::Int64 => write!(f, "Int64"),
            Self::UInt64 => write!(f, "UInt64"),
            Self::Float64 => write!(f, "Float64"),
            Self::String(n) => write!(f, "String[{}]", n * 2),
            Self::Bytes(n) => write!(f, "Bytes[{}]", n * 2),
        }
    }
}

impl std::str::FromStr for RegisterDataType {
    type Err = DataTypeParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let normalized = s.to_lowercase();

        // Check for string/bytes with length: "string[20]", "bytes[10]"
        if normalized.starts_with("string[") && normalized.ends_with(']') {
            let len_str = &normalized[7..normalized.len() - 1];
            let bytes: u16 = len_str
                .parse()
                .map_err(|_| DataTypeParseError(s.to_string()))?;
            return Ok(Self::String((bytes + 1) / 2));
        }

        if normalized.starts_with("bytes[") && normalized.ends_with(']') {
            let len_str = &normalized[6..normalized.len() - 1];
            let bytes: u16 = len_str
                .parse()
                .map_err(|_| DataTypeParseError(s.to_string()))?;
            return Ok(Self::Bytes((bytes + 1) / 2));
        }

        match normalized.as_str() {
            "bool" | "boolean" => Ok(Self::Bool),
            "int16" | "i16" | "short" => Ok(Self::Int16),
            "uint16" | "u16" | "word" | "ushort" => Ok(Self::UInt16),
            "int32" | "i32" | "int" | "dint" => Ok(Self::Int32),
            "uint32" | "u32" | "dword" | "uint" => Ok(Self::UInt32),
            "float32" | "f32" | "float" | "real" | "single" => Ok(Self::Float32),
            "int64" | "i64" | "long" | "lint" => Ok(Self::Int64),
            "uint64" | "u64" | "ulong" | "ulint" => Ok(Self::UInt64),
            "float64" | "f64" | "double" | "lreal" => Ok(Self::Float64),
            "string" => Ok(Self::String(16)), // Default 32-char string
            "bytes" => Ok(Self::Bytes(8)),    // Default 16-byte buffer
            _ => Err(DataTypeParseError(s.to_string())),
        }
    }
}

/// Error when parsing a data type from string.
#[derive(Debug, Clone)]
pub struct DataTypeParseError(String);

impl fmt::Display for DataTypeParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Invalid data type '{}'. Valid options: bool, int16, uint16, int32, uint32, float32, int64, uint64, float64, string[n], bytes[n]",
            self.0
        )
    }
}

impl std::error::Error for DataTypeParseError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_register_count() {
        assert_eq!(RegisterDataType::Bool.register_count(), 1);
        assert_eq!(RegisterDataType::Int16.register_count(), 1);
        assert_eq!(RegisterDataType::UInt16.register_count(), 1);
        assert_eq!(RegisterDataType::Int32.register_count(), 2);
        assert_eq!(RegisterDataType::UInt32.register_count(), 2);
        assert_eq!(RegisterDataType::Float32.register_count(), 2);
        assert_eq!(RegisterDataType::Int64.register_count(), 4);
        assert_eq!(RegisterDataType::UInt64.register_count(), 4);
        assert_eq!(RegisterDataType::Float64.register_count(), 4);
        assert_eq!(RegisterDataType::String(10).register_count(), 10);
        assert_eq!(RegisterDataType::Bytes(5).register_count(), 5);
    }

    #[test]
    fn test_is_flags() {
        assert!(!RegisterDataType::Bool.is_numeric());
        assert!(RegisterDataType::Int16.is_integer());
        assert!(RegisterDataType::Float32.is_float());
        assert!(RegisterDataType::Int32.is_signed());
        assert!(!RegisterDataType::UInt32.is_signed());
    }

    #[test]
    fn test_string_with_chars() {
        assert_eq!(RegisterDataType::string_with_chars(20).register_count(), 10);
        assert_eq!(RegisterDataType::string_with_chars(21).register_count(), 11);
    }

    #[test]
    fn test_parse() {
        assert_eq!(
            "bool".parse::<RegisterDataType>().unwrap(),
            RegisterDataType::Bool
        );
        assert_eq!(
            "float32".parse::<RegisterDataType>().unwrap(),
            RegisterDataType::Float32
        );
        assert_eq!(
            "real".parse::<RegisterDataType>().unwrap(),
            RegisterDataType::Float32
        );
        assert_eq!(
            "i32".parse::<RegisterDataType>().unwrap(),
            RegisterDataType::Int32
        );
        assert_eq!(
            "string[20]".parse::<RegisterDataType>().unwrap(),
            RegisterDataType::String(10)
        );
    }

    #[test]
    fn test_display() {
        assert_eq!(format!("{}", RegisterDataType::Float32), "Float32");
        assert_eq!(format!("{}", RegisterDataType::String(10)), "String[20]");
    }

    #[test]
    fn test_serde() {
        let dt = RegisterDataType::Float32;
        let json = serde_json::to_string(&dt).unwrap();
        assert_eq!(json, "\"float32\"");

        let parsed: RegisterDataType = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, dt);
    }

    #[test]
    fn test_min_max_values() {
        assert_eq!(RegisterDataType::Int16.min_value(), Some(i16::MIN as i128));
        assert_eq!(RegisterDataType::Int16.max_value(), Some(i16::MAX as i128));
        assert_eq!(RegisterDataType::UInt16.min_value(), Some(0));
        assert_eq!(RegisterDataType::UInt16.max_value(), Some(u16::MAX as i128));
        assert!(RegisterDataType::Float32.min_value().is_none());
    }
}

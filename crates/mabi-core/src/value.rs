//! Value types for data points.

use serde::{Deserialize, Serialize};
use std::fmt;

/// Dynamic value type for data points.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
#[derive(Default)]
pub enum Value {
    /// Boolean value.
    Bool(bool),
    /// 8-bit signed integer.
    I8(i8),
    /// 8-bit unsigned integer.
    U8(u8),
    /// 16-bit signed integer.
    I16(i16),
    /// 16-bit unsigned integer.
    U16(u16),
    /// 32-bit signed integer.
    I32(i32),
    /// 32-bit unsigned integer.
    U32(u32),
    /// 64-bit signed integer.
    I64(i64),
    /// 64-bit unsigned integer.
    U64(u64),
    /// 32-bit floating point.
    F32(f32),
    /// 64-bit floating point.
    F64(f64),
    /// String value.
    String(String),
    /// Binary data.
    Bytes(Vec<u8>),
    /// Array of values.
    Array(Vec<Value>),
    /// Null/empty value.
    #[default]
    Null,
}

impl Value {
    /// Create a new boolean value.
    pub fn bool(v: bool) -> Self {
        Self::Bool(v)
    }

    /// Create a new float value.
    pub fn float(v: f64) -> Self {
        Self::F64(v)
    }

    /// Create a new integer value.
    pub fn int(v: i64) -> Self {
        Self::I64(v)
    }

    /// Create a new string value.
    pub fn string(v: impl Into<String>) -> Self {
        Self::String(v.into())
    }

    /// Get value as boolean.
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Self::Bool(v) => Some(*v),
            Self::I8(v) => Some(*v != 0),
            Self::U8(v) => Some(*v != 0),
            Self::I16(v) => Some(*v != 0),
            Self::U16(v) => Some(*v != 0),
            Self::I32(v) => Some(*v != 0),
            Self::U32(v) => Some(*v != 0),
            Self::I64(v) => Some(*v != 0),
            Self::U64(v) => Some(*v != 0),
            _ => None,
        }
    }

    /// Get value as f64.
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Self::Bool(v) => Some(if *v { 1.0 } else { 0.0 }),
            Self::I8(v) => Some(*v as f64),
            Self::U8(v) => Some(*v as f64),
            Self::I16(v) => Some(*v as f64),
            Self::U16(v) => Some(*v as f64),
            Self::I32(v) => Some(*v as f64),
            Self::U32(v) => Some(*v as f64),
            Self::I64(v) => Some(*v as f64),
            Self::U64(v) => Some(*v as f64),
            Self::F32(v) => Some(*v as f64),
            Self::F64(v) => Some(*v),
            _ => None,
        }
    }

    /// Get value as i64.
    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Self::Bool(v) => Some(if *v { 1 } else { 0 }),
            Self::I8(v) => Some(*v as i64),
            Self::U8(v) => Some(*v as i64),
            Self::I16(v) => Some(*v as i64),
            Self::U16(v) => Some(*v as i64),
            Self::I32(v) => Some(*v as i64),
            Self::U32(v) => Some(*v as i64),
            Self::I64(v) => Some(*v),
            Self::U64(v) => i64::try_from(*v).ok(),
            Self::F32(v) => Some(*v as i64),
            Self::F64(v) => Some(*v as i64),
            _ => None,
        }
    }

    /// Get value as u16.
    pub fn as_u16(&self) -> Option<u16> {
        self.as_i64().and_then(|v| u16::try_from(v).ok())
    }

    /// Get value as string.
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::String(v) => Some(v.as_str()),
            _ => None,
        }
    }

    /// Get value as bytes.
    pub fn as_bytes(&self) -> Option<&[u8]> {
        match self {
            Self::Bytes(v) => Some(v.as_slice()),
            Self::String(v) => Some(v.as_bytes()),
            _ => None,
        }
    }

    /// Check if value is numeric.
    pub fn is_numeric(&self) -> bool {
        matches!(
            self,
            Self::I8(_)
                | Self::U8(_)
                | Self::I16(_)
                | Self::U16(_)
                | Self::I32(_)
                | Self::U32(_)
                | Self::I64(_)
                | Self::U64(_)
                | Self::F32(_)
                | Self::F64(_)
        )
    }

    /// Check if value is null.
    pub fn is_null(&self) -> bool {
        matches!(self, Self::Null)
    }

    /// Get the type name of this value.
    pub fn type_name(&self) -> &'static str {
        match self {
            Self::Bool(_) => "bool",
            Self::I8(_) => "i8",
            Self::U8(_) => "u8",
            Self::I16(_) => "i16",
            Self::U16(_) => "u16",
            Self::I32(_) => "i32",
            Self::U32(_) => "u32",
            Self::I64(_) => "i64",
            Self::U64(_) => "u64",
            Self::F32(_) => "f32",
            Self::F64(_) => "f64",
            Self::String(_) => "string",
            Self::Bytes(_) => "bytes",
            Self::Array(_) => "array",
            Self::Null => "null",
        }
    }

    /// Convert to u16 registers for Modbus.
    pub fn to_registers(&self) -> Vec<u16> {
        match self {
            Self::Bool(v) => vec![if *v { 1 } else { 0 }],
            Self::I8(v) => vec![*v as u16],
            Self::U8(v) => vec![*v as u16],
            Self::I16(v) => vec![*v as u16],
            Self::U16(v) => vec![*v],
            Self::I32(v) => {
                let bytes = v.to_be_bytes();
                vec![
                    u16::from_be_bytes([bytes[0], bytes[1]]),
                    u16::from_be_bytes([bytes[2], bytes[3]]),
                ]
            }
            Self::U32(v) => {
                let bytes = v.to_be_bytes();
                vec![
                    u16::from_be_bytes([bytes[0], bytes[1]]),
                    u16::from_be_bytes([bytes[2], bytes[3]]),
                ]
            }
            Self::F32(v) => {
                let bytes = v.to_be_bytes();
                vec![
                    u16::from_be_bytes([bytes[0], bytes[1]]),
                    u16::from_be_bytes([bytes[2], bytes[3]]),
                ]
            }
            Self::F64(v) => {
                let bytes = v.to_be_bytes();
                vec![
                    u16::from_be_bytes([bytes[0], bytes[1]]),
                    u16::from_be_bytes([bytes[2], bytes[3]]),
                    u16::from_be_bytes([bytes[4], bytes[5]]),
                    u16::from_be_bytes([bytes[6], bytes[7]]),
                ]
            }
            _ => vec![],
        }
    }

    /// Create value from u16 registers (for Modbus).
    pub fn from_registers(registers: &[u16], value_type: ValueType) -> Option<Self> {
        match value_type {
            ValueType::Bool => registers.first().map(|r| Value::Bool(*r != 0)),
            ValueType::U16 => registers.first().map(|r| Value::U16(*r)),
            ValueType::I16 => registers.first().map(|r| Value::I16(*r as i16)),
            ValueType::U32 if registers.len() >= 2 => {
                let bytes = [registers[0].to_be_bytes(), registers[1].to_be_bytes()];
                Some(Value::U32(u32::from_be_bytes([
                    bytes[0][0],
                    bytes[0][1],
                    bytes[1][0],
                    bytes[1][1],
                ])))
            }
            ValueType::I32 if registers.len() >= 2 => {
                let bytes = [registers[0].to_be_bytes(), registers[1].to_be_bytes()];
                Some(Value::I32(i32::from_be_bytes([
                    bytes[0][0],
                    bytes[0][1],
                    bytes[1][0],
                    bytes[1][1],
                ])))
            }
            ValueType::F32 if registers.len() >= 2 => {
                let bytes = [registers[0].to_be_bytes(), registers[1].to_be_bytes()];
                Some(Value::F32(f32::from_be_bytes([
                    bytes[0][0],
                    bytes[0][1],
                    bytes[1][0],
                    bytes[1][1],
                ])))
            }
            ValueType::F64 if registers.len() >= 4 => {
                let bytes: Vec<u8> = registers[..4]
                    .iter()
                    .flat_map(|r| r.to_be_bytes())
                    .collect();
                Some(Value::F64(f64::from_be_bytes([
                    bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
                ])))
            }
            _ => None,
        }
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Bool(v) => write!(f, "{}", v),
            Self::I8(v) => write!(f, "{}", v),
            Self::U8(v) => write!(f, "{}", v),
            Self::I16(v) => write!(f, "{}", v),
            Self::U16(v) => write!(f, "{}", v),
            Self::I32(v) => write!(f, "{}", v),
            Self::U32(v) => write!(f, "{}", v),
            Self::I64(v) => write!(f, "{}", v),
            Self::U64(v) => write!(f, "{}", v),
            Self::F32(v) => write!(f, "{:.3}", v),
            Self::F64(v) => write!(f, "{:.6}", v),
            Self::String(v) => write!(f, "{}", v),
            Self::Bytes(v) => write!(f, "[{} bytes]", v.len()),
            Self::Array(v) => write!(f, "[{} items]", v.len()),
            Self::Null => write!(f, "null"),
        }
    }
}


// Conversions
impl From<bool> for Value {
    fn from(v: bool) -> Self {
        Self::Bool(v)
    }
}

impl From<i32> for Value {
    fn from(v: i32) -> Self {
        Self::I32(v)
    }
}

impl From<u32> for Value {
    fn from(v: u32) -> Self {
        Self::U32(v)
    }
}

impl From<i64> for Value {
    fn from(v: i64) -> Self {
        Self::I64(v)
    }
}

impl From<u64> for Value {
    fn from(v: u64) -> Self {
        Self::U64(v)
    }
}

impl From<f32> for Value {
    fn from(v: f32) -> Self {
        Self::F32(v)
    }
}

impl From<f64> for Value {
    fn from(v: f64) -> Self {
        Self::F64(v)
    }
}

impl From<String> for Value {
    fn from(v: String) -> Self {
        Self::String(v)
    }
}

impl From<&str> for Value {
    fn from(v: &str) -> Self {
        Self::String(v.to_string())
    }
}

impl From<Vec<u8>> for Value {
    fn from(v: Vec<u8>) -> Self {
        Self::Bytes(v)
    }
}

/// Value types for conversion.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ValueType {
    Bool,
    I8,
    U8,
    I16,
    U16,
    I32,
    U32,
    I64,
    U64,
    F32,
    F64,
    String,
    Bytes,
}

impl ValueType {
    /// Get the size in bytes for this type.
    pub fn size_bytes(&self) -> usize {
        match self {
            Self::Bool | Self::I8 | Self::U8 => 1,
            Self::I16 | Self::U16 => 2,
            Self::I32 | Self::U32 | Self::F32 => 4,
            Self::I64 | Self::U64 | Self::F64 => 8,
            Self::String | Self::Bytes => 0, // Variable
        }
    }

    /// Get the number of registers for Modbus.
    pub fn register_count(&self) -> usize {
        match self {
            Self::Bool | Self::I8 | Self::U8 | Self::I16 | Self::U16 => 1,
            Self::I32 | Self::U32 | Self::F32 => 2,
            Self::I64 | Self::U64 | Self::F64 => 4,
            Self::String | Self::Bytes => 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_value_conversion() {
        let v = Value::F64(123.456);
        assert!((v.as_f64().unwrap() - 123.456).abs() < 0.001);
        assert_eq!(v.as_i64(), Some(123));
    }

    #[test]
    fn test_value_to_registers() {
        let v = Value::F32(1.5);
        let regs = v.to_registers();
        assert_eq!(regs.len(), 2);

        let v2 = Value::from_registers(&regs, ValueType::F32).unwrap();
        assert!((v2.as_f64().unwrap() - 1.5).abs() < 0.001);
    }

    #[test]
    fn test_value_display() {
        assert_eq!(Value::Bool(true).to_string(), "true");
        assert_eq!(Value::I32(42).to_string(), "42");
        assert_eq!(Value::F64(3.14159).to_string(), "3.141590");
    }
}

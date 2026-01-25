//! OPC UA Variant type implementation.
//!
//! Variant is a union type that can hold any OPC UA built-in data type.

use std::fmt;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::types::NodeId;
use mabi_core::Value;

/// Data type IDs matching OPC UA specification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[repr(u8)]
pub enum DataTypeId {
    Null = 0,
    Boolean = 1,
    SByte = 2,
    Byte = 3,
    Int16 = 4,
    UInt16 = 5,
    Int32 = 6,
    UInt32 = 7,
    Int64 = 8,
    UInt64 = 9,
    Float = 10,
    Double = 11,
    String = 12,
    DateTime = 13,
    Guid = 14,
    ByteString = 15,
    XmlElement = 16,
    NodeId = 17,
    ExpandedNodeId = 18,
    StatusCode = 19,
    QualifiedName = 20,
    LocalizedText = 21,
    ExtensionObject = 22,
    DataValue = 23,
    Variant = 24,
    DiagnosticInfo = 25,
}

impl DataTypeId {
    /// Get the data type ID from a numeric value.
    pub fn from_u8(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::Null),
            1 => Some(Self::Boolean),
            2 => Some(Self::SByte),
            3 => Some(Self::Byte),
            4 => Some(Self::Int16),
            5 => Some(Self::UInt16),
            6 => Some(Self::Int32),
            7 => Some(Self::UInt32),
            8 => Some(Self::Int64),
            9 => Some(Self::UInt64),
            10 => Some(Self::Float),
            11 => Some(Self::Double),
            12 => Some(Self::String),
            13 => Some(Self::DateTime),
            14 => Some(Self::Guid),
            15 => Some(Self::ByteString),
            16 => Some(Self::XmlElement),
            17 => Some(Self::NodeId),
            18 => Some(Self::ExpandedNodeId),
            19 => Some(Self::StatusCode),
            20 => Some(Self::QualifiedName),
            21 => Some(Self::LocalizedText),
            22 => Some(Self::ExtensionObject),
            23 => Some(Self::DataValue),
            24 => Some(Self::Variant),
            25 => Some(Self::DiagnosticInfo),
            _ => None,
        }
    }
}

/// Scalar variant values.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum VariantScalarValue {
    /// Null/empty value.
    Null,
    /// Boolean.
    Boolean(bool),
    /// Signed byte (-128 to 127).
    SByte(i8),
    /// Unsigned byte (0 to 255).
    Byte(u8),
    /// Signed 16-bit integer.
    Int16(i16),
    /// Unsigned 16-bit integer.
    UInt16(u16),
    /// Signed 32-bit integer.
    Int32(i32),
    /// Unsigned 32-bit integer.
    UInt32(u32),
    /// Signed 64-bit integer.
    Int64(i64),
    /// Unsigned 64-bit integer.
    UInt64(u64),
    /// 32-bit floating point.
    Float(f32),
    /// 64-bit floating point.
    Double(f64),
    /// UTF-8 string.
    String(String),
    /// Date and time.
    DateTime(DateTime<Utc>),
    /// GUID.
    Guid(Uuid),
    /// Byte string (binary data).
    ByteString(Vec<u8>),
    /// XML element (stored as string).
    XmlElement(String),
    /// Node ID.
    NodeId(NodeId),
    /// Status code.
    StatusCode(u32),
}

/// Array variant values.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum VariantArrayValue {
    Boolean(Vec<bool>),
    SByte(Vec<i8>),
    Byte(Vec<u8>),
    Int16(Vec<i16>),
    UInt16(Vec<u16>),
    Int32(Vec<i32>),
    UInt32(Vec<u32>),
    Int64(Vec<i64>),
    UInt64(Vec<u64>),
    Float(Vec<f32>),
    Double(Vec<f64>),
    String(Vec<String>),
    DateTime(Vec<DateTime<Utc>>),
    Guid(Vec<Uuid>),
    ByteString(Vec<Vec<u8>>),
    NodeId(Vec<NodeId>),
    StatusCode(Vec<u32>),
}

/// OPC UA Variant type.
///
/// A variant can hold either a scalar value or an array of values.
/// It represents the union of all OPC UA built-in data types.
///
/// # Examples
///
/// ```
/// use mabi_opcua::types::Variant;
///
/// // Create scalar variants
/// let bool_var = Variant::boolean(true);
/// let int_var = Variant::int32(42);
/// let float_var = Variant::double(3.14);
///
/// // Create array variants
/// let int_array = Variant::int32_array(vec![1, 2, 3, 4, 5]);
/// ```
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Variant {
    // Scalar shortcuts (for backwards compatibility)
    /// Boolean value.
    Boolean(bool),
    /// Signed byte.
    SByte(i8),
    /// Unsigned byte.
    Byte(u8),
    /// Signed 16-bit integer.
    Int16(i16),
    /// Unsigned 16-bit integer.
    UInt16(u16),
    /// Signed 32-bit integer.
    Int32(i32),
    /// Unsigned 32-bit integer.
    UInt32(u32),
    /// Signed 64-bit integer.
    Int64(i64),
    /// Unsigned 64-bit integer.
    UInt64(u64),
    /// 32-bit floating point.
    Float(f32),
    /// 64-bit floating point.
    Double(f64),
    /// UTF-8 string.
    String(String),
    /// Date and time.
    DateTime(chrono::DateTime<Utc>),
    /// GUID.
    Guid(Uuid),
    /// Byte string (binary data).
    ByteString(Vec<u8>),
    /// Node ID.
    NodeId(NodeId),
    /// Status code.
    StatusCode(u32),
    /// Null value.
    Null,
    /// Scalar value (wrapped).
    Scalar(VariantScalarValue),
    /// Array value.
    Array(VariantArrayValue),
}

impl Variant {
    // ==========================================================================
    // Scalar Constructors
    // ==========================================================================

    /// Create a null variant.
    pub fn null() -> Self {
        Self::Null
    }

    /// Create a boolean variant.
    pub fn boolean(value: bool) -> Self {
        Self::Boolean(value)
    }

    /// Create a signed byte variant.
    pub fn sbyte(value: i8) -> Self {
        Self::SByte(value)
    }

    /// Create an unsigned byte variant.
    pub fn byte(value: u8) -> Self {
        Self::Byte(value)
    }

    /// Create a signed 16-bit integer variant.
    pub fn int16(value: i16) -> Self {
        Self::Int16(value)
    }

    /// Create an unsigned 16-bit integer variant.
    pub fn uint16(value: u16) -> Self {
        Self::UInt16(value)
    }

    /// Create a signed 32-bit integer variant.
    pub fn int32(value: i32) -> Self {
        Self::Int32(value)
    }

    /// Create an unsigned 32-bit integer variant.
    pub fn uint32(value: u32) -> Self {
        Self::UInt32(value)
    }

    /// Create a signed 64-bit integer variant.
    pub fn int64(value: i64) -> Self {
        Self::Int64(value)
    }

    /// Create an unsigned 64-bit integer variant.
    pub fn uint64(value: u64) -> Self {
        Self::UInt64(value)
    }

    /// Create a 32-bit float variant.
    pub fn float(value: f32) -> Self {
        Self::Float(value)
    }

    /// Create a 64-bit float variant.
    pub fn double(value: f64) -> Self {
        Self::Double(value)
    }

    /// Create a string variant.
    pub fn string(value: impl Into<String>) -> Self {
        Self::String(value.into())
    }

    /// Create a date/time variant.
    pub fn datetime(value: DateTime<Utc>) -> Self {
        Self::DateTime(value)
    }

    /// Create a GUID variant.
    pub fn guid(value: Uuid) -> Self {
        Self::Guid(value)
    }

    /// Create a byte string variant.
    pub fn byte_string(value: impl Into<Vec<u8>>) -> Self {
        Self::ByteString(value.into())
    }

    /// Create a node ID variant.
    pub fn node_id(value: NodeId) -> Self {
        Self::NodeId(value)
    }

    /// Create a status code variant.
    pub fn status_code(value: u32) -> Self {
        Self::StatusCode(value)
    }

    // ==========================================================================
    // Array Constructors
    // ==========================================================================

    /// Create a boolean array variant.
    pub fn boolean_array(values: Vec<bool>) -> Self {
        Self::Array(VariantArrayValue::Boolean(values))
    }

    /// Create an int32 array variant.
    pub fn int32_array(values: Vec<i32>) -> Self {
        Self::Array(VariantArrayValue::Int32(values))
    }

    /// Create an int64 array variant.
    pub fn int64_array(values: Vec<i64>) -> Self {
        Self::Array(VariantArrayValue::Int64(values))
    }

    /// Create a uint32 array variant.
    pub fn uint32_array(values: Vec<u32>) -> Self {
        Self::Array(VariantArrayValue::UInt32(values))
    }

    /// Create a float array variant.
    pub fn float_array(values: Vec<f32>) -> Self {
        Self::Array(VariantArrayValue::Float(values))
    }

    /// Create a double array variant.
    pub fn double_array(values: Vec<f64>) -> Self {
        Self::Array(VariantArrayValue::Double(values))
    }

    /// Create a string array variant.
    pub fn string_array(values: Vec<String>) -> Self {
        Self::Array(VariantArrayValue::String(values))
    }

    /// Create a byte array variant.
    pub fn byte_array(values: Vec<u8>) -> Self {
        Self::Array(VariantArrayValue::Byte(values))
    }

    // ==========================================================================
    // Accessors
    // ==========================================================================

    /// Check if this is a null variant.
    pub fn is_null(&self) -> bool {
        matches!(self, Self::Null | Self::Scalar(VariantScalarValue::Null))
    }

    /// Check if this is a scalar variant.
    pub fn is_scalar(&self) -> bool {
        !matches!(self, Self::Array(_))
    }

    /// Check if this is an array variant.
    pub fn is_array(&self) -> bool {
        matches!(self, Self::Array(_))
    }

    /// Get the data type ID.
    pub fn data_type_id(&self) -> DataTypeId {
        match self {
            // Direct variants
            Self::Null => DataTypeId::Null,
            Self::Boolean(_) => DataTypeId::Boolean,
            Self::SByte(_) => DataTypeId::SByte,
            Self::Byte(_) => DataTypeId::Byte,
            Self::Int16(_) => DataTypeId::Int16,
            Self::UInt16(_) => DataTypeId::UInt16,
            Self::Int32(_) => DataTypeId::Int32,
            Self::UInt32(_) => DataTypeId::UInt32,
            Self::Int64(_) => DataTypeId::Int64,
            Self::UInt64(_) => DataTypeId::UInt64,
            Self::Float(_) => DataTypeId::Float,
            Self::Double(_) => DataTypeId::Double,
            Self::String(_) => DataTypeId::String,
            Self::DateTime(_) => DataTypeId::DateTime,
            Self::Guid(_) => DataTypeId::Guid,
            Self::ByteString(_) => DataTypeId::ByteString,
            Self::NodeId(_) => DataTypeId::NodeId,
            Self::StatusCode(_) => DataTypeId::StatusCode,
            // Wrapped scalar variants
            Self::Scalar(s) => match s {
                VariantScalarValue::Null => DataTypeId::Null,
                VariantScalarValue::Boolean(_) => DataTypeId::Boolean,
                VariantScalarValue::SByte(_) => DataTypeId::SByte,
                VariantScalarValue::Byte(_) => DataTypeId::Byte,
                VariantScalarValue::Int16(_) => DataTypeId::Int16,
                VariantScalarValue::UInt16(_) => DataTypeId::UInt16,
                VariantScalarValue::Int32(_) => DataTypeId::Int32,
                VariantScalarValue::UInt32(_) => DataTypeId::UInt32,
                VariantScalarValue::Int64(_) => DataTypeId::Int64,
                VariantScalarValue::UInt64(_) => DataTypeId::UInt64,
                VariantScalarValue::Float(_) => DataTypeId::Float,
                VariantScalarValue::Double(_) => DataTypeId::Double,
                VariantScalarValue::String(_) => DataTypeId::String,
                VariantScalarValue::DateTime(_) => DataTypeId::DateTime,
                VariantScalarValue::Guid(_) => DataTypeId::Guid,
                VariantScalarValue::ByteString(_) => DataTypeId::ByteString,
                VariantScalarValue::XmlElement(_) => DataTypeId::XmlElement,
                VariantScalarValue::NodeId(_) => DataTypeId::NodeId,
                VariantScalarValue::StatusCode(_) => DataTypeId::StatusCode,
            },
            // Array variants
            Self::Array(a) => match a {
                VariantArrayValue::Boolean(_) => DataTypeId::Boolean,
                VariantArrayValue::SByte(_) => DataTypeId::SByte,
                VariantArrayValue::Byte(_) => DataTypeId::Byte,
                VariantArrayValue::Int16(_) => DataTypeId::Int16,
                VariantArrayValue::UInt16(_) => DataTypeId::UInt16,
                VariantArrayValue::Int32(_) => DataTypeId::Int32,
                VariantArrayValue::UInt32(_) => DataTypeId::UInt32,
                VariantArrayValue::Int64(_) => DataTypeId::Int64,
                VariantArrayValue::UInt64(_) => DataTypeId::UInt64,
                VariantArrayValue::Float(_) => DataTypeId::Float,
                VariantArrayValue::Double(_) => DataTypeId::Double,
                VariantArrayValue::String(_) => DataTypeId::String,
                VariantArrayValue::DateTime(_) => DataTypeId::DateTime,
                VariantArrayValue::Guid(_) => DataTypeId::Guid,
                VariantArrayValue::ByteString(_) => DataTypeId::ByteString,
                VariantArrayValue::NodeId(_) => DataTypeId::NodeId,
                VariantArrayValue::StatusCode(_) => DataTypeId::StatusCode,
            },
        }
    }

    /// Get as boolean value.
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Self::Boolean(v) => Some(*v),
            Self::Scalar(VariantScalarValue::Boolean(v)) => Some(*v),
            _ => None,
        }
    }

    /// Get as i32 value.
    pub fn as_i32(&self) -> Option<i32> {
        match self {
            Self::Int32(v) => Some(*v),
            Self::Int16(v) => Some(*v as i32),
            Self::SByte(v) => Some(*v as i32),
            Self::Scalar(VariantScalarValue::Int32(v)) => Some(*v),
            Self::Scalar(VariantScalarValue::Int16(v)) => Some(*v as i32),
            Self::Scalar(VariantScalarValue::SByte(v)) => Some(*v as i32),
            _ => None,
        }
    }

    /// Get as i64 value.
    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Self::Int64(v) => Some(*v),
            Self::Int32(v) => Some(*v as i64),
            Self::Int16(v) => Some(*v as i64),
            Self::SByte(v) => Some(*v as i64),
            Self::Scalar(VariantScalarValue::Int64(v)) => Some(*v),
            Self::Scalar(VariantScalarValue::Int32(v)) => Some(*v as i64),
            Self::Scalar(VariantScalarValue::Int16(v)) => Some(*v as i64),
            Self::Scalar(VariantScalarValue::SByte(v)) => Some(*v as i64),
            _ => None,
        }
    }

    /// Get as u32 value.
    pub fn as_u32(&self) -> Option<u32> {
        match self {
            Self::UInt32(v) => Some(*v),
            Self::UInt16(v) => Some(*v as u32),
            Self::Byte(v) => Some(*v as u32),
            Self::Scalar(VariantScalarValue::UInt32(v)) => Some(*v),
            Self::Scalar(VariantScalarValue::UInt16(v)) => Some(*v as u32),
            Self::Scalar(VariantScalarValue::Byte(v)) => Some(*v as u32),
            _ => None,
        }
    }

    /// Get as u64 value.
    pub fn as_u64(&self) -> Option<u64> {
        match self {
            Self::UInt64(v) => Some(*v),
            Self::UInt32(v) => Some(*v as u64),
            Self::UInt16(v) => Some(*v as u64),
            Self::Byte(v) => Some(*v as u64),
            Self::Scalar(VariantScalarValue::UInt64(v)) => Some(*v),
            Self::Scalar(VariantScalarValue::UInt32(v)) => Some(*v as u64),
            Self::Scalar(VariantScalarValue::UInt16(v)) => Some(*v as u64),
            Self::Scalar(VariantScalarValue::Byte(v)) => Some(*v as u64),
            _ => None,
        }
    }

    /// Get as f32 value.
    pub fn as_f32(&self) -> Option<f32> {
        match self {
            Self::Float(v) => Some(*v),
            Self::Scalar(VariantScalarValue::Float(v)) => Some(*v),
            _ => None,
        }
    }

    /// Get as f64 value.
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Self::Double(v) => Some(*v),
            Self::Float(v) => Some(*v as f64),
            Self::Scalar(VariantScalarValue::Double(v)) => Some(*v),
            Self::Scalar(VariantScalarValue::Float(v)) => Some(*v as f64),
            _ => None,
        }
    }

    /// Get as string reference.
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::String(v) => Some(v),
            Self::Scalar(VariantScalarValue::String(v)) => Some(v),
            _ => None,
        }
    }

    /// Get as DateTime value.
    pub fn as_datetime(&self) -> Option<DateTime<Utc>> {
        match self {
            Self::DateTime(v) => Some(*v),
            Self::Scalar(VariantScalarValue::DateTime(v)) => Some(*v),
            _ => None,
        }
    }

    /// Get the array length (0 for scalars).
    pub fn array_len(&self) -> usize {
        match self {
            Self::Array(a) => match a {
                VariantArrayValue::Boolean(v) => v.len(),
                VariantArrayValue::SByte(v) => v.len(),
                VariantArrayValue::Byte(v) => v.len(),
                VariantArrayValue::Int16(v) => v.len(),
                VariantArrayValue::UInt16(v) => v.len(),
                VariantArrayValue::Int32(v) => v.len(),
                VariantArrayValue::UInt32(v) => v.len(),
                VariantArrayValue::Int64(v) => v.len(),
                VariantArrayValue::UInt64(v) => v.len(),
                VariantArrayValue::Float(v) => v.len(),
                VariantArrayValue::Double(v) => v.len(),
                VariantArrayValue::String(v) => v.len(),
                VariantArrayValue::DateTime(v) => v.len(),
                VariantArrayValue::Guid(v) => v.len(),
                VariantArrayValue::ByteString(v) => v.len(),
                VariantArrayValue::NodeId(v) => v.len(),
                VariantArrayValue::StatusCode(v) => v.len(),
            },
            // All scalar variants (direct and wrapped) have length 0
            _ => 0,
        }
    }
}

impl Default for Variant {
    fn default() -> Self {
        Self::null()
    }
}

impl fmt::Display for Variant {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            // Direct variants
            Self::Null => write!(f, "null"),
            Self::Boolean(v) => write!(f, "{}", v),
            Self::SByte(v) => write!(f, "{}", v),
            Self::Byte(v) => write!(f, "{}", v),
            Self::Int16(v) => write!(f, "{}", v),
            Self::UInt16(v) => write!(f, "{}", v),
            Self::Int32(v) => write!(f, "{}", v),
            Self::UInt32(v) => write!(f, "{}", v),
            Self::Int64(v) => write!(f, "{}", v),
            Self::UInt64(v) => write!(f, "{}", v),
            Self::Float(v) => write!(f, "{}", v),
            Self::Double(v) => write!(f, "{}", v),
            Self::String(v) => write!(f, "\"{}\"", v),
            Self::DateTime(v) => write!(f, "{}", v),
            Self::Guid(v) => write!(f, "{}", v),
            Self::ByteString(v) => write!(f, "[{} bytes]", v.len()),
            Self::NodeId(v) => write!(f, "{}", v),
            Self::StatusCode(v) => write!(f, "0x{:08X}", v),
            // Wrapped scalar variants
            Self::Scalar(s) => match s {
                VariantScalarValue::Null => write!(f, "null"),
                VariantScalarValue::Boolean(v) => write!(f, "{}", v),
                VariantScalarValue::SByte(v) => write!(f, "{}", v),
                VariantScalarValue::Byte(v) => write!(f, "{}", v),
                VariantScalarValue::Int16(v) => write!(f, "{}", v),
                VariantScalarValue::UInt16(v) => write!(f, "{}", v),
                VariantScalarValue::Int32(v) => write!(f, "{}", v),
                VariantScalarValue::UInt32(v) => write!(f, "{}", v),
                VariantScalarValue::Int64(v) => write!(f, "{}", v),
                VariantScalarValue::UInt64(v) => write!(f, "{}", v),
                VariantScalarValue::Float(v) => write!(f, "{}", v),
                VariantScalarValue::Double(v) => write!(f, "{}", v),
                VariantScalarValue::String(v) => write!(f, "\"{}\"", v),
                VariantScalarValue::DateTime(v) => write!(f, "{}", v),
                VariantScalarValue::Guid(v) => write!(f, "{}", v),
                VariantScalarValue::ByteString(v) => write!(f, "[{} bytes]", v.len()),
                VariantScalarValue::XmlElement(v) => write!(f, "<xml>{}</xml>", v),
                VariantScalarValue::NodeId(v) => write!(f, "{}", v),
                VariantScalarValue::StatusCode(v) => write!(f, "0x{:08X}", v),
            },
            // Array variants
            Self::Array(a) => {
                let len = self.array_len();
                let type_name = match a {
                    VariantArrayValue::Boolean(_) => "Boolean",
                    VariantArrayValue::SByte(_) => "SByte",
                    VariantArrayValue::Byte(_) => "Byte",
                    VariantArrayValue::Int16(_) => "Int16",
                    VariantArrayValue::UInt16(_) => "UInt16",
                    VariantArrayValue::Int32(_) => "Int32",
                    VariantArrayValue::UInt32(_) => "UInt32",
                    VariantArrayValue::Int64(_) => "Int64",
                    VariantArrayValue::UInt64(_) => "UInt64",
                    VariantArrayValue::Float(_) => "Float",
                    VariantArrayValue::Double(_) => "Double",
                    VariantArrayValue::String(_) => "String",
                    VariantArrayValue::DateTime(_) => "DateTime",
                    VariantArrayValue::Guid(_) => "Guid",
                    VariantArrayValue::ByteString(_) => "ByteString",
                    VariantArrayValue::NodeId(_) => "NodeId",
                    VariantArrayValue::StatusCode(_) => "StatusCode",
                };
                write!(f, "{}[{}]", type_name, len)
            }
        }
    }
}

// ==========================================================================
// Conversions from/to mabi_core::Value
// ==========================================================================

impl From<Value> for Variant {
    fn from(value: Value) -> Self {
        match value {
            Value::Bool(v) => Variant::boolean(v),
            Value::I8(v) => Variant::sbyte(v),
            Value::I16(v) => Variant::int16(v),
            Value::I32(v) => Variant::int32(v),
            Value::I64(v) => Variant::int64(v),
            Value::U8(v) => Variant::byte(v),
            Value::U16(v) => Variant::uint16(v),
            Value::U32(v) => Variant::uint32(v),
            Value::U64(v) => Variant::uint64(v),
            Value::F32(v) => Variant::float(v),
            Value::F64(v) => Variant::double(v),
            Value::String(v) => Variant::string(v),
            Value::Bytes(v) => Variant::byte_string(v),
            Value::Null => Variant::null(),
            Value::Array(arr) => {
                // Convert array - try to determine type from first element
                if arr.is_empty() {
                    return Variant::null();
                }
                // For simplicity, convert to array of f64 (most common numeric type)
                let doubles: Vec<f64> = arr
                    .into_iter()
                    .filter_map(|v| match v {
                        Value::F64(f) => Some(f),
                        Value::F32(f) => Some(f as f64),
                        Value::I32(i) => Some(i as f64),
                        Value::I64(i) => Some(i as f64),
                        Value::U32(u) => Some(u as f64),
                        Value::U64(u) => Some(u as f64),
                        _ => None,
                    })
                    .collect();
                Variant::double_array(doubles)
            }
        }
    }
}

impl From<Variant> for Value {
    fn from(variant: Variant) -> Self {
        match variant {
            // Direct variants
            Variant::Null => Value::Null,
            Variant::Boolean(v) => Value::Bool(v),
            Variant::SByte(v) => Value::I8(v),
            Variant::Byte(v) => Value::U8(v),
            Variant::Int16(v) => Value::I16(v),
            Variant::UInt16(v) => Value::U16(v),
            Variant::Int32(v) => Value::I32(v),
            Variant::UInt32(v) => Value::U32(v),
            Variant::Int64(v) => Value::I64(v),
            Variant::UInt64(v) => Value::U64(v),
            Variant::Float(v) => Value::F32(v),
            Variant::Double(v) => Value::F64(v),
            Variant::String(v) => Value::String(v),
            Variant::DateTime(v) => Value::String(v.to_rfc3339()),
            Variant::Guid(v) => Value::String(v.to_string()),
            Variant::ByteString(v) => Value::Bytes(v),
            Variant::NodeId(v) => Value::String(v.to_string()),
            Variant::StatusCode(v) => Value::U32(v),
            // Wrapped scalar variants
            Variant::Scalar(s) => match s {
                VariantScalarValue::Null => Value::Null,
                VariantScalarValue::Boolean(v) => Value::Bool(v),
                VariantScalarValue::SByte(v) => Value::I8(v),
                VariantScalarValue::Byte(v) => Value::U8(v),
                VariantScalarValue::Int16(v) => Value::I16(v),
                VariantScalarValue::UInt16(v) => Value::U16(v),
                VariantScalarValue::Int32(v) => Value::I32(v),
                VariantScalarValue::UInt32(v) => Value::U32(v),
                VariantScalarValue::Int64(v) => Value::I64(v),
                VariantScalarValue::UInt64(v) => Value::U64(v),
                VariantScalarValue::Float(v) => Value::F32(v),
                VariantScalarValue::Double(v) => Value::F64(v),
                VariantScalarValue::String(v) => Value::String(v),
                VariantScalarValue::DateTime(v) => Value::String(v.to_rfc3339()),
                VariantScalarValue::Guid(v) => Value::String(v.to_string()),
                VariantScalarValue::ByteString(v) => Value::Bytes(v),
                VariantScalarValue::XmlElement(v) => Value::String(v),
                VariantScalarValue::NodeId(v) => Value::String(v.to_string()),
                VariantScalarValue::StatusCode(v) => Value::U32(v),
            },
            // Array variants
            Variant::Array(a) => {
                // For arrays, convert to JSON string representation
                let json = serde_json::to_string(&a).unwrap_or_default();
                Value::String(json)
            }
        }
    }
}

// ==========================================================================
// Conversions from primitive types
// ==========================================================================

impl From<bool> for Variant {
    fn from(v: bool) -> Self {
        Self::boolean(v)
    }
}

impl From<i32> for Variant {
    fn from(v: i32) -> Self {
        Self::int32(v)
    }
}

impl From<i64> for Variant {
    fn from(v: i64) -> Self {
        Self::int64(v)
    }
}

impl From<u32> for Variant {
    fn from(v: u32) -> Self {
        Self::uint32(v)
    }
}

impl From<u64> for Variant {
    fn from(v: u64) -> Self {
        Self::uint64(v)
    }
}

impl From<f32> for Variant {
    fn from(v: f32) -> Self {
        Self::float(v)
    }
}

impl From<f64> for Variant {
    fn from(v: f64) -> Self {
        Self::double(v)
    }
}

impl From<String> for Variant {
    fn from(v: String) -> Self {
        Self::string(v)
    }
}

impl From<&str> for Variant {
    fn from(v: &str) -> Self {
        Self::string(v)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scalar_variants() {
        let v = Variant::boolean(true);
        assert!(v.is_scalar());
        assert!(!v.is_array());
        assert_eq!(v.as_bool(), Some(true));

        let v = Variant::int32(42);
        assert_eq!(v.as_i32(), Some(42));
        assert_eq!(v.as_i64(), Some(42));

        let v = Variant::double(3.14);
        assert_eq!(v.as_f64(), Some(3.14));
    }

    #[test]
    fn test_array_variants() {
        let v = Variant::int32_array(vec![1, 2, 3, 4, 5]);
        assert!(v.is_array());
        assert!(!v.is_scalar());
        assert_eq!(v.array_len(), 5);
    }

    #[test]
    fn test_data_type_id() {
        assert_eq!(Variant::boolean(true).data_type_id(), DataTypeId::Boolean);
        assert_eq!(Variant::int32(42).data_type_id(), DataTypeId::Int32);
        assert_eq!(Variant::double(3.14).data_type_id(), DataTypeId::Double);
        assert_eq!(Variant::string("test").data_type_id(), DataTypeId::String);
    }

    #[test]
    fn test_from_core_value() {
        let core_value = Value::I32(42);
        let variant: Variant = core_value.into();
        assert_eq!(variant.as_i32(), Some(42));

        let variant = Variant::double(3.14);
        let core_value: Value = variant.into();
        assert_eq!(core_value, Value::F64(3.14));
    }

    #[test]
    fn test_variant_display() {
        assert_eq!(Variant::null().to_string(), "null");
        assert_eq!(Variant::boolean(true).to_string(), "true");
        assert_eq!(Variant::int32(42).to_string(), "42");
        assert_eq!(Variant::string("hello").to_string(), "\"hello\"");
        assert_eq!(Variant::int32_array(vec![1, 2, 3]).to_string(), "Int32[3]");
    }
}

//! Type-safe data point handling.
//!
//! This module provides type-safe wrappers around data points to ensure
//! compile-time type checking and runtime type validation.
//!
//! # Example
//!
//! ```rust,ignore
//! use mabi_core::typed_point::{TypedDataPoint, FloatPoint, BoolPoint};
//!
//! // Create a typed temperature point
//! let temp_point = FloatPoint::new("temp", "Temperature")
//!     .with_units("°C")
//!     .with_range(-40.0, 80.0);
//!
//! // Type-safe read - returns f64, not Value
//! let temperature: f64 = temp_point.read(&device).await?;
//!
//! // Type-safe write - only accepts f64
//! temp_point.write(&mut device, 25.5).await?;
//! ```

use std::marker::PhantomData;

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::types::{AccessMode, DataPointDef, DataPointId, DataType, Quality};
use crate::value::Value;

/// A trait for types that can be stored in data points.
pub trait DataPointType: Clone + Send + Sync + 'static {
    /// The data type enum variant for this type.
    fn data_type() -> DataType;

    /// Convert from Value.
    fn from_value(value: &Value) -> Option<Self>;

    /// Convert to Value.
    fn to_value(&self) -> Value;

    /// Get the type name.
    fn type_name() -> &'static str;
}

// Implement DataPointType for common types

impl DataPointType for bool {
    fn data_type() -> DataType {
        DataType::Bool
    }

    fn from_value(value: &Value) -> Option<Self> {
        value.as_bool()
    }

    fn to_value(&self) -> Value {
        Value::Bool(*self)
    }

    fn type_name() -> &'static str {
        "bool"
    }
}

impl DataPointType for i32 {
    fn data_type() -> DataType {
        DataType::Int32
    }

    fn from_value(value: &Value) -> Option<Self> {
        value.as_i64().and_then(|v| i32::try_from(v).ok())
    }

    fn to_value(&self) -> Value {
        Value::I32(*self)
    }

    fn type_name() -> &'static str {
        "i32"
    }
}

impl DataPointType for i64 {
    fn data_type() -> DataType {
        DataType::Int64
    }

    fn from_value(value: &Value) -> Option<Self> {
        value.as_i64()
    }

    fn to_value(&self) -> Value {
        Value::I64(*self)
    }

    fn type_name() -> &'static str {
        "i64"
    }
}

impl DataPointType for u32 {
    fn data_type() -> DataType {
        DataType::UInt32
    }

    fn from_value(value: &Value) -> Option<Self> {
        value.as_i64().and_then(|v| u32::try_from(v).ok())
    }

    fn to_value(&self) -> Value {
        Value::U32(*self)
    }

    fn type_name() -> &'static str {
        "u32"
    }
}

impl DataPointType for u64 {
    fn data_type() -> DataType {
        DataType::UInt64
    }

    fn from_value(value: &Value) -> Option<Self> {
        value.as_i64().and_then(|v| u64::try_from(v).ok())
    }

    fn to_value(&self) -> Value {
        Value::U64(*self)
    }

    fn type_name() -> &'static str {
        "u64"
    }
}

impl DataPointType for f32 {
    fn data_type() -> DataType {
        DataType::Float32
    }

    fn from_value(value: &Value) -> Option<Self> {
        value.as_f64().map(|v| v as f32)
    }

    fn to_value(&self) -> Value {
        Value::F32(*self)
    }

    fn type_name() -> &'static str {
        "f32"
    }
}

impl DataPointType for f64 {
    fn data_type() -> DataType {
        DataType::Float64
    }

    fn from_value(value: &Value) -> Option<Self> {
        value.as_f64()
    }

    fn to_value(&self) -> Value {
        Value::F64(*self)
    }

    fn type_name() -> &'static str {
        "f64"
    }
}

impl DataPointType for String {
    fn data_type() -> DataType {
        DataType::String
    }

    fn from_value(value: &Value) -> Option<Self> {
        value.as_str().map(|s| s.to_string())
    }

    fn to_value(&self) -> Value {
        Value::String(self.clone())
    }

    fn type_name() -> &'static str {
        "string"
    }
}

/// A type-safe data point reference.
///
/// This struct wraps a data point with compile-time type information,
/// ensuring type safety when reading and writing values.
#[derive(Debug, Clone)]
pub struct TypedDataPoint<T: DataPointType> {
    /// Point ID within the device.
    pub id: String,
    /// Human-readable name.
    pub name: String,
    /// Description.
    pub description: String,
    /// Access mode.
    pub access: AccessMode,
    /// Engineering units.
    pub units: Option<String>,
    /// Minimum value (for numeric types).
    pub min_value: Option<f64>,
    /// Maximum value (for numeric types).
    pub max_value: Option<f64>,
    /// Default value.
    pub default_value: Option<T>,
    /// Phantom data for type safety.
    _marker: PhantomData<T>,
}

impl<T: DataPointType> TypedDataPoint<T> {
    /// Create a new typed data point.
    pub fn new(id: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            description: String::new(),
            access: AccessMode::ReadWrite,
            units: None,
            min_value: None,
            max_value: None,
            default_value: None,
            _marker: PhantomData,
        }
    }

    /// Set the description.
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = description.into();
        self
    }

    /// Set the access mode.
    pub fn with_access(mut self, access: AccessMode) -> Self {
        self.access = access;
        self
    }

    /// Set as read-only.
    pub fn read_only(mut self) -> Self {
        self.access = AccessMode::ReadOnly;
        self
    }

    /// Set as write-only.
    pub fn write_only(mut self) -> Self {
        self.access = AccessMode::WriteOnly;
        self
    }

    /// Set the engineering units.
    pub fn with_units(mut self, units: impl Into<String>) -> Self {
        self.units = Some(units.into());
        self
    }

    /// Set the value range.
    pub fn with_range(mut self, min: f64, max: f64) -> Self {
        self.min_value = Some(min);
        self.max_value = Some(max);
        self
    }

    /// Set the default value.
    pub fn with_default(mut self, value: T) -> Self {
        self.default_value = Some(value);
        self
    }

    /// Get the data type.
    pub fn data_type(&self) -> DataType {
        T::data_type()
    }

    /// Convert to an untyped DataPointDef.
    pub fn to_definition(&self) -> DataPointDef {
        DataPointDef {
            id: self.id.clone(),
            name: self.name.clone(),
            description: self.description.clone(),
            data_type: T::data_type(),
            access: self.access,
            units: self.units.clone(),
            min_value: self.min_value,
            max_value: self.max_value,
            default_value: self.default_value.as_ref().map(|v| v.to_value()),
            address: None,
        }
    }

    /// Check if a value is within the valid range.
    pub fn validate_range(&self, value: f64) -> Result<()> {
        if let Some(min) = self.min_value {
            if value < min {
                return Err(Error::InvalidValue {
                    point_id: self.id.clone(),
                    reason: format!("Value {} is below minimum {}", value, min),
                });
            }
        }
        if let Some(max) = self.max_value {
            if value > max {
                return Err(Error::InvalidValue {
                    point_id: self.id.clone(),
                    reason: format!("Value {} is above maximum {}", value, max),
                });
            }
        }
        Ok(())
    }

    /// Convert a Value to this point's type with validation.
    pub fn convert_value(&self, value: &Value) -> Result<T> {
        T::from_value(value).ok_or_else(|| Error::TypeMismatch {
            expected: T::type_name().to_string(),
            actual: value.type_name().to_string(),
        })
    }

    /// Create a DataPointId for a given device.
    pub fn id_for_device(&self, device_id: &str) -> DataPointId {
        DataPointId::new(device_id, &self.id)
    }
}

/// A typed data point value with quality and timestamp.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TypedPointValue<T> {
    /// The typed value.
    pub value: T,
    /// Quality flags.
    pub quality: Quality,
    /// Timestamp.
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

impl<T: DataPointType> TypedPointValue<T> {
    /// Create a new typed point value with good quality.
    pub fn new(value: T) -> Self {
        Self {
            value,
            quality: Quality::GOOD,
            timestamp: chrono::Utc::now(),
        }
    }

    /// Create with specified quality.
    pub fn with_quality(mut self, quality: Quality) -> Self {
        self.quality = quality;
        self
    }

    /// Create with specified timestamp.
    pub fn with_timestamp(mut self, timestamp: chrono::DateTime<chrono::Utc>) -> Self {
        self.timestamp = timestamp;
        self
    }

    /// Check if quality is good.
    pub fn is_good(&self) -> bool {
        self.quality.is_good()
    }
}

// Type aliases for common point types

/// Boolean data point.
pub type BoolPoint = TypedDataPoint<bool>;
/// 32-bit signed integer data point.
pub type Int32Point = TypedDataPoint<i32>;
/// 64-bit signed integer data point.
pub type Int64Point = TypedDataPoint<i64>;
/// 32-bit unsigned integer data point.
pub type UInt32Point = TypedDataPoint<u32>;
/// 64-bit unsigned integer data point.
pub type UInt64Point = TypedDataPoint<u64>;
/// 32-bit floating point data point.
pub type Float32Point = TypedDataPoint<f32>;
/// 64-bit floating point data point.
pub type Float64Point = TypedDataPoint<f64>;
/// String data point.
pub type StringPoint = TypedDataPoint<String>;

/// Common numeric point operations.
pub trait NumericPoint {
    /// Get the value as f64 for calculations.
    fn as_f64(&self) -> Option<f64>;
}

impl<T: DataPointType + Into<f64> + Copy> NumericPoint for TypedPointValue<T> {
    fn as_f64(&self) -> Option<f64> {
        Some(self.value.into())
    }
}

/// Helper trait for creating typed points from definitions.
pub trait FromDefinition: Sized {
    /// Try to create a typed point from a definition.
    fn from_definition(def: &DataPointDef) -> Result<Self>;
}

impl<T: DataPointType> FromDefinition for TypedDataPoint<T> {
    fn from_definition(def: &DataPointDef) -> Result<Self> {
        if def.data_type != T::data_type() {
            return Err(Error::TypeMismatch {
                expected: format!("{:?}", T::data_type()),
                actual: format!("{:?}", def.data_type),
            });
        }

        let mut point = TypedDataPoint::new(&def.id, &def.name)
            .with_description(&def.description)
            .with_access(def.access);

        if let Some(ref units) = def.units {
            point = point.with_units(units);
        }

        if let (Some(min), Some(max)) = (def.min_value, def.max_value) {
            point = point.with_range(min, max);
        }

        if let Some(ref default) = def.default_value {
            if let Some(value) = T::from_value(default) {
                point = point.with_default(value);
            }
        }

        Ok(point)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_typed_data_point_creation() {
        let point = Float64Point::new("temp", "Temperature")
            .with_description("Room temperature")
            .with_units("°C")
            .with_range(-40.0, 80.0)
            .read_only();

        assert_eq!(point.id, "temp");
        assert_eq!(point.data_type(), DataType::Float64);
        assert_eq!(point.access, AccessMode::ReadOnly);
        assert_eq!(point.min_value, Some(-40.0));
        assert_eq!(point.max_value, Some(80.0));
    }

    #[test]
    fn test_typed_data_point_conversion() {
        let point = Int32Point::new("count", "Counter");

        let value = Value::I32(42);
        let converted = point.convert_value(&value).unwrap();
        assert_eq!(converted, 42i32);

        // Type mismatch should fail
        let wrong_value = Value::String("hello".to_string());
        assert!(point.convert_value(&wrong_value).is_err());
    }

    #[test]
    fn test_typed_data_point_validation() {
        let point = Float64Point::new("temp", "Temperature").with_range(-40.0, 80.0);

        assert!(point.validate_range(25.0).is_ok());
        assert!(point.validate_range(-40.0).is_ok());
        assert!(point.validate_range(80.0).is_ok());
        assert!(point.validate_range(-50.0).is_err());
        assert!(point.validate_range(100.0).is_err());
    }

    #[test]
    fn test_typed_point_value() {
        let value = TypedPointValue::new(25.5f64);
        assert!(value.is_good());
        assert_eq!(value.value, 25.5);
    }

    #[test]
    fn test_to_definition() {
        let point = Float32Point::new("temp", "Temperature")
            .with_description("Sensor reading")
            .with_units("°C")
            .with_range(-40.0, 80.0);

        let def = point.to_definition();
        assert_eq!(def.id, "temp");
        assert_eq!(def.name, "Temperature");
        assert_eq!(def.data_type, DataType::Float32);
        assert_eq!(def.units, Some("°C".to_string()));
    }

    #[test]
    fn test_from_definition() {
        let def = DataPointDef::new("count", "Counter", DataType::Int32).with_range(0.0, 1000.0);

        let point: Int32Point = TypedDataPoint::from_definition(&def).unwrap();
        assert_eq!(point.id, "count");
        assert_eq!(point.min_value, Some(0.0));
        assert_eq!(point.max_value, Some(1000.0));

        // Wrong type should fail
        let result: Result<Float64Point> = TypedDataPoint::from_definition(&def);
        assert!(result.is_err());
    }

    #[test]
    fn test_data_point_type_implementations() {
        // Test bool
        assert_eq!(bool::data_type(), DataType::Bool);
        assert_eq!(bool::from_value(&Value::Bool(true)), Some(true));
        assert_eq!(true.to_value(), Value::Bool(true));

        // Test f64
        assert_eq!(f64::data_type(), DataType::Float64);
        assert_eq!(f64::from_value(&Value::F64(3.14)), Some(3.14));
        assert_eq!(3.14f64.to_value(), Value::F64(3.14));

        // Test String
        assert_eq!(String::data_type(), DataType::String);
        let s = "hello".to_string();
        assert_eq!(
            String::from_value(&Value::String(s.clone())),
            Some(s.clone())
        );
        assert_eq!(s.to_value(), Value::String("hello".to_string()));
    }
}

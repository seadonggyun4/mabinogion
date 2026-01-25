//! Core types for data points and addresses.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fmt;

use crate::value::Value;

/// Unique identifier for a data point.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DataPointId {
    /// Device ID.
    pub device_id: String,
    /// Point ID within the device.
    pub point_id: String,
}

impl DataPointId {
    /// Create a new data point ID.
    pub fn new(device_id: impl Into<String>, point_id: impl Into<String>) -> Self {
        Self {
            device_id: device_id.into(),
            point_id: point_id.into(),
        }
    }

    /// Parse from string format "device_id/point_id".
    pub fn parse(s: &str) -> Option<Self> {
        let parts: Vec<&str> = s.splitn(2, '/').collect();
        if parts.len() == 2 {
            Some(Self::new(parts[0], parts[1]))
        } else {
            None
        }
    }
}

impl fmt::Display for DataPointId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.device_id, self.point_id)
    }
}

/// Data quality flags.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub struct Quality {
    /// Quality bits.
    bits: u16,
}

impl Quality {
    /// Good quality.
    pub const GOOD: Quality = Quality { bits: 0xC0 };
    /// Bad quality.
    pub const BAD: Quality = Quality { bits: 0x00 };
    /// Uncertain quality.
    pub const UNCERTAIN: Quality = Quality { bits: 0x40 };

    /// Quality flags.
    pub const FLAG_SUBSTITUTED: u16 = 0x0010;
    pub const FLAG_LOCAL_OVERRIDE: u16 = 0x0008;
    pub const FLAG_CONFIGURATION_ERROR: u16 = 0x0004;
    pub const FLAG_NOT_CONNECTED: u16 = 0x0002;
    pub const FLAG_DEVICE_FAILURE: u16 = 0x0001;

    /// Create a new quality value.
    pub fn new(bits: u16) -> Self {
        Self { bits }
    }

    /// Check if quality is good.
    pub fn is_good(&self) -> bool {
        (self.bits & 0xC0) == 0xC0
    }

    /// Check if quality is bad.
    pub fn is_bad(&self) -> bool {
        (self.bits & 0xC0) == 0x00
    }

    /// Check if quality is uncertain.
    pub fn is_uncertain(&self) -> bool {
        (self.bits & 0xC0) == 0x40
    }

    /// Get the raw bits.
    pub fn bits(&self) -> u16 {
        self.bits
    }

    /// Check if a flag is set.
    pub fn has_flag(&self, flag: u16) -> bool {
        (self.bits & flag) != 0
    }

    /// Set a flag.
    pub fn with_flag(mut self, flag: u16) -> Self {
        self.bits |= flag;
        self
    }

    /// Clear a flag.
    pub fn without_flag(mut self, flag: u16) -> Self {
        self.bits &= !flag;
        self
    }
}

impl fmt::Display for Quality {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_good() {
            write!(f, "Good")
        } else if self.is_uncertain() {
            write!(f, "Uncertain")
        } else {
            write!(f, "Bad")
        }
    }
}

/// A data point with value, quality, and timestamp.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataPoint {
    /// Point identifier.
    pub id: DataPointId,
    /// Current value.
    pub value: Value,
    /// Quality.
    pub quality: Quality,
    /// Timestamp.
    pub timestamp: DateTime<Utc>,
    /// Engineering units (optional).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub units: Option<String>,
    /// Description (optional).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

impl DataPoint {
    /// Create a new data point.
    pub fn new(id: DataPointId, value: Value) -> Self {
        Self {
            id,
            value,
            quality: Quality::GOOD,
            timestamp: Utc::now(),
            units: None,
            description: None,
        }
    }

    /// Create a data point with bad quality.
    pub fn bad(id: DataPointId) -> Self {
        Self {
            id,
            value: Value::Null,
            quality: Quality::BAD,
            timestamp: Utc::now(),
            units: None,
            description: None,
        }
    }

    /// Set the quality.
    pub fn with_quality(mut self, quality: Quality) -> Self {
        self.quality = quality;
        self
    }

    /// Set the timestamp.
    pub fn with_timestamp(mut self, timestamp: DateTime<Utc>) -> Self {
        self.timestamp = timestamp;
        self
    }

    /// Set the units.
    pub fn with_units(mut self, units: impl Into<String>) -> Self {
        self.units = Some(units.into());
        self
    }

    /// Set the description.
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Update the value and timestamp.
    pub fn update(&mut self, value: Value) {
        self.value = value;
        self.timestamp = Utc::now();
        self.quality = Quality::GOOD;
    }

    /// Update with bad quality.
    pub fn set_bad(&mut self) {
        self.quality = Quality::BAD;
        self.timestamp = Utc::now();
    }
}

/// Data point definition (metadata).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataPointDef {
    /// Point ID.
    pub id: String,
    /// Human-readable name.
    pub name: String,
    /// Description.
    #[serde(default)]
    pub description: String,
    /// Data type.
    pub data_type: DataType,
    /// Access mode.
    #[serde(default)]
    pub access: AccessMode,
    /// Engineering units.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub units: Option<String>,
    /// Minimum value.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_value: Option<f64>,
    /// Maximum value.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_value: Option<f64>,
    /// Default value.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_value: Option<Value>,
    /// Protocol-specific address.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub address: Option<Address>,
}

impl DataPointDef {
    /// Create a new data point definition.
    pub fn new(id: impl Into<String>, name: impl Into<String>, data_type: DataType) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            description: String::new(),
            data_type,
            access: AccessMode::ReadWrite,
            units: None,
            min_value: None,
            max_value: None,
            default_value: None,
            address: None,
        }
    }

    /// Set access mode.
    pub fn with_access(mut self, access: AccessMode) -> Self {
        self.access = access;
        self
    }

    /// Set value range.
    pub fn with_range(mut self, min: f64, max: f64) -> Self {
        self.min_value = Some(min);
        self.max_value = Some(max);
        self
    }

    /// Set units.
    pub fn with_units(mut self, units: impl Into<String>) -> Self {
        self.units = Some(units.into());
        self
    }

    /// Set address.
    pub fn with_address(mut self, address: Address) -> Self {
        self.address = Some(address);
        self
    }
}

/// Data types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DataType {
    Bool,
    Int8,
    UInt8,
    Int16,
    UInt16,
    Int32,
    UInt32,
    Int64,
    UInt64,
    Float32,
    Float64,
    String,
    DateTime,
    ByteString,
}

impl DataType {
    /// Get size in bytes.
    pub fn size_bytes(&self) -> Option<usize> {
        match self {
            Self::Bool | Self::Int8 | Self::UInt8 => Some(1),
            Self::Int16 | Self::UInt16 => Some(2),
            Self::Int32 | Self::UInt32 | Self::Float32 => Some(4),
            Self::Int64 | Self::UInt64 | Self::Float64 | Self::DateTime => Some(8),
            Self::String | Self::ByteString => None,
        }
    }
}

/// Access mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum AccessMode {
    ReadOnly,
    #[default]
    ReadWrite,
    WriteOnly,
}

impl AccessMode {
    /// Check if readable.
    pub fn is_readable(&self) -> bool {
        matches!(self, Self::ReadOnly | Self::ReadWrite)
    }

    /// Check if writable.
    pub fn is_writable(&self) -> bool {
        matches!(self, Self::WriteOnly | Self::ReadWrite)
    }
}

/// Protocol-specific address.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Address {
    /// Modbus address.
    Modbus(ModbusAddress),
    /// OPC UA node ID.
    OpcUa { node_id: String },
    /// BACnet object.
    BacNet(BacNetAddress),
    /// KNX group address.
    Knx { group_address: String },
}

/// Modbus address.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModbusAddress {
    /// Register type.
    pub register_type: ModbusRegisterType,
    /// Address (0-based).
    pub address: u16,
    /// Number of registers.
    #[serde(default = "default_count")]
    pub count: u16,
}

fn default_count() -> u16 {
    1
}

/// Modbus register types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModbusRegisterType {
    Coil,
    DiscreteInput,
    HoldingRegister,
    InputRegister,
}

/// BACnet address.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BacNetAddress {
    /// Object type.
    pub object_type: String,
    /// Object instance.
    pub instance: u32,
    /// Property identifier.
    #[serde(default = "default_property")]
    pub property: String,
}

fn default_property() -> String {
    "present-value".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_data_point_id() {
        let id = DataPointId::new("device-001", "temp");
        assert_eq!(id.to_string(), "device-001/temp");

        let parsed = DataPointId::parse("device-001/temp").unwrap();
        assert_eq!(parsed, id);
    }

    #[test]
    fn test_quality() {
        assert!(Quality::GOOD.is_good());
        assert!(Quality::BAD.is_bad());
        assert!(Quality::UNCERTAIN.is_uncertain());

        let q = Quality::GOOD.with_flag(Quality::FLAG_SUBSTITUTED);
        assert!(q.has_flag(Quality::FLAG_SUBSTITUTED));
    }

    #[test]
    fn test_data_point() {
        let id = DataPointId::new("device-001", "temp");
        let dp = DataPoint::new(id, Value::F64(25.5)).with_units("°C");

        assert!(dp.quality.is_good());
        assert_eq!(dp.units, Some("°C".to_string()));
    }
}

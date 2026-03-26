//! Device builder pattern for fluent device construction.
//!
//! This module provides a builder pattern for creating devices with
//! a clean, chainable API that improves extensibility and maintainability.
//!
//! # Example
//!
//! ```rust,ignore
//! use mabi_core::device_builder::{DeviceBuilder, MockDeviceBuilder};
//!
//! let device = MockDeviceBuilder::new("device-001", "Temperature Sensor")
//!     .protocol(Protocol::ModbusTcp)
//!     .description("Main building temperature sensor")
//!     .metadata("location", "Building A, Floor 1")
//!     .metadata("manufacturer", "ACME Sensors")
//!     .add_point(DataPointDef::new("temp", "Temperature", DataType::Float32)
//!         .with_units("°C")
//!         .with_range(-40.0, 80.0))
//!     .add_point(DataPointDef::new("humidity", "Humidity", DataType::Float32)
//!         .with_units("%")
//!         .with_range(0.0, 100.0))
//!     .build()?;
//! ```

use std::collections::HashMap;

use crate::config::DeviceConfig;
use crate::error::{Error, Result};
use crate::protocol::Protocol;
use crate::tags::Tags;
use crate::types::{Address, DataPointDef, DataType};

/// Builder for creating device configurations.
///
/// This builder provides a fluent API for constructing `DeviceConfig`
/// instances with validation at build time.
#[derive(Debug, Clone)]
pub struct DeviceConfigBuilder {
    id: String,
    name: String,
    description: String,
    protocol: Protocol,
    address: Option<String>,
    points: Vec<DataPointDef>,
    metadata: HashMap<String, String>,
    tags: Tags,
    validation_enabled: bool,
}

impl DeviceConfigBuilder {
    /// Create a new device configuration builder.
    ///
    /// # Arguments
    ///
    /// * `id` - Unique device identifier
    /// * `name` - Human-readable device name
    pub fn new(id: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            description: String::new(),
            protocol: Protocol::ModbusTcp,
            address: None,
            points: Vec::new(),
            metadata: HashMap::new(),
            tags: Tags::new(),
            validation_enabled: true,
        }
    }

    /// Set the protocol for this device.
    pub fn protocol(mut self, protocol: Protocol) -> Self {
        self.protocol = protocol;
        self
    }

    /// Set the device description.
    pub fn description(mut self, description: impl Into<String>) -> Self {
        self.description = description.into();
        self
    }

    /// Set the protocol-specific address.
    pub fn address(mut self, address: impl Into<String>) -> Self {
        self.address = Some(address.into());
        self
    }

    /// Add a metadata key-value pair.
    pub fn metadata(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.metadata.insert(key.into(), value.into());
        self
    }

    /// Add multiple metadata entries.
    pub fn with_metadata(mut self, metadata: HashMap<String, String>) -> Self {
        self.metadata.extend(metadata);
        self
    }

    /// Set tags for this device.
    pub fn tags(mut self, tags: Tags) -> Self {
        self.tags = tags;
        self
    }

    /// Add a single tag (key-value pair).
    pub fn tag(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.tags.insert(key.into(), value.into());
        self
    }

    /// Add a label.
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.tags.add_label(label.into());
        self
    }

    /// Add multiple labels.
    pub fn labels<I, S>(mut self, labels: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        for label in labels {
            self.tags.add_label(label.into());
        }
        self
    }

    /// Add a data point definition.
    pub fn add_point(mut self, point: DataPointDef) -> Self {
        self.points.push(point);
        self
    }

    /// Add multiple data point definitions.
    pub fn add_points(mut self, points: impl IntoIterator<Item = DataPointDef>) -> Self {
        self.points.extend(points);
        self
    }

    /// Disable validation at build time.
    pub fn skip_validation(mut self) -> Self {
        self.validation_enabled = false;
        self
    }

    /// Validate the current builder state.
    pub fn validate(&self) -> Result<()> {
        if self.id.is_empty() {
            return Err(Error::Config("Device ID cannot be empty".into()));
        }
        if self.name.is_empty() {
            return Err(Error::Config("Device name cannot be empty".into()));
        }

        // Check for duplicate point IDs
        let mut point_ids = std::collections::HashSet::new();
        for point in &self.points {
            if !point_ids.insert(&point.id) {
                return Err(Error::Config(format!(
                    "Duplicate data point ID: {}",
                    point.id
                )));
            }
        }

        Ok(())
    }

    /// Build the device configuration.
    pub fn build(self) -> Result<DeviceConfig> {
        if self.validation_enabled {
            self.validate()?;
        }

        Ok(DeviceConfig {
            id: self.id,
            name: self.name,
            description: self.description,
            protocol: self.protocol,
            address: self.address,
            points: self
                .points
                .into_iter()
                .map(|p| crate::config::DataPointConfig {
                    id: p.id,
                    name: p.name,
                    data_type: format!("{:?}", p.data_type).to_lowercase(),
                    access: format!("{:?}", p.access).to_lowercase(),
                    address: p.address.map(|a| format!("{:?}", a)),
                    initial_value: p.default_value.map(|v| serde_json::to_value(v).unwrap()),
                    units: p.units,
                    min: p.min_value,
                    max: p.max_value,
                })
                .collect(),
            metadata: self.metadata,
            tags: self.tags,
        })
    }
}

/// Builder for data point definitions with chainable API.
#[derive(Debug, Clone)]
pub struct DataPointBuilder {
    id: String,
    name: String,
    data_type: DataType,
    description: String,
    access: crate::types::AccessMode,
    units: Option<String>,
    min_value: Option<f64>,
    max_value: Option<f64>,
    default_value: Option<crate::value::Value>,
    address: Option<Address>,
}

impl DataPointBuilder {
    /// Create a new data point builder.
    pub fn new(id: impl Into<String>, name: impl Into<String>, data_type: DataType) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            data_type,
            description: String::new(),
            access: crate::types::AccessMode::ReadWrite,
            units: None,
            min_value: None,
            max_value: None,
            default_value: None,
            address: None,
        }
    }

    /// Set the description.
    pub fn description(mut self, description: impl Into<String>) -> Self {
        self.description = description.into();
        self
    }

    /// Set the access mode.
    pub fn access(mut self, access: crate::types::AccessMode) -> Self {
        self.access = access;
        self
    }

    /// Set read-only access.
    pub fn read_only(mut self) -> Self {
        self.access = crate::types::AccessMode::ReadOnly;
        self
    }

    /// Set write-only access.
    pub fn write_only(mut self) -> Self {
        self.access = crate::types::AccessMode::WriteOnly;
        self
    }

    /// Set the engineering units.
    pub fn units(mut self, units: impl Into<String>) -> Self {
        self.units = Some(units.into());
        self
    }

    /// Set the value range.
    pub fn range(mut self, min: f64, max: f64) -> Self {
        self.min_value = Some(min);
        self.max_value = Some(max);
        self
    }

    /// Set the minimum value.
    pub fn min(mut self, min: f64) -> Self {
        self.min_value = Some(min);
        self
    }

    /// Set the maximum value.
    pub fn max(mut self, max: f64) -> Self {
        self.max_value = Some(max);
        self
    }

    /// Set the default value.
    pub fn default_value(mut self, value: impl Into<crate::value::Value>) -> Self {
        self.default_value = Some(value.into());
        self
    }

    /// Set the protocol-specific address.
    pub fn address(mut self, address: Address) -> Self {
        self.address = Some(address);
        self
    }

    /// Build the data point definition.
    pub fn build(self) -> DataPointDef {
        DataPointDef {
            id: self.id,
            name: self.name,
            description: self.description,
            data_type: self.data_type,
            access: self.access,
            units: self.units,
            min_value: self.min_value,
            max_value: self.max_value,
            default_value: self.default_value,
            address: self.address,
        }
    }
}

impl From<DataPointBuilder> for DataPointDef {
    fn from(builder: DataPointBuilder) -> Self {
        builder.build()
    }
}

/// Shorthand function to create a new device config builder.
pub fn device(id: impl Into<String>, name: impl Into<String>) -> DeviceConfigBuilder {
    DeviceConfigBuilder::new(id, name)
}

/// Shorthand function to create a new data point builder.
pub fn point(
    id: impl Into<String>,
    name: impl Into<String>,
    data_type: DataType,
) -> DataPointBuilder {
    DataPointBuilder::new(id, name, data_type)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_device_config_builder() {
        let config = DeviceConfigBuilder::new("dev-001", "Test Device")
            .protocol(Protocol::ModbusTcp)
            .description("A test device")
            .metadata("location", "Building A")
            .add_point(
                DataPointBuilder::new("temp", "Temperature", DataType::Float32)
                    .units("°C")
                    .range(-40.0, 80.0)
                    .build(),
            )
            .build()
            .unwrap();

        assert_eq!(config.id, "dev-001");
        assert_eq!(config.name, "Test Device");
        assert_eq!(config.protocol, Protocol::ModbusTcp);
        assert_eq!(config.points.len(), 1);
    }

    #[test]
    fn test_device_config_builder_validation() {
        // Empty ID should fail
        let result = DeviceConfigBuilder::new("", "Test").build();
        assert!(result.is_err());

        // Empty name should fail
        let result = DeviceConfigBuilder::new("test", "").build();
        assert!(result.is_err());

        // Duplicate point IDs should fail
        let result = DeviceConfigBuilder::new("dev-001", "Test")
            .add_point(DataPointBuilder::new("temp", "Temp 1", DataType::Float32).build())
            .add_point(DataPointBuilder::new("temp", "Temp 2", DataType::Float32).build())
            .build();
        assert!(result.is_err());
    }

    #[test]
    fn test_data_point_builder() {
        let point = DataPointBuilder::new("temp", "Temperature", DataType::Float32)
            .description("Room temperature")
            .units("°C")
            .range(-40.0, 80.0)
            .read_only()
            .default_value(25.0f64)
            .build();

        assert_eq!(point.id, "temp");
        assert_eq!(point.units, Some("°C".to_string()));
        assert_eq!(point.min_value, Some(-40.0));
        assert_eq!(point.max_value, Some(80.0));
        assert_eq!(point.access, crate::types::AccessMode::ReadOnly);
    }

    #[test]
    fn test_shorthand_functions() {
        let config = device("dev-001", "Test")
            .protocol(Protocol::BacnetIp)
            .add_point(point("temp", "Temperature", DataType::Float32).build())
            .build()
            .unwrap();

        assert_eq!(config.id, "dev-001");
        assert_eq!(config.protocol, Protocol::BacnetIp);
    }

    #[test]
    fn test_device_config_builder_with_tags() {
        let config = DeviceConfigBuilder::new("dev-001", "Tagged Device")
            .protocol(Protocol::ModbusTcp)
            .tag("location", "building-a")
            .tag("floor", "3")
            .label("hvac")
            .label("critical")
            .build()
            .unwrap();

        assert_eq!(config.tags.get("location"), Some("building-a"));
        assert_eq!(config.tags.get("floor"), Some("3"));
        assert!(config.tags.has_label("hvac"));
        assert!(config.tags.has_label("critical"));
    }

    #[test]
    fn test_device_config_builder_with_tags_object() {
        let tags = Tags::new().with_tag("env", "prod").with_label("monitored");

        let config = DeviceConfigBuilder::new("dev-002", "Device with Tags")
            .tags(tags)
            .build()
            .unwrap();

        assert_eq!(config.tags.get("env"), Some("prod"));
        assert!(config.tags.has_label("monitored"));
    }
}

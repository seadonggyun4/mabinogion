//! OPC UA device factory implementation.
//!
//! Provides factory for creating OPC UA devices from configuration,
//! implementing the trap-sim-core DeviceFactory trait.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use mabi_core::{
    config::{DeviceConfig, DataPointConfig},
    device::BoxedDevice,
    error::{Error, Result},
    factory::{DeviceFactory, FactoryMetadata, FactoryRegistry, Plugin},
    protocol::Protocol,
    tags::Tags,
    types::DataPointDef,
    value::Value,
};

use crate::config::OpcUaServerConfig;
use crate::device::OpcUaDevice;
use crate::nodes::{AddressSpaceConfig, NodeCacheConfig};
use crate::services::HistoryStoreConfig;
use crate::types::{NodeId, Variant};

/// OPC UA specific configuration in device metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpcUaDeviceMetadata {
    /// Server configuration.
    #[serde(default)]
    pub server_config: OpcUaServerConfig,
    /// Address space configuration.
    #[serde(default)]
    pub address_space_config: Option<AddressSpaceConfig>,
    /// Node cache configuration.
    #[serde(default)]
    pub cache_config: Option<NodeCacheConfig>,
    /// History store configuration.
    #[serde(default)]
    pub history_config: Option<HistoryStoreConfig>,
    /// Namespace URI for custom nodes.
    #[serde(default = "default_namespace")]
    pub namespace_uri: String,
    /// Namespace index for custom nodes.
    #[serde(default = "default_namespace_index")]
    pub namespace_index: u16,
}

fn default_namespace() -> String {
    "urn:trap-simulator:opcua".to_string()
}

fn default_namespace_index() -> u16 {
    2
}

impl Default for OpcUaDeviceMetadata {
    fn default() -> Self {
        Self {
            server_config: OpcUaServerConfig::default(),
            address_space_config: None,
            cache_config: None,
            history_config: None,
            namespace_uri: default_namespace(),
            namespace_index: default_namespace_index(),
        }
    }
}

/// Factory for creating OPC UA devices.
pub struct OpcUaDeviceFactory {
    /// Default server configuration.
    default_config: OpcUaServerConfig,
}

impl OpcUaDeviceFactory {
    /// Create a new OPC UA device factory.
    pub fn new() -> Self {
        Self {
            default_config: OpcUaServerConfig::default(),
        }
    }

    /// Create with custom default configuration.
    pub fn with_config(config: OpcUaServerConfig) -> Self {
        Self {
            default_config: config,
        }
    }

    /// Parse OPC UA metadata from device config.
    fn parse_metadata(&self, config: &DeviceConfig) -> OpcUaDeviceMetadata {
        if let Some(meta) = config.metadata.get("opcua") {
            // metadata is HashMap<String, String>, so we need to parse the value
            serde_json::from_str(meta).unwrap_or_default()
        } else {
            OpcUaDeviceMetadata::default()
        }
    }

    /// Convert DataPointConfig to Variant.
    fn point_to_variant(&self, config: &DataPointConfig) -> Variant {
        // Convert string data type to Variant
        match config.data_type.to_lowercase().as_str() {
            "bool" | "boolean" => Variant::Boolean(false),
            "i8" | "int8" | "sbyte" => Variant::SByte(0),
            "u8" | "uint8" | "byte" => Variant::Byte(0),
            "i16" | "int16" => Variant::Int16(0),
            "u16" | "uint16" => Variant::UInt16(0),
            "i32" | "int32" => Variant::Int32(0),
            "u32" | "uint32" => Variant::UInt32(0),
            "i64" | "int64" => Variant::Int64(0),
            "u64" | "uint64" => Variant::UInt64(0),
            "f32" | "float" | "float32" => Variant::Float(0.0),
            "f64" | "double" | "float64" => Variant::Double(0.0),
            "string" => Variant::String(String::new()),
            "datetime" => Variant::DateTime(chrono::Utc::now()),
            "bytestring" | "bytes" => Variant::ByteString(Vec::new()),
            _ => Variant::Double(0.0), // Default to double for unknown types
        }
    }

    /// Convert DataPointConfig to DataPointDef.
    fn config_to_def(&self, config: &DataPointConfig) -> DataPointDef {
        use mabi_core::types::{AccessMode, Address, DataType};

        let data_type = match config.data_type.to_lowercase().as_str() {
            "bool" | "boolean" => DataType::Bool,
            "i8" | "int8" | "sbyte" => DataType::Int8,
            "u8" | "uint8" | "byte" => DataType::UInt8,
            "i16" | "int16" => DataType::Int16,
            "u16" | "uint16" => DataType::UInt16,
            "i32" | "int32" => DataType::Int32,
            "u32" | "uint32" => DataType::UInt32,
            "i64" | "int64" => DataType::Int64,
            "u64" | "uint64" => DataType::UInt64,
            "f32" | "float" | "float32" => DataType::Float32,
            "f64" | "double" | "float64" => DataType::Float64,
            "string" => DataType::String,
            "datetime" => DataType::DateTime,
            "bytestring" | "bytes" => DataType::ByteString,
            _ => DataType::Float64,
        };

        let access = match config.access.to_lowercase().as_str() {
            "r" | "ro" | "readonly" => AccessMode::ReadOnly,
            "w" | "wo" | "writeonly" => AccessMode::WriteOnly,
            _ => AccessMode::ReadWrite,
        };

        let mut def = DataPointDef::new(&config.id, &config.name, data_type)
            .with_access(access);

        if let Some(unit) = &config.units {
            def = def.with_units(unit);
        }
        if let (Some(min), Some(max)) = (config.min, config.max) {
            def = def.with_range(min, max);
        }
        if let Some(addr) = &config.address {
            def = def.with_address(Address::OpcUa { node_id: addr.clone() });
        }

        def
    }

    /// Convert Variant to Value.
    fn variant_to_value(&self, variant: &Variant) -> Value {
        variant.clone().into()
    }
}

impl Default for OpcUaDeviceFactory {
    fn default() -> Self {
        Self::new()
    }
}

impl DeviceFactory for OpcUaDeviceFactory {
    fn protocol(&self) -> Protocol {
        Protocol::OpcUa
    }

    fn create(&self, config: DeviceConfig) -> Result<BoxedDevice> {
        let _metadata = self.parse_metadata(&config);

        // Create device
        let mut device = OpcUaDevice::new(&config.id, &config.name);

        // Add nodes/variables from point configurations
        for point_config in &config.points {
            let initial_variant = self.point_to_variant(point_config);
            let initial_value = self.variant_to_value(&initial_variant);
            let point_def = self.config_to_def(point_config);
            device.add_node(point_def, initial_value);
        }

        Ok(Box::new(device))
    }

    fn validate(&self, config: &DeviceConfig) -> Result<()> {
        if config.id.is_empty() {
            return Err(Error::Config("Device ID cannot be empty".into()));
        }
        if config.name.is_empty() {
            return Err(Error::Config("Device name cannot be empty".into()));
        }
        if config.protocol != Protocol::OpcUa {
            return Err(Error::Config(format!(
                "Protocol mismatch: expected OpcUa, got {:?}",
                config.protocol
            )));
        }

        // Validate OPC UA specific metadata if present
        if let Some(meta_str) = config.metadata.get("opcua") {
            let _: OpcUaDeviceMetadata = serde_json::from_str(meta_str)
                .map_err(|e| Error::Config(format!("Invalid OPC UA metadata: {}", e)))?;
        }

        // Validate point definitions
        for (i, point) in config.points.iter().enumerate() {
            if point.id.is_empty() {
                return Err(Error::Config(format!("Point {} has empty ID", i)));
            }
        }

        Ok(())
    }

    fn metadata(&self) -> FactoryMetadata {
        FactoryMetadata {
            protocol: Protocol::OpcUa,
            version: env!("CARGO_PKG_VERSION").to_string(),
            description: "OPC UA server simulator factory".to_string(),
            capabilities: vec![
                "subscription".to_string(),
                "history".to_string(),
                "browse".to_string(),
                "read".to_string(),
                "write".to_string(),
                "monitored_items".to_string(),
                "sessions".to_string(),
            ],
        }
    }
}

/// Builder for OPC UA devices with full configuration.
pub struct OpcUaDeviceBuilder {
    id: String,
    name: String,
    description: Option<String>,
    server_config: OpcUaServerConfig,
    address_space_config: AddressSpaceConfig,
    cache_config: Option<NodeCacheConfig>,
    history_config: Option<HistoryStoreConfig>,
    variables: Vec<VariableSpec>,
    namespace_uri: String,
}

/// Specification for a variable to create.
#[derive(Debug, Clone)]
pub struct VariableSpec {
    /// Variable ID within the device.
    pub id: String,
    /// Display name.
    pub name: String,
    /// Node ID.
    pub node_id: NodeId,
    /// Initial value.
    pub initial_value: Variant,
    /// Is writable.
    pub writable: bool,
    /// Engineering unit (optional).
    pub unit: Option<String>,
}

impl OpcUaDeviceBuilder {
    /// Create a new builder.
    pub fn new(id: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            description: None,
            server_config: OpcUaServerConfig::default(),
            address_space_config: AddressSpaceConfig::default(),
            cache_config: None,
            history_config: None,
            variables: Vec::new(),
            namespace_uri: "urn:trap-simulator:opcua".to_string(),
        }
    }

    /// Set description.
    pub fn description(mut self, desc: impl Into<String>) -> Self {
        self.description = Some(desc.into());
        self
    }

    /// Set server configuration.
    pub fn server_config(mut self, config: OpcUaServerConfig) -> Self {
        self.server_config = config;
        self
    }

    /// Set address space configuration.
    pub fn address_space_config(mut self, config: AddressSpaceConfig) -> Self {
        self.address_space_config = config;
        self
    }

    /// Enable node cache with configuration.
    pub fn with_cache(mut self, config: NodeCacheConfig) -> Self {
        self.cache_config = Some(config);
        self
    }

    /// Enable history storage with configuration.
    pub fn with_history(mut self, config: HistoryStoreConfig) -> Self {
        self.history_config = Some(config);
        self
    }

    /// Set namespace URI.
    pub fn namespace_uri(mut self, uri: impl Into<String>) -> Self {
        self.namespace_uri = uri.into();
        self
    }

    /// Add a variable.
    pub fn add_variable(mut self, spec: VariableSpec) -> Self {
        self.variables.push(spec);
        self
    }

    /// Add an analog variable.
    pub fn add_analog_variable(
        mut self,
        id: impl Into<String>,
        name: impl Into<String>,
        node_id: NodeId,
        initial_value: f64,
    ) -> Self {
        self.variables.push(VariableSpec {
            id: id.into(),
            name: name.into(),
            node_id,
            initial_value: Variant::Double(initial_value),
            writable: true,
            unit: None,
        });
        self
    }

    /// Add a discrete variable.
    pub fn add_discrete_variable(
        mut self,
        id: impl Into<String>,
        name: impl Into<String>,
        node_id: NodeId,
        initial_state: u32,
    ) -> Self {
        self.variables.push(VariableSpec {
            id: id.into(),
            name: name.into(),
            node_id,
            initial_value: Variant::UInt32(initial_state),
            writable: true,
            unit: None,
        });
        self
    }

    /// Add a boolean variable.
    pub fn add_boolean_variable(
        mut self,
        id: impl Into<String>,
        name: impl Into<String>,
        node_id: NodeId,
        initial_value: bool,
    ) -> Self {
        self.variables.push(VariableSpec {
            id: id.into(),
            name: name.into(),
            node_id,
            initial_value: Variant::Boolean(initial_value),
            writable: true,
            unit: None,
        });
        self
    }

    /// Add a string variable.
    pub fn add_string_variable(
        mut self,
        id: impl Into<String>,
        name: impl Into<String>,
        node_id: NodeId,
        initial_value: impl Into<String>,
    ) -> Self {
        self.variables.push(VariableSpec {
            id: id.into(),
            name: name.into(),
            node_id,
            initial_value: Variant::String(initial_value.into()),
            writable: true,
            unit: None,
        });
        self
    }

    /// Build the device.
    pub fn build(self) -> OpcUaDevice {
        let mut device = OpcUaDevice::new(&self.id, &self.name);

        // Add variables
        for var_spec in self.variables {
            use mabi_core::types::AccessMode;

            let mut point_def = DataPointDef::new(
                var_spec.id.clone(),
                var_spec.name.clone(),
                Self::variant_to_data_type(&var_spec.initial_value),
            );

            if var_spec.writable {
                point_def = point_def.with_access(AccessMode::ReadWrite);
            } else {
                point_def = point_def.with_access(AccessMode::ReadOnly);
            }

            if let Some(unit) = var_spec.unit {
                point_def = point_def.with_units(unit);
            }

            let value = Value::from(var_spec.initial_value);
            device.add_node(point_def, value);
        }

        device
    }

    /// Convert Variant to DataType.
    fn variant_to_data_type(variant: &Variant) -> mabi_core::types::DataType {
        use mabi_core::types::DataType;

        match variant {
            Variant::Boolean(_) => DataType::Bool,
            Variant::SByte(_) => DataType::Int8,
            Variant::Byte(_) => DataType::UInt8,
            Variant::Int16(_) => DataType::Int16,
            Variant::UInt16(_) => DataType::UInt16,
            Variant::Int32(_) => DataType::Int32,
            Variant::UInt32(_) => DataType::UInt32,
            Variant::Int64(_) => DataType::Int64,
            Variant::UInt64(_) => DataType::UInt64,
            Variant::Float(_) => DataType::Float32,
            Variant::Double(_) => DataType::Float64,
            Variant::String(_) | Variant::Guid(_) => DataType::String,
            Variant::DateTime(_) => DataType::DateTime,
            Variant::ByteString(_) => DataType::ByteString,
            _ => DataType::Float64, // Default
        }
    }
}

/// OPC UA plugin for registering with the plugin system.
pub struct OpcUaPlugin {
    name: String,
    factory_config: Option<OpcUaServerConfig>,
}

impl OpcUaPlugin {
    /// Create a new OPC UA plugin.
    pub fn new() -> Self {
        Self {
            name: "opcua".to_string(),
            factory_config: None,
        }
    }

    /// Create with custom name.
    pub fn with_name(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            factory_config: None,
        }
    }

    /// Set default factory configuration.
    pub fn with_factory_config(mut self, config: OpcUaServerConfig) -> Self {
        self.factory_config = Some(config);
        self
    }
}

impl Default for OpcUaPlugin {
    fn default() -> Self {
        Self::new()
    }
}

impl Plugin for OpcUaPlugin {
    fn name(&self) -> &str {
        &self.name
    }

    fn version(&self) -> &str {
        env!("CARGO_PKG_VERSION")
    }

    fn description(&self) -> &str {
        "OPC UA server simulator plugin providing OPC UA device support"
    }

    fn register_factories(&self, registry: &FactoryRegistry) -> Result<()> {
        let factory = if let Some(config) = &self.factory_config {
            OpcUaDeviceFactory::with_config(config.clone())
        } else {
            OpcUaDeviceFactory::new()
        };

        registry.register(factory)
    }
}

/// Helper to create a device configuration for OPC UA.
pub fn create_opcua_config(
    id: impl Into<String>,
    name: impl Into<String>,
    points: Vec<DataPointConfig>,
) -> DeviceConfig {
    DeviceConfig {
        id: id.into(),
        name: name.into(),
        description: String::new(),
        protocol: Protocol::OpcUa,
        address: None,
        points,
        metadata: HashMap::new(),
        tags: Tags::new(),
    }
}

/// Helper to create a device configuration with OPC UA metadata.
pub fn create_opcua_config_with_metadata(
    id: impl Into<String>,
    name: impl Into<String>,
    points: Vec<DataPointConfig>,
    metadata: OpcUaDeviceMetadata,
) -> DeviceConfig {
    let mut meta_map = HashMap::new();
    if let Ok(value) = serde_json::to_string(&metadata) {
        meta_map.insert("opcua".to_string(), value);
    }

    DeviceConfig {
        id: id.into(),
        name: name.into(),
        description: String::new(),
        protocol: Protocol::OpcUa,
        address: None,
        points,
        metadata: meta_map,
        tags: Tags::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mabi_core::device::Device;

    fn create_test_point(id: &str, name: &str, data_type: &str) -> DataPointConfig {
        DataPointConfig {
            id: id.to_string(),
            name: name.to_string(),
            data_type: data_type.to_string(),
            access: "rw".to_string(),
            address: None,
            initial_value: None,
            units: None,
            min: None,
            max: None,
        }
    }

    #[test]
    fn test_factory_protocol() {
        let factory = OpcUaDeviceFactory::new();
        assert_eq!(factory.protocol(), Protocol::OpcUa);
    }

    #[test]
    fn test_factory_create_device() {
        let factory = OpcUaDeviceFactory::new();

        let mut point = create_test_point("temp", "Temperature", "f64");
        point.units = Some("°C".to_string());

        let config = create_opcua_config("server-1", "OPC UA Server 1", vec![point]);

        let device = factory.create(config).unwrap();
        assert_eq!(device.id(), "server-1");
        assert_eq!(device.name(), "OPC UA Server 1");
    }

    #[test]
    fn test_factory_validation() {
        let factory = OpcUaDeviceFactory::new();

        // Empty ID should fail
        let config = DeviceConfig {
            id: String::new(),
            name: "Test".to_string(),
            description: String::new(),
            protocol: Protocol::OpcUa,
            address: None,
            points: vec![],
            metadata: HashMap::new(),
            tags: Tags::new(),
        };
        assert!(factory.validate(&config).is_err());

        // Wrong protocol should fail
        let config = DeviceConfig {
            id: "test".to_string(),
            name: "Test".to_string(),
            description: String::new(),
            protocol: Protocol::ModbusTcp,
            address: None,
            points: vec![],
            metadata: HashMap::new(),
            tags: Tags::new(),
        };
        assert!(factory.validate(&config).is_err());
    }

    #[test]
    fn test_device_builder() {
        let device = OpcUaDeviceBuilder::new("server-1", "Test Server")
            .description("A test OPC UA server")
            .add_analog_variable(
                "temp",
                "Temperature",
                NodeId::numeric(2, 1001),
                25.0,
            )
            .add_boolean_variable(
                "status",
                "Status",
                NodeId::numeric(2, 1002),
                true,
            )
            .build();

        assert_eq!(device.info().id, "server-1");
        assert!(device.point_definition("temp").is_some());
        assert!(device.point_definition("status").is_some());
    }

    #[test]
    fn test_plugin() {
        let registry = FactoryRegistry::new();
        let plugin = OpcUaPlugin::new();

        assert_eq!(plugin.name(), "opcua");
        plugin.register_factories(&registry).unwrap();

        assert!(registry.has(Protocol::OpcUa));
    }

    #[test]
    fn test_factory_metadata() {
        let factory = OpcUaDeviceFactory::new();
        let metadata = factory.metadata();

        assert_eq!(metadata.protocol, Protocol::OpcUa);
        assert!(metadata.capabilities.contains(&"subscription".to_string()));
        assert!(metadata.capabilities.contains(&"history".to_string()));
    }
}

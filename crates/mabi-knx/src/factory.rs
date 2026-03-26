//! KNX Device Factory.
//!
//! This module provides factory implementations for creating KNX devices.

use std::sync::Arc;

use mabi_core::{
    device::BoxedDevice, DeviceConfig, DeviceFactory, FactoryRegistry, Protocol,
    Result as CoreResult,
};

use crate::address::{GroupAddress, IndividualAddress};
use crate::config::{GroupObjectConfig, KnxDeviceConfig};
use crate::device::{KnxDevice, KnxDeviceBuilder};
use crate::dpt::{DptId, DptRegistry, DptValue};
use crate::error::KnxError;

/// Factory for creating KNX devices.
pub struct KnxDeviceFactory {
    dpt_registry: Arc<DptRegistry>,
}

impl KnxDeviceFactory {
    /// Create a new factory.
    pub fn new() -> Self {
        Self {
            dpt_registry: Arc::new(DptRegistry::new()),
        }
    }

    /// Create with custom DPT registry.
    pub fn with_registry(registry: Arc<DptRegistry>) -> Self {
        Self {
            dpt_registry: registry,
        }
    }

    /// Create a device from configuration.
    pub fn create_from_config(&self, config: KnxDeviceConfig) -> CoreResult<KnxDevice> {
        let mut builder = KnxDeviceBuilder::new(&config.id, &config.name)
            .individual_address(config.individual_address)
            .description(&config.description)
            .dpt_registry(self.dpt_registry.clone());

        // Add group objects from config
        for go_config in &config.group_objects {
            let address: GroupAddress = go_config
                .address
                .parse()
                .map_err(|e: KnxError| mabi_core::Error::Protocol(e.to_string()))?;

            let dpt_id: DptId = go_config
                .dpt
                .parse()
                .map_err(|e: KnxError| mabi_core::Error::Protocol(e.to_string()))?;

            // Parse initial value if provided
            if let Some(ref json_value) = go_config.initial_value {
                let dpt_value = parse_json_to_dpt(json_value, &dpt_id)?;
                builder =
                    builder.group_object_with_value(address, &go_config.name, dpt_id, dpt_value);
            } else {
                builder = builder.group_object(address, &go_config.name, dpt_id);
            }
        }

        builder
            .build()
            .map_err(|e| mabi_core::Error::Protocol(e.to_string()))
    }

    /// Quick create a simple device.
    pub fn create_simple(
        &self,
        id: &str,
        name: &str,
        individual_address: IndividualAddress,
    ) -> KnxDevice {
        KnxDeviceBuilder::new(id, name)
            .individual_address(individual_address)
            .dpt_registry(self.dpt_registry.clone())
            .build()
            .expect("Simple device creation should not fail")
    }
}

impl Default for KnxDeviceFactory {
    fn default() -> Self {
        Self::new()
    }
}

impl DeviceFactory for KnxDeviceFactory {
    fn protocol(&self) -> Protocol {
        Protocol::KnxIp
    }

    fn create(&self, config: DeviceConfig) -> CoreResult<BoxedDevice> {
        // Extract KNX-specific config from generic DeviceConfig
        let individual_address = config
            .metadata
            .get("individual_address")
            .and_then(|s| s.parse().ok())
            .unwrap_or_else(|| IndividualAddress::new(1, 1, 1));

        let mut builder = KnxDeviceBuilder::new(&config.id, &config.name)
            .individual_address(individual_address)
            .description(&config.description)
            .dpt_registry(self.dpt_registry.clone());

        // Parse group objects from metadata if present
        if let Some(group_objects_json) = config.metadata.get("group_objects") {
            if let Ok(objects) = serde_json::from_str::<Vec<GroupObjectConfig>>(group_objects_json)
            {
                for go in objects {
                    if let (Ok(addr), Ok(dpt)) =
                        (go.address.parse::<GroupAddress>(), go.dpt.parse::<DptId>())
                    {
                        builder = builder.group_object(addr, &go.name, dpt);
                    }
                }
            }
        }

        let device = builder
            .build()
            .map_err(|e| mabi_core::Error::Protocol(e.to_string()))?;

        Ok(Box::new(device))
    }
}

/// Parse JSON value to DptValue.
fn parse_json_to_dpt(json: &serde_json::Value, dpt_id: &DptId) -> CoreResult<DptValue> {
    match dpt_id.main {
        1 => {
            // Boolean
            let b = json
                .as_bool()
                .ok_or_else(|| mabi_core::Error::Config("Expected boolean".into()))?;
            Ok(DptValue::Bool(b))
        }
        5 | 6 => {
            // 8-bit integer
            let n = json
                .as_i64()
                .ok_or_else(|| mabi_core::Error::Config("Expected integer".into()))?;
            if dpt_id.main == 5 {
                Ok(DptValue::U8(n as u8))
            } else {
                Ok(DptValue::I8(n as i8))
            }
        }
        7 | 8 => {
            // 16-bit integer
            let n = json
                .as_i64()
                .ok_or_else(|| mabi_core::Error::Config("Expected integer".into()))?;
            if dpt_id.main == 7 {
                Ok(DptValue::U16(n as u16))
            } else {
                Ok(DptValue::I16(n as i16))
            }
        }
        9 => {
            // 16-bit float
            let f = json
                .as_f64()
                .ok_or_else(|| mabi_core::Error::Config("Expected number".into()))?;
            Ok(DptValue::F16(f as f32))
        }
        12 | 13 => {
            // 32-bit integer
            let n = json
                .as_i64()
                .ok_or_else(|| mabi_core::Error::Config("Expected integer".into()))?;
            if dpt_id.main == 12 {
                Ok(DptValue::U32(n as u32))
            } else {
                Ok(DptValue::I32(n as i32))
            }
        }
        14 => {
            // 32-bit float
            let f = json
                .as_f64()
                .ok_or_else(|| mabi_core::Error::Config("Expected number".into()))?;
            Ok(DptValue::F32(f as f32))
        }
        16 => {
            // String
            let s = json
                .as_str()
                .ok_or_else(|| mabi_core::Error::Config("Expected string".into()))?;
            Ok(DptValue::String(s.to_string()))
        }
        _ => {
            // Try to parse as generic number or bool
            if let Some(b) = json.as_bool() {
                Ok(DptValue::Bool(b))
            } else if let Some(n) = json.as_i64() {
                Ok(DptValue::I32(n as i32))
            } else if let Some(f) = json.as_f64() {
                Ok(DptValue::F32(f as f32))
            } else if let Some(s) = json.as_str() {
                Ok(DptValue::String(s.to_string()))
            } else {
                Err(mabi_core::Error::Config(format!(
                    "Cannot parse value for DPT {}",
                    dpt_id
                )))
            }
        }
    }
}

/// Register KNX factory with the registry.
pub fn register_knx_factory(registry: &FactoryRegistry) -> CoreResult<()> {
    registry.register(KnxDeviceFactory::new())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::GroupObjectFlagsConfig;

    #[test]
    fn test_factory_create_from_config() {
        let factory = KnxDeviceFactory::new();

        let config = KnxDeviceConfig {
            id: "knx-1".to_string(),
            name: "Living Room Controller".to_string(),
            description: "Controls living room lights".to_string(),
            individual_address: IndividualAddress::new(1, 2, 3),
            group_objects: vec![
                GroupObjectConfig {
                    address: "1/0/1".to_string(),
                    name: "Main Light".to_string(),
                    dpt: "1.001".to_string(),
                    flags: GroupObjectFlagsConfig::default(),
                    initial_value: Some(serde_json::json!(false)),
                },
                GroupObjectConfig {
                    address: "1/0/2".to_string(),
                    name: "Dimmer".to_string(),
                    dpt: "5.001".to_string(),
                    flags: GroupObjectFlagsConfig::default(),
                    initial_value: Some(serde_json::json!(50)),
                },
            ],
            tick_interval_ms: 100,
        };

        let device = factory.create_from_config(config).unwrap();
        assert_eq!(device.individual_address().to_string(), "1.2.3");
    }

    #[test]
    fn test_factory_create_simple() {
        let factory = KnxDeviceFactory::new();
        let device = factory.create_simple("test", "Test Device", IndividualAddress::new(1, 1, 1));
        assert_eq!(device.id(), "test");
    }

    #[test]
    fn test_factory_trait() {
        let factory = KnxDeviceFactory::new();
        assert_eq!(factory.protocol(), Protocol::KnxIp);
    }
}

//! BACnet device implementation integrating with the core Device trait.

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use parking_lot::RwLock;
use tokio::sync::broadcast;

use mabi_core::{
    device::{Device, DeviceInfo, DeviceState, DeviceStatistics},
    error::{Error, Result},
    protocol::Protocol,
    types::{AccessMode, DataPoint, DataPointDef, DataPointId, DataType},
    value::Value,
};

use crate::config::BacnetServerConfig;
use crate::object::property::{BACnetValue, PropertyId};
use crate::object::registry::ObjectRegistry;
use crate::object::standard::{
    AnalogInput, AnalogOutput, AnalogValue, BinaryInput, BinaryOutput, BinaryValue,
};
use crate::object::traits::{ArcObject, BACnetObject};
use crate::object::types::{ObjectId, ObjectType};

/// BACnet device implementation.
///
/// This device wraps an ObjectRegistry and implements the core Device trait,
/// providing integration between the BACnet object model and the simulator framework.
pub struct BacnetDevice {
    info: DeviceInfo,
    device_instance: u32,
    objects: Arc<ObjectRegistry>,
    point_defs: RwLock<HashMap<String, DataPointDef>>,
    object_to_point: RwLock<HashMap<ObjectId, String>>,
    stats: RwLock<DeviceStatistics>,
    event_tx: broadcast::Sender<DataPoint>,
}

impl BacnetDevice {
    /// Create a new BACnet device.
    pub fn new(config: &BacnetServerConfig) -> Self {
        let (event_tx, _) = broadcast::channel(1000);

        let info = DeviceInfo::new(
            format!("bacnet-{}", config.device_instance),
            &config.device_name,
            Protocol::BacnetIp,
        )
        .with_metadata("device_instance", config.device_instance.to_string())
        .with_metadata("vendor_id", config.vendor_id.to_string());

        Self {
            info,
            device_instance: config.device_instance,
            objects: Arc::new(ObjectRegistry::new()),
            point_defs: RwLock::new(HashMap::new()),
            object_to_point: RwLock::new(HashMap::new()),
            stats: RwLock::new(DeviceStatistics::default()),
            event_tx,
        }
    }

    /// Create a new BACnet device with existing object registry.
    pub fn with_registry(config: &BacnetServerConfig, registry: ObjectRegistry) -> Self {
        let mut device = Self::new(config);
        device.objects = Arc::new(registry);
        device.sync_point_definitions();
        device
    }

    /// Get device instance number.
    pub fn device_instance(&self) -> u32 {
        self.device_instance
    }

    /// Get the object registry.
    pub fn objects(&self) -> &Arc<ObjectRegistry> {
        &self.objects
    }

    /// Register an object and create its point definition.
    pub fn register_object(&self, object: ArcObject) {
        let object_id = object.object_identifier();
        let object_name = object.object_name().to_string();

        // Create point ID
        let point_id = format!("{}:{}", object_id.object_type as u16, object_id.instance);

        // Determine data type based on object type
        let data_type = match object_id.object_type {
            ObjectType::AnalogInput | ObjectType::AnalogOutput | ObjectType::AnalogValue => {
                DataType::Float32
            }
            ObjectType::BinaryInput | ObjectType::BinaryOutput | ObjectType::BinaryValue => {
                DataType::Bool
            }
            ObjectType::MultiStateInput
            | ObjectType::MultiStateOutput
            | ObjectType::MultiStateValue => DataType::UInt32,
            _ => DataType::String,
        };

        // Determine access mode
        let access = match object_id.object_type {
            ObjectType::AnalogOutput
            | ObjectType::AnalogValue
            | ObjectType::BinaryOutput
            | ObjectType::BinaryValue
            | ObjectType::MultiStateOutput
            | ObjectType::MultiStateValue => AccessMode::ReadWrite,
            _ => AccessMode::ReadOnly,
        };

        let def = DataPointDef::new(&point_id, &object_name, data_type).with_access(access);

        // Register object
        self.objects.register(object);

        // Add point definition
        self.object_to_point
            .write()
            .insert(object_id, point_id.clone());
        self.point_defs.write().insert(point_id, def);
    }

    /// Add an analog input object.
    pub fn add_analog_input(&self, instance: u32, name: &str) -> ObjectId {
        let object = Arc::new(AnalogInput::new(instance, name));
        let id = object.object_identifier();
        self.register_object(object);
        id
    }

    /// Add an analog output object.
    pub fn add_analog_output(&self, instance: u32, name: &str) -> ObjectId {
        let object = Arc::new(AnalogOutput::new(instance, name));
        let id = object.object_identifier();
        self.register_object(object);
        id
    }

    /// Add an analog value object.
    pub fn add_analog_value(&self, instance: u32, name: &str) -> ObjectId {
        let object = Arc::new(AnalogValue::new(instance, name));
        let id = object.object_identifier();
        self.register_object(object);
        id
    }

    /// Add a binary input object.
    pub fn add_binary_input(&self, instance: u32, name: &str) -> ObjectId {
        let object = Arc::new(BinaryInput::new(instance, name));
        let id = object.object_identifier();
        self.register_object(object);
        id
    }

    /// Add a binary output object.
    pub fn add_binary_output(&self, instance: u32, name: &str) -> ObjectId {
        let object = Arc::new(BinaryOutput::new(instance, name));
        let id = object.object_identifier();
        self.register_object(object);
        id
    }

    /// Add a binary value object.
    pub fn add_binary_value(&self, instance: u32, name: &str) -> ObjectId {
        let object = Arc::new(BinaryValue::new(instance, name));
        let id = object.object_identifier();
        self.register_object(object);
        id
    }

    /// Sync point definitions from the object registry.
    fn sync_point_definitions(&self) {
        for object in self.objects.iter() {
            let object_id = object.object_identifier();
            let object_name = object.object_name().to_string();
            let point_id = format!("{}:{}", object_id.object_type as u16, object_id.instance);

            let data_type = match object_id.object_type {
                ObjectType::AnalogInput | ObjectType::AnalogOutput | ObjectType::AnalogValue => {
                    DataType::Float32
                }
                ObjectType::BinaryInput | ObjectType::BinaryOutput | ObjectType::BinaryValue => {
                    DataType::Bool
                }
                ObjectType::MultiStateInput
                | ObjectType::MultiStateOutput
                | ObjectType::MultiStateValue => DataType::UInt32,
                _ => DataType::String,
            };

            let access = match object_id.object_type {
                ObjectType::AnalogOutput
                | ObjectType::AnalogValue
                | ObjectType::BinaryOutput
                | ObjectType::BinaryValue
                | ObjectType::MultiStateOutput
                | ObjectType::MultiStateValue => AccessMode::ReadWrite,
                _ => AccessMode::ReadOnly,
            };

            let def = DataPointDef::new(&point_id, &object_name, data_type).with_access(access);

            self.object_to_point
                .write()
                .insert(object_id, point_id.clone());
            self.point_defs.write().insert(point_id, def);
        }
    }

    /// Convert point ID to object ID.
    fn point_id_to_object_id(&self, point_id: &str) -> Option<ObjectId> {
        let parts: Vec<&str> = point_id.split(':').collect();
        if parts.len() != 2 {
            return None;
        }

        let type_code: u16 = parts[0].parse().ok()?;
        let instance: u32 = parts[1].parse().ok()?;
        let object_type = ObjectType::from_u16(type_code)?;

        Some(ObjectId::new(object_type, instance))
    }

    /// Convert BACnet value to core Value.
    fn bacnet_to_core_value(bacnet: &BACnetValue) -> Option<Value> {
        match bacnet {
            BACnetValue::Null => Some(Value::Null),
            BACnetValue::Boolean(b) => Some(Value::Bool(*b)),
            BACnetValue::Unsigned(u) => Some(Value::U32(*u)),
            BACnetValue::Signed(i) => Some(Value::I32(*i)),
            BACnetValue::Real(f) => Some(Value::F32(*f)),
            BACnetValue::Double(d) => Some(Value::F64(*d)),
            BACnetValue::CharacterString(s) => Some(Value::String(s.clone())),
            BACnetValue::Enumerated(e) => Some(Value::U32(*e)),
            _ => None,
        }
    }

    /// Convert core Value to BACnet value.
    fn core_to_bacnet_value(value: &Value) -> Option<BACnetValue> {
        match value {
            Value::Null => Some(BACnetValue::Null),
            Value::Bool(b) => Some(BACnetValue::Boolean(*b)),
            Value::U8(u) => Some(BACnetValue::Unsigned(*u as u32)),
            Value::U16(u) => Some(BACnetValue::Unsigned(*u as u32)),
            Value::U32(u) => Some(BACnetValue::Unsigned(*u)),
            Value::U64(u) => Some(BACnetValue::Unsigned(*u as u32)),
            Value::I8(i) => Some(BACnetValue::Signed(*i as i32)),
            Value::I16(i) => Some(BACnetValue::Signed(*i as i32)),
            Value::I32(i) => Some(BACnetValue::Signed(*i)),
            Value::I64(i) => Some(BACnetValue::Signed(*i as i32)),
            Value::F32(f) => Some(BACnetValue::Real(*f)),
            Value::F64(d) => Some(BACnetValue::Double(*d)),
            Value::String(s) => Some(BACnetValue::CharacterString(s.clone())),
            _ => None,
        }
    }
}

#[async_trait]
impl Device for BacnetDevice {
    fn info(&self) -> &DeviceInfo {
        &self.info
    }

    async fn initialize(&mut self) -> Result<()> {
        self.info.state = DeviceState::Online;
        self.info.point_count = self.point_defs.read().len();
        Ok(())
    }

    async fn start(&mut self) -> Result<()> {
        self.info.state = DeviceState::Online;
        Ok(())
    }

    async fn stop(&mut self) -> Result<()> {
        self.info.state = DeviceState::Offline;
        Ok(())
    }

    async fn tick(&mut self) -> Result<()> {
        Ok(())
    }

    fn point_definitions(&self) -> Vec<&DataPointDef> {
        // Limitation: returning empty since we use RwLock
        // In practice, use the objects() method to query directly
        vec![]
    }

    fn point_definition(&self, _point_id: &str) -> Option<&DataPointDef> {
        // Same limitation
        None
    }

    async fn read(&self, point_id: &str) -> Result<DataPoint> {
        let object_id = self
            .point_id_to_object_id(point_id)
            .ok_or_else(|| Error::point_not_found(&self.info.id, point_id))?;

        let bacnet_value = self
            .objects
            .read_property(&object_id, PropertyId::PresentValue)
            .map_err(|e| Error::point_not_found(&self.info.id, &e.to_string()))?;

        let value = Self::bacnet_to_core_value(&bacnet_value)
            .ok_or_else(|| Error::point_not_found(&self.info.id, "Cannot convert BACnet value"))?;

        self.stats.write().record_read();
        let id = DataPointId::new(&self.info.id, point_id);
        Ok(DataPoint::new(id, value))
    }

    async fn write(&mut self, point_id: &str, value: Value) -> Result<()> {
        let object_id = self
            .point_id_to_object_id(point_id)
            .ok_or_else(|| Error::point_not_found(&self.info.id, point_id))?;

        // Check if writable
        let is_writable = matches!(
            object_id.object_type,
            ObjectType::AnalogOutput
                | ObjectType::AnalogValue
                | ObjectType::BinaryOutput
                | ObjectType::BinaryValue
                | ObjectType::MultiStateOutput
                | ObjectType::MultiStateValue
        );

        if !is_writable {
            return Err(Error::NotSupported(format!(
                "Object {} is read-only",
                point_id
            )));
        }

        let bacnet_value = Self::core_to_bacnet_value(&value)
            .ok_or_else(|| Error::NotSupported("Cannot convert value to BACnet format".into()))?;

        self.objects
            .write_property(&object_id, PropertyId::PresentValue, bacnet_value)
            .map_err(|e| Error::point_not_found(&self.info.id, &e.to_string()))?;

        self.stats.write().record_write();

        let id = DataPointId::new(&self.info.id, point_id);
        let _ = self.event_tx.send(DataPoint::new(id, value));

        Ok(())
    }

    fn subscribe(&self) -> Option<broadcast::Receiver<DataPoint>> {
        Some(self.event_tx.subscribe())
    }

    fn statistics(&self) -> DeviceStatistics {
        self.stats.read().clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_bacnet_device() {
        let config = BacnetServerConfig::default();
        let device = BacnetDevice::new(&config);

        device.add_analog_input(1, "Temperature");
        device.add_analog_output(1, "Setpoint");

        assert_eq!(device.objects.len(), 2);
    }

    #[tokio::test]
    async fn test_bacnet_read_write() {
        let config = BacnetServerConfig::default();
        let mut device = BacnetDevice::new(&config);

        let id = device.add_analog_output(1, "Setpoint");
        device.initialize().await.unwrap();

        // Write using object registry directly first
        if let Some(object) = device.objects.get(&id) {
            let _ = object.write_property(PropertyId::PresentValue, BACnetValue::Real(21.5));
        }

        // Read
        let dp = device.read("1:1").await.unwrap();
        assert!((dp.value.as_f64().unwrap() - 21.5).abs() < 0.01);
    }
}

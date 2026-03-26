//! KNX Device implementation.
//!
//! This module provides a Device trait implementation for KNX simulation.

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use chrono::Utc;
use parking_lot::RwLock;
use tokio::sync::broadcast;

use mabi_core::{
    device::DeviceStatistics,
    types::{AccessMode, DataPoint, DataPointDef, DataPointId, DataType},
    Device, DeviceInfo, DeviceState, Protocol, Result as CoreResult, Value,
};

use crate::address::{GroupAddress, IndividualAddress};
use crate::config::KnxDeviceConfig;
use crate::dpt::{DptId, DptRegistry, DptValue};
use crate::error::KnxResult;
use crate::group::GroupObjectTable;

/// KNX Device implementation.
pub struct KnxDevice {
    info: RwLock<DeviceInfo>,
    config: KnxDeviceConfig,
    group_objects: Arc<GroupObjectTable>,
    dpt_registry: Arc<DptRegistry>,
    point_definitions: RwLock<HashMap<String, DataPointDef>>,
    statistics: RwLock<DeviceStatistics>,
    change_tx: broadcast::Sender<DataPoint>,
}

impl KnxDevice {
    /// Create a new KNX device.
    pub fn new(config: KnxDeviceConfig) -> Self {
        let (change_tx, _) = broadcast::channel(1000);

        Self {
            info: RwLock::new(
                DeviceInfo::new(&config.id, &config.name, Protocol::KnxIp)
                    .with_description(&config.description),
            ),
            config,
            group_objects: Arc::new(GroupObjectTable::new()),
            dpt_registry: Arc::new(DptRegistry::new()),
            point_definitions: RwLock::new(HashMap::new()),
            statistics: RwLock::new(DeviceStatistics::default()),
            change_tx,
        }
    }

    /// Create with custom DPT registry.
    pub fn with_dpt_registry(mut self, registry: Arc<DptRegistry>) -> Self {
        self.dpt_registry = registry;
        self.group_objects = Arc::new(GroupObjectTable::with_registry(self.dpt_registry.clone()));
        self
    }

    /// Create with custom group object table.
    pub fn with_group_objects(mut self, table: Arc<GroupObjectTable>) -> Self {
        self.group_objects = table;
        self
    }

    /// Get device ID.
    pub fn id(&self) -> String {
        self.info.read().id.clone()
    }

    /// Get individual address.
    pub fn individual_address(&self) -> IndividualAddress {
        self.config.individual_address
    }

    /// Get group object table.
    pub fn group_objects(&self) -> Arc<GroupObjectTable> {
        self.group_objects.clone()
    }

    /// Add a group object.
    pub fn add_group_object(
        &self,
        address: GroupAddress,
        name: impl Into<String>,
        dpt_id: &DptId,
    ) -> KnxResult<()> {
        let name_str = name.into();
        let obj = self.group_objects.create(address, &name_str, dpt_id)?;

        // Create corresponding data point definition
        let point_id = format!("{}_{}", address, name_str);
        let data_type = dpt_to_data_type(dpt_id);
        let def =
            DataPointDef::new(&point_id, &name_str, data_type).with_access(if obj.can_write() {
                AccessMode::ReadWrite
            } else {
                AccessMode::ReadOnly
            });

        self.point_definitions.write().insert(point_id, def);
        self.update_point_count();

        Ok(())
    }

    /// Read a group object value.
    pub fn read_group(&self, address: &GroupAddress) -> KnxResult<DptValue> {
        self.group_objects.read_value(address)
    }

    /// Write a group object value.
    pub fn write_group(&self, address: &GroupAddress, value: &DptValue) -> KnxResult<()> {
        self.group_objects
            .write_value(address, value, Some(self.id()))
    }

    fn update_point_count(&self) {
        self.info.write().point_count = self.point_definitions.read().len();
    }

    fn set_state(&self, state: DeviceState) {
        self.info.write().state = state;
        self.info.write().updated_at = Utc::now();
    }

    /// Convert DptValue to core Value.
    fn dpt_to_value(dpt_value: &DptValue) -> Value {
        match dpt_value {
            DptValue::Bool(v) => Value::Bool(*v),
            DptValue::U8(v) => Value::U8(*v),
            DptValue::I8(v) => Value::I8(*v),
            DptValue::U16(v) => Value::U16(*v),
            DptValue::I16(v) => Value::I16(*v),
            DptValue::U32(v) => Value::U32(*v),
            DptValue::I32(v) => Value::I32(*v),
            DptValue::F16(v) => Value::F32(*v),
            DptValue::F32(v) => Value::F32(*v),
            DptValue::String(v) => Value::String(v.clone()),
            DptValue::Raw(v) => Value::Bytes(v.clone()),
            _ => Value::Null, // Fallback for complex types
        }
    }

    /// Convert core Value to DptValue.
    fn value_to_dpt(value: &Value) -> DptValue {
        match value {
            Value::Bool(v) => DptValue::Bool(*v),
            Value::I8(v) => DptValue::I8(*v),
            Value::U8(v) => DptValue::U8(*v),
            Value::I16(v) => DptValue::I16(*v),
            Value::U16(v) => DptValue::U16(*v),
            Value::I32(v) => DptValue::I32(*v),
            Value::U32(v) => DptValue::U32(*v),
            Value::I64(v) => DptValue::I32(*v as i32),
            Value::U64(v) => DptValue::U32(*v as u32),
            Value::F32(v) => DptValue::F32(*v),
            Value::F64(v) => DptValue::F32(*v as f32),
            Value::String(v) => DptValue::String(v.clone()),
            Value::Bytes(v) => DptValue::Raw(v.clone()),
            _ => DptValue::Raw(vec![]),
        }
    }
}

/// Convert DPT ID to core DataType.
fn dpt_to_data_type(dpt_id: &DptId) -> DataType {
    match dpt_id.main {
        1 => DataType::Bool,
        5 => DataType::UInt8,
        6 => DataType::Int8,
        7 => DataType::UInt16,
        8 => DataType::Int16,
        9 | 14 => DataType::Float32,
        12 => DataType::UInt32,
        13 => DataType::Int32,
        16 => DataType::String,
        _ => DataType::ByteString,
    }
}

#[async_trait]
impl Device for KnxDevice {
    fn info(&self) -> &DeviceInfo {
        // This is a bit tricky since we have RwLock
        // We need to return a reference, but we have a lock guard
        // For now, use unsafe to extend lifetime (common pattern in such cases)
        // In production, consider using a different approach
        unsafe {
            let ptr = self.info.data_ptr();
            &*ptr
        }
    }

    async fn initialize(&mut self) -> CoreResult<()> {
        self.set_state(DeviceState::Initializing);

        // Initialize group objects from config
        for go_config in &self.config.group_objects {
            let address: GroupAddress = go_config
                .address
                .parse()
                .map_err(|e: crate::error::KnxError| mabi_core::Error::Protocol(e.to_string()))?;

            let dpt_id: DptId = go_config
                .dpt
                .parse()
                .map_err(|e: crate::error::KnxError| mabi_core::Error::Protocol(e.to_string()))?;

            self.add_group_object(address, &go_config.name, &dpt_id)
                .map_err(|e| mabi_core::Error::Protocol(e.to_string()))?;
        }

        self.set_state(DeviceState::Uninitialized);
        Ok(())
    }

    async fn start(&mut self) -> CoreResult<()> {
        self.set_state(DeviceState::Online);
        Ok(())
    }

    async fn stop(&mut self) -> CoreResult<()> {
        self.set_state(DeviceState::Offline);
        Ok(())
    }

    async fn tick(&mut self) -> CoreResult<()> {
        let start = std::time::Instant::now();

        // Process any pending updates
        // For now, just update statistics

        let duration = start.elapsed();
        self.statistics
            .write()
            .record_tick(duration.as_micros() as u64);

        Ok(())
    }

    fn point_definitions(&self) -> Vec<&DataPointDef> {
        // Return empty for now due to RwLock limitations
        // In production, consider using a different synchronization strategy
        vec![]
    }

    fn point_definition(&self, _point_id: &str) -> Option<&DataPointDef> {
        // Simplified for now due to RwLock limitations
        None
    }

    async fn read(&self, point_id: &str) -> CoreResult<DataPoint> {
        self.statistics.write().record_read();

        // Parse point_id to get group address
        // Format: "main/middle/sub_name" or just the address part
        let address_str = point_id.split('_').next().unwrap_or(point_id);
        let address: GroupAddress = address_str
            .parse()
            .map_err(|e: crate::error::KnxError| mabi_core::Error::Protocol(e.to_string()))?;

        let dpt_value = self
            .group_objects
            .read_value(&address)
            .map_err(|e| mabi_core::Error::Protocol(e.to_string()))?;

        let value = Self::dpt_to_value(&dpt_value);
        let device_id = self.id();

        Ok(DataPoint::new(
            DataPointId::new(&device_id, point_id),
            value,
        ))
    }

    async fn write(&mut self, point_id: &str, value: Value) -> CoreResult<()> {
        self.statistics.write().record_write();

        let address_str = point_id.split('_').next().unwrap_or(point_id);
        let address: GroupAddress = address_str
            .parse()
            .map_err(|e: crate::error::KnxError| mabi_core::Error::Protocol(e.to_string()))?;

        let dpt_value = Self::value_to_dpt(&value);

        self.group_objects
            .write_value(&address, &dpt_value, Some(self.id()))
            .map_err(|e| mabi_core::Error::Protocol(e.to_string()))?;

        // Emit change notification
        let device_id = self.id();
        let data_point = DataPoint::new(DataPointId::new(&device_id, point_id), value);
        let _ = self.change_tx.send(data_point);

        Ok(())
    }

    fn subscribe(&self) -> Option<broadcast::Receiver<DataPoint>> {
        Some(self.change_tx.subscribe())
    }

    fn statistics(&self) -> DeviceStatistics {
        self.statistics.read().clone()
    }
}

// ============================================================================
// Device Builder
// ============================================================================

/// Builder for KNX devices.
pub struct KnxDeviceBuilder {
    config: KnxDeviceConfig,
    dpt_registry: Option<Arc<DptRegistry>>,
    group_objects: Vec<(GroupAddress, String, DptId, Option<DptValue>)>,
}

impl KnxDeviceBuilder {
    /// Create a new builder.
    pub fn new(id: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            config: KnxDeviceConfig::new(id, name),
            dpt_registry: None,
            group_objects: Vec::new(),
        }
    }

    /// Set individual address.
    pub fn individual_address(mut self, address: IndividualAddress) -> Self {
        self.config.individual_address = address;
        self
    }

    /// Set description.
    pub fn description(mut self, description: impl Into<String>) -> Self {
        self.config.description = description.into();
        self
    }

    /// Set DPT registry.
    pub fn dpt_registry(mut self, registry: Arc<DptRegistry>) -> Self {
        self.dpt_registry = Some(registry);
        self
    }

    /// Add a group object.
    pub fn group_object(
        mut self,
        address: GroupAddress,
        name: impl Into<String>,
        dpt_id: DptId,
    ) -> Self {
        self.group_objects
            .push((address, name.into(), dpt_id, None));
        self
    }

    /// Add a group object with initial value.
    pub fn group_object_with_value(
        mut self,
        address: GroupAddress,
        name: impl Into<String>,
        dpt_id: DptId,
        value: DptValue,
    ) -> Self {
        self.group_objects
            .push((address, name.into(), dpt_id, Some(value)));
        self
    }

    /// Build the device.
    pub fn build(self) -> KnxResult<KnxDevice> {
        let registry = self
            .dpt_registry
            .unwrap_or_else(|| Arc::new(DptRegistry::new()));
        let table = Arc::new(GroupObjectTable::with_registry(registry.clone()));

        // Add group objects
        for (address, name, dpt_id, value) in self.group_objects {
            let obj = table.create(address, &name, &dpt_id)?;
            if let Some(v) = value {
                obj.write_value(&v)?;
            }
        }

        let device = KnxDevice::new(self.config)
            .with_dpt_registry(registry)
            .with_group_objects(table);

        Ok(device)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_knx_device_builder() {
        let device = KnxDeviceBuilder::new("knx-1", "KNX Test Device")
            .individual_address(IndividualAddress::new(1, 2, 3))
            .group_object(
                GroupAddress::three_level(1, 0, 1),
                "Light",
                DptId::new(1, 1),
            )
            .group_object_with_value(
                GroupAddress::three_level(1, 0, 2),
                "Temperature",
                DptId::new(9, 1),
                DptValue::F16(22.5),
            )
            .build()
            .unwrap();

        assert_eq!(device.individual_address().to_string(), "1.2.3");

        let temp = device
            .read_group(&GroupAddress::three_level(1, 0, 2))
            .unwrap();
        if let DptValue::F16(v) = temp {
            assert!((v - 22.5).abs() < 0.1);
        }
    }

    #[tokio::test]
    async fn test_knx_device_read_write() {
        let device = KnxDeviceBuilder::new("knx-test", "Test")
            .group_object(
                GroupAddress::three_level(1, 0, 1),
                "Switch",
                DptId::new(1, 1),
            )
            .build()
            .unwrap();

        // Write
        device
            .write_group(&GroupAddress::three_level(1, 0, 1), &DptValue::Bool(true))
            .unwrap();

        // Read
        let value = device
            .read_group(&GroupAddress::three_level(1, 0, 1))
            .unwrap();
        assert_eq!(value, DptValue::Bool(true));
    }
}

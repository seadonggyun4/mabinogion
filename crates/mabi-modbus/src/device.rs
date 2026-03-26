//! Modbus device implementation.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use parking_lot::RwLock;
use tokio::sync::broadcast;
use tracing::instrument;

use mabi_core::{
    device::{Device, DeviceInfo, DeviceState, DeviceStatistics},
    error::{Error, Result},
    protocol::Protocol,
    types::{
        AccessMode, DataPoint, DataPointDef, DataPointId, DataType, ModbusAddress,
        ModbusRegisterType,
    },
    value::Value,
};

use crate::config::ModbusDeviceConfig;
use crate::context::{DeviceContext, SharedAddressSpace};
use crate::error::ModbusResult;
use crate::profile::{DatastoreKind, PointProfile, UnitProfile};

/// Modbus device implementation.
pub struct ModbusDevice {
    /// Device info.
    info: DeviceInfo,

    /// Shared low-level device context.
    context: Arc<DeviceContext>,

    /// Data point definitions.
    point_defs: HashMap<String, DataPointDef>,

    /// Point ID to address mapping.
    point_addresses: HashMap<String, ModbusAddress>,

    /// Statistics.
    stats: RwLock<DeviceStatistics>,

    /// Value change broadcaster.
    event_tx: broadcast::Sender<DataPoint>,

    /// Response delay for simulation.
    response_delay: Duration,

    /// Start time.
    start_time: RwLock<Option<Instant>>,
}

impl ModbusDevice {
    /// Create a new Modbus device.
    pub fn new(config: ModbusDeviceConfig) -> Self {
        let context = Arc::new(
            DeviceContext::new(
                config.unit_id,
                config.name.clone(),
                DatastoreKind::dense_from_counts(
                    config.coils,
                    config.discrete_inputs,
                    config.holding_registers,
                    config.input_registers,
                )
                .build_address_space(),
                crate::types::WordOrder::default(),
            )
            .with_response_delay(Duration::from_millis(config.response_delay_ms)),
        );

        let info = DeviceInfo::new(
            format!("modbus-{}", config.unit_id),
            &config.name,
            Protocol::ModbusTcp,
        )
        .with_metadata("unit_id", config.unit_id.to_string())
        .with_tags(config.tags);

        let (event_tx, _) = broadcast::channel(1000);

        Self {
            info,
            context,
            point_defs: HashMap::new(),
            point_addresses: HashMap::new(),
            stats: RwLock::new(DeviceStatistics::default()),
            event_tx,
            response_delay: Duration::from_millis(config.response_delay_ms),
            start_time: RwLock::new(None),
        }
    }

    /// Create a device from a profile-defined unit.
    pub fn from_profile(profile: &UnitProfile) -> ModbusResult<Self> {
        let context = Arc::new(
            DeviceContext::new(
                profile.unit_id,
                profile.name.clone(),
                profile.datastore.build_address_space(),
                profile.word_order,
            )
            .with_broadcast(profile.broadcast_enabled)
            .with_response_delay(Duration::from_millis(profile.response_delay_ms)),
        );

        let info = DeviceInfo::new(
            format!("modbus-{}", profile.unit_id),
            &profile.name,
            Protocol::ModbusTcp,
        )
        .with_metadata("unit_id", profile.unit_id.to_string())
        .with_tags(profile.tags.clone());

        let (event_tx, _) = broadcast::channel(1000);
        let mut device = Self {
            info,
            context,
            point_defs: HashMap::new(),
            point_addresses: HashMap::new(),
            stats: RwLock::new(DeviceStatistics::default()),
            event_tx,
            response_delay: Duration::from_millis(profile.response_delay_ms),
            start_time: RwLock::new(None),
        };

        for point in &profile.points {
            device.apply_point_profile(point);
        }

        Ok(device)
    }

    /// Get the unit ID.
    pub fn unit_id(&self) -> u8 {
        self.context.unit_id()
    }

    /// Get the low-level device context.
    pub fn context(&self) -> &Arc<DeviceContext> {
        &self.context
    }

    /// Get the underlying address space for this device.
    pub fn address_space(&self) -> SharedAddressSpace {
        self.context.address_space()
    }

    /// Backward-compatible accessor for the unit register space.
    pub fn registers(&self) -> SharedAddressSpace {
        self.address_space()
    }

    /// Add a data point definition.
    pub fn add_point(&mut self, def: DataPointDef, address: ModbusAddress) {
        self.point_addresses.insert(def.id.clone(), address);
        self.point_defs.insert(def.id.clone(), def);
        self.info.point_count = self.point_defs.len();
    }

    fn apply_point_profile(&mut self, profile: &PointProfile) {
        match profile.register_type {
            ModbusRegisterType::HoldingRegister => self.add_holding_register(
                profile.id.clone(),
                profile.name.clone(),
                profile.address,
                profile.data_type,
            ),
            ModbusRegisterType::InputRegister => self.add_input_register(
                profile.id.clone(),
                profile.name.clone(),
                profile.address,
                profile.data_type,
            ),
            ModbusRegisterType::Coil => {
                self.add_coil(profile.id.clone(), profile.name.clone(), profile.address)
            }
            ModbusRegisterType::DiscreteInput => {
                self.add_discrete_input(profile.id.clone(), profile.name.clone(), profile.address)
            }
        }
    }

    /// Add a holding register point.
    pub fn add_holding_register(
        &mut self,
        id: impl Into<String>,
        name: impl Into<String>,
        address: u16,
        data_type: DataType,
    ) {
        let id = id.into();
        let register_count = match data_type {
            DataType::Bool
            | DataType::Int8
            | DataType::UInt8
            | DataType::Int16
            | DataType::UInt16 => 1,
            DataType::Int32 | DataType::UInt32 | DataType::Float32 => 2,
            DataType::Int64 | DataType::UInt64 | DataType::Float64 | DataType::DateTime => 4,
            _ => 1,
        };

        let def = DataPointDef::new(&id, name, data_type).with_access(AccessMode::ReadWrite);

        let addr = ModbusAddress {
            register_type: ModbusRegisterType::HoldingRegister,
            address,
            count: register_count,
        };

        self.add_point(def, addr);
    }

    /// Add an input register point.
    pub fn add_input_register(
        &mut self,
        id: impl Into<String>,
        name: impl Into<String>,
        address: u16,
        data_type: DataType,
    ) {
        let id = id.into();
        let register_count = match data_type {
            DataType::Bool
            | DataType::Int8
            | DataType::UInt8
            | DataType::Int16
            | DataType::UInt16 => 1,
            DataType::Int32 | DataType::UInt32 | DataType::Float32 => 2,
            DataType::Int64 | DataType::UInt64 | DataType::Float64 | DataType::DateTime => 4,
            _ => 1,
        };

        let def = DataPointDef::new(&id, name, data_type).with_access(AccessMode::ReadOnly);

        let addr = ModbusAddress {
            register_type: ModbusRegisterType::InputRegister,
            address,
            count: register_count,
        };

        self.add_point(def, addr);
    }

    /// Add a coil point.
    pub fn add_coil(&mut self, id: impl Into<String>, name: impl Into<String>, address: u16) {
        let id = id.into();
        let def = DataPointDef::new(&id, name, DataType::Bool).with_access(AccessMode::ReadWrite);

        let addr = ModbusAddress {
            register_type: ModbusRegisterType::Coil,
            address,
            count: 1,
        };

        self.add_point(def, addr);
    }

    /// Add a discrete input point.
    pub fn add_discrete_input(
        &mut self,
        id: impl Into<String>,
        name: impl Into<String>,
        address: u16,
    ) {
        let id = id.into();
        let def = DataPointDef::new(&id, name, DataType::Bool).with_access(AccessMode::ReadOnly);

        let addr = ModbusAddress {
            register_type: ModbusRegisterType::DiscreteInput,
            address,
            count: 1,
        };

        self.add_point(def, addr);
    }

    /// Read value from registers.
    fn read_value(&self, point_id: &str) -> Result<Value> {
        let def = self
            .point_defs
            .get(point_id)
            .ok_or_else(|| Error::point_not_found(&self.info.id, point_id))?;

        let addr = self
            .point_addresses
            .get(point_id)
            .ok_or_else(|| Error::point_not_found(&self.info.id, point_id))?;

        match addr.register_type {
            ModbusRegisterType::Coil => {
                let values = self.address_space().read_coils(addr.address, 1)?;
                Ok(Value::Bool(values[0]))
            }
            ModbusRegisterType::DiscreteInput => {
                let values = self.address_space().read_discrete_inputs(addr.address, 1)?;
                Ok(Value::Bool(values[0]))
            }
            ModbusRegisterType::HoldingRegister => {
                self.read_register_value(&def.data_type, addr.address, addr.count, true)
            }
            ModbusRegisterType::InputRegister => {
                self.read_register_value(&def.data_type, addr.address, addr.count, false)
            }
        }
    }

    fn read_register_value(
        &self,
        data_type: &DataType,
        address: u16,
        count: u16,
        is_holding: bool,
    ) -> Result<Value> {
        let registers = if is_holding {
            self.address_space()
                .read_holding_registers(address, count)?
        } else {
            self.address_space().read_input_registers(address, count)?
        };

        let value = match data_type {
            DataType::Bool => Value::Bool(registers[0] != 0),
            DataType::Int16 => Value::I16(registers[0] as i16),
            DataType::UInt16 => Value::U16(registers[0]),
            DataType::Int32 if registers.len() >= 2 => {
                let bytes = [registers[0].to_be_bytes(), registers[1].to_be_bytes()];
                Value::I32(i32::from_be_bytes([
                    bytes[0][0],
                    bytes[0][1],
                    bytes[1][0],
                    bytes[1][1],
                ]))
            }
            DataType::UInt32 if registers.len() >= 2 => {
                let bytes = [registers[0].to_be_bytes(), registers[1].to_be_bytes()];
                Value::U32(u32::from_be_bytes([
                    bytes[0][0],
                    bytes[0][1],
                    bytes[1][0],
                    bytes[1][1],
                ]))
            }
            DataType::Float32 if registers.len() >= 2 => {
                let bytes = [registers[0].to_be_bytes(), registers[1].to_be_bytes()];
                Value::F32(f32::from_be_bytes([
                    bytes[0][0],
                    bytes[0][1],
                    bytes[1][0],
                    bytes[1][1],
                ]))
            }
            DataType::Float64 if registers.len() >= 4 => {
                let bytes: Vec<u8> = registers[..4]
                    .iter()
                    .flat_map(|r| r.to_be_bytes())
                    .collect();
                Value::F64(f64::from_be_bytes([
                    bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
                ]))
            }
            _ => Value::U16(registers[0]),
        };

        Ok(value)
    }

    /// Write value to registers.
    fn write_value(&self, point_id: &str, value: Value) -> Result<()> {
        let def = self
            .point_defs
            .get(point_id)
            .ok_or_else(|| Error::point_not_found(&self.info.id, point_id))?;

        if !def.access.is_writable() {
            return Err(Error::NotSupported(format!(
                "Point {} is read-only",
                point_id
            )));
        }

        let addr = self
            .point_addresses
            .get(point_id)
            .ok_or_else(|| Error::point_not_found(&self.info.id, point_id))?;

        match addr.register_type {
            ModbusRegisterType::Coil => {
                let bool_value = value.as_bool().ok_or_else(|| Error::TypeMismatch {
                    expected: "bool".to_string(),
                    actual: value.type_name().to_string(),
                })?;
                self.address_space().write_coil(addr.address, bool_value)?;
            }
            ModbusRegisterType::HoldingRegister => {
                let registers = value.to_registers();
                if registers.is_empty() {
                    return Err(Error::InvalidValue {
                        point_id: point_id.to_string(),
                        reason: "Cannot convert value to registers".to_string(),
                    });
                }
                self.address_space()
                    .write_holding_registers(addr.address, &registers)?;
            }
            ModbusRegisterType::DiscreteInput | ModbusRegisterType::InputRegister => {
                return Err(Error::NotSupported(format!(
                    "Cannot write to {} register type",
                    match addr.register_type {
                        ModbusRegisterType::DiscreteInput => "discrete input",
                        ModbusRegisterType::InputRegister => "input",
                        _ => unreachable!(),
                    }
                )));
            }
        }

        Ok(())
    }

    /// Shared read surface for runtime-backed device ports.
    pub async fn read_point(&self, point_id: &str) -> Result<DataPoint> {
        if !self.response_delay.is_zero() {
            tokio::time::sleep(self.response_delay).await;
        }

        let value = self.read_value(point_id)?;
        self.stats.write().record_read();

        let id = DataPointId::new(&self.info.id, point_id);
        Ok(DataPoint::new(id, value))
    }

    /// Shared write surface for runtime-backed device ports.
    pub async fn write_point(&self, point_id: &str, value: Value) -> Result<()> {
        if !self.response_delay.is_zero() {
            tokio::time::sleep(self.response_delay).await;
        }

        self.write_value(point_id, value.clone())?;
        self.stats.write().record_write();

        let id = DataPointId::new(&self.info.id, point_id);
        let _ = self.event_tx.send(DataPoint::new(id, value));

        Ok(())
    }

    /// Returns cloned point definitions for control-plane catalog queries.
    pub fn point_definitions_owned(&self) -> Vec<DataPointDef> {
        self.point_defs.values().cloned().collect()
    }
}

#[async_trait]
impl Device for ModbusDevice {
    fn info(&self) -> &DeviceInfo {
        &self.info
    }

    async fn initialize(&mut self) -> Result<()> {
        self.info.state = DeviceState::Initializing;
        // Initialize registers to default values if needed
        self.info.state = DeviceState::Online;
        Ok(())
    }

    async fn start(&mut self) -> Result<()> {
        *self.start_time.write() = Some(Instant::now());
        self.info.state = DeviceState::Online;
        Ok(())
    }

    async fn stop(&mut self) -> Result<()> {
        self.info.state = DeviceState::Offline;
        if let Some(start) = *self.start_time.read() {
            self.stats.write().uptime_secs = start.elapsed().as_secs();
        }
        Ok(())
    }

    async fn tick(&mut self) -> Result<()> {
        let start = Instant::now();
        // Simulation updates can be done here
        self.stats
            .write()
            .record_tick(start.elapsed().as_micros() as u64);
        Ok(())
    }

    fn point_definitions(&self) -> Vec<&DataPointDef> {
        self.point_defs.values().collect()
    }

    fn point_definition(&self, point_id: &str) -> Option<&DataPointDef> {
        self.point_defs.get(point_id)
    }

    #[instrument(skip(self))]
    async fn read(&self, point_id: &str) -> Result<DataPoint> {
        self.read_point(point_id).await
    }

    #[instrument(skip(self, value))]
    async fn write(&mut self, point_id: &str, value: Value) -> Result<()> {
        self.write_point(point_id, value).await
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
    async fn test_modbus_device_creation() {
        let config = ModbusDeviceConfig::new(1, "Test Device");
        let device = ModbusDevice::new(config);

        assert_eq!(device.unit_id(), 1);
        assert_eq!(device.info().protocol, Protocol::ModbusTcp);
    }

    #[tokio::test]
    async fn test_modbus_device_points() {
        let config = ModbusDeviceConfig::new(1, "Test Device");
        let mut device = ModbusDevice::new(config);

        device.add_holding_register("temp", "Temperature", 0, DataType::Float32);
        device.add_coil("relay1", "Relay 1", 0);

        assert_eq!(device.point_definitions().len(), 2);
        assert!(device.point_definition("temp").is_some());
    }

    #[tokio::test]
    async fn test_modbus_device_read_write() {
        let config = ModbusDeviceConfig::new(1, "Test Device");
        let mut device = ModbusDevice::new(config);

        device.add_holding_register("value", "Value", 0, DataType::UInt16);
        device.initialize().await.unwrap();

        // Write
        device.write("value", Value::U16(12345)).await.unwrap();

        // Read
        let dp = device.read("value").await.unwrap();
        assert_eq!(dp.value.as_u16(), Some(12345));
    }
}

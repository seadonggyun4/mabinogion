//! OPC UA device implementation (placeholder).

use async_trait::async_trait;
use parking_lot::RwLock;
use std::collections::HashMap;
use tokio::sync::broadcast;

use mabi_core::{
    device::{Device, DeviceInfo, DeviceState, DeviceStatistics},
    error::{Error, Result},
    protocol::Protocol,
    types::{DataPoint, DataPointDef, DataPointId},
    value::Value,
};

/// OPC UA device implementation.
pub struct OpcUaDevice {
    info: DeviceInfo,
    point_defs: HashMap<String, DataPointDef>,
    values: RwLock<HashMap<String, Value>>,
    stats: RwLock<DeviceStatistics>,
    event_tx: broadcast::Sender<DataPoint>,
}

impl OpcUaDevice {
    /// Create a new OPC UA device.
    pub fn new(id: impl Into<String>, name: impl Into<String>) -> Self {
        let (event_tx, _) = broadcast::channel(1000);

        Self {
            info: DeviceInfo::new(id, name, Protocol::OpcUa),
            point_defs: HashMap::new(),
            values: RwLock::new(HashMap::new()),
            stats: RwLock::new(DeviceStatistics::default()),
            event_tx,
        }
    }

    /// Add a node/variable to the device.
    pub fn add_node(&mut self, def: DataPointDef, initial_value: Value) {
        self.values.write().insert(def.id.clone(), initial_value);
        self.point_defs.insert(def.id.clone(), def);
        self.info.point_count = self.point_defs.len();
    }
}

#[async_trait]
impl Device for OpcUaDevice {
    fn info(&self) -> &DeviceInfo {
        &self.info
    }

    async fn initialize(&mut self) -> Result<()> {
        self.info.state = DeviceState::Online;
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
        self.point_defs.values().collect()
    }

    fn point_definition(&self, point_id: &str) -> Option<&DataPointDef> {
        self.point_defs.get(point_id)
    }

    async fn read(&self, point_id: &str) -> Result<DataPoint> {
        let value = self
            .values
            .read()
            .get(point_id)
            .cloned()
            .ok_or_else(|| Error::point_not_found(&self.info.id, point_id))?;

        self.stats.write().record_read();
        let id = DataPointId::new(&self.info.id, point_id);
        Ok(DataPoint::new(id, value))
    }

    async fn write(&mut self, point_id: &str, value: Value) -> Result<()> {
        if !self.point_defs.contains_key(point_id) {
            return Err(Error::point_not_found(&self.info.id, point_id));
        }

        self.values
            .write()
            .insert(point_id.to_string(), value.clone());
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

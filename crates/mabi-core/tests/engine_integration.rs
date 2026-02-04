//! Integration tests for SimulatorEngine.
//!
//! These tests verify the core engine functionality including:
//! - Device lifecycle management
//! - Data point read/write operations
//! - Event broadcasting
//! - Metrics collection

use std::sync::Arc;
use std::time::Duration;

use mabi_core::config::{DeviceConfig, EngineConfig};
use mabi_core::device::{
    BoxedDevice, Device, DeviceInfo, DeviceState, DeviceStatistics,
};
use mabi_core::engine::{EngineState, SimulatorEngine};
use mabi_core::error::Result;
use mabi_core::factory::{DeviceFactory, FactoryRegistry};
use mabi_core::protocol::Protocol;
use mabi_core::types::{
    AccessMode, DataPoint, DataPointDef, DataPointId, DataType, Quality,
};
use mabi_core::value::Value;

use async_trait::async_trait;
use parking_lot::RwLock;
use tokio::sync::broadcast;

// =============================================================================
// Mock Device Implementation
// =============================================================================

/// A mock device for testing purposes.
struct MockDevice {
    info: DeviceInfo,
    points: Vec<DataPointDef>,
    values: Arc<RwLock<std::collections::HashMap<String, Value>>>,
    change_tx: broadcast::Sender<DataPoint>,
    statistics: Arc<RwLock<DeviceStatistics>>,
}

impl MockDevice {
    fn new(id: &str, name: &str, protocol: Protocol) -> Self {
        let mut info = DeviceInfo::new(id, name, protocol);
        info.point_count = 3;

        let points = vec![
            DataPointDef::new("temperature", "Temperature", DataType::Float64)
                .with_units("°C")
                .with_range(-40.0, 100.0),
            DataPointDef::new("humidity", "Humidity", DataType::Float64)
                .with_units("%")
                .with_range(0.0, 100.0),
            DataPointDef::new("status", "Status", DataType::Bool)
                .with_access(AccessMode::ReadOnly),
        ];

        let mut values = std::collections::HashMap::new();
        values.insert("temperature".to_string(), Value::F64(25.0));
        values.insert("humidity".to_string(), Value::F64(50.0));
        values.insert("status".to_string(), Value::Bool(true));

        let (change_tx, _) = broadcast::channel(100);

        Self {
            info,
            points,
            values: Arc::new(RwLock::new(values)),
            change_tx,
            statistics: Arc::new(RwLock::new(DeviceStatistics::default())),
        }
    }
}

#[async_trait]
impl Device for MockDevice {
    fn info(&self) -> &DeviceInfo {
        &self.info
    }

    async fn initialize(&mut self) -> Result<()> {
        self.info.state = DeviceState::Initializing;
        tokio::time::sleep(Duration::from_millis(10)).await;
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
        let start = std::time::Instant::now();
        // Simulate some work
        tokio::time::sleep(Duration::from_micros(100)).await;
        let duration = start.elapsed();
        self.statistics
            .write()
            .record_tick(duration.as_micros() as u64);
        Ok(())
    }

    fn point_definitions(&self) -> Vec<&DataPointDef> {
        self.points.iter().collect()
    }

    fn point_definition(&self, point_id: &str) -> Option<&DataPointDef> {
        self.points.iter().find(|p| p.id == point_id)
    }

    async fn read(&self, point_id: &str) -> Result<DataPoint> {
        let values = self.values.read();
        let value = values
            .get(point_id)
            .cloned()
            .unwrap_or(Value::Null);

        self.statistics.write().record_read();

        Ok(DataPoint::new(
            DataPointId::new(&self.info.id, point_id),
            value,
        )
        .with_quality(Quality::GOOD))
    }

    async fn write(&mut self, point_id: &str, value: Value) -> Result<()> {
        self.values.write().insert(point_id.to_string(), value.clone());
        self.statistics.write().record_write();

        let dp = DataPoint::new(DataPointId::new(&self.info.id, point_id), value);
        let _ = self.change_tx.send(dp);

        Ok(())
    }

    fn subscribe(&self) -> Option<broadcast::Receiver<DataPoint>> {
        Some(self.change_tx.subscribe())
    }

    fn statistics(&self) -> DeviceStatistics {
        self.statistics.read().clone()
    }
}

// =============================================================================
// Mock Factory Implementation
// =============================================================================

struct MockDeviceFactory {
    protocol: Protocol,
}

impl MockDeviceFactory {
    fn new(protocol: Protocol) -> Self {
        Self { protocol }
    }
}

impl DeviceFactory for MockDeviceFactory {
    fn protocol(&self) -> Protocol {
        self.protocol
    }

    fn create(&self, config: DeviceConfig) -> Result<BoxedDevice> {
        Ok(Box::new(MockDevice::new(
            &config.id,
            &config.name,
            config.protocol,
        )))
    }
}

// =============================================================================
// Engine Lifecycle Tests
// =============================================================================

#[tokio::test]
async fn test_engine_start_stop() {
    let config = EngineConfig::default();
    let engine = SimulatorEngine::new(config);

    assert_eq!(engine.state(), EngineState::Stopped);

    engine.start().await.unwrap();
    assert_eq!(engine.state(), EngineState::Running);

    engine.stop().await.unwrap();
    assert_eq!(engine.state(), EngineState::Stopped);
}

#[tokio::test]
async fn test_engine_cannot_start_twice() {
    let config = EngineConfig::default();
    let engine = SimulatorEngine::new(config);

    engine.start().await.unwrap();
    let result = engine.start().await;
    assert!(result.is_err());

    engine.stop().await.unwrap();
}

#[tokio::test]
async fn test_engine_cannot_stop_when_stopped() {
    let config = EngineConfig::default();
    let engine = SimulatorEngine::new(config);

    let result = engine.stop().await;
    assert!(result.is_err());
}

// =============================================================================
// Device Management Tests
// =============================================================================

#[tokio::test]
async fn test_add_and_remove_device() {
    let config = EngineConfig::default();
    let engine = SimulatorEngine::new(config);

    let device = MockDevice::new("dev-001", "Test Device", Protocol::ModbusTcp);

    engine.add_device(Box::new(device)).await.unwrap();
    assert_eq!(engine.device_count(), 1);

    // Verify device is accessible
    let handle = engine.device("dev-001").unwrap();
    assert_eq!(handle.id(), "dev-001");

    // Remove device
    engine.remove_device("dev-001").await.unwrap();
    assert_eq!(engine.device_count(), 0);
    assert!(engine.device("dev-001").is_none());
}

#[tokio::test]
async fn test_add_duplicate_device() {
    let config = EngineConfig::default();
    let engine = SimulatorEngine::new(config);

    let device1 = MockDevice::new("dev-001", "Test Device 1", Protocol::ModbusTcp);
    let device2 = MockDevice::new("dev-001", "Test Device 2", Protocol::ModbusTcp);

    engine.add_device(Box::new(device1)).await.unwrap();
    let result = engine.add_device(Box::new(device2)).await;

    assert!(result.is_err());
    assert_eq!(engine.device_count(), 1);
}

#[tokio::test]
async fn test_device_capacity_limit() {
    let config = EngineConfig::default().with_max_devices(2);
    let engine = SimulatorEngine::new(config);

    let device1 = MockDevice::new("dev-001", "Device 1", Protocol::ModbusTcp);
    let device2 = MockDevice::new("dev-002", "Device 2", Protocol::ModbusTcp);
    let device3 = MockDevice::new("dev-003", "Device 3", Protocol::ModbusTcp);

    engine.add_device(Box::new(device1)).await.unwrap();
    engine.add_device(Box::new(device2)).await.unwrap();

    let result = engine.add_device(Box::new(device3)).await;
    assert!(result.is_err());
    assert_eq!(engine.device_count(), 2);
}

#[tokio::test]
async fn test_list_devices_by_protocol() {
    let config = EngineConfig::default();
    let engine = SimulatorEngine::new(config);

    let device1 = MockDevice::new("modbus-001", "Modbus Device", Protocol::ModbusTcp);
    let device2 = MockDevice::new("opcua-001", "OPC UA Device", Protocol::OpcUa);
    let device3 = MockDevice::new("modbus-002", "Modbus Device 2", Protocol::ModbusTcp);

    engine.add_device(Box::new(device1)).await.unwrap();
    engine.add_device(Box::new(device2)).await.unwrap();
    engine.add_device(Box::new(device3)).await.unwrap();

    let modbus_devices = engine.list_devices_by_protocol(Protocol::ModbusTcp);
    assert_eq!(modbus_devices.len(), 2);

    let opcua_devices = engine.list_devices_by_protocol(Protocol::OpcUa);
    assert_eq!(opcua_devices.len(), 1);

    let bacnet_devices = engine.list_devices_by_protocol(Protocol::BacnetIp);
    assert_eq!(bacnet_devices.len(), 0);
}

// =============================================================================
// Data Point Operations Tests
// =============================================================================

#[tokio::test]
async fn test_read_point() {
    let config = EngineConfig::default();
    let engine = SimulatorEngine::new(config);

    let device = MockDevice::new("dev-001", "Test Device", Protocol::ModbusTcp);
    engine.add_device(Box::new(device)).await.unwrap();

    let dp = engine.read_point("dev-001", "temperature").await.unwrap();

    assert_eq!(dp.id.device_id, "dev-001");
    assert_eq!(dp.id.point_id, "temperature");
    assert!(dp.quality.is_good());

    if let Value::F64(temp) = dp.value {
        assert!((temp - 25.0).abs() < 0.001);
    } else {
        panic!("Expected F64 value");
    }
}

#[tokio::test]
async fn test_write_point() {
    let config = EngineConfig::default();
    let engine = SimulatorEngine::new(config);

    let device = MockDevice::new("dev-001", "Test Device", Protocol::ModbusTcp);
    engine.add_device(Box::new(device)).await.unwrap();

    // Write new value
    engine
        .write_point("dev-001", "temperature", Value::F64(30.0))
        .await
        .unwrap();

    // Read back
    let dp = engine.read_point("dev-001", "temperature").await.unwrap();

    if let Value::F64(temp) = dp.value {
        assert!((temp - 30.0).abs() < 0.001);
    } else {
        panic!("Expected F64 value");
    }
}

#[tokio::test]
async fn test_read_nonexistent_device() {
    let config = EngineConfig::default();
    let engine = SimulatorEngine::new(config);

    let result = engine.read_point("nonexistent", "temperature").await;
    assert!(result.is_err());
}

// =============================================================================
// Engine Tick Tests
// =============================================================================

#[tokio::test]
async fn test_engine_tick() {
    let config = EngineConfig::default();
    let engine = SimulatorEngine::new(config);

    let device = MockDevice::new("dev-001", "Test Device", Protocol::ModbusTcp);
    engine.add_device(Box::new(device)).await.unwrap();

    engine.start().await.unwrap();

    // Run several ticks
    for _ in 0..5 {
        engine.tick().await.unwrap();
    }

    assert_eq!(engine.tick_count(), 5);

    engine.stop().await.unwrap();
}

#[tokio::test]
async fn test_engine_tick_when_stopped() {
    let config = EngineConfig::default();
    let engine = SimulatorEngine::new(config);

    // Tick should be no-op when engine is stopped
    engine.tick().await.unwrap();
    assert_eq!(engine.tick_count(), 0);
}

// =============================================================================
// Event Broadcasting Tests
// =============================================================================

#[tokio::test]
async fn test_engine_events() {
    let config = EngineConfig::default();
    let engine = SimulatorEngine::new(config);

    let mut rx = engine.subscribe();

    let device = MockDevice::new("dev-001", "Test Device", Protocol::ModbusTcp);
    engine.add_device(Box::new(device)).await.unwrap();

    // Should receive DeviceAdded event
    let event = tokio::time::timeout(Duration::from_millis(100), rx.recv())
        .await
        .expect("Timeout waiting for event")
        .expect("Channel closed");

    match event {
        mabi_core::engine::EngineEvent::DeviceAdded { device_id, protocol } => {
            assert_eq!(device_id, "dev-001");
            assert_eq!(protocol, Protocol::ModbusTcp);
        }
        _ => panic!("Expected DeviceAdded event"),
    }
}

// =============================================================================
// Factory Registry Integration Tests
// =============================================================================

#[tokio::test]
async fn test_factory_registry_with_engine() {
    let registry = Arc::new(FactoryRegistry::new());
    registry
        .register(MockDeviceFactory::new(Protocol::ModbusTcp))
        .unwrap();
    registry
        .register(MockDeviceFactory::new(Protocol::OpcUa))
        .unwrap();

    let engine = SimulatorEngine::new(EngineConfig::default());

    // Create devices via factory
    let modbus_config = DeviceConfig {
        id: "modbus-001".to_string(),
        name: "Modbus Device".to_string(),
        description: String::new(),
        protocol: Protocol::ModbusTcp,
        address: None,
        points: vec![],
        metadata: Default::default(),
        tags: Default::default(),
    };

    let opcua_config = DeviceConfig {
        id: "opcua-001".to_string(),
        name: "OPC UA Device".to_string(),
        description: String::new(),
        protocol: Protocol::OpcUa,
        address: None,
        points: vec![],
        metadata: Default::default(),
        tags: Default::default(),
    };

    let modbus_device = registry.create_device(modbus_config).unwrap();
    let opcua_device = registry.create_device(opcua_config).unwrap();

    engine.add_device(modbus_device).await.unwrap();
    engine.add_device(opcua_device).await.unwrap();

    assert_eq!(engine.device_count(), 2);
    assert_eq!(
        engine.list_devices_by_protocol(Protocol::ModbusTcp).len(),
        1
    );
    assert_eq!(engine.list_devices_by_protocol(Protocol::OpcUa).len(), 1);
}

// =============================================================================
// Metrics Integration Tests
// =============================================================================

#[tokio::test]
async fn test_metrics_collection() {
    let config = EngineConfig::default();
    let engine = SimulatorEngine::new(config);

    let device = MockDevice::new("dev-001", "Test Device", Protocol::ModbusTcp);
    engine.add_device(Box::new(device)).await.unwrap();

    // Perform some operations
    for _ in 0..10 {
        engine.read_point("dev-001", "temperature").await.unwrap();
    }

    for _ in 0..5 {
        engine
            .write_point("dev-001", "temperature", Value::F64(25.0))
            .await
            .unwrap();
    }

    // Check metrics
    let snapshot = engine.metrics().snapshot();
    assert!(snapshot.devices_active >= 1);
}

// =============================================================================
// Shutdown Tests
// =============================================================================

#[tokio::test]
async fn test_shutdown_request() {
    let config = EngineConfig::default();
    let engine = SimulatorEngine::new(config);

    assert!(!engine.is_shutdown_requested());

    engine.request_shutdown();

    assert!(engine.is_shutdown_requested());
}

#[tokio::test]
async fn test_uptime() {
    let config = EngineConfig::default();
    let engine = SimulatorEngine::new(config);

    assert!(engine.uptime().is_none());

    engine.start().await.unwrap();

    tokio::time::sleep(Duration::from_millis(50)).await;

    let uptime = engine.uptime().unwrap();
    assert!(uptime.as_millis() >= 50);

    engine.stop().await.unwrap();
}

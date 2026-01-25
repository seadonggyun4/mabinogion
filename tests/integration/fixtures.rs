//! Test fixtures and factory utilities.
//!
//! Provides reusable test fixtures and factories.

use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::RwLock;

// =============================================================================
// Server Fixtures
// =============================================================================

/// Fixture for managing test servers.
pub struct ServerFixture<S> {
    server: Arc<RwLock<Option<S>>>,
    addr: SocketAddr,
    shutdown: tokio::sync::Notify,
}

impl<S> ServerFixture<S> {
    /// Create a new server fixture.
    pub fn new(addr: SocketAddr) -> Self {
        Self {
            server: Arc::new(RwLock::new(None)),
            addr,
            shutdown: tokio::sync::Notify::new(),
        }
    }

    /// Get the server address.
    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    /// Get the server instance.
    pub async fn server(&self) -> Option<Arc<RwLock<Option<S>>>> {
        Some(self.server.clone())
    }

    /// Signal shutdown.
    pub fn shutdown(&self) {
        self.shutdown.notify_waiters();
    }

    /// Wait for shutdown signal.
    pub async fn wait_shutdown(&self) {
        self.shutdown.notified().await;
    }
}

// =============================================================================
// Scenario Fixtures
// =============================================================================

/// Factory for creating test scenarios.
pub struct ScenarioFactory;

impl ScenarioFactory {
    /// Create a minimal scenario for testing.
    pub fn minimal() -> String {
        r#"
name: Minimal Test Scenario
description: A minimal scenario for testing

devices:
  - id: test-device-1
    protocol: modbus_tcp
    name: Test Device
    points:
      - id: point-1
        data_type: uint16
        initial_value: 0
"#.to_string()
    }

    /// Create a multi-device scenario.
    pub fn multi_device(count: usize) -> String {
        let mut devices = Vec::new();
        for i in 0..count {
            devices.push(format!(
                r#"  - id: device-{}
    protocol: modbus_tcp
    name: Device {}
    points:
      - id: temp
        data_type: float32
        initial_value: 20.0
      - id: humidity
        data_type: float32
        initial_value: 50.0"#,
                i, i
            ));
        }

        format!(
            r#"
name: Multi-Device Test Scenario
description: Scenario with {} devices

devices:
{}
"#,
            count,
            devices.join("\n")
        )
    }

    /// Create a scenario with events.
    pub fn with_events() -> String {
        r#"
name: Event Test Scenario
description: Scenario with event triggers

devices:
  - id: event-device
    protocol: modbus_tcp
    name: Event Device
    points:
      - id: trigger
        data_type: bool
        initial_value: false
      - id: value
        data_type: uint16
        initial_value: 0

events:
  - name: on_trigger
    trigger: "point:trigger == true"
    actions:
      - "set:value:100"
"#.to_string()
    }

    /// Create a chaos scenario.
    pub fn chaos() -> String {
        r#"
name: Chaos Test Scenario
description: Scenario with chaos injection

devices:
  - id: chaos-device
    protocol: modbus_tcp
    name: Chaos Device
    points:
      - id: value
        data_type: uint16
        initial_value: 0

chaos:
  - type: latency
    target: chaos-device
    delay_ms: 100
    probability: 0.5
  - type: packet_loss
    target: chaos-device
    probability: 0.1
"#.to_string()
    }

    /// Create a pattern scenario.
    pub fn with_patterns() -> String {
        r#"
name: Pattern Test Scenario
description: Scenario with data patterns

devices:
  - id: pattern-device
    protocol: modbus_tcp
    name: Pattern Device
    points:
      - id: sine_value
        data_type: float32
        initial_value: 0.0
        pattern:
          type: sine
          amplitude: 10.0
          period_secs: 60
          offset: 50.0
      - id: ramp_value
        data_type: float32
        initial_value: 0.0
        pattern:
          type: ramp
          start: 0.0
          end: 100.0
          period_secs: 120
"#.to_string()
    }
}

// =============================================================================
// Configuration Fixtures
// =============================================================================

/// Factory for creating test configurations.
pub struct ConfigFactory;

impl ConfigFactory {
    /// Create a Modbus TCP configuration.
    pub fn modbus_tcp(port: u16) -> String {
        format!(
            r#"
modbus:
  tcp:
    port: {}
    bind: "0.0.0.0"
    max_connections: 100
    timeout_ms: 5000
"#,
            port
        )
    }

    /// Create an OPC UA configuration.
    pub fn opcua(port: u16) -> String {
        format!(
            r#"
opcua:
  port: {}
  endpoint: "/"
  security_mode: None
  max_sessions: 100
"#,
            port
        )
    }

    /// Create a BACnet configuration.
    pub fn bacnet(port: u16, device_instance: u32) -> String {
        format!(
            r#"
bacnet:
  port: {}
  device_instance: {}
  vendor_id: 9999
  max_apdu_length: 1476
"#,
            port, device_instance
        )
    }

    /// Create a KNX configuration.
    pub fn knx(port: u16) -> String {
        format!(
            r#"
knx:
  port: {}
  individual_address: "1.1.1"
  tunneling:
    enabled: true
    max_connections: 5
"#,
            port
        )
    }

    /// Create a full engine configuration.
    pub fn engine() -> String {
        r#"
engine:
  max_devices: 10000
  max_points_per_device: 1000
  tick_interval_ms: 100
  metrics:
    enabled: true
    export_interval_secs: 60
"#.to_string()
    }
}

// =============================================================================
// Data Fixtures
// =============================================================================

/// Factory for creating test data.
pub struct DataFactory;

impl DataFactory {
    /// Generate random register values.
    pub fn random_registers(count: usize) -> Vec<u16> {
        use rand::Rng;
        let mut rng = rand::thread_rng();
        (0..count).map(|_| rng.gen()).collect()
    }

    /// Generate sequential register values.
    pub fn sequential_registers(start: u16, count: usize) -> Vec<u16> {
        (0..count).map(|i| start.wrapping_add(i as u16)).collect()
    }

    /// Generate random coil values.
    pub fn random_coils(count: usize) -> Vec<bool> {
        use rand::Rng;
        let mut rng = rand::thread_rng();
        (0..count).map(|_| rng.gen()).collect()
    }

    /// Generate a sine wave.
    pub fn sine_wave(samples: usize, amplitude: f64, offset: f64) -> Vec<f64> {
        use std::f64::consts::PI;
        (0..samples)
            .map(|i| {
                let t = i as f64 / samples as f64 * 2.0 * PI;
                offset + amplitude * t.sin()
            })
            .collect()
    }

    /// Generate noise data.
    pub fn noise(count: usize, min: f64, max: f64) -> Vec<f64> {
        use rand::Rng;
        let mut rng = rand::thread_rng();
        (0..count).map(|_| rng.gen_range(min..max)).collect()
    }
}

// =============================================================================
// Mock Fixtures
// =============================================================================

/// Mock device for testing.
#[derive(Debug, Clone)]
pub struct MockDevice {
    pub id: String,
    pub points: std::collections::HashMap<String, MockPoint>,
}

#[derive(Debug, Clone)]
pub struct MockPoint {
    pub id: String,
    pub value: MockValue,
}

#[derive(Debug, Clone)]
pub enum MockValue {
    Bool(bool),
    U16(u16),
    U32(u32),
    F32(f32),
    F64(f64),
    String(String),
}

impl MockDevice {
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            points: std::collections::HashMap::new(),
        }
    }

    pub fn with_point(mut self, id: impl Into<String>, value: MockValue) -> Self {
        let id = id.into();
        self.points.insert(id.clone(), MockPoint { id, value });
        self
    }
}

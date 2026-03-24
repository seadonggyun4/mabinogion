//! Scenario schema definitions.

use mabi_core::tags::Tags;
use mabi_runtime::RuntimeSessionSpec;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Scenario definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Scenario {
    /// Scenario name.
    pub name: String,

    /// Description.
    #[serde(default)]
    pub description: String,

    /// Duration in seconds (0 = infinite).
    #[serde(default)]
    pub duration_secs: u64,

    /// Time scale factor (1.0 = real-time).
    #[serde(default = "default_time_scale")]
    pub time_scale: f64,

    /// Device definitions for the scenario.
    #[serde(default)]
    pub devices: Vec<ScenarioDevice>,

    /// Data points.
    #[serde(default)]
    pub points: Vec<ScenarioPoint>,

    /// Events.
    #[serde(default)]
    pub events: Vec<ScenarioEvent>,

    /// Variables.
    #[serde(default)]
    pub variables: HashMap<String, f64>,

    /// Optional same-process runtime session used for execution.
    #[serde(default)]
    pub runtime: Option<RuntimeSessionSpec>,
}

fn default_time_scale() -> f64 {
    1.0
}

impl Default for Scenario {
    fn default() -> Self {
        Self {
            name: "Unnamed Scenario".to_string(),
            description: String::new(),
            duration_secs: 0,
            time_scale: 1.0,
            devices: Vec::new(),
            points: Vec::new(),
            events: Vec::new(),
            variables: HashMap::new(),
            runtime: None,
        }
    }
}

/// Device definition in a scenario.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScenarioDevice {
    /// Device ID.
    pub id: String,

    /// Device name.
    #[serde(default)]
    pub name: String,

    /// Protocol type.
    pub protocol: String,

    /// Protocol-specific configuration.
    #[serde(default)]
    pub config: HashMap<String, serde_yaml::Value>,

    /// Device tags for organization and filtering.
    #[serde(default, skip_serializing_if = "Tags::is_empty")]
    pub tags: Tags,
}

impl ScenarioDevice {
    /// Create a new scenario device.
    pub fn new(id: impl Into<String>, protocol: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            name: String::new(),
            protocol: protocol.into(),
            config: HashMap::new(),
            tags: Tags::new(),
        }
    }

    /// Set the device name.
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = name.into();
        self
    }

    /// Set device tags.
    pub fn with_tags(mut self, tags: Tags) -> Self {
        self.tags = tags;
        self
    }

    /// Add a single tag.
    pub fn with_tag(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.tags.insert(key.into(), value.into());
        self
    }

    /// Add a label.
    pub fn with_label(mut self, label: impl Into<String>) -> Self {
        self.tags.add_label(label.into());
        self
    }
}

/// Scenario data point.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScenarioPoint {
    /// Point ID.
    pub id: String,

    /// Device ID (optional if using device_tags).
    #[serde(default)]
    pub device_id: String,

    /// Point ID within device.
    pub point_id: String,

    /// Pattern configuration.
    pub pattern: PatternConfig,

    /// Update interval in milliseconds.
    #[serde(default = "default_interval")]
    pub interval_ms: u64,

    /// Filter by device tags (applies pattern to all matching devices).
    /// If specified, device_id can be empty and the pattern applies to
    /// all devices that match these tags.
    #[serde(default, skip_serializing_if = "Tags::is_empty")]
    pub device_tags: Tags,
}

fn default_interval() -> u64 {
    1000
}

/// Pattern configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum PatternConfig {
    /// Constant value.
    Constant { value: f64 },

    /// Sine wave.
    Sine {
        amplitude: f64,
        offset: f64,
        period_secs: f64,
        #[serde(default)]
        phase: f64,
    },

    /// Cosine wave.
    Cosine {
        amplitude: f64,
        offset: f64,
        period_secs: f64,
        #[serde(default)]
        phase: f64,
    },

    /// Linear ramp.
    Ramp {
        start: f64,
        end: f64,
        duration_secs: f64,
        #[serde(default)]
        repeat: bool,
    },

    /// Step function.
    Step {
        levels: Vec<f64>,
        step_duration_secs: f64,
    },

    /// Random values.
    Random {
        min: f64,
        max: f64,
        #[serde(default = "default_distribution")]
        distribution: String,
    },

    /// Gaussian noise.
    Noise { mean: f64, std_dev: f64 },

    /// Follow another point.
    Follow {
        source: String,
        #[serde(default)]
        offset: f64,
        #[serde(default = "default_gain")]
        gain: f64,
        #[serde(default)]
        delay_ms: u64,
    },

    /// Replay from CSV/JSON.
    Replay {
        file: String,
        #[serde(default)]
        loop_replay: bool,
    },
}

fn default_distribution() -> String {
    "uniform".to_string()
}

fn default_gain() -> f64 {
    1.0
}

/// Scenario event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScenarioEvent {
    /// Event name.
    pub name: String,

    /// Trigger condition.
    pub trigger: EventTrigger,

    /// Actions to perform.
    pub actions: Vec<EventAction>,
}

/// Event trigger.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum EventTrigger {
    /// Time-based trigger.
    Time { at_secs: f64 },

    /// Periodic trigger.
    Periodic {
        interval_secs: f64,
        #[serde(default)]
        start_secs: f64,
    },

    /// Condition-based trigger.
    Condition {
        point: String,
        operator: String,
        value: f64,
    },
}

/// Event action.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum EventAction {
    /// Set a point value.
    SetValue { point: String, value: f64 },

    /// Change pattern.
    ChangePattern {
        point: String,
        pattern: PatternConfig,
    },

    /// Log a message.
    Log {
        message: String,
        #[serde(default = "default_log_level")]
        level: String,
    },

    /// Pause scenario.
    Pause,

    /// Stop scenario.
    Stop,
}

fn default_log_level() -> String {
    "info".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scenario_deserialization() {
        let yaml = r#"
name: Test Scenario
description: A test scenario
duration_secs: 3600
time_scale: 1.0
points:
  - id: temp
    device_id: device-001
    point_id: temperature
    pattern:
      type: sine
      amplitude: 5.0
      offset: 22.0
      period_secs: 3600
    interval_ms: 1000
"#;

        let scenario: Scenario = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(scenario.name, "Test Scenario");
        assert_eq!(scenario.points.len(), 1);
    }

    #[test]
    fn test_scenario_with_devices_and_tags() {
        let yaml = r#"
name: Tagged Devices Scenario
description: A scenario with tagged devices
duration_secs: 3600

devices:
  - id: hvac-001
    name: HVAC Unit 1
    protocol: modbus
    config:
      unit_id: 1
      port: 5020
    tags:
      tags:
        location: building-a
        floor: "3"
      labels:
        - hvac
        - critical

  - id: hvac-002
    name: HVAC Unit 2
    protocol: modbus
    config:
      unit_id: 2
    tags:
      tags:
        location: building-a
        floor: "4"
      labels:
        - hvac

points:
  - id: temp_all_hvac
    point_id: temperature
    pattern:
      type: sine
      amplitude: 2.0
      offset: 22.0
      period_secs: 3600
    device_tags:
      labels:
        - hvac
"#;

        let scenario: Scenario = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(scenario.name, "Tagged Devices Scenario");
        assert_eq!(scenario.devices.len(), 2);

        let device1 = &scenario.devices[0];
        assert_eq!(device1.id, "hvac-001");
        assert_eq!(device1.tags.get("location"), Some("building-a"));
        assert!(device1.tags.has_label("hvac"));
        assert!(device1.tags.has_label("critical"));

        let device2 = &scenario.devices[1];
        assert_eq!(device2.tags.get("floor"), Some("4"));
        assert!(device2.tags.has_label("hvac"));
        assert!(!device2.tags.has_label("critical"));

        // Point with device_tags filter
        let point = &scenario.points[0];
        assert!(point.device_tags.has_label("hvac"));
    }

    #[test]
    fn test_scenario_device_builder() {
        let device = ScenarioDevice::new("sensor-001", "modbus")
            .with_name("Temperature Sensor")
            .with_tag("zone", "hvac")
            .with_tag("floor", "1")
            .with_label("monitored")
            .with_label("critical");

        assert_eq!(device.id, "sensor-001");
        assert_eq!(device.protocol, "modbus");
        assert_eq!(device.tags.get("zone"), Some("hvac"));
        assert!(device.tags.has_label("monitored"));
        assert!(device.tags.has_label("critical"));
    }
}

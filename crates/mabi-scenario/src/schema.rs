//! Scenario schema definitions.

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

    /// Data points.
    #[serde(default)]
    pub points: Vec<ScenarioPoint>,

    /// Events.
    #[serde(default)]
    pub events: Vec<ScenarioEvent>,

    /// Variables.
    #[serde(default)]
    pub variables: HashMap<String, f64>,
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
            points: Vec::new(),
            events: Vec::new(),
            variables: HashMap::new(),
        }
    }
}

/// Scenario data point.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScenarioPoint {
    /// Point ID.
    pub id: String,

    /// Device ID.
    pub device_id: String,

    /// Point ID within device.
    pub point_id: String,

    /// Pattern configuration.
    pub pattern: PatternConfig,

    /// Update interval in milliseconds.
    #[serde(default = "default_interval")]
    pub interval_ms: u64,
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
}

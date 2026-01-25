//! Configuration types for the chaos engineering module.

use std::collections::HashMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::{ChaosError, ChaosResult};
use crate::fault::FaultSeverity;
use crate::network::{LatencyConfig, PacketLossConfig, ConnectionConfig, BandwidthConfig};
use crate::device::{OfflineConfig, SlowResponseConfig, CorruptionConfig, TransitionConfig};
use crate::protocol::{MalformedConfig, ChecksumConfig, TimeoutConfig, ReorderConfig};
use crate::scheduler::{ChaosSchedule, ChaosEntry, ChaosType};

// =============================================================================
// Global Configuration
// =============================================================================

/// Global chaos engineering configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChaosConfig {
    /// Global settings.
    #[serde(default)]
    pub global: GlobalConfig,

    /// Fault definitions.
    #[serde(default)]
    pub faults: HashMap<String, FaultConfig>,

    /// Schedules.
    #[serde(default)]
    pub schedules: Vec<ScheduleConfig>,
}

impl Default for ChaosConfig {
    fn default() -> Self {
        Self {
            global: GlobalConfig::default(),
            faults: HashMap::new(),
            schedules: Vec::new(),
        }
    }
}

impl ChaosConfig {
    /// Create a new empty configuration.
    pub fn new() -> Self {
        Self::default()
    }

    /// Load configuration from a YAML file.
    pub fn from_yaml_file(path: impl AsRef<Path>) -> ChaosResult<Self> {
        let content = std::fs::read_to_string(path.as_ref())
            .map_err(|e| ChaosError::Config(format!("Failed to read file: {}", e)))?;
        Self::from_yaml(&content)
    }

    /// Parse configuration from YAML string.
    pub fn from_yaml(yaml: &str) -> ChaosResult<Self> {
        serde_yaml::from_str(yaml)
            .map_err(|e| ChaosError::Config(format!("Failed to parse YAML: {}", e)))
    }

    /// Load configuration from a JSON file.
    pub fn from_json_file(path: impl AsRef<Path>) -> ChaosResult<Self> {
        let content = std::fs::read_to_string(path.as_ref())
            .map_err(|e| ChaosError::Config(format!("Failed to read file: {}", e)))?;
        Self::from_json(&content)
    }

    /// Parse configuration from JSON string.
    pub fn from_json(json: &str) -> ChaosResult<Self> {
        serde_json::from_str(json)
            .map_err(|e| ChaosError::Config(format!("Failed to parse JSON: {}", e)))
    }

    /// Serialize to YAML.
    pub fn to_yaml(&self) -> ChaosResult<String> {
        serde_yaml::to_string(self)
            .map_err(|e| ChaosError::Config(format!("Failed to serialize YAML: {}", e)))
    }

    /// Serialize to JSON.
    pub fn to_json(&self) -> ChaosResult<String> {
        serde_json::to_string_pretty(self)
            .map_err(|e| ChaosError::Config(format!("Failed to serialize JSON: {}", e)))
    }

    /// Add a fault configuration.
    pub fn add_fault(&mut self, id: impl Into<String>, fault: FaultConfig) {
        self.faults.insert(id.into(), fault);
    }

    /// Add a schedule configuration.
    pub fn add_schedule(&mut self, schedule: ScheduleConfig) {
        self.schedules.push(schedule);
    }

    /// Validate the configuration.
    pub fn validate(&self) -> ChaosResult<()> {
        // Validate global config
        self.global.validate()?;

        // Validate faults
        for (id, fault) in &self.faults {
            fault.validate().map_err(|e| {
                ChaosError::Config(format!("Invalid fault '{}': {}", id, e))
            })?;
        }

        // Validate schedules
        for (i, schedule) in self.schedules.iter().enumerate() {
            schedule.validate().map_err(|e| {
                ChaosError::Config(format!("Invalid schedule {}: {}", i, e))
            })?;
        }

        Ok(())
    }
}

// =============================================================================
// Global Config
// =============================================================================

/// Global settings for the chaos module.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GlobalConfig {
    /// Whether chaos engineering is enabled globally.
    #[serde(default = "default_true")]
    pub enabled: bool,

    /// Default probability for all faults (0.0-1.0).
    #[serde(default = "default_probability")]
    pub default_probability: f64,

    /// Maximum severity level allowed.
    #[serde(default)]
    pub max_severity: FaultSeverity,

    /// Whether to log all fault applications.
    #[serde(default)]
    pub verbose_logging: bool,

    /// Global target patterns (applied to all faults).
    #[serde(default)]
    pub target_patterns: Vec<String>,

    /// Exclude patterns (never apply faults to these).
    #[serde(default)]
    pub exclude_patterns: Vec<String>,

    /// Dry run mode (log but don't apply faults).
    #[serde(default)]
    pub dry_run: bool,

    /// Seed for random number generators (for reproducibility).
    #[serde(default)]
    pub seed: Option<u64>,
}

fn default_true() -> bool {
    true
}

fn default_probability() -> f64 {
    1.0
}

impl Default for GlobalConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            default_probability: 1.0,
            max_severity: FaultSeverity::Critical,
            verbose_logging: false,
            target_patterns: Vec::new(),
            exclude_patterns: Vec::new(),
            dry_run: false,
            seed: None,
        }
    }
}

impl GlobalConfig {
    /// Validate the global configuration.
    pub fn validate(&self) -> ChaosResult<()> {
        if self.default_probability < 0.0 || self.default_probability > 1.0 {
            return Err(ChaosError::Config(
                "default_probability must be between 0.0 and 1.0".to_string(),
            ));
        }
        Ok(())
    }
}

// =============================================================================
// Fault Configuration
// =============================================================================

/// Configuration for a single fault.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FaultConfig {
    /// Fault type and type-specific configuration.
    #[serde(flatten)]
    pub fault_type: FaultTypeConfig,

    /// Whether the fault is enabled.
    #[serde(default = "default_true")]
    pub enabled: bool,

    /// Activation probability (0.0-1.0).
    #[serde(default = "default_probability")]
    pub probability: f64,

    /// Fault severity.
    #[serde(default)]
    pub severity: FaultSeverity,

    /// Target patterns (glob patterns for device IDs).
    #[serde(default)]
    pub targets: Vec<String>,

    /// Human-readable name.
    #[serde(default)]
    pub name: Option<String>,

    /// Description.
    #[serde(default)]
    pub description: Option<String>,

    /// Tags for grouping.
    #[serde(default)]
    pub tags: Vec<String>,
}

impl FaultConfig {
    /// Validate the fault configuration.
    pub fn validate(&self) -> ChaosResult<()> {
        if self.probability < 0.0 || self.probability > 1.0 {
            return Err(ChaosError::Config(
                "probability must be between 0.0 and 1.0".to_string(),
            ));
        }
        self.fault_type.validate()
    }
}

/// Fault type-specific configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum FaultTypeConfig {
    // Network faults
    Latency(LatencyConfig),
    PacketLoss(PacketLossConfig),
    Connection(ConnectionConfig),
    Bandwidth(BandwidthConfig),

    // Device faults
    Offline(OfflineConfig),
    SlowResponse(SlowResponseConfig),
    CorruptedData(CorruptionConfig),
    StateTransition(TransitionConfig),

    // Protocol faults
    Malformed(MalformedConfig),
    Checksum(ChecksumConfig),
    Timeout(TimeoutConfig),
    Reorder(ReorderConfig),
}

impl FaultTypeConfig {
    /// Validate the fault type configuration.
    pub fn validate(&self) -> ChaosResult<()> {
        match self {
            Self::Latency(c) => {
                if c.base_ms == 0 && c.jitter_ms == 0 {
                    return Err(ChaosError::Config(
                        "Latency fault must have base_ms or jitter_ms > 0".to_string(),
                    ));
                }
            }
            Self::PacketLoss(c) => {
                if c.loss_rate < 0.0 || c.loss_rate > 1.0 {
                    return Err(ChaosError::Config(
                        "loss_rate must be between 0.0 and 1.0".to_string(),
                    ));
                }
            }
            Self::Bandwidth(c) => {
                if c.bytes_per_second == 0 {
                    return Err(ChaosError::Config(
                        "bytes_per_second must be > 0".to_string(),
                    ));
                }
            }
            Self::SlowResponse(c) => {
                if c.base_delay_ms == 0 && c.max_additional_delay_ms == 0 {
                    return Err(ChaosError::Config(
                        "Slow response must have delay > 0".to_string(),
                    ));
                }
            }
            Self::CorruptedData(c) => {
                if c.value_corruption_rate < 0.0 || c.value_corruption_rate > 1.0 {
                    return Err(ChaosError::Config(
                        "value_corruption_rate must be between 0.0 and 1.0".to_string(),
                    ));
                }
            }
            Self::Timeout(c) => {
                if c.timeout_ms == 0 {
                    return Err(ChaosError::Config(
                        "timeout_ms must be > 0".to_string(),
                    ));
                }
            }
            Self::Reorder(c) => {
                if c.buffer_size == 0 {
                    return Err(ChaosError::Config(
                        "buffer_size must be > 0".to_string(),
                    ));
                }
            }
            // Other types don't have specific validation
            _ => {}
        }
        Ok(())
    }

    /// Get the category of this fault type.
    pub fn category(&self) -> &'static str {
        match self {
            Self::Latency(_) | Self::PacketLoss(_) | Self::Connection(_) | Self::Bandwidth(_) => {
                "network"
            }
            Self::Offline(_)
            | Self::SlowResponse(_)
            | Self::CorruptedData(_)
            | Self::StateTransition(_) => "device",
            Self::Malformed(_) | Self::Checksum(_) | Self::Timeout(_) | Self::Reorder(_) => {
                "protocol"
            }
        }
    }
}

// =============================================================================
// Schedule Configuration
// =============================================================================

/// Configuration for a chaos schedule.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduleConfig {
    /// Schedule name.
    pub name: String,

    /// Whether the schedule is enabled.
    #[serde(default = "default_true")]
    pub enabled: bool,

    /// Description.
    #[serde(default)]
    pub description: Option<String>,

    /// Whether to loop the schedule.
    #[serde(default)]
    pub loop_schedule: bool,

    /// Total duration for looping (seconds).
    #[serde(default)]
    pub total_duration_secs: Option<f64>,

    /// Schedule entries.
    pub entries: Vec<ScheduleEntryConfig>,
}

impl ScheduleConfig {
    /// Validate the schedule configuration.
    pub fn validate(&self) -> ChaosResult<()> {
        if self.name.is_empty() {
            return Err(ChaosError::Config("Schedule name cannot be empty".to_string()));
        }

        if self.entries.is_empty() {
            return Err(ChaosError::Config("Schedule must have at least one entry".to_string()));
        }

        for (i, entry) in self.entries.iter().enumerate() {
            entry.validate().map_err(|e| {
                ChaosError::Config(format!("Invalid entry {}: {}", i, e))
            })?;
        }

        Ok(())
    }

    /// Convert to a ChaosSchedule.
    pub fn to_schedule(&self) -> ChaosResult<ChaosSchedule> {
        let mut schedule = ChaosSchedule::new(&self.name);

        if let Some(desc) = &self.description {
            schedule.description = desc.clone();
        }

        schedule.loop_schedule = self.loop_schedule;
        schedule.total_duration_secs = self.total_duration_secs;

        for entry_config in &self.entries {
            let entry = entry_config.to_entry()?;
            schedule.add_entry(entry);
        }

        Ok(schedule)
    }
}

/// Configuration for a schedule entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduleEntryConfig {
    /// Fault reference (ID from faults map) or inline fault type.
    #[serde(flatten)]
    pub fault: ScheduleFaultConfig,

    /// Start time in seconds from schedule start.
    pub start_secs: f64,

    /// Duration in seconds.
    pub duration_secs: f64,

    /// Target patterns.
    #[serde(default)]
    pub targets: Vec<String>,

    /// Intensity (0.0-1.0).
    #[serde(default = "default_probability")]
    pub intensity: f64,
}

impl ScheduleEntryConfig {
    /// Validate the entry configuration.
    pub fn validate(&self) -> ChaosResult<()> {
        if self.start_secs < 0.0 {
            return Err(ChaosError::Config("start_secs cannot be negative".to_string()));
        }

        if self.duration_secs <= 0.0 {
            return Err(ChaosError::Config("duration_secs must be positive".to_string()));
        }

        if self.intensity < 0.0 || self.intensity > 1.0 {
            return Err(ChaosError::Config(
                "intensity must be between 0.0 and 1.0".to_string(),
            ));
        }

        Ok(())
    }

    /// Convert to a ChaosEntry.
    pub fn to_entry(&self) -> ChaosResult<ChaosEntry> {
        let chaos_type = self.fault.to_chaos_type()?;

        let mut entry = ChaosEntry::new(self.start_secs, self.duration_secs, chaos_type);
        entry.targets = self.targets.clone();
        entry.intensity = self.intensity;

        Ok(entry)
    }
}

/// Fault configuration for schedule entries.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ScheduleFaultConfig {
    /// Reference to a fault by ID.
    Reference { fault_id: String },

    /// Inline fault type configuration.
    Inline(FaultTypeConfig),
}

impl ScheduleFaultConfig {
    /// Convert to a ChaosType.
    pub fn to_chaos_type(&self) -> ChaosResult<ChaosType> {
        match self {
            Self::Reference { fault_id } => Ok(ChaosType::Custom {
                fault_id: fault_id.clone(),
            }),
            Self::Inline(config) => match config {
                FaultTypeConfig::Latency(c) => Ok(ChaosType::NetworkLatency {
                    base_ms: c.base_ms,
                    jitter_ms: c.jitter_ms,
                }),
                FaultTypeConfig::PacketLoss(c) => Ok(ChaosType::PacketLoss {
                    rate: c.loss_rate,
                }),
                FaultTypeConfig::Connection(_) => Ok(ChaosType::Disconnect),
                FaultTypeConfig::Offline(_) => Ok(ChaosType::DeviceOffline),
                FaultTypeConfig::SlowResponse(c) => Ok(ChaosType::SlowResponse {
                    delay_ms: c.base_delay_ms,
                }),
                FaultTypeConfig::CorruptedData(c) => Ok(ChaosType::CorruptedData {
                    corruption_rate: c.value_corruption_rate,
                }),
                FaultTypeConfig::Timeout(c) => Ok(ChaosType::Timeout {
                    timeout_ms: c.timeout_ms,
                }),
                FaultTypeConfig::Malformed(_) => Ok(ChaosType::MalformedPacket),
                FaultTypeConfig::Checksum(_) => Ok(ChaosType::InvalidChecksum),
                FaultTypeConfig::Bandwidth(_) | FaultTypeConfig::StateTransition(_) | FaultTypeConfig::Reorder(_) => {
                    Ok(ChaosType::Custom {
                        fault_id: config.category().to_string(),
                    })
                }
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_yaml_config() {
        let yaml = r#"
global:
  enabled: true
  default_probability: 0.5
  verbose_logging: true

faults:
  network-latency:
    type: latency
    base_ms: 100
    jitter_ms: 50
    probability: 0.8
    targets:
      - "device-*"

schedules:
  - name: test-schedule
    entries:
      - fault_id: network-latency
        start_secs: 0.0
        duration_secs: 60.0
        targets:
          - "modbus-*"
"#;

        let config = ChaosConfig::from_yaml(yaml).unwrap();
        assert!(config.global.enabled);
        assert_eq!(config.global.default_probability, 0.5);
        assert!(config.global.verbose_logging);
        assert!(config.faults.contains_key("network-latency"));
        assert_eq!(config.schedules.len(), 1);
    }

    #[test]
    fn test_validate_config() {
        let mut config = ChaosConfig::new();
        config.global.default_probability = 1.5; // Invalid

        assert!(config.validate().is_err());
    }

    #[test]
    fn test_fault_type_category() {
        let latency = FaultTypeConfig::Latency(LatencyConfig::default());
        assert_eq!(latency.category(), "network");

        let offline = FaultTypeConfig::Offline(OfflineConfig::default());
        assert_eq!(offline.category(), "device");

        let timeout = FaultTypeConfig::Timeout(TimeoutConfig::default());
        assert_eq!(timeout.category(), "protocol");
    }
}

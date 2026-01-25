//! Configuration types and hot reload support.
//!
//! This module provides comprehensive configuration management for the simulator including:
//! - Engine configuration
//! - Protocol-specific configurations (Modbus, OPC UA, BACnet, KNX)
//! - Device configurations
//! - Multi-format file loading (YAML, JSON, TOML)
//! - Environment variable overrides
//! - Configuration validation
//! - Hot reload with file watching
//!
//! # Quick Start
//!
//! ```rust,no_run
//! use mabi_core::config::{EngineConfig, ConfigLoader, ConfigFormat};
//!
//! // Load configuration from file (auto-detect format)
//! let config: EngineConfig = ConfigLoader::load("config.yaml").unwrap();
//!
//! // Or use builder pattern with defaults
//! let config = EngineConfig::default()
//!     .with_max_devices(50_000);
//!
//! // Validate configuration
//! config.validate().unwrap();
//! ```
//!
//! # Configuration Formats
//!
//! The configuration system supports multiple formats:
//!
//! - **YAML** (`.yaml`, `.yml`): Human-readable, good for complex configs
//! - **JSON** (`.json`): Wide compatibility, strict syntax
//! - **TOML** (`.toml`): Rust-native, good for nested structures
//!
//! # Environment Variable Overrides
//!
//! Configuration values can be overridden via environment variables:
//!
//! ```text
//! TRAP_SIM_ENGINE_MAX_DEVICES=50000
//! TRAP_SIM_ENGINE_TICK_INTERVAL_MS=50
//! TRAP_SIM_LOG_LEVEL=debug
//! ```
//!
//! # Hot Reload
//!
//! The configuration system supports hot reload through file watching:
//!
//! ```rust,ignore
//! use mabi_core::config::{ConfigWatcher, FileWatcherService, ConfigSource};
//!
//! let watcher = Arc::new(ConfigWatcher::new());
//! let service = FileWatcherService::new(watcher.clone());
//!
//! service.watch(ConfigSource::Main(PathBuf::from("config.yaml")))?;
//! service.start()?;
//!
//! // Subscribe to changes
//! let mut rx = watcher.subscribe();
//! while let Ok(event) = rx.recv().await {
//!     println!("Config changed: {:?}", event);
//! }
//! ```
//!
//! # Validation
//!
//! Configurations can be validated using the `Validatable` trait:
//!
//! ```rust,ignore
//! use mabi_core::config::{EngineConfig, Validatable};
//!
//! let config = EngineConfig::default();
//! if let Err(errors) = config.validate() {
//!     for (field, messages) in errors.iter() {
//!         println!("{}: {:?}", field, messages);
//!     }
//! }
//! ```

pub mod env;
pub mod file_watcher;
pub mod hot_reload;
pub mod loader;
pub mod validation;
pub mod watcher;

use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::protocol::Protocol;

// Re-export watcher types
pub use watcher::{
    CallbackHandler, ConfigEvent, ConfigEventHandler, ConfigSource, ConfigWatcher,
    SharedConfigWatcher, WatcherState, create_config_watcher,
};

// Re-export loader types
pub use loader::{ConfigFormat, ConfigLoader, ConfigDiscovery, LayeredConfigBuilder};

// Re-export environment variable types
pub use env::{
    EnvOverrides, EnvRule, EnvRuleBuilder, EnvApplyResult, EnvConfigurable,
    EnvSnapshot, EnvVarDoc, DEFAULT_PREFIX,
    get_env, get_env_or, get_env_bool, get_env_bool_or,
};

// Re-export validation types
pub use validation::{
    Validatable, Validator, ValidationContext, ValidationRule,
    RangeRule, StringLengthRule, PathExistsRule, SocketAddrRule,
    CrossFieldValidator,
};

// Re-export file watcher types
pub use file_watcher::{
    FileWatcherService, FileWatcherServiceBuilder, FileWatcherConfig,
    DEFAULT_DEBOUNCE_MS,
};

// Re-export hot reload types
pub use hot_reload::{
    HotReloadManager, HotReloadManagerBuilder, ReloadEvent, ReloadStrategy,
    ConfigChange,
};

/// Main engine configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EngineConfig {
    /// Engine name.
    #[serde(default = "default_engine_name")]
    pub name: String,

    /// Maximum number of devices.
    #[serde(default = "default_max_devices")]
    pub max_devices: usize,

    /// Maximum number of data points.
    #[serde(default = "default_max_points")]
    pub max_points: usize,

    /// Tick interval in milliseconds.
    #[serde(default = "default_tick_interval_ms")]
    pub tick_interval_ms: u64,

    /// Number of worker threads.
    #[serde(default = "default_workers")]
    pub workers: usize,

    /// Enable metrics collection.
    #[serde(default = "default_true")]
    pub enable_metrics: bool,

    /// Metrics export interval in seconds.
    #[serde(default = "default_metrics_interval")]
    pub metrics_interval_secs: u64,

    /// Log level.
    #[serde(default = "default_log_level")]
    pub log_level: String,

    /// Protocol configurations.
    #[serde(default)]
    pub protocols: HashMap<String, ProtocolConfig>,
}

fn default_engine_name() -> String {
    "trap-simulator".to_string()
}

fn default_max_devices() -> usize {
    10_000
}

fn default_max_points() -> usize {
    1_000_000
}

fn default_tick_interval_ms() -> u64 {
    100
}

fn default_workers() -> usize {
    num_cpus::get().max(4)
}

fn default_true() -> bool {
    true
}

fn default_metrics_interval() -> u64 {
    10
}

fn default_log_level() -> String {
    "info".to_string()
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            name: default_engine_name(),
            max_devices: default_max_devices(),
            max_points: default_max_points(),
            tick_interval_ms: default_tick_interval_ms(),
            workers: default_workers(),
            enable_metrics: true,
            metrics_interval_secs: default_metrics_interval(),
            log_level: default_log_level(),
            protocols: HashMap::new(),
        }
    }
}

impl EngineConfig {
    /// Create a new engine config with default values.
    pub fn new() -> Self {
        Self::default()
    }

    /// Load config from a YAML file.
    pub fn from_yaml_file(path: impl Into<PathBuf>) -> crate::Result<Self> {
        ConfigLoader::load_with_format(path.into(), ConfigFormat::Yaml)
    }

    /// Load config from a JSON file.
    pub fn from_json_file(path: impl Into<PathBuf>) -> crate::Result<Self> {
        ConfigLoader::load_with_format(path.into(), ConfigFormat::Json)
    }

    /// Load config from a TOML file.
    pub fn from_toml_file(path: impl Into<PathBuf>) -> crate::Result<Self> {
        ConfigLoader::load_with_format(path.into(), ConfigFormat::Toml)
    }

    /// Load config from any supported file (auto-detect format).
    pub fn from_file(path: impl Into<PathBuf>) -> crate::Result<Self> {
        ConfigLoader::load(path.into())
    }

    /// Get tick interval as Duration.
    pub fn tick_interval(&self) -> Duration {
        Duration::from_millis(self.tick_interval_ms)
    }

    /// Set maximum devices.
    pub fn with_max_devices(mut self, max: usize) -> Self {
        self.max_devices = max;
        self
    }

    /// Set maximum data points.
    pub fn with_max_points(mut self, max: usize) -> Self {
        self.max_points = max;
        self
    }

    /// Set tick interval.
    pub fn with_tick_interval(mut self, interval: Duration) -> Self {
        self.tick_interval_ms = interval.as_millis() as u64;
        self
    }

    /// Set number of worker threads.
    pub fn with_workers(mut self, workers: usize) -> Self {
        self.workers = workers;
        self
    }

    /// Set log level.
    pub fn with_log_level(mut self, level: impl Into<String>) -> Self {
        self.log_level = level.into();
        self
    }

    /// Enable or disable metrics.
    pub fn with_metrics(mut self, enable: bool) -> Self {
        self.enable_metrics = enable;
        self
    }

    /// Add protocol config.
    pub fn with_protocol(mut self, name: impl Into<String>, config: ProtocolConfig) -> Self {
        self.protocols.insert(name.into(), config);
        self
    }

    /// Apply environment variable overrides.
    ///
    /// Reads environment variables with the `TRAP_SIM_` prefix and applies
    /// matching values to the configuration.
    pub fn apply_env_overrides(&mut self) -> EnvApplyResult {
        Self::env_overrides().apply(self)
    }

    /// Create environment overrides configuration.
    pub fn env_overrides() -> EnvOverrides<Self> {
        EnvOverrides::with_prefix(DEFAULT_PREFIX)
            .add_rule(
                EnvRuleBuilder::new("ENGINE_NAME")
                    .field_path("name")
                    .description("Engine instance name")
                    .as_string(|c: &mut Self, v| c.name = v),
            )
            .add_rule(
                EnvRuleBuilder::new("ENGINE_MAX_DEVICES")
                    .field_path("max_devices")
                    .description("Maximum number of devices")
                    .parse_into(|c: &mut Self, v: usize| c.max_devices = v),
            )
            .add_rule(
                EnvRuleBuilder::new("ENGINE_MAX_POINTS")
                    .field_path("max_points")
                    .description("Maximum number of data points")
                    .parse_into(|c: &mut Self, v: usize| c.max_points = v),
            )
            .add_rule(
                EnvRuleBuilder::new("ENGINE_TICK_INTERVAL_MS")
                    .field_path("tick_interval_ms")
                    .description("Tick interval in milliseconds")
                    .parse_into(|c: &mut Self, v: u64| c.tick_interval_ms = v),
            )
            .add_rule(
                EnvRuleBuilder::new("ENGINE_WORKERS")
                    .field_path("workers")
                    .description("Number of worker threads")
                    .parse_into(|c: &mut Self, v: usize| c.workers = v),
            )
            .add_rule(
                EnvRuleBuilder::new("ENGINE_METRICS")
                    .field_path("enable_metrics")
                    .description("Enable metrics collection")
                    .as_bool(|c: &mut Self, v| c.enable_metrics = v),
            )
            .add_rule(
                EnvRuleBuilder::new("ENGINE_METRICS_INTERVAL")
                    .field_path("metrics_interval_secs")
                    .description("Metrics export interval in seconds")
                    .parse_into(|c: &mut Self, v: u64| c.metrics_interval_secs = v),
            )
            .add_rule(
                EnvRuleBuilder::new("LOG_LEVEL")
                    .field_path("log_level")
                    .description("Log level (trace, debug, info, warn, error)")
                    .as_string(|c: &mut Self, v| c.log_level = v),
            )
    }
}

impl EnvConfigurable for EngineConfig {
    fn env_overrides() -> EnvOverrides<Self> {
        Self::env_overrides()
    }
}

impl Validatable for EngineConfig {
    fn validate(&self) -> crate::Result<()> {
        let mut errors = crate::error::ValidationErrors::new();
        self.validate_collect(&mut errors);
        errors.into_result(())
    }

    fn validate_collect(&self, errors: &mut crate::error::ValidationErrors) {
        // Validate name
        if self.name.trim().is_empty() {
            errors.add("name", "Engine name cannot be empty");
        }

        // Validate max_devices
        if self.max_devices == 0 {
            errors.add("max_devices", "Max devices must be greater than 0");
        }
        if self.max_devices > 1_000_000 {
            errors.add("max_devices", "Max devices cannot exceed 1,000,000");
        }

        // Validate max_points
        if self.max_points == 0 {
            errors.add("max_points", "Max points must be greater than 0");
        }
        if self.max_points > 100_000_000 {
            errors.add("max_points", "Max points cannot exceed 100,000,000");
        }

        // Validate tick_interval_ms
        if self.tick_interval_ms < 1 {
            errors.add("tick_interval_ms", "Tick interval must be at least 1ms");
        }
        if self.tick_interval_ms > 60_000 {
            errors.add("tick_interval_ms", "Tick interval cannot exceed 60 seconds");
        }

        // Validate workers
        if self.workers == 0 {
            errors.add("workers", "Workers must be greater than 0");
        }
        if self.workers > 1024 {
            errors.add("workers", "Workers cannot exceed 1024");
        }

        // Validate metrics_interval_secs
        if self.enable_metrics && self.metrics_interval_secs == 0 {
            errors.add("metrics_interval_secs", "Metrics interval must be greater than 0 when metrics are enabled");
        }

        // Validate log_level
        let valid_levels = ["trace", "debug", "info", "warn", "error"];
        if !valid_levels.contains(&self.log_level.to_lowercase().as_str()) {
            errors.add(
                "log_level",
                format!(
                    "Invalid log level '{}', must be one of: {:?}",
                    self.log_level, valid_levels
                ),
            );
        }

        // Cross-field validation: max_points should be sensible relative to max_devices
        let points_per_device = self.max_points / self.max_devices.max(1);
        if points_per_device > 10_000 {
            errors.add(
                "max_points, max_devices",
                format!(
                    "Average points per device ({}) seems too high",
                    points_per_device
                ),
            );
        }
    }
}

/// Protocol-specific configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum ProtocolConfig {
    /// Modbus TCP configuration.
    ModbusTcp(ModbusTcpConfig),
    /// Modbus RTU configuration.
    ModbusRtu(ModbusRtuConfig),
    /// OPC UA configuration.
    OpcUa(OpcUaConfig),
    /// BACnet/IP configuration.
    BacnetIp(BacnetIpConfig),
    /// KNXnet/IP configuration.
    KnxIp(KnxIpConfig),
}

/// Modbus TCP configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModbusTcpConfig {
    /// Bind address.
    #[serde(default = "default_modbus_bind")]
    pub bind_address: SocketAddr,

    /// Maximum connections.
    #[serde(default = "default_max_connections")]
    pub max_connections: usize,

    /// Connection timeout in seconds.
    #[serde(default = "default_timeout")]
    pub timeout_secs: u64,

    /// Enable keep-alive.
    #[serde(default = "default_true")]
    pub keep_alive: bool,
}

fn default_modbus_bind() -> SocketAddr {
    "0.0.0.0:502".parse().unwrap()
}

fn default_max_connections() -> usize {
    1000
}

fn default_timeout() -> u64 {
    30
}

impl Default for ModbusTcpConfig {
    fn default() -> Self {
        Self {
            bind_address: default_modbus_bind(),
            max_connections: default_max_connections(),
            timeout_secs: default_timeout(),
            keep_alive: true,
        }
    }
}

/// Modbus RTU configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModbusRtuConfig {
    /// Serial port path.
    pub serial_port: String,

    /// Baud rate.
    #[serde(default = "default_baud_rate")]
    pub baud_rate: u32,

    /// Data bits.
    #[serde(default = "default_data_bits")]
    pub data_bits: u8,

    /// Parity.
    #[serde(default = "default_parity")]
    pub parity: String,

    /// Stop bits.
    #[serde(default = "default_stop_bits")]
    pub stop_bits: u8,
}

fn default_baud_rate() -> u32 {
    9600
}

fn default_data_bits() -> u8 {
    8
}

fn default_parity() -> String {
    "none".to_string()
}

fn default_stop_bits() -> u8 {
    1
}

impl Default for ModbusRtuConfig {
    fn default() -> Self {
        Self {
            serial_port: "/dev/ttyUSB0".to_string(),
            baud_rate: default_baud_rate(),
            data_bits: default_data_bits(),
            parity: default_parity(),
            stop_bits: default_stop_bits(),
        }
    }
}

/// OPC UA configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpcUaConfig {
    /// Endpoint URL.
    #[serde(default = "default_opcua_endpoint")]
    pub endpoint_url: String,

    /// Server name.
    #[serde(default = "default_opcua_server_name")]
    pub server_name: String,

    /// Security policy.
    #[serde(default = "default_security_policy")]
    pub security_policy: String,

    /// Certificate path.
    pub certificate_path: Option<PathBuf>,

    /// Private key path.
    pub private_key_path: Option<PathBuf>,

    /// Max subscriptions.
    #[serde(default = "default_max_subscriptions")]
    pub max_subscriptions: usize,
}

fn default_opcua_endpoint() -> String {
    "opc.tcp://0.0.0.0:4840".to_string()
}

fn default_opcua_server_name() -> String {
    "TRAP Simulator OPC UA Server".to_string()
}

fn default_security_policy() -> String {
    "None".to_string()
}

fn default_max_subscriptions() -> usize {
    100
}

impl Default for OpcUaConfig {
    fn default() -> Self {
        Self {
            endpoint_url: default_opcua_endpoint(),
            server_name: default_opcua_server_name(),
            security_policy: default_security_policy(),
            certificate_path: None,
            private_key_path: None,
            max_subscriptions: default_max_subscriptions(),
        }
    }
}

/// BACnet/IP configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BacnetIpConfig {
    /// Bind address.
    #[serde(default = "default_bacnet_bind")]
    pub bind_address: SocketAddr,

    /// BACnet device instance.
    #[serde(default = "default_device_instance")]
    pub device_instance: u32,

    /// BACnet device name.
    #[serde(default = "default_bacnet_device_name")]
    pub device_name: String,

    /// Enable BBMD (BACnet Broadcast Management Device).
    #[serde(default)]
    pub enable_bbmd: bool,

    /// BBMD table.
    #[serde(default)]
    pub bbmd_table: Vec<String>,
}

fn default_bacnet_bind() -> SocketAddr {
    "0.0.0.0:47808".parse().unwrap()
}

fn default_device_instance() -> u32 {
    1234
}

fn default_bacnet_device_name() -> String {
    "TRAP Simulator BACnet Device".to_string()
}

impl Default for BacnetIpConfig {
    fn default() -> Self {
        Self {
            bind_address: default_bacnet_bind(),
            device_instance: default_device_instance(),
            device_name: default_bacnet_device_name(),
            enable_bbmd: false,
            bbmd_table: Vec::new(),
        }
    }
}

/// KNXnet/IP configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnxIpConfig {
    /// Bind address.
    #[serde(default = "default_knx_bind")]
    pub bind_address: SocketAddr,

    /// Individual address.
    #[serde(default = "default_individual_address")]
    pub individual_address: String,

    /// Enable tunneling.
    #[serde(default = "default_true")]
    pub enable_tunneling: bool,

    /// Enable routing.
    #[serde(default)]
    pub enable_routing: bool,

    /// Multicast address for routing.
    #[serde(default = "default_multicast_address")]
    pub multicast_address: String,
}

fn default_knx_bind() -> SocketAddr {
    "0.0.0.0:3671".parse().unwrap()
}

fn default_individual_address() -> String {
    "1.1.1".to_string()
}

fn default_multicast_address() -> String {
    "224.0.23.12".to_string()
}

impl Default for KnxIpConfig {
    fn default() -> Self {
        Self {
            bind_address: default_knx_bind(),
            individual_address: default_individual_address(),
            enable_tunneling: true,
            enable_routing: false,
            multicast_address: default_multicast_address(),
        }
    }
}

/// Device configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceConfig {
    /// Device ID.
    pub id: String,

    /// Device name.
    pub name: String,

    /// Device description.
    #[serde(default)]
    pub description: String,

    /// Protocol.
    pub protocol: Protocol,

    /// Protocol-specific address (e.g., unit ID for Modbus).
    #[serde(default)]
    pub address: Option<String>,

    /// Data points.
    #[serde(default)]
    pub points: Vec<DataPointConfig>,

    /// Custom metadata.
    #[serde(default)]
    pub metadata: HashMap<String, String>,
}

/// Data point configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataPointConfig {
    /// Point ID.
    pub id: String,

    /// Point name.
    pub name: String,

    /// Data type.
    pub data_type: String,

    /// Access mode.
    #[serde(default = "default_access")]
    pub access: String,

    /// Protocol-specific address.
    #[serde(default)]
    pub address: Option<String>,

    /// Initial value.
    #[serde(default)]
    pub initial_value: Option<serde_json::Value>,

    /// Engineering units.
    #[serde(default)]
    pub units: Option<String>,

    /// Minimum value.
    #[serde(default)]
    pub min: Option<f64>,

    /// Maximum value.
    #[serde(default)]
    pub max: Option<f64>,
}

fn default_access() -> String {
    "rw".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    #[test]
    fn test_engine_config_default() {
        let config = EngineConfig::default();
        assert_eq!(config.max_devices, 10_000);
        assert_eq!(config.tick_interval_ms, 100);
        assert_eq!(config.name, "trap-simulator");
    }

    #[test]
    fn test_engine_config_builder() {
        let config = EngineConfig::new()
            .with_max_devices(50_000)
            .with_max_points(5_000_000)
            .with_tick_interval(Duration::from_millis(50))
            .with_workers(8)
            .with_log_level("debug")
            .with_metrics(false);

        assert_eq!(config.max_devices, 50_000);
        assert_eq!(config.max_points, 5_000_000);
        assert_eq!(config.tick_interval_ms, 50);
        assert_eq!(config.workers, 8);
        assert_eq!(config.log_level, "debug");
        assert!(!config.enable_metrics);
    }

    #[test]
    fn test_modbus_tcp_config_default() {
        let config = ModbusTcpConfig::default();
        assert_eq!(config.bind_address.port(), 502);
        assert_eq!(config.max_connections, 1000);
    }

    #[test]
    fn test_config_serialization_yaml() {
        let config = EngineConfig::default();
        let yaml = ConfigLoader::serialize(&config, ConfigFormat::Yaml).unwrap();
        let parsed: EngineConfig = ConfigLoader::parse(&yaml, ConfigFormat::Yaml).unwrap();
        assert_eq!(config.max_devices, parsed.max_devices);
        assert_eq!(config.name, parsed.name);
    }

    #[test]
    fn test_config_serialization_json() {
        let config = EngineConfig::default();
        let json = ConfigLoader::serialize(&config, ConfigFormat::Json).unwrap();
        let parsed: EngineConfig = ConfigLoader::parse(&json, ConfigFormat::Json).unwrap();
        assert_eq!(config.max_devices, parsed.max_devices);
    }

    #[test]
    fn test_config_serialization_toml() {
        let config = EngineConfig::default();
        let toml = ConfigLoader::serialize(&config, ConfigFormat::Toml).unwrap();
        let parsed: EngineConfig = ConfigLoader::parse(&toml, ConfigFormat::Toml).unwrap();
        assert_eq!(config.max_devices, parsed.max_devices);
    }

    #[test]
    fn test_config_validation_valid() {
        let config = EngineConfig::default();
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_config_validation_invalid_max_devices() {
        let config = EngineConfig::default().with_max_devices(0);
        let result = config.validate();
        assert!(result.is_err());
    }

    #[test]
    fn test_config_validation_invalid_log_level() {
        let mut config = EngineConfig::default();
        config.log_level = "invalid".to_string();
        let result = config.validate();
        assert!(result.is_err());
    }

    #[test]
    fn test_config_validation_cross_field() {
        // Very high points per device ratio
        let config = EngineConfig::default()
            .with_max_devices(10)
            .with_max_points(1_000_000);
        let result = config.validate();
        assert!(result.is_err());
    }

    #[test]
    fn test_env_overrides() {
        // Set environment variables
        env::set_var("TRAP_SIM_ENGINE_MAX_DEVICES", "25000");
        env::set_var("TRAP_SIM_ENGINE_WORKERS", "16");
        env::set_var("TRAP_SIM_LOG_LEVEL", "debug");

        let mut config = EngineConfig::default();
        let result = config.apply_env_overrides();

        assert!(result.has_changes());
        assert_eq!(config.max_devices, 25000);
        assert_eq!(config.workers, 16);
        assert_eq!(config.log_level, "debug");

        // Cleanup
        env::remove_var("TRAP_SIM_ENGINE_MAX_DEVICES");
        env::remove_var("TRAP_SIM_ENGINE_WORKERS");
        env::remove_var("TRAP_SIM_LOG_LEVEL");
    }

    #[test]
    fn test_env_overrides_documentation() {
        let overrides = EngineConfig::env_overrides();
        let docs = overrides.documentation();

        assert!(docs.len() > 0);
        assert!(docs.iter().any(|d| d.var_name == "TRAP_SIM_ENGINE_MAX_DEVICES"));
        assert!(docs.iter().any(|d| d.var_name == "TRAP_SIM_LOG_LEVEL"));
    }

    #[test]
    fn test_protocol_config_modbus_tcp() {
        let config = ProtocolConfig::ModbusTcp(ModbusTcpConfig::default());
        let yaml = serde_yaml::to_string(&config).unwrap();
        assert!(yaml.contains("type: modbustcp"));
    }

    #[test]
    fn test_protocol_config_opcua() {
        let config = ProtocolConfig::OpcUa(OpcUaConfig::default());
        let yaml = serde_yaml::to_string(&config).unwrap();
        assert!(yaml.contains("type: opcua"));
    }

    #[test]
    fn test_protocol_config_bacnet() {
        let config = ProtocolConfig::BacnetIp(BacnetIpConfig::default());
        assert_eq!(BacnetIpConfig::default().device_instance, 1234);
    }

    #[test]
    fn test_protocol_config_knx() {
        let config = ProtocolConfig::KnxIp(KnxIpConfig::default());
        assert_eq!(KnxIpConfig::default().individual_address, "1.1.1");
    }

    #[test]
    fn test_engine_config_with_protocol() {
        let config = EngineConfig::default()
            .with_protocol("modbus", ProtocolConfig::ModbusTcp(ModbusTcpConfig::default()));

        assert!(config.protocols.contains_key("modbus"));
    }

    #[test]
    fn test_config_format_detection() {
        assert_eq!(ConfigFormat::from_path("config.yaml"), Some(ConfigFormat::Yaml));
        assert_eq!(ConfigFormat::from_path("config.yml"), Some(ConfigFormat::Yaml));
        assert_eq!(ConfigFormat::from_path("config.json"), Some(ConfigFormat::Json));
        assert_eq!(ConfigFormat::from_path("config.toml"), Some(ConfigFormat::Toml));
        assert_eq!(ConfigFormat::from_path("config.txt"), None);
    }
}

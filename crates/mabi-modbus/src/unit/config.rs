//! Configuration types for multi-unit management.

use serde::{Deserialize, Serialize};

use crate::context::BroadcastPolicy;
use crate::registers::RegisterStoreConfig;
use crate::types::WordOrder;

/// Configuration for the MultiUnitManager.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnitManagerConfig {
    /// Maximum number of units that can be managed.
    ///
    /// Default: 247 (Modbus max addressable units: 1-247)
    pub max_units: usize,

    /// Default word order for new units.
    ///
    /// Individual units can override this.
    pub default_word_order: WordOrder,

    /// Broadcast handling mode for Unit ID 0.
    pub broadcast_mode: BroadcastPolicy,

    /// Whether to allow creating units on-demand.
    ///
    /// If true, accessing a non-existent unit will create it with default config.
    /// If false, accessing a non-existent unit returns None.
    pub auto_create_units: bool,

    /// Default register store configuration for new units.
    #[serde(default)]
    pub default_register_config: RegisterStoreConfig,

    /// Enable metrics collection per unit.
    pub enable_unit_metrics: bool,
}

impl Default for UnitManagerConfig {
    fn default() -> Self {
        Self {
            max_units: 247,
            default_word_order: WordOrder::BigEndian,
            broadcast_mode: BroadcastPolicy::WriteAll,
            auto_create_units: false,
            default_register_config: RegisterStoreConfig::default(),
            enable_unit_metrics: true,
        }
    }
}

impl UnitManagerConfig {
    /// Create a new config with specified max units.
    pub fn with_max_units(mut self, max_units: usize) -> Self {
        self.max_units = max_units;
        self
    }

    /// Set the default word order.
    pub fn with_word_order(mut self, word_order: WordOrder) -> Self {
        self.default_word_order = word_order;
        self
    }

    /// Set the broadcast mode.
    pub fn with_broadcast_mode(mut self, mode: BroadcastPolicy) -> Self {
        self.broadcast_mode = mode;
        self
    }

    /// Enable auto-creation of units.
    pub fn with_auto_create(mut self, auto_create: bool) -> Self {
        self.auto_create_units = auto_create;
        self
    }

    /// Set the default register configuration.
    pub fn with_register_config(mut self, config: RegisterStoreConfig) -> Self {
        self.default_register_config = config;
        self
    }

    /// Create a config for testing with minimal resources.
    pub fn for_testing() -> Self {
        Self {
            max_units: 10,
            default_word_order: WordOrder::BigEndian,
            broadcast_mode: BroadcastPolicy::WriteAll,
            auto_create_units: true,
            default_register_config: RegisterStoreConfig::minimal(),
            enable_unit_metrics: false,
        }
    }

    /// Create a config for large-scale simulation.
    pub fn for_large_scale() -> Self {
        Self {
            max_units: 247,
            default_word_order: WordOrder::BigEndian,
            broadcast_mode: BroadcastPolicy::WriteAll,
            auto_create_units: false,
            default_register_config: RegisterStoreConfig::large_scale(),
            enable_unit_metrics: true,
        }
    }
}

/// Backward-compatible alias for the canonical broadcast policy type.
pub type BroadcastMode = BroadcastPolicy;

/// Configuration for a single Modbus unit.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnitConfig {
    /// Human-readable name for this unit.
    pub name: String,

    /// Optional description.
    #[serde(default)]
    pub description: String,

    /// Word order for this unit (overrides manager default).
    pub word_order: Option<WordOrder>,

    /// Register store configuration for this unit.
    #[serde(default)]
    pub register_config: Option<RegisterStoreConfig>,

    /// Whether this unit responds to broadcasts.
    #[serde(default = "default_broadcast_enabled")]
    pub broadcast_enabled: bool,

    /// Response delay in microseconds (for simulation).
    #[serde(default)]
    pub response_delay_us: u64,

    /// Whether this unit is enabled.
    #[serde(default = "default_enabled")]
    pub enabled: bool,

    /// Custom metadata for this unit.
    #[serde(default)]
    pub metadata: std::collections::HashMap<String, String>,
}

fn default_broadcast_enabled() -> bool {
    true
}

fn default_enabled() -> bool {
    true
}

impl Default for UnitConfig {
    fn default() -> Self {
        Self {
            name: "Unnamed Unit".to_string(),
            description: String::new(),
            word_order: None,
            register_config: None,
            broadcast_enabled: true,
            response_delay_us: 0,
            enabled: true,
            metadata: std::collections::HashMap::new(),
        }
    }
}

impl UnitConfig {
    /// Create a new unit config with a name.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            ..Default::default()
        }
    }

    /// Create a unit config with a specific word order.
    pub fn with_word_order(name: impl Into<String>, word_order: WordOrder) -> Self {
        Self {
            name: name.into(),
            word_order: Some(word_order),
            ..Default::default()
        }
    }

    /// Set the description.
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = description.into();
        self
    }

    /// Set the register configuration.
    pub fn with_register_config(mut self, config: RegisterStoreConfig) -> Self {
        self.register_config = Some(config);
        self
    }

    /// Set the response delay.
    pub fn with_response_delay_us(mut self, delay: u64) -> Self {
        self.response_delay_us = delay;
        self
    }

    /// Set broadcast enabled state.
    pub fn with_broadcast(mut self, enabled: bool) -> Self {
        self.broadcast_enabled = enabled;
        self
    }

    /// Set enabled state.
    pub fn with_enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Add metadata.
    pub fn with_metadata(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.metadata.insert(key.into(), value.into());
        self
    }

    /// Get the effective word order (unit-specific or None for manager default).
    pub fn effective_word_order(&self, default: WordOrder) -> WordOrder {
        self.word_order.unwrap_or(default)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_unit_manager_config_defaults() {
        let config = UnitManagerConfig::default();
        assert_eq!(config.max_units, 247);
        assert_eq!(config.default_word_order, WordOrder::BigEndian);
        assert!(!config.auto_create_units);
    }

    #[test]
    fn test_unit_manager_config_builder() {
        let config = UnitManagerConfig::default()
            .with_max_units(100)
            .with_word_order(WordOrder::LittleEndian)
            .with_auto_create(true);

        assert_eq!(config.max_units, 100);
        assert_eq!(config.default_word_order, WordOrder::LittleEndian);
        assert!(config.auto_create_units);
    }

    #[test]
    fn test_broadcast_mode_display() {
        assert_eq!(BroadcastMode::WriteAll.to_string(), "Write to all units");
        assert_eq!(BroadcastMode::Disabled.to_string(), "Disabled");
        assert_eq!(BroadcastMode::EchoToUnit(5).to_string(), "Echo to unit 5");
    }

    #[test]
    fn test_unit_config_builder() {
        let config = UnitConfig::new("Pump #1")
            .with_description("Main circulation pump")
            .with_response_delay_us(1000)
            .with_metadata("location", "Building A");

        assert_eq!(config.name, "Pump #1");
        assert_eq!(config.description, "Main circulation pump");
        assert_eq!(config.response_delay_us, 1000);
        assert_eq!(
            config.metadata.get("location"),
            Some(&"Building A".to_string())
        );
    }

    #[test]
    fn test_unit_config_effective_word_order() {
        let config1 = UnitConfig::new("Test1");
        let config2 = UnitConfig::with_word_order("Test2", WordOrder::LittleEndian);

        assert_eq!(
            config1.effective_word_order(WordOrder::BigEndian),
            WordOrder::BigEndian
        );
        assert_eq!(
            config2.effective_word_order(WordOrder::BigEndian),
            WordOrder::LittleEndian
        );
    }

    #[test]
    fn test_serde_roundtrip() {
        let config = UnitManagerConfig::default()
            .with_max_units(50)
            .with_word_order(WordOrder::BigEndianWordSwap);

        let json = serde_json::to_string(&config).unwrap();
        let parsed: UnitManagerConfig = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.max_units, 50);
        assert_eq!(parsed.default_word_order, WordOrder::BigEndianWordSwap);
    }
}

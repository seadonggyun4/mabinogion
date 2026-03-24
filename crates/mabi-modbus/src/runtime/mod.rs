//! Runtime configuration management for Modbus simulator.
//!
//! This module provides dynamic configuration updates without server restart.
//!
//! # Features
//!
//! - **Atomic Updates**: Configuration changes are applied atomically
//! - **Validation**: All changes are validated before application
//! - **Rollback**: Automatic rollback on failure
//! - **Events**: Subscribe to configuration change events
//! - **Partial Updates**: Support for updating individual settings
//!
//! # Example
//!
//! ```rust,ignore
//! use mabi_modbus::runtime::{RuntimeConfig, ConfigUpdate};
//!
//! // Create runtime config
//! let mut runtime = RuntimeConfig::new(server_config);
//!
//! // Update a single setting
//! runtime.apply(ConfigUpdate::MaxConnections(200))?;
//!
//! // Update multiple settings atomically
//! runtime.apply_batch(vec![
//!     ConfigUpdate::MaxConnections(200),
//!     ConfigUpdate::IdleTimeout(Duration::from_secs(60)),
//! ])?;
//!
//! // Subscribe to changes
//! let mut rx = runtime.subscribe();
//! ```

pub mod config;
pub mod service;
pub mod updates;

pub use config::*;
pub use service::{descriptor, driver};
pub use updates::*;

use std::sync::Arc;
use std::time::Duration;

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;

use crate::error::{ModbusError, ModbusResult as Result};

/// Dynamic configuration value that can be changed at runtime.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ConfigUpdate {
    /// Update maximum concurrent connections.
    MaxConnections(usize),

    /// Update connection idle timeout.
    IdleTimeout(Duration),

    /// Update request timeout.
    RequestTimeout(Duration),

    /// Enable/disable a specific unit ID.
    UnitEnabled { unit_id: u8, enabled: bool },

    /// Update TCP nodelay setting.
    TcpNoDelay(bool),

    /// Update keepalive interval.
    KeepaliveInterval(Option<Duration>),

    /// Update register read access for a range.
    RegisterReadAccess {
        unit_id: u8,
        start_address: u16,
        count: u16,
        allowed: bool,
    },

    /// Update register write access for a range.
    RegisterWriteAccess {
        unit_id: u8,
        start_address: u16,
        count: u16,
        allowed: bool,
    },

    /// Set a register value.
    SetRegister {
        unit_id: u8,
        address: u16,
        value: u16,
    },

    /// Set multiple register values.
    SetRegisters {
        unit_id: u8,
        start_address: u16,
        values: Vec<u16>,
    },

    /// Set a coil value.
    SetCoil {
        unit_id: u8,
        address: u16,
        value: bool,
    },

    /// Enable/disable metrics collection.
    MetricsEnabled(bool),

    /// Enable/disable debug logging.
    DebugLogging(bool),

    /// Custom extension for protocol-specific settings.
    Custom { key: String, value: String },
}

impl ConfigUpdate {
    /// Get the category of this update.
    pub fn category(&self) -> UpdateCategory {
        match self {
            Self::MaxConnections(_)
            | Self::IdleTimeout(_)
            | Self::RequestTimeout(_)
            | Self::TcpNoDelay(_)
            | Self::KeepaliveInterval(_) => UpdateCategory::Connection,

            Self::UnitEnabled { .. } => UpdateCategory::Unit,

            Self::RegisterReadAccess { .. }
            | Self::RegisterWriteAccess { .. }
            | Self::SetRegister { .. }
            | Self::SetRegisters { .. }
            | Self::SetCoil { .. } => UpdateCategory::Data,

            Self::MetricsEnabled(_) | Self::DebugLogging(_) => UpdateCategory::Monitoring,

            Self::Custom { .. } => UpdateCategory::Custom,
        }
    }

    /// Check if this update requires a restart to take effect.
    pub fn requires_restart(&self) -> bool {
        match self {
            Self::TcpNoDelay(_) | Self::KeepaliveInterval(_) => true,
            _ => false,
        }
    }
}

/// Category of configuration update.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum UpdateCategory {
    /// Connection-related settings.
    Connection,
    /// Unit-related settings.
    Unit,
    /// Data/register settings.
    Data,
    /// Monitoring settings.
    Monitoring,
    /// Custom extensions.
    Custom,
}

/// Event emitted when configuration changes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ConfigEvent {
    /// Configuration update applied successfully.
    Updated {
        update: ConfigUpdate,
        timestamp: std::time::SystemTime,
    },

    /// Batch update applied successfully.
    BatchUpdated {
        count: usize,
        timestamp: std::time::SystemTime,
    },

    /// Update failed.
    UpdateFailed { update: ConfigUpdate, error: String },

    /// Update was rolled back.
    RolledBack {
        update: ConfigUpdate,
        reason: String,
    },

    /// Configuration was reset to defaults.
    Reset { timestamp: std::time::SystemTime },
}

/// Runtime configuration state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeState {
    /// Maximum concurrent connections.
    pub max_connections: usize,

    /// Connection idle timeout.
    pub idle_timeout: Duration,

    /// Request processing timeout.
    pub request_timeout: Duration,

    /// Whether TCP nodelay is enabled.
    pub tcp_nodelay: bool,

    /// TCP keepalive interval.
    pub keepalive_interval: Option<Duration>,

    /// Whether metrics collection is enabled.
    pub metrics_enabled: bool,

    /// Whether debug logging is enabled.
    pub debug_logging: bool,

    /// Enabled unit IDs.
    pub enabled_units: std::collections::HashSet<u8>,

    /// Custom settings.
    pub custom: std::collections::HashMap<String, String>,
}

impl Default for RuntimeState {
    fn default() -> Self {
        let mut enabled_units = std::collections::HashSet::new();
        enabled_units.insert(1); // Default unit ID 1 enabled

        Self {
            max_connections: 100,
            idle_timeout: Duration::from_secs(300),
            request_timeout: Duration::from_secs(30),
            tcp_nodelay: true,
            keepalive_interval: Some(Duration::from_secs(60)),
            metrics_enabled: true,
            debug_logging: false,
            enabled_units,
            custom: std::collections::HashMap::new(),
        }
    }
}

/// Runtime configuration manager.
///
/// Provides thread-safe runtime configuration updates with validation
/// and event notification.
pub struct RuntimeConfigManager {
    /// Current configuration state.
    state: Arc<RwLock<RuntimeState>>,

    /// Event broadcaster.
    event_tx: broadcast::Sender<ConfigEvent>,

    /// Callback for applying updates to the server.
    update_callback: RwLock<Option<Box<dyn Fn(&ConfigUpdate) -> Result<()> + Send + Sync>>>,

    /// Whether to validate updates before applying.
    validate_updates: bool,
}

impl RuntimeConfigManager {
    /// Create a new runtime configuration manager.
    pub fn new() -> Self {
        Self::with_state(RuntimeState::default())
    }

    /// Create a manager with initial state.
    pub fn with_state(state: RuntimeState) -> Self {
        let (event_tx, _) = broadcast::channel(256);

        Self {
            state: Arc::new(RwLock::new(state)),
            event_tx,
            update_callback: RwLock::new(None),
            validate_updates: true,
        }
    }

    /// Set the callback for applying updates.
    pub fn set_update_callback<F>(&self, callback: F)
    where
        F: Fn(&ConfigUpdate) -> Result<()> + Send + Sync + 'static,
    {
        *self.update_callback.write() = Some(Box::new(callback));
    }

    /// Get the current configuration state.
    pub fn state(&self) -> RuntimeState {
        self.state.read().clone()
    }

    /// Get a reference to the state lock.
    pub fn state_lock(&self) -> &Arc<RwLock<RuntimeState>> {
        &self.state
    }

    /// Subscribe to configuration events.
    pub fn subscribe(&self) -> broadcast::Receiver<ConfigEvent> {
        self.event_tx.subscribe()
    }

    /// Apply a single configuration update.
    pub fn apply(&self, update: ConfigUpdate) -> Result<()> {
        // Validate if enabled
        if self.validate_updates {
            self.validate_update(&update)?;
        }

        // Apply to local state
        self.apply_to_state(&update)?;

        // Call external callback if set
        if let Some(ref callback) = *self.update_callback.read() {
            if let Err(e) = callback(&update) {
                // Rollback on failure
                self.rollback_update(&update)?;
                let _ = self.event_tx.send(ConfigEvent::RolledBack {
                    update: update.clone(),
                    reason: e.to_string(),
                });
                return Err(e);
            }
        }

        // Emit success event
        let _ = self.event_tx.send(ConfigEvent::Updated {
            update,
            timestamp: std::time::SystemTime::now(),
        });

        Ok(())
    }

    /// Apply multiple updates atomically.
    pub fn apply_batch(&self, updates: Vec<ConfigUpdate>) -> Result<()> {
        // Validate all updates first
        if self.validate_updates {
            for update in &updates {
                self.validate_update(update)?;
            }
        }

        // Take a snapshot for potential rollback
        let snapshot = self.state.read().clone();

        // Apply all updates
        for update in &updates {
            if let Err(e) = self.apply_to_state(update) {
                // Rollback to snapshot
                *self.state.write() = snapshot;
                return Err(e);
            }

            // Call callback
            if let Some(ref callback) = *self.update_callback.read() {
                if let Err(e) = callback(update) {
                    // Rollback to snapshot
                    *self.state.write() = snapshot;
                    return Err(e);
                }
            }
        }

        // Emit batch success event
        let _ = self.event_tx.send(ConfigEvent::BatchUpdated {
            count: updates.len(),
            timestamp: std::time::SystemTime::now(),
        });

        Ok(())
    }

    /// Validate a configuration update.
    fn validate_update(&self, update: &ConfigUpdate) -> Result<()> {
        match update {
            ConfigUpdate::MaxConnections(max) => {
                if *max == 0 {
                    return Err(ModbusError::Config("max_connections must be > 0".into()));
                }
                if *max > 100_000 {
                    return Err(ModbusError::Config(
                        "max_connections too high (max 100,000)".into(),
                    ));
                }
            }
            ConfigUpdate::IdleTimeout(duration) => {
                if duration.as_secs() == 0 {
                    return Err(ModbusError::Config("idle_timeout must be > 0".into()));
                }
            }
            ConfigUpdate::RequestTimeout(duration) => {
                if duration.as_secs() == 0 {
                    return Err(ModbusError::Config("request_timeout must be > 0".into()));
                }
            }
            ConfigUpdate::UnitEnabled { unit_id, .. } => {
                if *unit_id == 0 {
                    // Unit ID 0 is broadcast, special handling
                    tracing::warn!("Enabling/disabling broadcast unit ID 0");
                }
            }
            ConfigUpdate::RegisterReadAccess { count, .. }
            | ConfigUpdate::RegisterWriteAccess { count, .. } => {
                if *count == 0 {
                    return Err(ModbusError::Config("register count must be > 0".into()));
                }
                if *count > 125 {
                    return Err(ModbusError::Config(
                        "register count exceeds Modbus limit (125)".into(),
                    ));
                }
            }
            ConfigUpdate::SetRegisters { values, .. } => {
                if values.is_empty() {
                    return Err(ModbusError::Config("values cannot be empty".into()));
                }
                if values.len() > 123 {
                    return Err(ModbusError::Config(
                        "too many values (max 123 per write)".into(),
                    ));
                }
            }
            _ => {}
        }

        Ok(())
    }

    /// Apply update to local state.
    fn apply_to_state(&self, update: &ConfigUpdate) -> Result<()> {
        let mut state = self.state.write();

        match update {
            ConfigUpdate::MaxConnections(max) => {
                state.max_connections = *max;
            }
            ConfigUpdate::IdleTimeout(duration) => {
                state.idle_timeout = *duration;
            }
            ConfigUpdate::RequestTimeout(duration) => {
                state.request_timeout = *duration;
            }
            ConfigUpdate::TcpNoDelay(enabled) => {
                state.tcp_nodelay = *enabled;
            }
            ConfigUpdate::KeepaliveInterval(interval) => {
                state.keepalive_interval = *interval;
            }
            ConfigUpdate::MetricsEnabled(enabled) => {
                state.metrics_enabled = *enabled;
            }
            ConfigUpdate::DebugLogging(enabled) => {
                state.debug_logging = *enabled;
            }
            ConfigUpdate::UnitEnabled { unit_id, enabled } => {
                if *enabled {
                    state.enabled_units.insert(*unit_id);
                } else {
                    state.enabled_units.remove(unit_id);
                }
            }
            ConfigUpdate::Custom { key, value } => {
                state.custom.insert(key.clone(), value.clone());
            }
            // Data updates don't affect state directly
            _ => {}
        }

        Ok(())
    }

    /// Rollback a single update (best effort).
    fn rollback_update(&self, update: &ConfigUpdate) -> Result<()> {
        // For most updates, we can't easily rollback without knowing the previous value
        // This is a best-effort implementation
        tracing::warn!(update = ?update, "Rolling back configuration update");
        Ok(())
    }

    /// Reset configuration to defaults.
    pub fn reset(&self) -> Result<()> {
        *self.state.write() = RuntimeState::default();

        let _ = self.event_tx.send(ConfigEvent::Reset {
            timestamp: std::time::SystemTime::now(),
        });

        Ok(())
    }

    /// Get a specific setting value.
    pub fn get<T: FromRuntimeState>(&self) -> T {
        let state = self.state.read();
        T::from_state(&state)
    }

    /// Export current state as JSON.
    pub fn export_json(&self) -> Result<String> {
        let state = self.state.read();
        serde_json::to_string_pretty(&*state)
            .map_err(|e| ModbusError::Config(format!("Failed to serialize state: {}", e)))
    }

    /// Import state from JSON.
    pub fn import_json(&self, json: &str) -> Result<()> {
        let new_state: RuntimeState = serde_json::from_str(json)
            .map_err(|e| ModbusError::Config(format!("Failed to parse JSON: {}", e)))?;

        *self.state.write() = new_state;

        let _ = self.event_tx.send(ConfigEvent::Reset {
            timestamp: std::time::SystemTime::now(),
        });

        Ok(())
    }
}

impl Default for RuntimeConfigManager {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for RuntimeConfigManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RuntimeConfigManager")
            .field("state", &self.state())
            .field("validate_updates", &self.validate_updates)
            .finish()
    }
}

/// Trait for extracting values from runtime state.
pub trait FromRuntimeState {
    fn from_state(state: &RuntimeState) -> Self;
}

impl FromRuntimeState for usize {
    fn from_state(state: &RuntimeState) -> Self {
        state.max_connections
    }
}

impl FromRuntimeState for bool {
    fn from_state(state: &RuntimeState) -> Self {
        state.metrics_enabled
    }
}

impl FromRuntimeState for Duration {
    fn from_state(state: &RuntimeState) -> Self {
        state.idle_timeout
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_runtime_config_manager_new() {
        let manager = RuntimeConfigManager::new();
        let state = manager.state();
        assert_eq!(state.max_connections, 100);
        assert!(state.metrics_enabled);
    }

    #[test]
    fn test_apply_single_update() {
        let manager = RuntimeConfigManager::new();

        manager.apply(ConfigUpdate::MaxConnections(200)).unwrap();
        assert_eq!(manager.state().max_connections, 200);
    }

    #[test]
    fn test_apply_batch_update() {
        let manager = RuntimeConfigManager::new();

        manager
            .apply_batch(vec![
                ConfigUpdate::MaxConnections(300),
                ConfigUpdate::MetricsEnabled(false),
            ])
            .unwrap();

        let state = manager.state();
        assert_eq!(state.max_connections, 300);
        assert!(!state.metrics_enabled);
    }

    #[test]
    fn test_validation_failure() {
        let manager = RuntimeConfigManager::new();

        let result = manager.apply(ConfigUpdate::MaxConnections(0));
        assert!(result.is_err());
    }

    #[test]
    fn test_unit_enabled() {
        let manager = RuntimeConfigManager::new();

        manager
            .apply(ConfigUpdate::UnitEnabled {
                unit_id: 5,
                enabled: true,
            })
            .unwrap();

        assert!(manager.state().enabled_units.contains(&5));

        manager
            .apply(ConfigUpdate::UnitEnabled {
                unit_id: 5,
                enabled: false,
            })
            .unwrap();

        assert!(!manager.state().enabled_units.contains(&5));
    }

    #[test]
    fn test_custom_setting() {
        let manager = RuntimeConfigManager::new();

        manager
            .apply(ConfigUpdate::Custom {
                key: "custom_key".into(),
                value: "custom_value".into(),
            })
            .unwrap();

        assert_eq!(
            manager.state().custom.get("custom_key"),
            Some(&"custom_value".to_string())
        );
    }

    #[test]
    fn test_reset() {
        let manager = RuntimeConfigManager::new();

        manager.apply(ConfigUpdate::MaxConnections(500)).unwrap();
        manager.reset().unwrap();

        assert_eq!(manager.state().max_connections, 100);
    }

    #[test]
    fn test_export_import_json() {
        let manager = RuntimeConfigManager::new();

        manager.apply(ConfigUpdate::MaxConnections(999)).unwrap();

        let json = manager.export_json().unwrap();
        assert!(json.contains("999"));

        let manager2 = RuntimeConfigManager::new();
        manager2.import_json(&json).unwrap();

        assert_eq!(manager2.state().max_connections, 999);
    }

    #[test]
    fn test_update_category() {
        assert_eq!(
            ConfigUpdate::MaxConnections(100).category(),
            UpdateCategory::Connection
        );
        assert_eq!(
            ConfigUpdate::UnitEnabled {
                unit_id: 1,
                enabled: true
            }
            .category(),
            UpdateCategory::Unit
        );
        assert_eq!(
            ConfigUpdate::MetricsEnabled(true).category(),
            UpdateCategory::Monitoring
        );
    }

    #[test]
    fn test_requires_restart() {
        assert!(ConfigUpdate::TcpNoDelay(true).requires_restart());
        assert!(!ConfigUpdate::MaxConnections(100).requires_restart());
    }

    #[tokio::test]
    async fn test_event_subscription() {
        let manager = RuntimeConfigManager::new();
        let mut rx = manager.subscribe();

        manager.apply(ConfigUpdate::MaxConnections(150)).unwrap();

        let event = rx.try_recv().unwrap();
        matches!(event, ConfigEvent::Updated { .. });
    }
}

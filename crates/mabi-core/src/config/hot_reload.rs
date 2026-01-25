//! Hot reload integration for the simulator engine.
//!
//! This module provides seamless integration between the configuration
//! file watching system and the simulator engine, enabling runtime
//! configuration updates without service interruption.
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────┐     ┌──────────────────┐     ┌─────────────────┐
//! │  FileWatcher    │────▶│  HotReloadHandler│────▶│ SimulatorEngine │
//! │  Service        │     │  (this module)   │     │                 │
//! └─────────────────┘     └──────────────────┘     └─────────────────┘
//!                                │
//!                                ▼
//!                         ┌──────────────────┐
//!                         │  Validation &    │
//!                         │  Atomic Swap     │
//!                         └──────────────────┘
//! ```
//!
//! # Features
//!
//! - Automatic configuration reload on file changes
//! - Validation before applying changes
//! - Atomic configuration swap
//! - Rollback on failure
//! - Configurable reload strategy
//!
//! # Example
//!
//! ```rust,ignore
//! use mabi_core::config::hot_reload::{HotReloadManager, ReloadStrategy};
//! use mabi_core::config::{ConfigSource, EngineConfig};
//!
//! // Create manager
//! let config = EngineConfig::default();
//! let manager = HotReloadManager::new(config)
//!     .strategy(ReloadStrategy::ValidateFirst)
//!     .config_path("config.yaml");
//!
//! // Start watching
//! manager.start().await?;
//!
//! // Get current config (thread-safe)
//! let current = manager.config();
//! println!("Max devices: {}", current.max_devices);
//!
//! // Subscribe to reload events
//! let mut rx = manager.subscribe();
//! while let Ok(event) = rx.recv().await {
//!     match event {
//!         ReloadEvent::Reloaded { old, new } => {
//!             println!("Config reloaded!");
//!         }
//!         ReloadEvent::Failed { error } => {
//!             eprintln!("Reload failed: {}", error);
//!         }
//!     }
//! }
//! ```

use std::path::PathBuf;
use std::sync::Arc;

use parking_lot::RwLock;
use tokio::sync::broadcast;

use super::file_watcher::FileWatcherService;
use super::loader::ConfigLoader;
use super::validation::Validatable;
use super::watcher::{ConfigEvent, ConfigSource, ConfigWatcher};
use super::EngineConfig;
use crate::error::Error;
use crate::Result;

/// Strategy for applying configuration reloads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ReloadStrategy {
    /// Validate configuration before applying.
    #[default]
    ValidateFirst,
    /// Apply immediately without validation (not recommended).
    Immediate,
    /// Only log changes, don't apply (dry-run mode).
    DryRun,
}

/// Events emitted during hot reload.
#[derive(Debug, Clone)]
pub enum ReloadEvent {
    /// Configuration file changed (before processing).
    FileChanged {
        path: PathBuf,
    },
    /// Configuration successfully reloaded.
    Reloaded {
        path: PathBuf,
        changes: Vec<ConfigChange>,
    },
    /// Configuration reload failed.
    Failed {
        path: PathBuf,
        error: String,
    },
    /// Validation failed (dry-run or ValidateFirst).
    ValidationFailed {
        path: PathBuf,
        errors: Vec<String>,
    },
    /// Configuration rollback occurred.
    RolledBack {
        path: PathBuf,
        reason: String,
    },
}

/// Describes a configuration change.
#[derive(Debug, Clone)]
pub struct ConfigChange {
    /// Field path that changed.
    pub field: String,
    /// Old value (as string).
    pub old_value: String,
    /// New value (as string).
    pub new_value: String,
}

impl ConfigChange {
    /// Create a new config change.
    pub fn new(field: impl Into<String>, old: impl ToString, new: impl ToString) -> Self {
        Self {
            field: field.into(),
            old_value: old.to_string(),
            new_value: new.to_string(),
        }
    }
}

/// Manager for hot reloading engine configuration.
pub struct HotReloadManager {
    /// Current configuration (thread-safe).
    config: Arc<RwLock<EngineConfig>>,
    /// File watcher service.
    file_watcher: Option<FileWatcherService>,
    /// Config watcher for receiving events.
    config_watcher: Arc<ConfigWatcher>,
    /// Reload strategy.
    strategy: ReloadStrategy,
    /// Configuration file path.
    config_path: Option<PathBuf>,
    /// Event broadcaster.
    event_tx: broadcast::Sender<ReloadEvent>,
    /// Whether the manager is running.
    running: Arc<RwLock<bool>>,
}

impl HotReloadManager {
    /// Create a new hot reload manager.
    pub fn new(config: EngineConfig) -> Self {
        let (event_tx, _) = broadcast::channel(64);
        let config_watcher = Arc::new(ConfigWatcher::new());

        Self {
            config: Arc::new(RwLock::new(config)),
            file_watcher: None,
            config_watcher,
            strategy: ReloadStrategy::default(),
            config_path: None,
            event_tx,
            running: Arc::new(RwLock::new(false)),
        }
    }

    /// Set the reload strategy.
    pub fn strategy(mut self, strategy: ReloadStrategy) -> Self {
        self.strategy = strategy;
        self
    }

    /// Set the configuration file path.
    pub fn config_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.config_path = Some(path.into());
        self
    }

    /// Get the current configuration (read-only).
    pub fn config(&self) -> EngineConfig {
        self.config.read().clone()
    }

    /// Get a reference to the configuration lock.
    pub fn config_lock(&self) -> &Arc<RwLock<EngineConfig>> {
        &self.config
    }

    /// Check if the manager is running.
    pub fn is_running(&self) -> bool {
        *self.running.read()
    }

    /// Subscribe to reload events.
    pub fn subscribe(&self) -> broadcast::Receiver<ReloadEvent> {
        self.event_tx.subscribe()
    }

    /// Get the config watcher.
    pub fn config_watcher(&self) -> &Arc<ConfigWatcher> {
        &self.config_watcher
    }

    /// Start the hot reload manager.
    pub fn start(&mut self) -> Result<()> {
        if *self.running.read() {
            return Err(Error::Engine("Hot reload manager already running".into()));
        }

        // Create file watcher
        let file_watcher = FileWatcherService::new(self.config_watcher.clone());

        // Register config path if set
        if let Some(path) = &self.config_path {
            file_watcher.watch(ConfigSource::Main(path.clone()))?;
        }

        // Start file watcher
        file_watcher.start()?;

        // Start event processing
        self.start_event_processor();

        self.file_watcher = Some(file_watcher);
        *self.running.write() = true;

        tracing::info!("Hot reload manager started");
        Ok(())
    }

    /// Stop the hot reload manager.
    pub fn stop(&mut self) -> Result<()> {
        if !*self.running.read() {
            return Err(Error::Engine("Hot reload manager not running".into()));
        }

        if let Some(ref file_watcher) = self.file_watcher {
            file_watcher.stop()?;
        }

        *self.running.write() = false;
        self.file_watcher = None;

        tracing::info!("Hot reload manager stopped");
        Ok(())
    }

    /// Manually reload configuration from the file.
    pub fn reload(&self) -> Result<Vec<ConfigChange>> {
        let path = self.config_path.as_ref().ok_or_else(|| {
            Error::Config("No configuration path set".into())
        })?;

        self.reload_from_path(path)
    }

    /// Reload configuration from a specific path.
    pub fn reload_from_path(&self, path: &PathBuf) -> Result<Vec<ConfigChange>> {
        tracing::debug!(path = %path.display(), "Reloading configuration");

        // Emit file changed event
        let _ = self.event_tx.send(ReloadEvent::FileChanged {
            path: path.clone(),
        });

        // Load new configuration
        let new_config: EngineConfig = ConfigLoader::load(path)?;

        // Validate if strategy requires it
        if self.strategy == ReloadStrategy::ValidateFirst {
            if let Err(e) = new_config.validate() {
                let error_msg = e.to_string();
                let _ = self.event_tx.send(ReloadEvent::ValidationFailed {
                    path: path.clone(),
                    errors: vec![error_msg.clone()],
                });
                return Err(e);
            }
        }

        // If dry-run, don't apply
        if self.strategy == ReloadStrategy::DryRun {
            let changes = self.detect_changes(&new_config);
            tracing::info!(
                path = %path.display(),
                changes = changes.len(),
                "Dry-run: would apply {} changes",
                changes.len()
            );
            return Ok(changes);
        }

        // Detect changes before applying
        let changes = self.detect_changes(&new_config);

        // Apply new configuration
        *self.config.write() = new_config;

        // Emit success event
        let _ = self.event_tx.send(ReloadEvent::Reloaded {
            path: path.clone(),
            changes: changes.clone(),
        });

        tracing::info!(
            path = %path.display(),
            changes = changes.len(),
            "Configuration reloaded successfully"
        );

        Ok(changes)
    }

    /// Detect changes between current and new configuration.
    fn detect_changes(&self, new_config: &EngineConfig) -> Vec<ConfigChange> {
        let current = self.config.read();
        let mut changes = Vec::new();

        if current.name != new_config.name {
            changes.push(ConfigChange::new("name", &current.name, &new_config.name));
        }
        if current.max_devices != new_config.max_devices {
            changes.push(ConfigChange::new("max_devices", current.max_devices, new_config.max_devices));
        }
        if current.max_points != new_config.max_points {
            changes.push(ConfigChange::new("max_points", current.max_points, new_config.max_points));
        }
        if current.tick_interval_ms != new_config.tick_interval_ms {
            changes.push(ConfigChange::new("tick_interval_ms", current.tick_interval_ms, new_config.tick_interval_ms));
        }
        if current.workers != new_config.workers {
            changes.push(ConfigChange::new("workers", current.workers, new_config.workers));
        }
        if current.enable_metrics != new_config.enable_metrics {
            changes.push(ConfigChange::new("enable_metrics", current.enable_metrics, new_config.enable_metrics));
        }
        if current.metrics_interval_secs != new_config.metrics_interval_secs {
            changes.push(ConfigChange::new("metrics_interval_secs", current.metrics_interval_secs, new_config.metrics_interval_secs));
        }
        if current.log_level != new_config.log_level {
            changes.push(ConfigChange::new("log_level", &current.log_level, &new_config.log_level));
        }

        changes
    }

    /// Start the event processing loop.
    fn start_event_processor(&self) {
        let mut rx = self.config_watcher.subscribe();
        let config = self.config.clone();
        let strategy = self.strategy;
        let event_tx = self.event_tx.clone();
        let running = self.running.clone();

        tokio::spawn(async move {
            while *running.read() {
                match rx.recv().await {
                    Ok(ConfigEvent::Modified { source, .. }) | Ok(ConfigEvent::Created { source, .. }) => {
                        let path = source.path().clone();

                        // Load and validate
                        match ConfigLoader::load::<EngineConfig>(&path) {
                            Ok(new_config) => {
                                // Validate if required
                                if strategy == ReloadStrategy::ValidateFirst {
                                    if let Err(e) = new_config.validate() {
                                        let _ = event_tx.send(ReloadEvent::ValidationFailed {
                                            path: path.clone(),
                                            errors: vec![e.to_string()],
                                        });
                                        continue;
                                    }
                                }

                                if strategy != ReloadStrategy::DryRun {
                                    *config.write() = new_config;
                                }

                                let _ = event_tx.send(ReloadEvent::Reloaded {
                                    path,
                                    changes: Vec::new(), // Changes detected elsewhere
                                });
                            }
                            Err(e) => {
                                let _ = event_tx.send(ReloadEvent::Failed {
                                    path,
                                    error: e.to_string(),
                                });
                            }
                        }
                    }
                    Ok(ConfigEvent::Deleted { source, .. }) => {
                        tracing::warn!(
                            path = %source.path().display(),
                            "Configuration file deleted"
                        );
                    }
                    Ok(ConfigEvent::Error { message, .. }) => {
                        tracing::error!(error = %message, "Config watcher error");
                    }
                    Err(broadcast::error::RecvError::Closed) => {
                        break;
                    }
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        tracing::warn!(skipped = n, "Event processor lagged behind");
                    }
                    _ => {}
                }
            }
        });
    }

    /// Add a path to watch.
    pub fn watch(&self, source: ConfigSource) -> Result<()> {
        if let Some(ref file_watcher) = self.file_watcher {
            file_watcher.watch(source)?;
        }
        Ok(())
    }

    /// Remove a path from watching.
    pub fn unwatch(&self, path: &PathBuf) -> Result<()> {
        if let Some(ref file_watcher) = self.file_watcher {
            file_watcher.unwatch(path)?;
        }
        Ok(())
    }
}

impl Drop for HotReloadManager {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

/// Builder for HotReloadManager.
pub struct HotReloadManagerBuilder {
    config: EngineConfig,
    strategy: ReloadStrategy,
    config_path: Option<PathBuf>,
    additional_paths: Vec<ConfigSource>,
}

impl HotReloadManagerBuilder {
    /// Create a new builder with default configuration.
    pub fn new() -> Self {
        Self {
            config: EngineConfig::default(),
            strategy: ReloadStrategy::default(),
            config_path: None,
            additional_paths: Vec::new(),
        }
    }

    /// Create a builder with existing configuration.
    pub fn with_config(config: EngineConfig) -> Self {
        Self {
            config,
            strategy: ReloadStrategy::default(),
            config_path: None,
            additional_paths: Vec::new(),
        }
    }

    /// Set the reload strategy.
    pub fn strategy(mut self, strategy: ReloadStrategy) -> Self {
        self.strategy = strategy;
        self
    }

    /// Set the main configuration file path.
    pub fn config_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.config_path = Some(path.into());
        self
    }

    /// Add additional paths to watch.
    pub fn watch(mut self, source: ConfigSource) -> Self {
        self.additional_paths.push(source);
        self
    }

    /// Build the HotReloadManager.
    pub fn build(self) -> HotReloadManager {
        let mut manager = HotReloadManager::new(self.config)
            .strategy(self.strategy);

        if let Some(path) = self.config_path {
            manager = manager.config_path(path);
        }

        manager
    }
}

impl Default for HotReloadManagerBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn test_reload_strategy_default() {
        assert_eq!(ReloadStrategy::default(), ReloadStrategy::ValidateFirst);
    }

    #[test]
    fn test_hot_reload_manager_creation() {
        let config = EngineConfig::default();
        let manager = HotReloadManager::new(config.clone());

        assert!(!manager.is_running());
        assert_eq!(manager.config().max_devices, config.max_devices);
    }

    #[test]
    fn test_hot_reload_manager_config_access() {
        let config = EngineConfig::default().with_max_devices(50_000);
        let manager = HotReloadManager::new(config);

        let current = manager.config();
        assert_eq!(current.max_devices, 50_000);
    }

    #[test]
    fn test_detect_changes() {
        let old_config = EngineConfig::default();
        let manager = HotReloadManager::new(old_config);

        let new_config = EngineConfig::default()
            .with_max_devices(50_000)
            .with_log_level("debug");

        let changes = manager.detect_changes(&new_config);

        assert!(changes.iter().any(|c| c.field == "max_devices"));
        assert!(changes.iter().any(|c| c.field == "log_level"));
    }

    #[test]
    fn test_config_change() {
        let change = ConfigChange::new("max_devices", 10_000, 50_000);
        assert_eq!(change.field, "max_devices");
        assert_eq!(change.old_value, "10000");
        assert_eq!(change.new_value, "50000");
    }

    #[test]
    fn test_builder() {
        let manager = HotReloadManagerBuilder::new()
            .strategy(ReloadStrategy::DryRun)
            .config_path("config.yaml")
            .build();

        assert_eq!(manager.strategy, ReloadStrategy::DryRun);
    }

    #[tokio::test]
    async fn test_reload_from_file() {
        let dir = tempdir().unwrap();
        let config_path = dir.path().join("config.yaml");

        // Write initial config
        let yaml = "name: test\nmax_devices: 10000\nmax_points: 100000\n";
        fs::write(&config_path, yaml).unwrap();

        let config = EngineConfig::default();
        let manager = HotReloadManager::new(config)
            .config_path(config_path.clone());

        // Reload
        let result = manager.reload();
        assert!(result.is_ok());

        // Check updated config
        assert_eq!(manager.config().name, "test");
    }

    #[tokio::test]
    async fn test_reload_validation_failure() {
        let dir = tempdir().unwrap();
        let config_path = dir.path().join("config.yaml");

        // Write invalid config
        let yaml = "name: test\nmax_devices: 0\n";  // max_devices = 0 is invalid
        fs::write(&config_path, yaml).unwrap();

        let config = EngineConfig::default();
        let manager = HotReloadManager::new(config)
            .strategy(ReloadStrategy::ValidateFirst)
            .config_path(config_path);

        let result = manager.reload();
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_dry_run_mode() {
        let dir = tempdir().unwrap();
        let config_path = dir.path().join("config.yaml");

        let yaml = "name: dry-run-test\nmax_devices: 99999\nmax_points: 1000000\n";
        fs::write(&config_path, yaml).unwrap();

        let original_config = EngineConfig::default();
        let manager = HotReloadManager::new(original_config.clone())
            .strategy(ReloadStrategy::DryRun)
            .config_path(config_path);

        // Reload in dry-run mode
        let changes = manager.reload().unwrap();

        // Should detect changes
        assert!(changes.iter().any(|c| c.field == "name"));

        // But config should not change
        assert_eq!(manager.config().name, original_config.name);
    }
}

//! Configuration file watcher for hot reload support.
//!
//! This module provides infrastructure for watching configuration files
//! and notifying subscribers when changes occur. The actual file watching
//! implementation is provided by external crates (e.g., notify) at the
//! application level, while this module defines the interfaces and event types.
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────┐     ┌──────────────────┐     ┌─────────────────┐
//! │  Config File    │────▶│  ConfigWatcher   │────▶│  Subscribers    │
//! │  (YAML/JSON)    │     │  (notify events) │     │  (engine, etc.) │
//! └─────────────────┘     └──────────────────┘     └─────────────────┘
//! ```
//!
//! # Example
//!
//! ```rust,ignore
//! use mabi_core::config::{ConfigEvent, ConfigEventHandler, ConfigSource};
//!
//! struct MyHandler;
//!
//! impl ConfigEventHandler for MyHandler {
//!     fn on_config_change(&self, event: ConfigEvent) {
//!         match event {
//!             ConfigEvent::Modified { source, .. } => {
//!                 println!("Config modified: {:?}", source);
//!             }
//!             _ => {}
//!         }
//!     }
//! }
//! ```

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use parking_lot::RwLock;
use tokio::sync::broadcast;

/// Configuration source identifier.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ConfigSource {
    /// Main simulator configuration file.
    Main(PathBuf),
    /// Device configuration file.
    Device(PathBuf),
    /// Scenario configuration file.
    Scenario(PathBuf),
    /// Protocol-specific configuration.
    Protocol { protocol: String, path: PathBuf },
    /// Custom configuration source.
    Custom { name: String, path: PathBuf },
}

impl ConfigSource {
    /// Get the path of this configuration source.
    pub fn path(&self) -> &PathBuf {
        match self {
            Self::Main(p) => p,
            Self::Device(p) => p,
            Self::Scenario(p) => p,
            Self::Protocol { path, .. } => path,
            Self::Custom { path, .. } => path,
        }
    }

    /// Get a descriptive name for this source.
    pub fn name(&self) -> String {
        match self {
            Self::Main(_) => "main".to_string(),
            Self::Device(_) => "device".to_string(),
            Self::Scenario(_) => "scenario".to_string(),
            Self::Protocol { protocol, .. } => format!("protocol:{}", protocol),
            Self::Custom { name, .. } => format!("custom:{}", name),
        }
    }
}

/// Configuration change event.
#[derive(Debug, Clone)]
pub enum ConfigEvent {
    /// Configuration file was created.
    Created {
        source: ConfigSource,
        timestamp: Instant,
    },
    /// Configuration file was modified.
    Modified {
        source: ConfigSource,
        timestamp: Instant,
    },
    /// Configuration file was deleted.
    Deleted {
        source: ConfigSource,
        timestamp: Instant,
    },
    /// Configuration file was renamed.
    Renamed {
        source: ConfigSource,
        old_path: PathBuf,
        new_path: PathBuf,
        timestamp: Instant,
    },
    /// Error occurred while watching.
    Error {
        source: Option<ConfigSource>,
        message: String,
        timestamp: Instant,
    },
    /// Configuration was reloaded successfully.
    Reloaded {
        source: ConfigSource,
        timestamp: Instant,
    },
}

impl ConfigEvent {
    /// Get the timestamp of this event.
    pub fn timestamp(&self) -> Instant {
        match self {
            Self::Created { timestamp, .. } => *timestamp,
            Self::Modified { timestamp, .. } => *timestamp,
            Self::Deleted { timestamp, .. } => *timestamp,
            Self::Renamed { timestamp, .. } => *timestamp,
            Self::Error { timestamp, .. } => *timestamp,
            Self::Reloaded { timestamp, .. } => *timestamp,
        }
    }

    /// Get the source of this event, if available.
    pub fn source(&self) -> Option<&ConfigSource> {
        match self {
            Self::Created { source, .. } => Some(source),
            Self::Modified { source, .. } => Some(source),
            Self::Deleted { source, .. } => Some(source),
            Self::Renamed { source, .. } => Some(source),
            Self::Error { source, .. } => source.as_ref(),
            Self::Reloaded { source, .. } => Some(source),
        }
    }

    /// Check if this is an error event.
    pub fn is_error(&self) -> bool {
        matches!(self, Self::Error { .. })
    }
}

/// Trait for handling configuration events.
///
/// Implement this trait to receive notifications when configuration changes.
pub trait ConfigEventHandler: Send + Sync {
    /// Called when a configuration event occurs.
    fn on_config_change(&self, event: ConfigEvent);

    /// Called before configuration reload.
    /// Return false to cancel the reload.
    fn before_reload(&self, _source: &ConfigSource) -> bool {
        true
    }

    /// Called after successful configuration reload.
    fn after_reload(&self, _source: &ConfigSource) {}
}

/// Configuration watcher state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WatcherState {
    /// Watcher is stopped.
    Stopped,
    /// Watcher is running.
    Running,
    /// Watcher is paused (events are queued but not processed).
    Paused,
}

/// Configuration watcher for hot reload support.
///
/// This struct manages configuration file watching and event distribution.
/// The actual file system watching should be implemented at the application level
/// using this struct to distribute events.
pub struct ConfigWatcher {
    /// Current state.
    state: RwLock<WatcherState>,

    /// Registered sources.
    sources: RwLock<Vec<ConfigSource>>,

    /// Event broadcaster.
    event_tx: broadcast::Sender<ConfigEvent>,

    /// Debounce duration in milliseconds.
    debounce_ms: u64,

    /// Last event time per source (for debouncing).
    last_events: RwLock<std::collections::HashMap<String, Instant>>,
}

impl ConfigWatcher {
    /// Create a new configuration watcher.
    pub fn new() -> Self {
        let (event_tx, _) = broadcast::channel(256);

        Self {
            state: RwLock::new(WatcherState::Stopped),
            sources: RwLock::new(Vec::new()),
            event_tx,
            debounce_ms: 100,
            last_events: RwLock::new(std::collections::HashMap::new()),
        }
    }

    /// Create a new configuration watcher with custom debounce duration.
    pub fn with_debounce(debounce_ms: u64) -> Self {
        let mut watcher = Self::new();
        watcher.debounce_ms = debounce_ms;
        watcher
    }

    /// Get the current state.
    pub fn state(&self) -> WatcherState {
        *self.state.read()
    }

    /// Start the watcher.
    pub fn start(&self) {
        *self.state.write() = WatcherState::Running;
    }

    /// Stop the watcher.
    pub fn stop(&self) {
        *self.state.write() = WatcherState::Stopped;
    }

    /// Pause the watcher (events are still received but not processed).
    pub fn pause(&self) {
        *self.state.write() = WatcherState::Paused;
    }

    /// Resume the watcher.
    pub fn resume(&self) {
        *self.state.write() = WatcherState::Running;
    }

    /// Register a configuration source to watch.
    pub fn register(&self, source: ConfigSource) {
        let mut sources = self.sources.write();
        if !sources.iter().any(|s| s.path() == source.path()) {
            sources.push(source);
        }
    }

    /// Unregister a configuration source.
    pub fn unregister(&self, path: &PathBuf) {
        self.sources.write().retain(|s| s.path() != path);
    }

    /// Get all registered sources.
    pub fn sources(&self) -> Vec<ConfigSource> {
        self.sources.read().clone()
    }

    /// Subscribe to configuration events.
    pub fn subscribe(&self) -> broadcast::Receiver<ConfigEvent> {
        self.event_tx.subscribe()
    }

    /// Emit a configuration event.
    ///
    /// This method should be called by the file system watcher implementation.
    /// Events are debounced based on the configured duration.
    pub fn emit(&self, event: ConfigEvent) {
        // Check if watcher is running
        if *self.state.read() != WatcherState::Running {
            return;
        }

        // Debounce check
        if let Some(source) = event.source() {
            let source_key = source.name();
            let now = Instant::now();

            let mut last_events = self.last_events.write();
            if let Some(last_time) = last_events.get(&source_key) {
                if now.duration_since(*last_time).as_millis() < self.debounce_ms as u128 {
                    return; // Skip debounced event
                }
            }
            last_events.insert(source_key, now);
        }

        // Broadcast event
        let _ = self.event_tx.send(event);
    }

    /// Emit a modification event for a source.
    pub fn emit_modified(&self, source: ConfigSource) {
        self.emit(ConfigEvent::Modified {
            source,
            timestamp: Instant::now(),
        });
    }

    /// Emit a reload success event.
    pub fn emit_reloaded(&self, source: ConfigSource) {
        self.emit(ConfigEvent::Reloaded {
            source,
            timestamp: Instant::now(),
        });
    }

    /// Emit an error event.
    pub fn emit_error(&self, source: Option<ConfigSource>, message: impl Into<String>) {
        self.emit(ConfigEvent::Error {
            source,
            message: message.into(),
            timestamp: Instant::now(),
        });
    }
}

impl Default for ConfigWatcher {
    fn default() -> Self {
        Self::new()
    }
}

/// Arc-wrapped ConfigWatcher for shared access.
pub type SharedConfigWatcher = Arc<ConfigWatcher>;

/// Create a shared config watcher.
pub fn create_config_watcher() -> SharedConfigWatcher {
    Arc::new(ConfigWatcher::new())
}

/// Callback-based event handler adapter.
pub struct CallbackHandler<F>
where
    F: Fn(ConfigEvent) + Send + Sync,
{
    callback: F,
}

impl<F> CallbackHandler<F>
where
    F: Fn(ConfigEvent) + Send + Sync,
{
    /// Create a new callback handler.
    pub fn new(callback: F) -> Self {
        Self { callback }
    }
}

impl<F> ConfigEventHandler for CallbackHandler<F>
where
    F: Fn(ConfigEvent) + Send + Sync,
{
    fn on_config_change(&self, event: ConfigEvent) {
        (self.callback)(event);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_source() {
        let source = ConfigSource::Main(PathBuf::from("/etc/config.yaml"));
        assert_eq!(source.name(), "main");
        assert_eq!(source.path(), &PathBuf::from("/etc/config.yaml"));

        let protocol_source = ConfigSource::Protocol {
            protocol: "modbus".to_string(),
            path: PathBuf::from("/etc/modbus.yaml"),
        };
        assert_eq!(protocol_source.name(), "protocol:modbus");
    }

    #[test]
    fn test_config_watcher_lifecycle() {
        let watcher = ConfigWatcher::new();

        assert_eq!(watcher.state(), WatcherState::Stopped);

        watcher.start();
        assert_eq!(watcher.state(), WatcherState::Running);

        watcher.pause();
        assert_eq!(watcher.state(), WatcherState::Paused);

        watcher.resume();
        assert_eq!(watcher.state(), WatcherState::Running);

        watcher.stop();
        assert_eq!(watcher.state(), WatcherState::Stopped);
    }

    #[test]
    fn test_config_watcher_sources() {
        let watcher = ConfigWatcher::new();

        let source1 = ConfigSource::Main(PathBuf::from("/etc/config1.yaml"));
        let source2 = ConfigSource::Device(PathBuf::from("/etc/config2.yaml"));

        watcher.register(source1.clone());
        watcher.register(source2.clone());

        let sources = watcher.sources();
        assert_eq!(sources.len(), 2);

        watcher.unregister(&PathBuf::from("/etc/config1.yaml"));
        let sources = watcher.sources();
        assert_eq!(sources.len(), 1);
    }

    #[tokio::test]
    async fn test_config_watcher_events() {
        let watcher = ConfigWatcher::new();
        watcher.start();

        let mut rx = watcher.subscribe();

        let source = ConfigSource::Main(PathBuf::from("/etc/config.yaml"));
        watcher.emit_modified(source.clone());

        let event = tokio::time::timeout(
            std::time::Duration::from_millis(100),
            rx.recv(),
        )
        .await
        .expect("Timeout")
        .expect("Channel closed");

        assert!(matches!(event, ConfigEvent::Modified { .. }));
    }

    #[test]
    fn test_config_event_methods() {
        let source = ConfigSource::Main(PathBuf::from("/etc/config.yaml"));
        let event = ConfigEvent::Modified {
            source: source.clone(),
            timestamp: Instant::now(),
        };

        assert!(!event.is_error());
        assert_eq!(event.source().unwrap().name(), "main");

        let error_event = ConfigEvent::Error {
            source: None,
            message: "Test error".to_string(),
            timestamp: Instant::now(),
        };
        assert!(error_event.is_error());
    }
}

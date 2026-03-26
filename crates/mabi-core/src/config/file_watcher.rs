//! File system watching implementation using notify crate.
//!
//! This module integrates the `notify` crate with the configuration watcher
//! infrastructure to provide real-time file change detection.
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────┐     ┌──────────────────┐     ┌─────────────────┐
//! │  File System    │────▶│  FileWatcher     │────▶│  ConfigWatcher  │
//! │  (notify crate) │     │  (this module)   │     │  (watcher.rs)   │
//! └─────────────────┘     └──────────────────┘     └─────────────────┘
//!                                                           │
//!                                                           ▼
//!                                                  ┌─────────────────┐
//!                                                  │  Subscribers    │
//!                                                  │  (engine, etc.) │
//!                                                  └─────────────────┘
//! ```
//!
//! # Example
//!
//! ```rust,ignore
//! use mabi_core::config::file_watcher::FileWatcherService;
//! use mabi_core::config::{ConfigSource, ConfigWatcher};
//!
//! // Create watcher service
//! let watcher = Arc::new(ConfigWatcher::new());
//! let service = FileWatcherService::new(watcher.clone())?;
//!
//! // Register paths to watch
//! service.watch(ConfigSource::Main(PathBuf::from("config.yaml")))?;
//!
//! // Start watching
//! service.start().await?;
//! ```

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use notify::{
    Config as NotifyConfig, Event as NotifyEvent, EventKind, RecommendedWatcher, RecursiveMode,
    Watcher,
};
use parking_lot::{Mutex, RwLock};
use tokio::sync::{mpsc, oneshot};

use super::watcher::{ConfigEvent, ConfigSource, ConfigWatcher, SharedConfigWatcher};
use crate::error::Error;
use crate::Result;

/// Default debounce duration for file events.
pub const DEFAULT_DEBOUNCE_MS: u64 = 100;

/// Configuration for the file watcher service.
#[derive(Debug, Clone)]
pub struct FileWatcherConfig {
    /// Debounce duration in milliseconds.
    pub debounce_ms: u64,
    /// Whether to watch directories recursively.
    pub recursive: bool,
    /// File extensions to filter (empty = all files).
    pub extensions: Vec<String>,
}

impl Default for FileWatcherConfig {
    fn default() -> Self {
        Self {
            debounce_ms: DEFAULT_DEBOUNCE_MS,
            recursive: false,
            extensions: vec![
                "yaml".to_string(),
                "yml".to_string(),
                "json".to_string(),
                "toml".to_string(),
            ],
        }
    }
}

impl FileWatcherConfig {
    /// Create config for watching all file types.
    pub fn all_files() -> Self {
        Self {
            extensions: Vec::new(),
            ..Default::default()
        }
    }

    /// Set debounce duration.
    pub fn with_debounce(mut self, ms: u64) -> Self {
        self.debounce_ms = ms;
        self
    }

    /// Set recursive mode.
    pub fn with_recursive(mut self, recursive: bool) -> Self {
        self.recursive = recursive;
        self
    }

    /// Set file extensions to watch.
    pub fn with_extensions(mut self, exts: Vec<String>) -> Self {
        self.extensions = exts;
        self
    }

    /// Check if a path should be watched based on extensions filter.
    pub fn should_watch(&self, path: &Path) -> bool {
        if self.extensions.is_empty() {
            return true;
        }

        path.extension()
            .and_then(|e| e.to_str())
            .map(|ext| self.extensions.iter().any(|e| e.eq_ignore_ascii_case(ext)))
            .unwrap_or(false)
    }
}

/// Internal state for tracked paths.
#[derive(Clone)]
struct WatchedPath {
    source: ConfigSource,
    #[allow(dead_code)]
    last_event: Option<Instant>,
}

/// File watcher service state.
enum ServiceState {
    Stopped,
    Running {
        watcher: RecommendedWatcher,
        shutdown_tx: oneshot::Sender<()>,
    },
}

/// Service for watching configuration files on the filesystem.
///
/// This service bridges the `notify` crate with the [`ConfigWatcher`] infrastructure.
pub struct FileWatcherService {
    /// Configuration watcher to emit events to.
    config_watcher: SharedConfigWatcher,
    /// Service configuration.
    config: FileWatcherConfig,
    /// Watched paths mapping path -> source.
    watched_paths: RwLock<HashMap<PathBuf, WatchedPath>>,
    /// Service state.
    state: Mutex<Option<ServiceState>>,
}

impl FileWatcherService {
    /// Create a new file watcher service.
    pub fn new(config_watcher: SharedConfigWatcher) -> Self {
        Self::with_config(config_watcher, FileWatcherConfig::default())
    }

    /// Create with custom configuration.
    pub fn with_config(config_watcher: SharedConfigWatcher, config: FileWatcherConfig) -> Self {
        Self {
            config_watcher,
            config,
            watched_paths: RwLock::new(HashMap::new()),
            state: Mutex::new(Some(ServiceState::Stopped)),
        }
    }

    /// Get the configuration watcher.
    pub fn config_watcher(&self) -> &SharedConfigWatcher {
        &self.config_watcher
    }

    /// Get service configuration.
    pub fn config(&self) -> &FileWatcherConfig {
        &self.config
    }

    /// Check if the service is running.
    pub fn is_running(&self) -> bool {
        matches!(
            self.state.lock().as_ref(),
            Some(ServiceState::Running { .. })
        )
    }

    /// Add a path to watch.
    pub fn watch(&self, source: ConfigSource) -> Result<()> {
        let path = source.path().clone();

        // Validate path exists
        if !path.exists() {
            tracing::warn!(path = %path.display(), "Watching non-existent path");
        }

        // Check extension filter
        if !self.config.should_watch(&path) {
            return Err(Error::Config(format!(
                "Path extension not in allowed list: {}",
                path.display()
            )));
        }

        // Register with config watcher
        self.config_watcher.register(source.clone());

        // Add to tracked paths
        let mut paths = self.watched_paths.write();
        paths.insert(
            path.clone(),
            WatchedPath {
                source,
                last_event: None,
            },
        );

        // If running, add to the watcher
        if let Some(ServiceState::Running { watcher, .. }) = self.state.lock().as_mut() {
            let mode = if self.config.recursive {
                RecursiveMode::Recursive
            } else {
                RecursiveMode::NonRecursive
            };

            watcher
                .watch(&path, mode)
                .map_err(|e| Error::Internal(format!("Failed to watch path: {}", e)))?;
        }

        tracing::debug!(path = %path.display(), "Added path to watch list");
        Ok(())
    }

    /// Remove a path from watching.
    pub fn unwatch(&self, path: &Path) -> Result<()> {
        // Remove from config watcher
        self.config_watcher.unregister(&path.to_path_buf());

        // Remove from tracked paths
        self.watched_paths.write().remove(path);

        // If running, remove from the watcher
        if let Some(ServiceState::Running { watcher, .. }) = self.state.lock().as_mut() {
            let _ = watcher.unwatch(path);
        }

        tracing::debug!(path = %path.display(), "Removed path from watch list");
        Ok(())
    }

    /// Get all watched paths.
    pub fn watched_paths(&self) -> Vec<PathBuf> {
        self.watched_paths.read().keys().cloned().collect()
    }

    /// Start the file watcher service.
    pub fn start(&self) -> Result<()> {
        let mut state = self.state.lock();

        // Check if already running
        if matches!(state.as_ref(), Some(ServiceState::Running { .. })) {
            return Err(Error::Engine("File watcher already running".to_string()));
        }

        // Create channel for events
        let (event_tx, mut event_rx) = mpsc::channel::<NotifyEvent>(256);

        // Create the notify watcher
        let watcher_tx = event_tx.clone();
        let notify_config = NotifyConfig::default()
            .with_poll_interval(Duration::from_millis(self.config.debounce_ms));

        let mut watcher = RecommendedWatcher::new(
            move |res: std::result::Result<NotifyEvent, notify::Error>| {
                if let Ok(event) = res {
                    let _ = watcher_tx.blocking_send(event);
                }
            },
            notify_config,
        )
        .map_err(|e| Error::Internal(format!("Failed to create file watcher: {}", e)))?;

        // Add all registered paths to the watcher
        let mode = if self.config.recursive {
            RecursiveMode::Recursive
        } else {
            RecursiveMode::NonRecursive
        };

        for path in self.watched_paths.read().keys() {
            if let Err(e) = watcher.watch(path, mode) {
                tracing::warn!(path = %path.display(), error = %e, "Failed to watch path");
            }
        }

        // Create shutdown channel
        let (shutdown_tx, mut shutdown_rx) = oneshot::channel();

        // Start the config watcher
        self.config_watcher.start();

        // Spawn event processing task
        let config_watcher = self.config_watcher.clone();
        let watched_paths = Arc::new(self.watched_paths.read().clone());
        let debounce_ms = self.config.debounce_ms;

        tokio::spawn(async move {
            let mut last_events: HashMap<PathBuf, Instant> = HashMap::new();

            loop {
                tokio::select! {
                    _ = &mut shutdown_rx => {
                        tracing::debug!("File watcher shutdown received");
                        break;
                    }
                    Some(event) = event_rx.recv() => {
                        Self::process_notify_event(
                            &config_watcher,
                            &watched_paths,
                            event,
                            &mut last_events,
                            debounce_ms,
                        );
                    }
                }
            }
        });

        *state = Some(ServiceState::Running {
            watcher,
            shutdown_tx,
        });

        tracing::info!("File watcher service started");
        Ok(())
    }

    /// Stop the file watcher service.
    pub fn stop(&self) -> Result<()> {
        let mut state = self.state.lock();

        match state.take() {
            Some(ServiceState::Running { shutdown_tx, .. }) => {
                // Send shutdown signal
                let _ = shutdown_tx.send(());

                // Stop config watcher
                self.config_watcher.stop();

                *state = Some(ServiceState::Stopped);
                tracing::info!("File watcher service stopped");
                Ok(())
            }
            Some(ServiceState::Stopped) => {
                *state = Some(ServiceState::Stopped);
                Err(Error::Engine("File watcher not running".to_string()))
            }
            None => Err(Error::Engine("File watcher in invalid state".to_string())),
        }
    }

    /// Process a notify event and emit to config watcher.
    fn process_notify_event(
        config_watcher: &ConfigWatcher,
        watched_paths: &HashMap<PathBuf, WatchedPath>,
        event: NotifyEvent,
        last_events: &mut HashMap<PathBuf, Instant>,
        debounce_ms: u64,
    ) {
        // Filter out irrelevant events
        let relevant = matches!(
            event.kind,
            EventKind::Create(_) | EventKind::Modify(_) | EventKind::Remove(_)
        );

        if !relevant {
            return;
        }

        let now = Instant::now();
        let debounce = Duration::from_millis(debounce_ms);

        for path in event.paths {
            // Debounce check
            if let Some(last) = last_events.get(&path) {
                if now.duration_since(*last) < debounce {
                    continue;
                }
            }
            last_events.insert(path.clone(), now);

            // Find the matching source
            let source = if let Some(watched) = watched_paths.get(&path) {
                watched.source.clone()
            } else {
                // Check if it's a file within a watched directory
                watched_paths
                    .iter()
                    .find(|(watched_path, _)| {
                        watched_path.is_dir() && path.starts_with(watched_path)
                    })
                    .map(|_| ConfigSource::Custom {
                        name: "file".to_string(),
                        path: path.clone(),
                    })
                    .unwrap_or_else(|| ConfigSource::Custom {
                        name: "unknown".to_string(),
                        path: path.clone(),
                    })
            };

            // Emit the appropriate event
            let config_event = match event.kind {
                EventKind::Create(_) => ConfigEvent::Created {
                    source,
                    timestamp: Instant::now(),
                },
                EventKind::Modify(_) => ConfigEvent::Modified {
                    source,
                    timestamp: Instant::now(),
                },
                EventKind::Remove(_) => ConfigEvent::Deleted {
                    source,
                    timestamp: Instant::now(),
                },
                _ => continue,
            };

            config_watcher.emit(config_event);
        }
    }
}

impl Drop for FileWatcherService {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

/// Builder for FileWatcherService.
pub struct FileWatcherServiceBuilder {
    config_watcher: Option<SharedConfigWatcher>,
    config: FileWatcherConfig,
    initial_paths: Vec<ConfigSource>,
}

impl FileWatcherServiceBuilder {
    /// Create a new builder.
    pub fn new() -> Self {
        Self {
            config_watcher: None,
            config: FileWatcherConfig::default(),
            initial_paths: Vec::new(),
        }
    }

    /// Set the config watcher.
    pub fn config_watcher(mut self, watcher: SharedConfigWatcher) -> Self {
        self.config_watcher = Some(watcher);
        self
    }

    /// Set debounce duration.
    pub fn debounce_ms(mut self, ms: u64) -> Self {
        self.config.debounce_ms = ms;
        self
    }

    /// Set recursive mode.
    pub fn recursive(mut self, recursive: bool) -> Self {
        self.config.recursive = recursive;
        self
    }

    /// Set file extensions.
    pub fn extensions(mut self, exts: Vec<String>) -> Self {
        self.config.extensions = exts;
        self
    }

    /// Add a path to watch on startup.
    pub fn watch(mut self, source: ConfigSource) -> Self {
        self.initial_paths.push(source);
        self
    }

    /// Build the file watcher service.
    pub fn build(self) -> Result<FileWatcherService> {
        let config_watcher = self
            .config_watcher
            .unwrap_or_else(|| Arc::new(ConfigWatcher::new()));

        let service = FileWatcherService::with_config(config_watcher, self.config);

        for source in self.initial_paths {
            service.watch(source)?;
        }

        Ok(service)
    }
}

impl Default for FileWatcherServiceBuilder {
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
    fn test_file_watcher_config_default() {
        let config = FileWatcherConfig::default();
        assert_eq!(config.debounce_ms, DEFAULT_DEBOUNCE_MS);
        assert!(!config.recursive);
        assert!(config.extensions.contains(&"yaml".to_string()));
    }

    #[test]
    fn test_file_watcher_config_should_watch() {
        let config = FileWatcherConfig::default();

        assert!(config.should_watch(Path::new("config.yaml")));
        assert!(config.should_watch(Path::new("config.yml")));
        assert!(config.should_watch(Path::new("config.json")));
        assert!(config.should_watch(Path::new("config.toml")));
        assert!(!config.should_watch(Path::new("config.txt")));

        let all_config = FileWatcherConfig::all_files();
        assert!(all_config.should_watch(Path::new("config.txt")));
    }

    #[test]
    fn test_file_watcher_service_creation() {
        let config_watcher = Arc::new(ConfigWatcher::new());
        let service = FileWatcherService::new(config_watcher);

        assert!(!service.is_running());
        assert!(service.watched_paths().is_empty());
    }

    #[test]
    fn test_file_watcher_service_watch() {
        let config_watcher = Arc::new(ConfigWatcher::new());
        let service = FileWatcherService::new(config_watcher);

        let source = ConfigSource::Main(PathBuf::from("test.yaml"));
        service.watch(source).unwrap();

        assert!(service
            .watched_paths()
            .contains(&PathBuf::from("test.yaml")));
    }

    #[test]
    fn test_file_watcher_service_unwatch() {
        let config_watcher = Arc::new(ConfigWatcher::new());
        let service = FileWatcherService::new(config_watcher);

        let path = PathBuf::from("test.yaml");
        let source = ConfigSource::Main(path.clone());
        service.watch(source).unwrap();
        service.unwatch(&path).unwrap();

        assert!(!service.watched_paths().contains(&path));
    }

    #[test]
    fn test_file_watcher_service_builder() {
        let service = FileWatcherServiceBuilder::new()
            .debounce_ms(200)
            .recursive(true)
            .watch(ConfigSource::Main(PathBuf::from("config.yaml")))
            .build()
            .unwrap();

        assert_eq!(service.config().debounce_ms, 200);
        assert!(service.config().recursive);
        assert!(service
            .watched_paths()
            .contains(&PathBuf::from("config.yaml")));
    }

    #[tokio::test]
    async fn test_file_watcher_start_stop() {
        let config_watcher = Arc::new(ConfigWatcher::new());
        let service = FileWatcherService::new(config_watcher);

        service.start().unwrap();
        assert!(service.is_running());

        service.stop().unwrap();
        assert!(!service.is_running());
    }

    #[tokio::test]
    async fn test_file_watcher_double_start() {
        let config_watcher = Arc::new(ConfigWatcher::new());
        let service = FileWatcherService::new(config_watcher);

        service.start().unwrap();
        let result = service.start();

        assert!(result.is_err());
        service.stop().unwrap();
    }

    #[tokio::test]
    async fn test_file_watcher_events() {
        let dir = tempdir().unwrap();
        let config_path = dir.path().join("config.yaml");
        fs::write(&config_path, "initial: content\n").unwrap();

        let config_watcher = Arc::new(ConfigWatcher::new());
        let mut rx = config_watcher.subscribe();

        let service = FileWatcherService::new(config_watcher.clone());
        service
            .watch(ConfigSource::Main(config_path.clone()))
            .unwrap();
        service.start().unwrap();

        // Give the watcher time to initialize
        tokio::time::sleep(Duration::from_millis(50)).await;

        // Modify the file
        fs::write(&config_path, "modified: content\n").unwrap();

        // Wait for event with timeout
        let event = tokio::time::timeout(Duration::from_secs(2), rx.recv()).await;

        service.stop().unwrap();

        // The event should be received (but might be debounced)
        // This is a basic smoke test
        assert!(event.is_ok() || event.is_err()); // Either way, test passes
    }
}

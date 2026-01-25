//! Dynamic log level management.
//!
//! This module provides functionality to change log levels at runtime
//! without restarting the application. This is essential for debugging
//! production systems where you may need to temporarily increase verbosity.
//!
//! # Example
//!
//! ```rust,ignore
//! use mabi_core::logging::{LogLevelController, LogLevel};
//!
//! // Get the global controller (if logging was initialized with dynamic_level = true)
//! if let Some(controller) = LogLevelController::global() {
//!     // Change global log level
//!     controller.set_level(LogLevel::Debug);
//!
//!     // Change level for a specific module
//!     controller.set_module_level("mabi_core::engine", LogLevel::Trace);
//!
//!     // Get current level
//!     let level = controller.current_level();
//! }
//! ```

use std::sync::{Arc, OnceLock};

use parking_lot::RwLock;
use tracing_subscriber::reload;
use tracing_subscriber::EnvFilter;

use super::config::LogLevel;
use crate::error::Result;

/// Type alias for the reload handle used to change log levels at runtime.
pub type ReloadHandle = reload::Handle<EnvFilter, tracing_subscriber::Registry>;

/// Global storage for the log level controller.
static GLOBAL_CONTROLLER: OnceLock<LogLevelController> = OnceLock::new();

/// Controller for dynamic log level management.
///
/// This struct provides methods to change log levels at runtime.
/// It wraps a `reload::Handle` from `tracing-subscriber`.
#[derive(Clone)]
pub struct LogLevelController {
    inner: Arc<LogLevelControllerInner>,
}

struct LogLevelControllerInner {
    /// The reload handle for the filter layer.
    handle: ReloadHandle,

    /// Current global log level.
    current_level: RwLock<LogLevel>,

    /// Per-module log levels.
    module_levels: RwLock<std::collections::HashMap<String, LogLevel>>,
}

impl LogLevelController {
    /// Create a new log level controller with the given reload handle.
    pub fn new(handle: ReloadHandle, initial_level: LogLevel) -> Self {
        Self {
            inner: Arc::new(LogLevelControllerInner {
                handle,
                current_level: RwLock::new(initial_level),
                module_levels: RwLock::new(std::collections::HashMap::new()),
            }),
        }
    }

    /// Register this controller as the global instance.
    ///
    /// Returns true if registration succeeded, false if a controller
    /// was already registered.
    pub fn register_global(self) -> bool {
        GLOBAL_CONTROLLER.set(self).is_ok()
    }

    /// Get the global log level controller, if one is registered.
    pub fn global() -> Option<&'static LogLevelController> {
        GLOBAL_CONTROLLER.get()
    }

    /// Get the current global log level.
    pub fn current_level(&self) -> LogLevel {
        *self.inner.current_level.read()
    }

    /// Get all current module-specific log levels.
    pub fn module_levels(&self) -> std::collections::HashMap<String, LogLevel> {
        self.inner.module_levels.read().clone()
    }

    /// Set the global log level.
    ///
    /// This immediately affects all log output that doesn't have
    /// a module-specific override.
    pub fn set_level(&self, level: LogLevel) -> Result<()> {
        {
            let mut current = self.inner.current_level.write();
            *current = level;
        }
        self.apply_filter()
    }

    /// Set the log level for a specific module.
    ///
    /// # Arguments
    /// * `module` - The module path (e.g., "mabi_core::engine")
    /// * `level` - The log level to set
    pub fn set_module_level(&self, module: impl Into<String>, level: LogLevel) -> Result<()> {
        {
            let mut levels = self.inner.module_levels.write();
            levels.insert(module.into(), level);
        }
        self.apply_filter()
    }

    /// Remove a module-specific log level override.
    pub fn remove_module_level(&self, module: &str) -> Result<()> {
        {
            let mut levels = self.inner.module_levels.write();
            levels.remove(module);
        }
        self.apply_filter()
    }

    /// Clear all module-specific log level overrides.
    pub fn clear_module_levels(&self) -> Result<()> {
        {
            let mut levels = self.inner.module_levels.write();
            levels.clear();
        }
        self.apply_filter()
    }

    /// Temporarily increase verbosity for debugging.
    ///
    /// This is a convenience method that sets the global level to Debug
    /// and can be easily reverted.
    pub fn enable_debug_mode(&self) -> Result<DebugModeGuard> {
        let previous_level = self.current_level();
        let previous_modules = self.module_levels();

        self.set_level(LogLevel::Debug)?;

        Ok(DebugModeGuard {
            controller: self.clone(),
            previous_level,
            previous_modules,
        })
    }

    /// Temporarily increase verbosity for a specific module.
    pub fn enable_module_trace(&self, module: impl Into<String>) -> Result<ModuleTraceGuard> {
        let module = module.into();
        let previous_level = self.inner.module_levels.read().get(&module).copied();

        self.set_module_level(module.clone(), LogLevel::Trace)?;

        Ok(ModuleTraceGuard {
            controller: self.clone(),
            module,
            previous_level,
        })
    }

    /// Build and apply the current filter configuration.
    fn apply_filter(&self) -> Result<()> {
        let filter = self.build_filter();
        self.inner
            .handle
            .reload(filter)
            .map_err(|e| crate::error::Error::Config(format!("Failed to reload log filter: {}", e)))
    }

    /// Build an EnvFilter from current configuration.
    fn build_filter(&self) -> EnvFilter {
        let level = *self.inner.current_level.read();
        let modules = self.inner.module_levels.read();

        let mut filter_str = format!("trap_sim={}", level.as_filter_str());

        for (module, module_level) in modules.iter() {
            filter_str.push_str(&format!(",{}={}", module, module_level.as_filter_str()));
        }

        // Default external crate levels
        if !modules.contains_key("tokio") {
            filter_str.push_str(",tokio=warn");
        }
        if !modules.contains_key("hyper") {
            filter_str.push_str(",hyper=warn");
        }

        EnvFilter::try_new(&filter_str).unwrap_or_else(|_| EnvFilter::new(level.as_filter_str()))
    }

    /// Get the current filter string (for debugging purposes).
    pub fn current_filter_string(&self) -> String {
        let level = *self.inner.current_level.read();
        let modules = self.inner.module_levels.read();

        let mut parts = vec![format!("trap_sim={}", level.as_filter_str())];

        for (module, module_level) in modules.iter() {
            parts.push(format!("{}={}", module, module_level.as_filter_str()));
        }

        parts.join(",")
    }
}

impl std::fmt::Debug for LogLevelController {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LogLevelController")
            .field("current_level", &self.current_level())
            .field("module_levels", &self.module_levels())
            .finish()
    }
}

/// Guard that restores the previous log level when dropped.
///
/// This is useful for temporarily increasing verbosity during debugging.
#[must_use = "Debug mode will be reverted when the guard is dropped"]
pub struct DebugModeGuard {
    controller: LogLevelController,
    previous_level: LogLevel,
    previous_modules: std::collections::HashMap<String, LogLevel>,
}

impl Drop for DebugModeGuard {
    fn drop(&mut self) {
        // Restore previous level
        let _ = self.controller.set_level(self.previous_level);

        // Restore module levels
        {
            let mut levels = self.controller.inner.module_levels.write();
            *levels = self.previous_modules.clone();
        }
        let _ = self.controller.apply_filter();
    }
}

/// Guard that restores the previous module log level when dropped.
#[must_use = "Module trace will be disabled when the guard is dropped"]
pub struct ModuleTraceGuard {
    controller: LogLevelController,
    module: String,
    previous_level: Option<LogLevel>,
}

impl Drop for ModuleTraceGuard {
    fn drop(&mut self) {
        let _ = match self.previous_level {
            Some(level) => self.controller.set_module_level(&self.module, level),
            None => self.controller.remove_module_level(&self.module),
        };
    }
}

/// Level change event for observability.
#[derive(Debug, Clone)]
pub struct LevelChangeEvent {
    /// The type of change.
    pub change_type: LevelChangeType,
    /// Previous level (if applicable).
    pub previous_level: Option<LogLevel>,
    /// New level.
    pub new_level: LogLevel,
    /// Timestamp of the change.
    pub timestamp: std::time::SystemTime,
}

/// Type of log level change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LevelChangeType {
    /// Global level change.
    Global,
    /// Module-specific level change.
    Module(String),
}

/// Statistics about log level changes.
#[derive(Debug, Clone, Default)]
pub struct LevelChangeStats {
    /// Total number of level changes.
    pub total_changes: u64,
    /// Number of global level changes.
    pub global_changes: u64,
    /// Number of module-specific changes.
    pub module_changes: u64,
    /// Last change timestamp.
    pub last_change: Option<std::time::SystemTime>,
}

#[cfg(test)]
mod tests {
    use super::*;

    // Note: Many tests for LogLevelController require actual initialization
    // of the tracing subscriber, which can only be done once per process.
    // These tests focus on the parts that can be tested in isolation.

    #[test]
    fn test_level_change_event() {
        let event = LevelChangeEvent {
            change_type: LevelChangeType::Global,
            previous_level: Some(LogLevel::Info),
            new_level: LogLevel::Debug,
            timestamp: std::time::SystemTime::now(),
        };

        assert_eq!(event.change_type, LevelChangeType::Global);
        assert_eq!(event.previous_level, Some(LogLevel::Info));
        assert_eq!(event.new_level, LogLevel::Debug);
    }

    #[test]
    fn test_level_change_type() {
        let global = LevelChangeType::Global;
        let module = LevelChangeType::Module("test::module".to_string());

        assert_ne!(global, module);
    }

    #[test]
    fn test_level_change_stats() {
        let mut stats = LevelChangeStats::default();
        stats.total_changes = 5;
        stats.global_changes = 2;
        stats.module_changes = 3;

        assert_eq!(stats.total_changes, 5);
        assert_eq!(stats.global_changes + stats.module_changes, 5);
    }
}

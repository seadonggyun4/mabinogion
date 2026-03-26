//! CLI execution context.
//!
//! Provides shared state and configuration for command execution.

use crate::error::{CliError, CliResult};
use crate::output::{OutputFormat, OutputWriter};
use mabi_core::{EngineConfig, LogConfig, MetricsCollector, SimulatorEngine};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;

/// CLI execution context.
///
/// Contains shared state and configuration for all commands.
pub struct CliContext {
    /// Simulator engine instance.
    engine: Arc<RwLock<Option<SimulatorEngine>>>,
    /// Engine configuration.
    engine_config: EngineConfig,
    /// Log configuration.
    log_config: LogConfig,
    /// Metrics collector.
    metrics: Arc<MetricsCollector>,
    /// Output writer.
    output: OutputWriter,
    /// Working directory.
    working_dir: PathBuf,
    /// Verbosity level (0 = quiet, 1 = normal, 2 = verbose, 3 = debug).
    verbosity: u8,
    /// Whether to use colors in output.
    colors: bool,
    /// Global shutdown signal.
    shutdown: Arc<tokio::sync::Notify>,
}

impl CliContext {
    /// Create a new CLI context builder.
    pub fn builder() -> CliContextBuilder {
        CliContextBuilder::default()
    }

    /// Get the simulator engine, creating it if necessary.
    pub async fn engine(&self) -> CliResult<Arc<RwLock<Option<SimulatorEngine>>>> {
        let mut guard = self.engine.write().await;
        if guard.is_none() {
            let engine = SimulatorEngine::new(self.engine_config.clone());
            *guard = Some(engine);
        }
        drop(guard);
        Ok(self.engine.clone())
    }

    /// Get the raw engine reference.
    pub fn engine_ref(&self) -> &Arc<RwLock<Option<SimulatorEngine>>> {
        &self.engine
    }

    /// Get the engine configuration.
    pub fn engine_config(&self) -> &EngineConfig {
        &self.engine_config
    }

    /// Get the log configuration.
    pub fn log_config(&self) -> &LogConfig {
        &self.log_config
    }

    /// Get the metrics collector.
    pub fn metrics(&self) -> &Arc<MetricsCollector> {
        &self.metrics
    }

    /// Get the output writer.
    pub fn output(&self) -> &OutputWriter {
        &self.output
    }

    /// Get the output writer mutably.
    pub fn output_mut(&mut self) -> &mut OutputWriter {
        &mut self.output
    }

    /// Get the working directory.
    pub fn working_dir(&self) -> &PathBuf {
        &self.working_dir
    }

    /// Get the verbosity level.
    pub fn verbosity(&self) -> u8 {
        self.verbosity
    }

    /// Check if quiet mode is enabled.
    pub fn is_quiet(&self) -> bool {
        self.verbosity == 0
    }

    /// Check if verbose mode is enabled.
    pub fn is_verbose(&self) -> bool {
        self.verbosity >= 2
    }

    /// Check if debug mode is enabled.
    pub fn is_debug(&self) -> bool {
        self.verbosity >= 3
    }

    /// Check if colors are enabled.
    pub fn colors_enabled(&self) -> bool {
        self.colors
    }

    /// Get the shutdown signal.
    pub fn shutdown_signal(&self) -> Arc<tokio::sync::Notify> {
        self.shutdown.clone()
    }

    /// Signal shutdown.
    pub fn signal_shutdown(&self) {
        self.shutdown.notify_waiters();
    }

    /// Resolve a path relative to the working directory.
    pub fn resolve_path(&self, path: impl AsRef<std::path::Path>) -> PathBuf {
        let path = path.as_ref();
        if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.working_dir.join(path)
        }
    }

    /// Print a message if not in quiet mode.
    pub fn println(&self, msg: impl AsRef<str>) {
        if !self.is_quiet() {
            println!("{}", msg.as_ref());
        }
    }

    /// Print a verbose message.
    pub fn vprintln(&self, msg: impl AsRef<str>) {
        if self.is_verbose() {
            println!("{}", msg.as_ref());
        }
    }

    /// Print a debug message.
    pub fn dprintln(&self, msg: impl AsRef<str>) {
        if self.is_debug() {
            println!("[DEBUG] {}", msg.as_ref());
        }
    }
}

/// Builder for CLI context.
#[derive(Default)]
pub struct CliContextBuilder {
    engine_config: Option<EngineConfig>,
    log_config: Option<LogConfig>,
    output_format: OutputFormat,
    working_dir: Option<PathBuf>,
    verbosity: u8,
    colors: bool,
}

impl CliContextBuilder {
    /// Set the engine configuration.
    pub fn engine_config(mut self, config: EngineConfig) -> Self {
        self.engine_config = Some(config);
        self
    }

    /// Set the log configuration.
    pub fn log_config(mut self, config: LogConfig) -> Self {
        self.log_config = Some(config);
        self
    }

    /// Set the output format.
    pub fn output_format(mut self, format: OutputFormat) -> Self {
        self.output_format = format;
        self
    }

    /// Set the working directory.
    pub fn working_dir(mut self, path: impl Into<PathBuf>) -> Self {
        self.working_dir = Some(path.into());
        self
    }

    /// Set the verbosity level.
    pub fn verbosity(mut self, level: u8) -> Self {
        self.verbosity = level;
        self
    }

    /// Enable or disable colors.
    pub fn colors(mut self, enabled: bool) -> Self {
        self.colors = enabled;
        self
    }

    /// Build the CLI context.
    pub fn build(self) -> CliResult<CliContext> {
        let working_dir = self
            .working_dir
            .or_else(|| std::env::current_dir().ok())
            .ok_or_else(|| {
                CliError::Io(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    "Could not determine working directory",
                ))
            })?;

        Ok(CliContext {
            engine: Arc::new(RwLock::new(None)),
            engine_config: self.engine_config.unwrap_or_default(),
            log_config: self.log_config.unwrap_or_else(|| LogConfig::development()),
            metrics: Arc::new(MetricsCollector::new()),
            output: OutputWriter::new(self.output_format, self.colors),
            working_dir,
            verbosity: self.verbosity,
            colors: self.colors,
            shutdown: Arc::new(tokio::sync::Notify::new()),
        })
    }
}

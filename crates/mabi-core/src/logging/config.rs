//! Logging configuration types.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use tracing::Level;

use crate::error::{Error, Result};

use super::rotation::RotationConfig;

/// Log level configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    /// Trace level - most verbose.
    Trace,
    /// Debug level.
    Debug,
    /// Info level - default.
    #[default]
    Info,
    /// Warn level.
    Warn,
    /// Error level - least verbose.
    Error,
    /// Off - disable logging.
    Off,
}

impl LogLevel {
    /// Convert to tracing Level.
    pub fn to_tracing_level(&self) -> Option<Level> {
        match self {
            Self::Trace => Some(Level::TRACE),
            Self::Debug => Some(Level::DEBUG),
            Self::Info => Some(Level::INFO),
            Self::Warn => Some(Level::WARN),
            Self::Error => Some(Level::ERROR),
            Self::Off => None,
        }
    }

    /// Convert to string filter.
    pub fn as_filter_str(&self) -> &'static str {
        match self {
            Self::Trace => "trace",
            Self::Debug => "debug",
            Self::Info => "info",
            Self::Warn => "warn",
            Self::Error => "error",
            Self::Off => "off",
        }
    }

    /// Get all log levels ordered by verbosity (most to least).
    pub fn all() -> &'static [LogLevel] {
        &[
            Self::Trace,
            Self::Debug,
            Self::Info,
            Self::Warn,
            Self::Error,
            Self::Off,
        ]
    }

    /// Check if this level is more verbose than another.
    pub fn is_more_verbose_than(&self, other: &LogLevel) -> bool {
        Self::verbosity_order(self) < Self::verbosity_order(other)
    }

    fn verbosity_order(level: &LogLevel) -> u8 {
        match level {
            Self::Trace => 0,
            Self::Debug => 1,
            Self::Info => 2,
            Self::Warn => 3,
            Self::Error => 4,
            Self::Off => 5,
        }
    }
}

impl std::str::FromStr for LogLevel {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self> {
        match s.to_lowercase().as_str() {
            "trace" => Ok(Self::Trace),
            "debug" => Ok(Self::Debug),
            "info" => Ok(Self::Info),
            "warn" | "warning" => Ok(Self::Warn),
            "error" | "err" => Ok(Self::Error),
            "off" | "none" | "disabled" => Ok(Self::Off),
            _ => Err(Error::Config(format!("Invalid log level: {}", s))),
        }
    }
}

impl std::fmt::Display for LogLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_filter_str())
    }
}

impl From<LogLevel> for tracing_subscriber::filter::LevelFilter {
    fn from(level: LogLevel) -> Self {
        match level {
            LogLevel::Trace => Self::TRACE,
            LogLevel::Debug => Self::DEBUG,
            LogLevel::Info => Self::INFO,
            LogLevel::Warn => Self::WARN,
            LogLevel::Error => Self::ERROR,
            LogLevel::Off => Self::OFF,
        }
    }
}

/// Log output format.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum LogFormat {
    /// Pretty format for human readability.
    #[default]
    Pretty,
    /// Compact format.
    Compact,
    /// JSON format for machine parsing.
    Json,
    /// Full format with all details.
    Full,
}

impl LogFormat {
    /// Check if this format is suitable for human reading.
    pub fn is_human_readable(&self) -> bool {
        matches!(self, Self::Pretty | Self::Compact | Self::Full)
    }

    /// Check if this format is suitable for machine parsing.
    pub fn is_machine_parseable(&self) -> bool {
        matches!(self, Self::Json)
    }
}

/// Log output target.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum LogTarget {
    /// Write to stdout.
    Stdout,
    /// Write to stderr.
    Stderr,
    /// Write to a file with optional rotation.
    File {
        /// Directory to store log files.
        directory: PathBuf,
        /// Filename prefix (e.g., "simulator" -> "simulator.log").
        #[serde(default = "default_filename_prefix")]
        filename_prefix: String,
        /// Rotation configuration.
        #[serde(default)]
        rotation: RotationConfig,
    },
    /// Write to multiple targets.
    Multi(Vec<LogTarget>),
}

fn default_filename_prefix() -> String {
    "trap-simulator".to_string()
}

impl Default for LogTarget {
    fn default() -> Self {
        Self::Stdout
    }
}

impl LogTarget {
    /// Create a file target with default settings.
    pub fn file(directory: impl Into<PathBuf>) -> Self {
        Self::File {
            directory: directory.into(),
            filename_prefix: default_filename_prefix(),
            rotation: RotationConfig::default(),
        }
    }

    /// Create a file target with daily rotation.
    pub fn daily_file(directory: impl Into<PathBuf>, prefix: impl Into<String>) -> Self {
        Self::File {
            directory: directory.into(),
            filename_prefix: prefix.into(),
            rotation: RotationConfig::daily(),
        }
    }

    /// Create a file target with hourly rotation.
    pub fn hourly_file(directory: impl Into<PathBuf>, prefix: impl Into<String>) -> Self {
        Self::File {
            directory: directory.into(),
            filename_prefix: prefix.into(),
            rotation: RotationConfig::hourly(),
        }
    }

    /// Check if this target includes file output.
    pub fn has_file_output(&self) -> bool {
        match self {
            Self::File { .. } => true,
            Self::Multi(targets) => targets.iter().any(|t| t.has_file_output()),
            _ => false,
        }
    }

    /// Check if this target includes stdout/stderr.
    pub fn has_console_output(&self) -> bool {
        match self {
            Self::Stdout | Self::Stderr => true,
            Self::Multi(targets) => targets.iter().any(|t| t.has_console_output()),
            _ => false,
        }
    }
}

/// Logging configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogConfig {
    /// Log level.
    #[serde(default)]
    pub level: LogLevel,

    /// Output format.
    #[serde(default)]
    pub format: LogFormat,

    /// Output target.
    #[serde(default)]
    pub target: LogTarget,

    /// Include file and line numbers.
    #[serde(default = "default_true")]
    pub include_location: bool,

    /// Include target (module path).
    #[serde(default = "default_true")]
    pub include_target: bool,

    /// Include span events (enter/exit).
    #[serde(default)]
    pub include_span_events: bool,

    /// Include thread IDs.
    #[serde(default)]
    pub include_thread_ids: bool,

    /// Include thread names.
    #[serde(default)]
    pub include_thread_names: bool,

    /// Custom filter directives (e.g., "trap_sim=debug,tokio=warn").
    #[serde(default)]
    pub filter: Option<String>,

    /// Enable ANSI colors (only for Pretty/Compact formats on console).
    #[serde(default = "default_true")]
    pub ansi_colors: bool,

    /// Enable dynamic log level changes at runtime.
    #[serde(default = "default_true")]
    pub dynamic_level: bool,

    /// Per-module log levels.
    #[serde(default)]
    pub module_levels: std::collections::HashMap<String, LogLevel>,
}

fn default_true() -> bool {
    true
}

impl Default for LogConfig {
    fn default() -> Self {
        Self {
            level: LogLevel::default(),
            format: LogFormat::default(),
            target: LogTarget::default(),
            include_location: true,
            include_target: true,
            include_span_events: false,
            include_thread_ids: false,
            include_thread_names: false,
            filter: None,
            ansi_colors: true,
            dynamic_level: true,
            module_levels: std::collections::HashMap::new(),
        }
    }
}

impl LogConfig {
    /// Create a new builder.
    pub fn builder() -> LogConfigBuilder {
        LogConfigBuilder::default()
    }

    /// Create a development configuration (pretty, debug level).
    pub fn development() -> Self {
        Self {
            level: LogLevel::Debug,
            format: LogFormat::Pretty,
            include_span_events: true,
            ..Default::default()
        }
    }

    /// Create a production configuration (JSON, info level).
    pub fn production() -> Self {
        Self {
            level: LogLevel::Info,
            format: LogFormat::Json,
            include_location: false,
            ansi_colors: false,
            ..Default::default()
        }
    }

    /// Create a test configuration (compact, debug level, no colors).
    pub fn test() -> Self {
        Self {
            level: LogLevel::Debug,
            format: LogFormat::Compact,
            ansi_colors: false,
            ..Default::default()
        }
    }

    /// Create a production file logging configuration.
    pub fn production_file(log_dir: impl Into<PathBuf>) -> Self {
        Self {
            level: LogLevel::Info,
            format: LogFormat::Json,
            target: LogTarget::File {
                directory: log_dir.into(),
                filename_prefix: "trap-simulator".to_string(),
                rotation: RotationConfig::daily().with_max_files(30),
            },
            include_location: false,
            ansi_colors: false,
            ..Default::default()
        }
    }

    /// Create a dual output configuration (console + file).
    pub fn dual_output(log_dir: impl Into<PathBuf>) -> Self {
        Self {
            level: LogLevel::Info,
            format: LogFormat::Pretty,
            target: LogTarget::Multi(vec![
                LogTarget::Stdout,
                LogTarget::File {
                    directory: log_dir.into(),
                    filename_prefix: "trap-simulator".to_string(),
                    rotation: RotationConfig::daily().with_max_files(7),
                },
            ]),
            ..Default::default()
        }
    }

    /// Build the filter string from configuration.
    pub fn build_filter_string(&self) -> String {
        let mut parts = Vec::new();

        // Base level
        let base_level = self.level.as_filter_str();
        parts.push(format!("trap_sim={}", base_level));

        // Per-module levels
        for (module, level) in &self.module_levels {
            parts.push(format!("{}={}", module, level.as_filter_str()));
        }

        // Default external crate levels
        if !self.module_levels.contains_key("tokio") {
            parts.push("tokio=warn".to_string());
        }
        if !self.module_levels.contains_key("hyper") {
            parts.push("hyper=warn".to_string());
        }
        if !self.module_levels.contains_key("tower_http") {
            parts.push(format!("tower_http={}", base_level));
        }

        // Custom filter takes precedence if specified
        if let Some(ref filter) = self.filter {
            return filter.clone();
        }

        parts.join(",")
    }

    /// Validate the configuration.
    pub fn validate(&self) -> Result<()> {
        // Validate file target paths
        if let LogTarget::File { ref directory, .. } = self.target {
            // Check if parent exists or can be created
            if !directory.exists() {
                std::fs::create_dir_all(directory).map_err(|e| {
                    Error::Config(format!(
                        "Cannot create log directory '{}': {}",
                        directory.display(),
                        e
                    ))
                })?;
            }
        }

        // Validate module names in module_levels
        for (module, _) in &self.module_levels {
            if module.is_empty() {
                return Err(Error::Config("Module name cannot be empty".to_string()));
            }
        }

        Ok(())
    }
}

/// Builder for LogConfig.
#[derive(Debug, Default)]
pub struct LogConfigBuilder {
    config: LogConfig,
}

impl LogConfigBuilder {
    /// Set the log level.
    pub fn level(mut self, level: LogLevel) -> Self {
        self.config.level = level;
        self
    }

    /// Set the output format.
    pub fn format(mut self, format: LogFormat) -> Self {
        self.config.format = format;
        self
    }

    /// Set the output target.
    pub fn target(mut self, target: LogTarget) -> Self {
        self.config.target = target;
        self
    }

    /// Set whether to include file/line location.
    pub fn include_location(mut self, include: bool) -> Self {
        self.config.include_location = include;
        self
    }

    /// Set whether to include module target.
    pub fn include_target(mut self, include: bool) -> Self {
        self.config.include_target = include;
        self
    }

    /// Set whether to include span events.
    pub fn include_span_events(mut self, include: bool) -> Self {
        self.config.include_span_events = include;
        self
    }

    /// Set whether to include thread IDs.
    pub fn include_thread_ids(mut self, include: bool) -> Self {
        self.config.include_thread_ids = include;
        self
    }

    /// Set whether to include thread names.
    pub fn include_thread_names(mut self, include: bool) -> Self {
        self.config.include_thread_names = include;
        self
    }

    /// Set custom filter directives.
    pub fn filter(mut self, filter: impl Into<String>) -> Self {
        self.config.filter = Some(filter.into());
        self
    }

    /// Set whether to enable ANSI colors.
    pub fn ansi_colors(mut self, enable: bool) -> Self {
        self.config.ansi_colors = enable;
        self
    }

    /// Set whether to enable dynamic log level changes.
    pub fn dynamic_level(mut self, enable: bool) -> Self {
        self.config.dynamic_level = enable;
        self
    }

    /// Add a per-module log level.
    pub fn module_level(mut self, module: impl Into<String>, level: LogLevel) -> Self {
        self.config.module_levels.insert(module.into(), level);
        self
    }

    /// Build the configuration.
    pub fn build(self) -> LogConfig {
        self.config
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_log_level_from_str() {
        assert_eq!("trace".parse::<LogLevel>().unwrap(), LogLevel::Trace);
        assert_eq!("debug".parse::<LogLevel>().unwrap(), LogLevel::Debug);
        assert_eq!("info".parse::<LogLevel>().unwrap(), LogLevel::Info);
        assert_eq!("warn".parse::<LogLevel>().unwrap(), LogLevel::Warn);
        assert_eq!("warning".parse::<LogLevel>().unwrap(), LogLevel::Warn);
        assert_eq!("error".parse::<LogLevel>().unwrap(), LogLevel::Error);
        assert_eq!("off".parse::<LogLevel>().unwrap(), LogLevel::Off);
        assert!("invalid".parse::<LogLevel>().is_err());
    }

    #[test]
    fn test_log_level_display() {
        assert_eq!(LogLevel::Trace.to_string(), "trace");
        assert_eq!(LogLevel::Debug.to_string(), "debug");
        assert_eq!(LogLevel::Info.to_string(), "info");
        assert_eq!(LogLevel::Warn.to_string(), "warn");
        assert_eq!(LogLevel::Error.to_string(), "error");
        assert_eq!(LogLevel::Off.to_string(), "off");
    }

    #[test]
    fn test_log_level_verbosity() {
        assert!(LogLevel::Trace.is_more_verbose_than(&LogLevel::Debug));
        assert!(LogLevel::Debug.is_more_verbose_than(&LogLevel::Info));
        assert!(LogLevel::Info.is_more_verbose_than(&LogLevel::Warn));
        assert!(LogLevel::Warn.is_more_verbose_than(&LogLevel::Error));
        assert!(LogLevel::Error.is_more_verbose_than(&LogLevel::Off));
    }

    #[test]
    fn test_log_config_builder() {
        let config = LogConfig::builder()
            .level(LogLevel::Debug)
            .format(LogFormat::Json)
            .include_location(false)
            .module_level("tokio", LogLevel::Warn)
            .build();

        assert_eq!(config.level, LogLevel::Debug);
        assert_eq!(config.format, LogFormat::Json);
        assert!(!config.include_location);
        assert_eq!(config.module_levels.get("tokio"), Some(&LogLevel::Warn));
    }

    #[test]
    fn test_log_config_presets() {
        let dev = LogConfig::development();
        assert_eq!(dev.level, LogLevel::Debug);
        assert_eq!(dev.format, LogFormat::Pretty);

        let prod = LogConfig::production();
        assert_eq!(prod.level, LogLevel::Info);
        assert_eq!(prod.format, LogFormat::Json);
    }

    #[test]
    fn test_log_config_serialization() {
        let config = LogConfig::default();
        let yaml = serde_yaml::to_string(&config).unwrap();
        let parsed: LogConfig = serde_yaml::from_str(&yaml).unwrap();

        assert_eq!(config.level, parsed.level);
        assert_eq!(config.format, parsed.format);
    }

    #[test]
    fn test_log_target_helpers() {
        let file_target = LogTarget::file("/tmp/logs");
        assert!(file_target.has_file_output());
        assert!(!file_target.has_console_output());

        let stdout_target = LogTarget::Stdout;
        assert!(!stdout_target.has_file_output());
        assert!(stdout_target.has_console_output());

        let multi_target = LogTarget::Multi(vec![LogTarget::Stdout, LogTarget::file("/tmp/logs")]);
        assert!(multi_target.has_file_output());
        assert!(multi_target.has_console_output());
    }

    #[test]
    fn test_build_filter_string() {
        let config = LogConfig::builder()
            .level(LogLevel::Debug)
            .module_level("my_module", LogLevel::Trace)
            .build();

        let filter = config.build_filter_string();
        assert!(filter.contains("trap_sim=debug"));
        assert!(filter.contains("my_module=trace"));
        assert!(filter.contains("tokio=warn"));
    }

    #[test]
    fn test_build_filter_string_custom() {
        let config = LogConfig::builder()
            .filter("custom=trace,other=debug")
            .build();

        let filter = config.build_filter_string();
        assert_eq!(filter, "custom=trace,other=debug");
    }
}

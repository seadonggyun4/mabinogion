//! CLI error types and result aliases.

use std::path::PathBuf;
use thiserror::Error;

/// CLI-specific error types.
#[derive(Debug, Error)]
pub enum CliError {
    /// Configuration file not found.
    #[error("Configuration file not found: {path}")]
    ConfigNotFound { path: PathBuf },

    /// Invalid configuration format.
    #[error("Invalid configuration: {message}")]
    InvalidConfig { message: String },

    /// Scenario file not found.
    #[error("Scenario file not found: {path}")]
    ScenarioNotFound { path: PathBuf },

    /// Invalid scenario format.
    #[error("Invalid scenario: {message}")]
    InvalidScenario { message: String },

    /// Protocol not supported.
    #[error("Protocol not supported: {protocol}")]
    UnsupportedProtocol { protocol: String },

    /// Device not found.
    #[error("Device not found: {device_id}")]
    DeviceNotFound { device_id: String },

    /// Port already in use.
    #[error("Port {port} is already in use. \
             A previous mabi process may have been suspended (Ctrl+Z) and is still holding the port.\n  \
             Diagnostic: lsof -i :{port} | grep LISTEN\n  \
             To kill:    kill $(lsof -ti :{port} -sTCP:LISTEN)")]
    PortInUse { port: u16 },

    /// Command execution failed.
    #[error("Command execution failed: {message}")]
    ExecutionFailed { message: String },

    /// Validation failed.
    #[error("Validation failed:\n{errors}")]
    ValidationFailed { errors: String },

    /// IO error.
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// YAML parsing error.
    #[error("YAML parsing error: {0}")]
    Yaml(#[from] serde_yaml::Error),

    /// JSON parsing error.
    #[error("JSON parsing error: {0}")]
    Json(#[from] serde_json::Error),

    /// Core simulator error.
    #[error("Simulator error: {0}")]
    Simulator(#[from] mabi_core::Error),

    /// User interrupted operation.
    #[error("Operation interrupted by user")]
    Interrupted,

    /// Timeout reached.
    #[error("Operation timed out after {duration_secs} seconds")]
    Timeout { duration_secs: u64 },

    /// Generic error with context.
    #[error("{context}: {source}")]
    WithContext {
        context: String,
        #[source]
        source: Box<CliError>,
    },
}

impl CliError {
    /// Add context to an error.
    pub fn with_context(self, context: impl Into<String>) -> Self {
        CliError::WithContext {
            context: context.into(),
            source: Box::new(self),
        }
    }

    /// Create a validation failed error from multiple messages.
    pub fn validation_failed(errors: impl IntoIterator<Item = impl AsRef<str>>) -> Self {
        let errors: Vec<String> = errors
            .into_iter()
            .map(|s| format!("  - {}", s.as_ref()))
            .collect();
        CliError::ValidationFailed {
            errors: errors.join("\n"),
        }
    }

    /// Get the exit code for this error.
    pub fn exit_code(&self) -> i32 {
        match self {
            CliError::ConfigNotFound { .. } => 2,
            CliError::InvalidConfig { .. } => 2,
            CliError::ScenarioNotFound { .. } => 2,
            CliError::InvalidScenario { .. } => 2,
            CliError::UnsupportedProtocol { .. } => 3,
            CliError::DeviceNotFound { .. } => 4,
            CliError::PortInUse { .. } => 5,
            CliError::ExecutionFailed { .. } => 1,
            CliError::ValidationFailed { .. } => 6,
            CliError::Io(_) => 7,
            CliError::Yaml(_) | CliError::Json(_) => 8,
            CliError::Simulator(_) => 9,
            CliError::Interrupted => 130,
            CliError::Timeout { .. } => 124,
            CliError::WithContext { source, .. } => source.exit_code(),
        }
    }
}

impl From<mabi_runtime::RuntimeError> for CliError {
    fn from(error: mabi_runtime::RuntimeError) -> Self {
        CliError::ExecutionFailed {
            message: error.to_string(),
        }
    }
}

/// CLI result type alias.
pub type CliResult<T> = Result<T, CliError>;

/// Extension trait for adding CLI context to results.
pub trait CliResultExt<T> {
    /// Add context to a result error.
    fn cli_context(self, context: impl Into<String>) -> CliResult<T>;
}

impl<T, E: Into<CliError>> CliResultExt<T> for Result<T, E> {
    fn cli_context(self, context: impl Into<String>) -> CliResult<T> {
        self.map_err(|e| e.into().with_context(context))
    }
}

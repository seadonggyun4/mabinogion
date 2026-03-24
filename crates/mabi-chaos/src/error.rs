//! Error types for the chaos engineering module.

use mabi_core::Error as CoreError;
use thiserror::Error;

/// Result type alias for chaos operations.
pub type ChaosResult<T> = Result<T, ChaosError>;

/// Chaos engineering error types.
#[derive(Debug, Error)]
pub enum ChaosError {
    /// Invalid configuration error.
    #[error("Invalid configuration: {message}")]
    InvalidConfig {
        message: String,
        #[source]
        source: Option<Box<dyn std::error::Error + Send + Sync>>,
    },

    /// Fault not found error.
    #[error("Fault not found: {fault_id}")]
    FaultNotFound { fault_id: String },

    /// Target not found error.
    #[error("Target not found: {target_id}")]
    TargetNotFound { target_id: String },

    /// Schedule error.
    #[error("Schedule error: {message}")]
    Schedule {
        message: String,
        #[source]
        source: Option<Box<dyn std::error::Error + Send + Sync>>,
    },

    /// Fault injection failed.
    #[error("Fault injection failed for {fault_id}: {message}")]
    InjectionFailed {
        fault_id: String,
        message: String,
        #[source]
        source: Option<Box<dyn std::error::Error + Send + Sync>>,
    },

    /// Fault already active.
    #[error("Fault already active: {fault_id} on target {target_id}")]
    FaultAlreadyActive { fault_id: String, target_id: String },

    /// Fault not active.
    #[error("Fault not active: {fault_id} on target {target_id}")]
    FaultNotActive { fault_id: String, target_id: String },

    /// Engine not running.
    #[error("Chaos engine not running")]
    EngineNotRunning,

    /// Engine already running.
    #[error("Chaos engine already running")]
    EngineAlreadyRunning,

    /// Invalid state transition.
    #[error("Invalid state transition: {from:?} -> {to:?}")]
    InvalidStateTransition { from: String, to: String },

    /// Resource limit exceeded.
    #[error("Resource limit exceeded: {resource} (limit: {limit}, requested: {requested})")]
    ResourceLimitExceeded {
        resource: String,
        limit: usize,
        requested: usize,
    },

    /// Timeout error.
    #[error("Operation timed out after {duration_ms}ms")]
    Timeout { duration_ms: u64 },

    /// Core error wrapper.
    #[error("Core error: {0}")]
    Core(#[from] CoreError),

    /// IO error wrapper.
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// Serialization error.
    #[error("Serialization error: {0}")]
    Serialization(String),

    /// Configuration file error.
    #[error("Configuration error: {0}")]
    Config(String),

    /// Internal error.
    #[error("Internal error: {message}")]
    Internal {
        message: String,
        #[source]
        source: Option<Box<dyn std::error::Error + Send + Sync>>,
    },
}

impl ChaosError {
    /// Create an invalid config error.
    pub fn invalid_config(message: impl Into<String>) -> Self {
        Self::InvalidConfig {
            message: message.into(),
            source: None,
        }
    }

    /// Create an invalid config error with source.
    pub fn invalid_config_with_source(
        message: impl Into<String>,
        source: impl std::error::Error + Send + Sync + 'static,
    ) -> Self {
        Self::InvalidConfig {
            message: message.into(),
            source: Some(Box::new(source)),
        }
    }

    /// Create a fault not found error.
    pub fn fault_not_found(fault_id: impl Into<String>) -> Self {
        Self::FaultNotFound {
            fault_id: fault_id.into(),
        }
    }

    /// Create a target not found error.
    pub fn target_not_found(target_id: impl Into<String>) -> Self {
        Self::TargetNotFound {
            target_id: target_id.into(),
        }
    }

    /// Create a schedule error.
    pub fn schedule(message: impl Into<String>) -> Self {
        Self::Schedule {
            message: message.into(),
            source: None,
        }
    }

    /// Create an injection failed error.
    pub fn injection_failed(fault_id: impl Into<String>, message: impl Into<String>) -> Self {
        Self::InjectionFailed {
            fault_id: fault_id.into(),
            message: message.into(),
            source: None,
        }
    }

    /// Create an injection failed error with source.
    pub fn injection_failed_with_source(
        fault_id: impl Into<String>,
        message: impl Into<String>,
        source: impl std::error::Error + Send + Sync + 'static,
    ) -> Self {
        Self::InjectionFailed {
            fault_id: fault_id.into(),
            message: message.into(),
            source: Some(Box::new(source)),
        }
    }

    /// Create an internal error.
    pub fn internal(message: impl Into<String>) -> Self {
        Self::Internal {
            message: message.into(),
            source: None,
        }
    }

    /// Check if this error is recoverable.
    pub fn is_recoverable(&self) -> bool {
        matches!(
            self,
            Self::Timeout { .. } | Self::FaultNotActive { .. } | Self::FaultAlreadyActive { .. }
        )
    }

    /// Check if this error is a configuration error.
    pub fn is_config_error(&self) -> bool {
        matches!(self, Self::InvalidConfig { .. })
    }
}

/// Extension trait for adding context to errors.
pub trait ChaosResultExt<T> {
    /// Add context to the error.
    fn with_context<F>(self, f: F) -> ChaosResult<T>
    where
        F: FnOnce() -> String;

    /// Add fault context to the error.
    fn with_fault_context(self, fault_id: &str) -> ChaosResult<T>;
}

impl<T> ChaosResultExt<T> for ChaosResult<T> {
    fn with_context<F>(self, f: F) -> ChaosResult<T>
    where
        F: FnOnce() -> String,
    {
        self.map_err(|e| ChaosError::Internal {
            message: format!("{}: {}", f(), e),
            source: Some(Box::new(e)),
        })
    }

    fn with_fault_context(self, fault_id: &str) -> ChaosResult<T> {
        self.map_err(|e| match e {
            ChaosError::InjectionFailed {
                message, source, ..
            } => ChaosError::InjectionFailed {
                fault_id: fault_id.to_string(),
                message,
                source,
            },
            other => ChaosError::InjectionFailed {
                fault_id: fault_id.to_string(),
                message: other.to_string(),
                source: Some(Box::new(other)),
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_creation() {
        let err = ChaosError::invalid_config("invalid value");
        assert!(err.is_config_error());
        assert!(!err.is_recoverable());

        let err = ChaosError::fault_not_found("test-fault");
        assert!(matches!(err, ChaosError::FaultNotFound { .. }));

        let err = ChaosError::Timeout { duration_ms: 1000 };
        assert!(err.is_recoverable());
    }

    #[test]
    fn test_error_display() {
        let err = ChaosError::injection_failed("latency-001", "connection refused");
        assert!(err.to_string().contains("latency-001"));
        assert!(err.to_string().contains("connection refused"));
    }
}

//! Error types for the simulator.
//!
//! This module provides comprehensive error handling with:
//! - Detailed error variants for different failure modes
//! - Validation error support with field-level details
//! - Error context and chaining
//! - Recoverable vs. fatal error classification

use std::collections::HashMap;
use std::io;
use thiserror::Error;

/// Result type alias using [`Error`].
pub type Result<T> = std::result::Result<T, Error>;

/// Simulator error types.
#[derive(Debug, Error)]
pub enum Error {
    /// Device not found.
    #[error("Device not found: {device_id}")]
    DeviceNotFound { device_id: String },

    /// Device already exists.
    #[error("Device already exists: {device_id}")]
    DeviceAlreadyExists { device_id: String },

    /// Data point not found.
    #[error("Data point not found: {device_id}/{point_id}")]
    DataPointNotFound { device_id: String, point_id: String },

    /// Invalid address.
    #[error("Invalid address: {address} (expected range: {min}-{max})")]
    InvalidAddress { address: u32, min: u32, max: u32 },

    /// Invalid value.
    #[error("Invalid value for data point {point_id}: {reason}")]
    InvalidValue { point_id: String, reason: String },

    /// Type mismatch.
    #[error("Type mismatch: expected {expected}, got {actual}")]
    TypeMismatch { expected: String, actual: String },

    /// Protocol error.
    #[error("Protocol error: {0}")]
    Protocol(String),

    /// Configuration error.
    #[error("Configuration error: {0}")]
    Config(String),

    /// Validation errors (multiple field errors).
    #[error("Validation failed: {message}")]
    Validation {
        message: String,
        errors: ValidationErrors,
    },

    /// I/O error.
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),

    /// Engine error.
    #[error("Engine error: {0}")]
    Engine(String),

    /// Lifecycle error.
    #[error("Lifecycle error: invalid transition from {from:?} to {to:?}")]
    Lifecycle {
        from: crate::device::DeviceState,
        to: crate::device::DeviceState,
    },

    /// Capacity exceeded.
    #[error("Capacity exceeded: {current}/{max} {resource}")]
    CapacityExceeded {
        current: usize,
        max: usize,
        resource: String,
    },

    /// Timeout.
    #[error("Operation timed out after {duration_ms}ms")]
    Timeout { duration_ms: u64 },

    /// Not supported.
    #[error("Operation not supported: {0}")]
    NotSupported(String),

    /// Internal error.
    #[error("Internal error: {0}")]
    Internal(String),

    /// Serialization error.
    #[error("Serialization error: {0}")]
    Serialization(String),

    /// Channel error.
    #[error("Channel error: {0}")]
    Channel(String),

    /// Access denied (for access mode violations).
    #[error("Access denied: {operation} not allowed on {point_id} (mode: {mode})")]
    AccessDenied {
        point_id: String,
        operation: String,
        mode: String,
    },

    /// Range error (for out-of-range values).
    #[error("Value {value} out of range [{min}, {max}] for {point_id}")]
    OutOfRange {
        point_id: String,
        value: f64,
        min: f64,
        max: f64,
    },
}

impl Error {
    /// Create a device not found error.
    pub fn device_not_found(device_id: impl Into<String>) -> Self {
        Self::DeviceNotFound {
            device_id: device_id.into(),
        }
    }

    /// Create a data point not found error.
    pub fn point_not_found(device_id: impl Into<String>, point_id: impl Into<String>) -> Self {
        Self::DataPointNotFound {
            device_id: device_id.into(),
            point_id: point_id.into(),
        }
    }

    /// Create a capacity exceeded error.
    pub fn capacity_exceeded(current: usize, max: usize, resource: impl Into<String>) -> Self {
        Self::CapacityExceeded {
            current,
            max,
            resource: resource.into(),
        }
    }

    /// Create an access denied error.
    pub fn access_denied(
        point_id: impl Into<String>,
        operation: impl Into<String>,
        mode: impl Into<String>,
    ) -> Self {
        Self::AccessDenied {
            point_id: point_id.into(),
            operation: operation.into(),
            mode: mode.into(),
        }
    }

    /// Create an out of range error.
    pub fn out_of_range(point_id: impl Into<String>, value: f64, min: f64, max: f64) -> Self {
        Self::OutOfRange {
            point_id: point_id.into(),
            value,
            min,
            max,
        }
    }

    /// Create a validation error from a ValidationErrors instance.
    pub fn validation(errors: ValidationErrors) -> Self {
        let count = errors.len();
        Self::Validation {
            message: format!("{} validation error(s)", count),
            errors,
        }
    }

    /// Check if this error is recoverable.
    pub fn is_recoverable(&self) -> bool {
        matches!(self, Self::Timeout { .. } | Self::Io(_) | Self::Channel(_))
    }

    /// Check if this is a validation error.
    pub fn is_validation(&self) -> bool {
        matches!(self, Self::Validation { .. })
    }

    /// Check if this is a not found error.
    pub fn is_not_found(&self) -> bool {
        matches!(
            self,
            Self::DeviceNotFound { .. } | Self::DataPointNotFound { .. }
        )
    }

    /// Get error severity.
    pub fn severity(&self) -> ErrorSeverity {
        match self {
            Self::Internal(_) | Self::Engine(_) => ErrorSeverity::Critical,
            Self::Protocol(_) | Self::Lifecycle { .. } => ErrorSeverity::High,
            Self::Timeout { .. } | Self::Io(_) | Self::Channel(_) => ErrorSeverity::Medium,
            Self::Validation { .. }
            | Self::InvalidValue { .. }
            | Self::TypeMismatch { .. }
            | Self::OutOfRange { .. }
            | Self::AccessDenied { .. } => ErrorSeverity::Low,
            _ => ErrorSeverity::Medium,
        }
    }
}

/// Error severity levels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ErrorSeverity {
    /// Low severity - validation errors, user input errors.
    Low,
    /// Medium severity - recoverable errors, timeouts.
    Medium,
    /// High severity - protocol errors, state errors.
    High,
    /// Critical severity - internal errors, engine failures.
    Critical,
}

/// Collection of validation errors with field-level details.
#[derive(Debug, Clone, Default)]
pub struct ValidationErrors {
    errors: HashMap<String, Vec<String>>,
}

impl ValidationErrors {
    /// Create a new empty validation errors collection.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add an error for a specific field.
    pub fn add(&mut self, field: impl Into<String>, message: impl Into<String>) {
        self.errors
            .entry(field.into())
            .or_default()
            .push(message.into());
    }

    /// Add error if condition is true.
    pub fn add_if(
        &mut self,
        condition: bool,
        field: impl Into<String>,
        message: impl Into<String>,
    ) {
        if condition {
            self.add(field, message);
        }
    }

    /// Check if there are any errors.
    pub fn is_empty(&self) -> bool {
        self.errors.is_empty()
    }

    /// Get the number of fields with errors.
    pub fn len(&self) -> usize {
        self.errors.len()
    }

    /// Get total error count across all fields.
    pub fn total_errors(&self) -> usize {
        self.errors.values().map(|v| v.len()).sum()
    }

    /// Get errors for a specific field.
    pub fn get(&self, field: &str) -> Option<&Vec<String>> {
        self.errors.get(field)
    }

    /// Get all field names with errors.
    pub fn fields(&self) -> impl Iterator<Item = &String> {
        self.errors.keys()
    }

    /// Iterate over all errors.
    pub fn iter(&self) -> impl Iterator<Item = (&String, &Vec<String>)> {
        self.errors.iter()
    }

    /// Convert to an Error if not empty.
    pub fn into_result<T>(self, ok: T) -> Result<T> {
        if self.is_empty() {
            Ok(ok)
        } else {
            Err(Error::validation(self))
        }
    }

    /// Merge with another ValidationErrors.
    pub fn merge(&mut self, other: ValidationErrors) {
        for (field, messages) in other.errors {
            self.errors.entry(field).or_default().extend(messages);
        }
    }
}

impl std::fmt::Display for ValidationErrors {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut first = true;
        for (field, messages) in &self.errors {
            for message in messages {
                if !first {
                    write!(f, "; ")?;
                }
                write!(f, "{}: {}", field, message)?;
                first = false;
            }
        }
        Ok(())
    }
}

/// Builder for ValidationErrors.
#[derive(Debug, Default)]
pub struct ValidationErrorsBuilder {
    errors: ValidationErrors,
}

impl ValidationErrorsBuilder {
    /// Create a new builder.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add an error.
    pub fn add(mut self, field: impl Into<String>, message: impl Into<String>) -> Self {
        self.errors.add(field, message);
        self
    }

    /// Add error if condition is true.
    pub fn add_if(
        mut self,
        condition: bool,
        field: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        self.errors.add_if(condition, field, message);
        self
    }

    /// Build and return the ValidationErrors.
    pub fn build(self) -> ValidationErrors {
        self.errors
    }

    /// Build and return as Result.
    pub fn into_result<T>(self, ok: T) -> Result<T> {
        self.errors.into_result(ok)
    }
}

impl From<serde_json::Error> for Error {
    fn from(err: serde_json::Error) -> Self {
        Self::Serialization(err.to_string())
    }
}

impl From<serde_yaml::Error> for Error {
    fn from(err: serde_yaml::Error) -> Self {
        Self::Serialization(err.to_string())
    }
}

/// Extension trait for Result to add context.
pub trait ResultExt<T> {
    /// Add context to an error.
    fn with_context<F, S>(self, f: F) -> Result<T>
    where
        F: FnOnce() -> S,
        S: Into<String>;
}

impl<T> ResultExt<T> for Result<T> {
    fn with_context<F, S>(self, f: F) -> Result<T>
    where
        F: FnOnce() -> S,
        S: Into<String>,
    {
        self.map_err(|e| {
            let context = f().into();
            Error::Internal(format!("{}: {}", context, e))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_display() {
        let err = Error::device_not_found("device-001");
        assert_eq!(err.to_string(), "Device not found: device-001");
    }

    #[test]
    fn test_error_recoverable() {
        assert!(Error::Timeout { duration_ms: 1000 }.is_recoverable());
        assert!(!Error::DeviceNotFound {
            device_id: "x".into()
        }
        .is_recoverable());
    }

    #[test]
    fn test_validation_errors() {
        let mut errors = ValidationErrors::new();
        errors.add("name", "cannot be empty");
        errors.add("name", "must be at least 3 characters");
        errors.add("id", "must be unique");

        assert_eq!(errors.len(), 2);
        assert_eq!(errors.total_errors(), 3);
        assert_eq!(errors.get("name").unwrap().len(), 2);
    }

    #[test]
    fn test_validation_errors_builder() {
        let errors = ValidationErrorsBuilder::new()
            .add("field1", "error1")
            .add_if(true, "field2", "error2")
            .add_if(false, "field3", "error3") // Should not be added
            .build();

        assert_eq!(errors.len(), 2);
        assert!(errors.get("field3").is_none());
    }

    #[test]
    fn test_validation_into_result() {
        let errors = ValidationErrors::new();
        let result: Result<i32> = errors.into_result(42);
        assert!(result.is_ok());

        let mut errors = ValidationErrors::new();
        errors.add("field", "error");
        let result: Result<i32> = errors.into_result(42);
        assert!(result.is_err());
    }

    #[test]
    fn test_error_severity() {
        assert_eq!(
            Error::Internal("test".into()).severity(),
            ErrorSeverity::Critical
        );
        assert_eq!(
            Error::Timeout { duration_ms: 1000 }.severity(),
            ErrorSeverity::Medium
        );
        assert_eq!(
            Error::InvalidValue {
                point_id: "x".into(),
                reason: "y".into()
            }
            .severity(),
            ErrorSeverity::Low
        );
    }

    #[test]
    fn test_access_denied_error() {
        let err = Error::access_denied("temp", "write", "readonly");
        assert_eq!(
            err.to_string(),
            "Access denied: write not allowed on temp (mode: readonly)"
        );
    }

    #[test]
    fn test_out_of_range_error() {
        let err = Error::out_of_range("temp", 150.0, 0.0, 100.0);
        assert_eq!(err.to_string(), "Value 150 out of range [0, 100] for temp");
    }
}

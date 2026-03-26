//! Configuration validation system.
//!
//! This module provides comprehensive validation for configuration types with:
//! - Field-level validation rules
//! - Cross-field validation (constraints between multiple fields)
//! - Nested configuration validation
//! - Protocol-specific validation
//! - Custom validation rules
//!
//! # Example
//!
//! ```rust,ignore
//! use mabi_core::config::{EngineConfig, Validator};
//!
//! let config = EngineConfig::default();
//! let result = config.validate();
//!
//! match result {
//!     Ok(()) => println!("Configuration is valid"),
//!     Err(errors) => {
//!         for (field, messages) in errors.iter() {
//!             for msg in messages {
//!                 eprintln!("  {}: {}", field, msg);
//!             }
//!         }
//!     }
//! }
//! ```

use std::fmt;
use std::net::SocketAddr;
use std::path::Path;

use crate::error::ValidationErrors;
use crate::Result;

/// Trait for types that can be validated.
pub trait Validatable {
    /// Validate this configuration.
    ///
    /// Returns Ok(()) if valid, or Err with validation errors.
    fn validate(&self) -> Result<()>;

    /// Validate and collect errors without failing immediately.
    fn validate_collect(&self, errors: &mut ValidationErrors);
}

/// Validation context for tracking nested paths.
#[derive(Debug, Clone, Default)]
pub struct ValidationContext {
    /// Current path in the configuration tree.
    path: Vec<String>,
}

impl ValidationContext {
    /// Create a new validation context.
    pub fn new() -> Self {
        Self::default()
    }

    /// Enter a nested field.
    pub fn enter(&mut self, field: impl Into<String>) {
        self.path.push(field.into());
    }

    /// Leave the current nested field.
    pub fn leave(&mut self) {
        self.path.pop();
    }

    /// Get the current full path as a string.
    pub fn path(&self) -> String {
        self.path.join(".")
    }

    /// Create a field path combining context path and field name.
    pub fn field(&self, name: &str) -> String {
        if self.path.is_empty() {
            name.to_string()
        } else {
            format!("{}.{}", self.path(), name)
        }
    }

    /// Execute validation within a nested context.
    pub fn with_field<F>(&mut self, field: impl Into<String>, f: F)
    where
        F: FnOnce(&mut Self),
    {
        self.enter(field);
        f(self);
        self.leave();
    }
}

/// A validation rule that can be applied to a value.
pub trait ValidationRule<T: ?Sized>: Send + Sync {
    /// Validate the value.
    fn validate(&self, value: &T) -> std::result::Result<(), String>;

    /// Get a description of this rule.
    fn description(&self) -> &str;
}

/// Rule that checks if a value is within a range.
pub struct RangeRule<T> {
    min: Option<T>,
    max: Option<T>,
    description: String,
}

impl<T: PartialOrd + fmt::Display + Copy> RangeRule<T> {
    /// Create a range rule with minimum and maximum.
    pub fn new(min: Option<T>, max: Option<T>) -> Self {
        let description = match (&min, &max) {
            (Some(min), Some(max)) => format!("Value must be between {} and {}", min, max),
            (Some(min), None) => format!("Value must be at least {}", min),
            (None, Some(max)) => format!("Value must be at most {}", max),
            (None, None) => "No range constraint".to_string(),
        };
        Self {
            min,
            max,
            description,
        }
    }

    /// Create a minimum-only rule.
    pub fn min(min: T) -> Self {
        Self::new(Some(min), None)
    }

    /// Create a maximum-only rule.
    pub fn max(max: T) -> Self {
        Self::new(None, Some(max))
    }

    /// Create a between rule.
    pub fn between(min: T, max: T) -> Self {
        Self::new(Some(min), Some(max))
    }
}

impl<T: PartialOrd + fmt::Display + Copy + Send + Sync> ValidationRule<T> for RangeRule<T> {
    fn validate(&self, value: &T) -> std::result::Result<(), String> {
        if let Some(min) = &self.min {
            if value < min {
                return Err(format!("Value {} is below minimum {}", value, min));
            }
        }
        if let Some(max) = &self.max {
            if value > max {
                return Err(format!("Value {} exceeds maximum {}", value, max));
            }
        }
        Ok(())
    }

    fn description(&self) -> &str {
        &self.description
    }
}

/// Rule that checks string length.
pub struct StringLengthRule {
    min: Option<usize>,
    max: Option<usize>,
    description: String,
}

impl StringLengthRule {
    /// Create a string length rule.
    pub fn new(min: Option<usize>, max: Option<usize>) -> Self {
        let description = match (min, max) {
            (Some(min), Some(max)) => format!("Length must be between {} and {}", min, max),
            (Some(min), None) => format!("Length must be at least {}", min),
            (None, Some(max)) => format!("Length must be at most {}", max),
            (None, None) => "No length constraint".to_string(),
        };
        Self {
            min,
            max,
            description,
        }
    }

    /// Create a non-empty rule.
    pub fn non_empty() -> Self {
        Self::new(Some(1), None)
    }

    /// Create a max length rule.
    pub fn max(max: usize) -> Self {
        Self::new(None, Some(max))
    }
}

impl ValidationRule<String> for StringLengthRule {
    fn validate(&self, value: &String) -> std::result::Result<(), String> {
        let len = value.len();
        if let Some(min) = self.min {
            if len < min {
                return Err(format!("String length {} is below minimum {}", len, min));
            }
        }
        if let Some(max) = self.max {
            if len > max {
                return Err(format!("String length {} exceeds maximum {}", len, max));
            }
        }
        Ok(())
    }

    fn description(&self) -> &str {
        &self.description
    }
}

impl ValidationRule<str> for StringLengthRule {
    fn validate(&self, value: &str) -> std::result::Result<(), String> {
        self.validate(&value.to_string())
    }

    fn description(&self) -> &str {
        &self.description
    }
}

/// Rule that checks if a path exists.
pub struct PathExistsRule {
    check_file: bool,
    check_dir: bool,
    description: String,
}

impl PathExistsRule {
    /// Create a rule that checks if a file exists.
    pub fn file() -> Self {
        Self {
            check_file: true,
            check_dir: false,
            description: "Path must be an existing file".to_string(),
        }
    }

    /// Create a rule that checks if a directory exists.
    pub fn directory() -> Self {
        Self {
            check_file: false,
            check_dir: true,
            description: "Path must be an existing directory".to_string(),
        }
    }

    /// Create a rule that checks if path exists (file or directory).
    pub fn exists() -> Self {
        Self {
            check_file: false,
            check_dir: false,
            description: "Path must exist".to_string(),
        }
    }
}

impl<P: AsRef<Path>> ValidationRule<P> for PathExistsRule {
    fn validate(&self, value: &P) -> std::result::Result<(), String> {
        let path = value.as_ref();
        if self.check_file {
            if !path.is_file() {
                return Err(format!("Path '{}' is not a file", path.display()));
            }
        } else if self.check_dir {
            if !path.is_dir() {
                return Err(format!("Path '{}' is not a directory", path.display()));
            }
        } else if !path.exists() {
            return Err(format!("Path '{}' does not exist", path.display()));
        }
        Ok(())
    }

    fn description(&self) -> &str {
        &self.description
    }
}

/// Rule that validates socket addresses.
pub struct SocketAddrRule {
    require_ipv4: bool,
    port_range: Option<(u16, u16)>,
    description: String,
}

impl SocketAddrRule {
    /// Create a basic socket address rule.
    pub fn new() -> Self {
        Self {
            require_ipv4: false,
            port_range: None,
            description: "Must be a valid socket address".to_string(),
        }
    }

    /// Require IPv4 address.
    pub fn ipv4_only(mut self) -> Self {
        self.require_ipv4 = true;
        self.description = "Must be a valid IPv4 socket address".to_string();
        self
    }

    /// Require port in range.
    pub fn port_range(mut self, min: u16, max: u16) -> Self {
        self.port_range = Some((min, max));
        self.description = format!(
            "Must be a valid socket address with port between {} and {}",
            min, max
        );
        self
    }

    /// Require non-privileged port (>= 1024).
    pub fn non_privileged_port(self) -> Self {
        self.port_range(1024, 65535)
    }
}

impl Default for SocketAddrRule {
    fn default() -> Self {
        Self::new()
    }
}

impl ValidationRule<SocketAddr> for SocketAddrRule {
    fn validate(&self, value: &SocketAddr) -> std::result::Result<(), String> {
        if self.require_ipv4 && value.is_ipv6() {
            return Err("IPv4 address required".to_string());
        }

        if let Some((min, max)) = self.port_range {
            let port = value.port();
            if port < min || port > max {
                return Err(format!("Port {} must be between {} and {}", port, min, max));
            }
        }

        Ok(())
    }

    fn description(&self) -> &str {
        &self.description
    }
}

/// Validator for collecting and running multiple validation rules.
#[derive(Default)]
pub struct Validator {
    errors: ValidationErrors,
    context: ValidationContext,
}

impl Validator {
    /// Create a new validator.
    pub fn new() -> Self {
        Self::default()
    }

    /// Get current validation context.
    pub fn context(&self) -> &ValidationContext {
        &self.context
    }

    /// Get mutable validation context.
    pub fn context_mut(&mut self) -> &mut ValidationContext {
        &mut self.context
    }

    /// Add an error for a field.
    pub fn add_error(&mut self, field: &str, message: impl Into<String>) {
        let full_path = self.context.field(field);
        self.errors.add(full_path, message);
    }

    /// Add error if condition is true.
    pub fn add_if(&mut self, condition: bool, field: &str, message: impl Into<String>) {
        if condition {
            self.add_error(field, message);
        }
    }

    /// Validate a value with a rule.
    pub fn validate_field<T, R>(&mut self, field: &str, value: &T, rule: &R)
    where
        R: ValidationRule<T>,
    {
        if let Err(msg) = rule.validate(value) {
            self.add_error(field, msg);
        }
    }

    /// Validate a required string field is not empty.
    pub fn require_non_empty(&mut self, field: &str, value: &str) {
        if value.trim().is_empty() {
            self.add_error(field, "Value cannot be empty");
        }
    }

    /// Validate a numeric value is positive.
    pub fn require_positive<T: PartialOrd + Default + fmt::Display>(
        &mut self,
        field: &str,
        value: T,
    ) {
        if value <= T::default() {
            self.add_error(field, format!("Value must be positive, got {}", value));
        }
    }

    /// Validate a value is in a range.
    pub fn require_range<T: PartialOrd + fmt::Display + Copy>(
        &mut self,
        field: &str,
        value: T,
        min: T,
        max: T,
    ) {
        if value < min || value > max {
            self.add_error(
                field,
                format!("Value {} must be between {} and {}", value, min, max),
            );
        }
    }

    /// Validate with a nested context.
    pub fn validate_nested<F>(&mut self, field: &str, f: F)
    where
        F: FnOnce(&mut Self),
    {
        self.context.enter(field);
        f(self);
        self.context.leave();
    }

    /// Check if there are any errors.
    pub fn has_errors(&self) -> bool {
        !self.errors.is_empty()
    }

    /// Get the collected errors.
    pub fn errors(&self) -> &ValidationErrors {
        &self.errors
    }

    /// Consume and return the errors.
    pub fn into_errors(self) -> ValidationErrors {
        self.errors
    }

    /// Convert to Result.
    pub fn into_result(self) -> Result<()> {
        self.errors.into_result(())
    }

    /// Merge errors from another validator.
    pub fn merge(&mut self, other: Validator) {
        self.errors.merge(other.errors);
    }
}

/// Cross-field validation helper.
pub struct CrossFieldValidator<'a, T> {
    config: &'a T,
    errors: ValidationErrors,
}

impl<'a, T> CrossFieldValidator<'a, T> {
    /// Create a new cross-field validator.
    pub fn new(config: &'a T) -> Self {
        Self {
            config,
            errors: ValidationErrors::new(),
        }
    }

    /// Get the configuration being validated.
    pub fn config(&self) -> &T {
        self.config
    }

    /// Add a cross-field validation error.
    pub fn add_error(&mut self, fields: &[&str], message: impl Into<String>) {
        let field_name = fields.join(", ");
        self.errors.add(field_name, message);
    }

    /// Add error if condition is true.
    pub fn add_if(&mut self, condition: bool, fields: &[&str], message: impl Into<String>) {
        if condition {
            self.add_error(fields, message);
        }
    }

    /// Check if there are any errors.
    pub fn has_errors(&self) -> bool {
        !self.errors.is_empty()
    }

    /// Consume and return the errors.
    pub fn into_errors(self) -> ValidationErrors {
        self.errors
    }
}

/// Macro to simplify validation checks.
#[macro_export]
macro_rules! validate {
    // Simple condition check
    ($validator:expr, $field:expr, $cond:expr, $msg:expr) => {
        $validator.add_if(!$cond, $field, $msg);
    };

    // Range check
    ($validator:expr, $field:expr, range $value:expr, $min:expr, $max:expr) => {
        $validator.require_range($field, $value, $min, $max);
    };

    // Non-empty string check
    ($validator:expr, $field:expr, non_empty $value:expr) => {
        $validator.require_non_empty($field, $value);
    };

    // Positive number check
    ($validator:expr, $field:expr, positive $value:expr) => {
        $validator.require_positive($field, $value);
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_range_rule() {
        let rule = RangeRule::between(0, 100);

        assert!(rule.validate(&50).is_ok());
        assert!(rule.validate(&0).is_ok());
        assert!(rule.validate(&100).is_ok());
        assert!(rule.validate(&-1).is_err());
        assert!(rule.validate(&101).is_err());
    }

    #[test]
    fn test_string_length_rule() {
        let rule = StringLengthRule::new(Some(3), Some(10));

        assert!(rule.validate(&"hello".to_string()).is_ok());
        assert!(rule.validate(&"ab".to_string()).is_err());
        assert!(rule.validate(&"this is too long".to_string()).is_err());
    }

    #[test]
    fn test_string_non_empty() {
        let rule = StringLengthRule::non_empty();

        assert!(rule.validate(&"hello".to_string()).is_ok());
        assert!(rule.validate(&"".to_string()).is_err());
    }

    #[test]
    fn test_socket_addr_rule() {
        let rule = SocketAddrRule::new().non_privileged_port();

        let valid: SocketAddr = "127.0.0.1:8080".parse().unwrap();
        let invalid: SocketAddr = "127.0.0.1:80".parse().unwrap();

        assert!(rule.validate(&valid).is_ok());
        assert!(rule.validate(&invalid).is_err());
    }

    #[test]
    fn test_validator() {
        let mut validator = Validator::new();

        validator.require_non_empty("name", "");
        validator.require_positive("count", -1i32);
        validator.require_range("percent", 150, 0, 100);

        assert!(validator.has_errors());
        assert_eq!(validator.errors().len(), 3);
    }

    #[test]
    fn test_validator_nested() {
        let mut validator = Validator::new();

        validator.validate_nested("engine", |v| {
            v.add_error("max_devices", "Too low");
        });

        let errors = validator.into_errors();
        assert!(errors.get("engine.max_devices").is_some());
    }

    #[test]
    fn test_validation_context() {
        let mut ctx = ValidationContext::new();

        ctx.enter("engine");
        assert_eq!(ctx.field("max_devices"), "engine.max_devices");

        ctx.enter("modbus");
        assert_eq!(ctx.field("port"), "engine.modbus.port");

        ctx.leave();
        assert_eq!(ctx.field("workers"), "engine.workers");

        ctx.leave();
        assert_eq!(ctx.field("name"), "name");
    }

    #[test]
    fn test_cross_field_validator() {
        struct Config {
            min: u32,
            max: u32,
        }

        let config = Config { min: 100, max: 50 };
        let mut validator = CrossFieldValidator::new(&config);

        validator.add_if(
            config.min > config.max,
            &["min", "max"],
            "min cannot be greater than max",
        );

        assert!(validator.has_errors());
    }

    #[test]
    fn test_validate_macro() {
        let mut validator = Validator::new();
        let value = 150;

        validate!(validator, "percent", value <= 100, "Must be <= 100");
        assert!(validator.has_errors());

        let mut validator2 = Validator::new();
        let value2 = 50;
        validate!(validator2, "percent", value2 <= 100, "Must be <= 100");
        assert!(!validator2.has_errors());
    }

    #[test]
    fn test_path_exists_rule_file() {
        let rule = PathExistsRule::file();
        let non_existent = PathBuf::from("/non/existent/file.txt");
        assert!(rule.validate(&non_existent).is_err());
    }
}

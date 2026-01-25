//! Environment variable override system.
//!
//! This module provides hierarchical environment variable support for configuration
//! with type-safe parsing and customizable prefix.
//!
//! # Naming Convention
//!
//! Environment variables follow a hierarchical pattern:
//! `{PREFIX}_{SECTION}_{FIELD}` where:
//! - `PREFIX`: Application prefix (default: `TRAP_SIM`)
//! - `SECTION`: Configuration section (e.g., `ENGINE`, `MODBUS`)
//! - `FIELD`: Specific field name in SCREAMING_SNAKE_CASE
//!
//! # Examples
//!
//! ```text
//! TRAP_SIM_ENGINE_MAX_DEVICES=50000
//! TRAP_SIM_ENGINE_TICK_INTERVAL_MS=50
//! TRAP_SIM_MODBUS_TCP_BIND_ADDRESS=0.0.0.0:5502
//! TRAP_SIM_LOG_LEVEL=debug
//! ```
//!
//! # Usage
//!
//! ```rust,ignore
//! use mabi_core::config::env::EnvOverrides;
//!
//! let overrides = EnvOverrides::with_prefix("TRAP_SIM")
//!     .add_rule("ENGINE_MAX_DEVICES", |c, v| {
//!         c.max_devices = v.parse().ok()?;
//!         Some(())
//!     })
//!     .build();
//!
//! overrides.apply(&mut config)?;
//! ```

use std::collections::HashMap;
use std::env;
use std::fmt;
use std::marker::PhantomData;
use std::str::FromStr;

use crate::error::{Error, ValidationErrors};
use crate::Result;

/// Default environment variable prefix.
pub const DEFAULT_PREFIX: &str = "TRAP_SIM";

/// Environment variable override result.
#[derive(Debug, Clone)]
pub struct EnvApplyResult {
    /// Number of overrides successfully applied.
    pub applied: usize,
    /// Fields that were overridden.
    pub overridden_fields: Vec<String>,
    /// Errors encountered during application.
    pub errors: Vec<EnvOverrideError>,
}

impl EnvApplyResult {
    /// Check if any overrides were applied.
    pub fn has_changes(&self) -> bool {
        self.applied > 0
    }

    /// Check if there were any errors.
    pub fn has_errors(&self) -> bool {
        !self.errors.is_empty()
    }

    /// Convert to Result, failing if there were errors.
    pub fn into_result(self) -> Result<Self> {
        if self.errors.is_empty() {
            Ok(self)
        } else {
            let mut validation = ValidationErrors::new();
            for err in &self.errors {
                validation.add(&err.env_var, &err.message);
            }
            Err(Error::validation(validation))
        }
    }
}

/// Error that occurred while applying an environment override.
#[derive(Debug, Clone)]
pub struct EnvOverrideError {
    /// Environment variable name.
    pub env_var: String,
    /// Field path being overridden.
    pub field: String,
    /// Error message.
    pub message: String,
}

impl fmt::Display for EnvOverrideError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Failed to apply {} to {}: {}",
            self.env_var, self.field, self.message
        )
    }
}

/// Type for override application functions.
pub type OverrideFn<T> = Box<dyn Fn(&mut T, &str) -> std::result::Result<(), String> + Send + Sync>;

/// Rule for applying an environment variable override.
pub struct EnvRule<T> {
    /// Environment variable suffix (without prefix).
    pub suffix: String,
    /// Target field path for documentation.
    pub field_path: String,
    /// Description of what this override does.
    pub description: String,
    /// Function to apply the override.
    pub apply: OverrideFn<T>,
}

impl<T> fmt::Debug for EnvRule<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EnvRule")
            .field("suffix", &self.suffix)
            .field("field_path", &self.field_path)
            .field("description", &self.description)
            .finish()
    }
}

/// Builder for creating environment override rules.
pub struct EnvRuleBuilder<T> {
    suffix: String,
    field_path: String,
    description: String,
    _phantom: PhantomData<T>,
}

impl<T> EnvRuleBuilder<T> {
    /// Create a new rule builder.
    pub fn new(suffix: impl Into<String>) -> Self {
        let suffix = suffix.into();
        Self {
            field_path: suffix.to_lowercase().replace('_', "."),
            suffix,
            description: String::new(),
            _phantom: PhantomData,
        }
    }

    /// Set the field path for documentation.
    pub fn field_path(mut self, path: impl Into<String>) -> Self {
        self.field_path = path.into();
        self
    }

    /// Set the description.
    pub fn description(mut self, desc: impl Into<String>) -> Self {
        self.description = desc.into();
        self
    }

    /// Build with a parsing function for types that implement FromStr.
    pub fn parse_into<F, V>(self, setter: F) -> EnvRule<T>
    where
        F: Fn(&mut T, V) + Send + Sync + 'static,
        V: FromStr,
        V::Err: fmt::Display,
    {
        EnvRule {
            suffix: self.suffix,
            field_path: self.field_path,
            description: self.description,
            apply: Box::new(move |config, value| {
                let parsed = value
                    .parse::<V>()
                    .map_err(|e| format!("Failed to parse: {}", e))?;
                setter(config, parsed);
                Ok(())
            }),
        }
    }

    /// Build with a custom application function.
    pub fn apply_with<F>(self, f: F) -> EnvRule<T>
    where
        F: Fn(&mut T, &str) -> std::result::Result<(), String> + Send + Sync + 'static,
    {
        EnvRule {
            suffix: self.suffix,
            field_path: self.field_path,
            description: self.description,
            apply: Box::new(f),
        }
    }

    /// Build for string fields (no parsing needed).
    pub fn as_string<F>(self, setter: F) -> EnvRule<T>
    where
        F: Fn(&mut T, String) + Send + Sync + 'static,
    {
        EnvRule {
            suffix: self.suffix,
            field_path: self.field_path,
            description: self.description,
            apply: Box::new(move |config, value| {
                setter(config, value.to_string());
                Ok(())
            }),
        }
    }

    /// Build for boolean fields with flexible parsing.
    pub fn as_bool<F>(self, setter: F) -> EnvRule<T>
    where
        F: Fn(&mut T, bool) + Send + Sync + 'static,
    {
        EnvRule {
            suffix: self.suffix,
            field_path: self.field_path,
            description: self.description,
            apply: Box::new(move |config, value| {
                let parsed = parse_bool(value)
                    .ok_or_else(|| format!("Invalid boolean value: {}", value))?;
                setter(config, parsed);
                Ok(())
            }),
        }
    }
}

/// Parse a boolean from various string representations.
fn parse_bool(s: &str) -> Option<bool> {
    match s.to_lowercase().as_str() {
        "true" | "1" | "yes" | "on" | "enabled" => Some(true),
        "false" | "0" | "no" | "off" | "disabled" => Some(false),
        _ => None,
    }
}

/// Environment variable overrides configuration.
pub struct EnvOverrides<T> {
    /// Prefix for all environment variables.
    prefix: String,
    /// Override rules.
    rules: Vec<EnvRule<T>>,
    /// Whether to ignore missing environment variables.
    ignore_missing: bool,
    /// Whether to fail on parse errors.
    fail_on_error: bool,
}

impl<T> fmt::Debug for EnvOverrides<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EnvOverrides")
            .field("prefix", &self.prefix)
            .field("rules_count", &self.rules.len())
            .field("ignore_missing", &self.ignore_missing)
            .field("fail_on_error", &self.fail_on_error)
            .finish()
    }
}

impl<T> EnvOverrides<T> {
    /// Create a new environment overrides handler with default prefix.
    pub fn new() -> Self {
        Self::with_prefix(DEFAULT_PREFIX)
    }

    /// Create with a custom prefix.
    pub fn with_prefix(prefix: impl Into<String>) -> Self {
        Self {
            prefix: prefix.into(),
            rules: Vec::new(),
            ignore_missing: true,
            fail_on_error: false,
        }
    }

    /// Get the full environment variable name for a suffix.
    pub fn full_var_name(&self, suffix: &str) -> String {
        format!("{}_{}", self.prefix, suffix)
    }

    /// Add a rule to the overrides.
    pub fn add_rule(mut self, rule: EnvRule<T>) -> Self {
        self.rules.push(rule);
        self
    }

    /// Set whether to ignore missing environment variables.
    pub fn ignore_missing(mut self, ignore: bool) -> Self {
        self.ignore_missing = ignore;
        self
    }

    /// Set whether to fail on parse errors.
    pub fn fail_on_error(mut self, fail: bool) -> Self {
        self.fail_on_error = fail;
        self
    }

    /// Apply all environment overrides to the configuration.
    pub fn apply(&self, config: &mut T) -> EnvApplyResult {
        let mut result = EnvApplyResult {
            applied: 0,
            overridden_fields: Vec::new(),
            errors: Vec::new(),
        };

        for rule in &self.rules {
            let var_name = self.full_var_name(&rule.suffix);

            match env::var(&var_name) {
                Ok(value) => {
                    match (rule.apply)(config, &value) {
                        Ok(()) => {
                            result.applied += 1;
                            result.overridden_fields.push(rule.field_path.clone());
                            tracing::debug!(
                                env_var = %var_name,
                                field = %rule.field_path,
                                "Applied environment override"
                            );
                        }
                        Err(msg) => {
                            result.errors.push(EnvOverrideError {
                                env_var: var_name,
                                field: rule.field_path.clone(),
                                message: msg,
                            });
                        }
                    }
                }
                Err(env::VarError::NotPresent) => {
                    // Variable not set, skip
                }
                Err(env::VarError::NotUnicode(_)) => {
                    result.errors.push(EnvOverrideError {
                        env_var: var_name,
                        field: rule.field_path.clone(),
                        message: "Value is not valid UTF-8".to_string(),
                    });
                }
            }
        }

        result
    }

    /// Apply and return Result based on fail_on_error setting.
    pub fn apply_checked(&self, config: &mut T) -> Result<EnvApplyResult> {
        let result = self.apply(config);
        if self.fail_on_error && result.has_errors() {
            result.into_result()
        } else {
            Ok(result)
        }
    }

    /// Get documentation for all registered rules.
    pub fn documentation(&self) -> Vec<EnvVarDoc> {
        self.rules
            .iter()
            .map(|rule| EnvVarDoc {
                var_name: self.full_var_name(&rule.suffix),
                field_path: rule.field_path.clone(),
                description: rule.description.clone(),
            })
            .collect()
    }

    /// Get the number of registered rules.
    pub fn rule_count(&self) -> usize {
        self.rules.len()
    }
}

impl<T> Default for EnvOverrides<T> {
    fn default() -> Self {
        Self::new()
    }
}

/// Documentation entry for an environment variable.
#[derive(Debug, Clone)]
pub struct EnvVarDoc {
    /// Full environment variable name.
    pub var_name: String,
    /// Configuration field path.
    pub field_path: String,
    /// Description.
    pub description: String,
}

impl fmt::Display for EnvVarDoc {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {} - {}", self.var_name, self.field_path, self.description)
    }
}

/// Helper trait for creating typed environment rules.
pub trait EnvConfigurable: Sized {
    /// Create environment overrides for this type.
    fn env_overrides() -> EnvOverrides<Self>;
}

/// Convenience function to read and parse an environment variable.
pub fn get_env<T: FromStr>(name: &str) -> Option<T> {
    env::var(name).ok().and_then(|v| v.parse().ok())
}

/// Convenience function to read an environment variable with a default.
pub fn get_env_or<T: FromStr>(name: &str, default: T) -> T {
    get_env(name).unwrap_or(default)
}

/// Read an environment variable as a boolean with flexible parsing.
pub fn get_env_bool(name: &str) -> Option<bool> {
    env::var(name).ok().and_then(|v| parse_bool(&v))
}

/// Read an environment variable as a boolean with a default.
pub fn get_env_bool_or(name: &str, default: bool) -> bool {
    get_env_bool(name).unwrap_or(default)
}

/// A collection of environment variable values for testing.
#[derive(Debug, Default)]
pub struct EnvSnapshot {
    vars: HashMap<String, String>,
}

impl EnvSnapshot {
    /// Create a new empty snapshot.
    pub fn new() -> Self {
        Self::default()
    }

    /// Capture current environment variables with a specific prefix.
    pub fn capture(prefix: &str) -> Self {
        let vars = env::vars()
            .filter(|(k, _)| k.starts_with(prefix))
            .collect();
        Self { vars }
    }

    /// Set a variable in this snapshot.
    pub fn set(&mut self, key: impl Into<String>, value: impl Into<String>) {
        self.vars.insert(key.into(), value.into());
    }

    /// Apply this snapshot to the environment.
    pub fn apply(&self) {
        for (key, value) in &self.vars {
            env::set_var(key, value);
        }
    }

    /// Clear variables from environment (for testing).
    pub fn clear_from_env(&self) {
        for key in self.vars.keys() {
            env::remove_var(key);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Default)]
    struct TestConfig {
        max_devices: usize,
        name: String,
        enabled: bool,
    }

    fn test_overrides() -> EnvOverrides<TestConfig> {
        EnvOverrides::with_prefix("TEST")
            .add_rule(
                EnvRuleBuilder::new("MAX_DEVICES")
                    .field_path("max_devices")
                    .description("Maximum device count")
                    .parse_into(|c: &mut TestConfig, v: usize| c.max_devices = v),
            )
            .add_rule(
                EnvRuleBuilder::new("NAME")
                    .field_path("name")
                    .description("Config name")
                    .as_string(|c: &mut TestConfig, v| c.name = v),
            )
            .add_rule(
                EnvRuleBuilder::new("ENABLED")
                    .field_path("enabled")
                    .description("Enable flag")
                    .as_bool(|c: &mut TestConfig, v| c.enabled = v),
            )
    }

    #[test]
    fn test_env_var_name() {
        let overrides: EnvOverrides<TestConfig> = EnvOverrides::with_prefix("TRAP_SIM");
        assert_eq!(
            overrides.full_var_name("ENGINE_MAX_DEVICES"),
            "TRAP_SIM_ENGINE_MAX_DEVICES"
        );
    }

    #[test]
    fn test_parse_bool() {
        assert_eq!(parse_bool("true"), Some(true));
        assert_eq!(parse_bool("True"), Some(true));
        assert_eq!(parse_bool("1"), Some(true));
        assert_eq!(parse_bool("yes"), Some(true));
        assert_eq!(parse_bool("on"), Some(true));
        assert_eq!(parse_bool("enabled"), Some(true));

        assert_eq!(parse_bool("false"), Some(false));
        assert_eq!(parse_bool("False"), Some(false));
        assert_eq!(parse_bool("0"), Some(false));
        assert_eq!(parse_bool("no"), Some(false));
        assert_eq!(parse_bool("off"), Some(false));
        assert_eq!(parse_bool("disabled"), Some(false));

        assert_eq!(parse_bool("invalid"), None);
    }

    #[test]
    fn test_apply_overrides() {
        // Set test environment variables
        env::set_var("TEST_MAX_DEVICES", "5000");
        env::set_var("TEST_NAME", "test-config");
        env::set_var("TEST_ENABLED", "true");

        let overrides = test_overrides();
        let mut config = TestConfig::default();

        let result = overrides.apply(&mut config);

        assert_eq!(result.applied, 3);
        assert_eq!(config.max_devices, 5000);
        assert_eq!(config.name, "test-config");
        assert!(config.enabled);

        // Cleanup
        env::remove_var("TEST_MAX_DEVICES");
        env::remove_var("TEST_NAME");
        env::remove_var("TEST_ENABLED");
    }

    #[test]
    fn test_parse_error() {
        env::set_var("TEST_MAX_DEVICES", "not_a_number");

        let overrides = test_overrides();
        let mut config = TestConfig::default();

        let result = overrides.apply(&mut config);

        assert!(result.has_errors());
        assert_eq!(result.errors.len(), 1);
        assert!(result.errors[0].message.contains("Failed to parse"));

        env::remove_var("TEST_MAX_DEVICES");
    }

    #[test]
    fn test_documentation() {
        let overrides = test_overrides();
        let docs = overrides.documentation();

        assert_eq!(docs.len(), 3);
        assert_eq!(docs[0].var_name, "TEST_MAX_DEVICES");
        assert_eq!(docs[0].field_path, "max_devices");
    }

    #[test]
    fn test_env_snapshot() {
        let mut snapshot = EnvSnapshot::new();
        snapshot.set("TEST_VAR", "value");
        snapshot.apply();

        assert_eq!(env::var("TEST_VAR").ok(), Some("value".to_string()));

        snapshot.clear_from_env();
        assert!(env::var("TEST_VAR").is_err());
    }
}

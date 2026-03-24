//! Validate command implementation.
//!
//! Validates scenario and configuration files.

use crate::context::CliContext;
use crate::error::{CliError, CliResult};
use crate::output::{
    StatusType, TableBuilder, ValidationError, ValidationResult, ValidationWarning,
};
use crate::runner::{Command, CommandOutput};
use async_trait::async_trait;
use mabi_scenario::Scenario;
use std::path::PathBuf;

/// Validate command for checking configuration files.
pub struct ValidateCommand {
    /// Paths to validate.
    paths: Vec<PathBuf>,
    /// Whether to show detailed output.
    detailed: bool,
    /// Whether to check for warnings only (no errors = success).
    strict: bool,
}

impl ValidateCommand {
    /// Create a new validate command.
    pub fn new(paths: Vec<PathBuf>) -> Self {
        Self {
            paths,
            detailed: false,
            strict: false,
        }
    }

    /// Enable detailed output.
    pub fn with_detailed(mut self, detailed: bool) -> Self {
        self.detailed = detailed;
        self
    }

    /// Enable strict mode (warnings become errors).
    pub fn with_strict(mut self, strict: bool) -> Self {
        self.strict = strict;
        self
    }

    /// Validate a single file.
    async fn validate_file(&self, ctx: &CliContext, path: &PathBuf) -> ValidationResult {
        let resolved_path = ctx.resolve_path(path);
        let mut errors = Vec::new();
        let mut warnings = Vec::new();

        // Check file exists
        if !resolved_path.exists() {
            errors.push(ValidationError {
                path: path.display().to_string(),
                message: "File not found".into(),
            });
            return ValidationResult {
                valid: false,
                errors,
                warnings,
            };
        }

        // Read file content
        let content = match tokio::fs::read_to_string(&resolved_path).await {
            Ok(c) => c,
            Err(e) => {
                errors.push(ValidationError {
                    path: path.display().to_string(),
                    message: format!("Failed to read file: {}", e),
                });
                return ValidationResult {
                    valid: false,
                    errors,
                    warnings,
                };
            }
        };

        // Determine file type and validate
        let extension = resolved_path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("");

        match extension {
            "yaml" | "yml" => {
                self.validate_yaml(&content, path, &mut errors, &mut warnings);
            }
            "json" => {
                self.validate_json(&content, path, &mut errors, &mut warnings);
            }
            "toml" => {
                self.validate_toml(&content, path, &mut errors, &mut warnings);
            }
            _ => {
                warnings.push(ValidationWarning {
                    path: path.display().to_string(),
                    message: format!("Unknown file extension: {}", extension),
                });
            }
        }

        // Check for scenario-specific validation
        if content.contains("devices:") || content.contains("\"devices\"") {
            self.validate_scenario_content(&content, path, &mut errors, &mut warnings);
        }

        let valid = errors.is_empty() && (!self.strict || warnings.is_empty());

        ValidationResult {
            valid,
            errors,
            warnings,
        }
    }

    /// Validate YAML content.
    fn validate_yaml(
        &self,
        content: &str,
        path: &PathBuf,
        errors: &mut Vec<ValidationError>,
        _warnings: &mut Vec<ValidationWarning>,
    ) {
        if let Err(e) = serde_yaml::from_str::<serde_yaml::Value>(content) {
            errors.push(ValidationError {
                path: path.display().to_string(),
                message: format!("Invalid YAML: {}", e),
            });
        }
    }

    /// Validate JSON content.
    fn validate_json(
        &self,
        content: &str,
        path: &PathBuf,
        errors: &mut Vec<ValidationError>,
        _warnings: &mut Vec<ValidationWarning>,
    ) {
        if let Err(e) = serde_json::from_str::<serde_json::Value>(content) {
            errors.push(ValidationError {
                path: path.display().to_string(),
                message: format!("Invalid JSON: {}", e),
            });
        }
    }

    /// Validate TOML content.
    fn validate_toml(
        &self,
        content: &str,
        path: &PathBuf,
        errors: &mut Vec<ValidationError>,
        _warnings: &mut Vec<ValidationWarning>,
    ) {
        if let Err(e) = toml::from_str::<toml::Value>(content) {
            errors.push(ValidationError {
                path: path.display().to_string(),
                message: format!("Invalid TOML: {}", e),
            });
        }
    }

    /// Validate scenario-specific content.
    fn validate_scenario_content(
        &self,
        content: &str,
        path: &PathBuf,
        errors: &mut Vec<ValidationError>,
        warnings: &mut Vec<ValidationWarning>,
    ) {
        // Try to parse as scenario
        if let Ok(scenario) = serde_yaml::from_str::<Scenario>(content) {
            // Validate scenario fields
            if scenario.name.is_empty() {
                errors.push(ValidationError {
                    path: path.display().to_string(),
                    message: "Scenario name is required".into(),
                });
            }

            if scenario.devices.is_empty() {
                warnings.push(ValidationWarning {
                    path: path.display().to_string(),
                    message: "Scenario has no devices".into(),
                });
            }

            // Validate devices
            for (idx, device) in scenario.devices.iter().enumerate() {
                if device.id.is_empty() {
                    errors.push(ValidationError {
                        path: format!("{}:devices[{}]", path.display(), idx),
                        message: "Device ID is required".into(),
                    });
                }

                if device.protocol.is_empty() {
                    errors.push(ValidationError {
                        path: format!("{}:devices[{}]", path.display(), idx),
                        message: "Device protocol is required".into(),
                    });
                }

                // Validate protocol
                let valid_protocols = ["modbus_tcp", "modbus_rtu", "opcua", "bacnet", "knx"];
                if !valid_protocols.contains(&device.protocol.to_lowercase().as_str()) {
                    warnings.push(ValidationWarning {
                        path: format!("{}:devices[{}]", path.display(), idx),
                        message: format!("Unknown protocol: {}", device.protocol),
                    });
                }
            }

            // Validate scenario points
            for (idx, point) in scenario.points.iter().enumerate() {
                if point.id.is_empty() {
                    errors.push(ValidationError {
                        path: format!("{}:points[{}]", path.display(), idx),
                        message: "Point ID is required".into(),
                    });
                }

                if point.point_id.is_empty() {
                    errors.push(ValidationError {
                        path: format!("{}:points[{}]", path.display(), idx),
                        message: "Target point_id is required".into(),
                    });
                }

                if point.device_id.is_empty() && point.device_tags.is_empty() {
                    warnings.push(ValidationWarning {
                        path: format!("{}:points[{}]", path.display(), idx),
                        message: "Point has neither device_id nor device_tags".into(),
                    });
                }
            }
        }
    }
}

#[async_trait]
impl Command for ValidateCommand {
    fn name(&self) -> &str {
        "validate"
    }

    fn description(&self) -> &str {
        "Validate scenario and configuration files"
    }

    fn validate(&self) -> CliResult<()> {
        if self.paths.is_empty() {
            return Err(CliError::InvalidConfig {
                message: "At least one file path is required".into(),
            });
        }
        Ok(())
    }

    async fn execute(&self, ctx: &mut CliContext) -> CliResult<CommandOutput> {
        let output = ctx.output();
        let mut all_valid = true;
        let mut results = Vec::new();

        output.header("Validating Files");

        for path in &self.paths {
            let result = self.validate_file(ctx, path).await;
            if !result.valid {
                all_valid = false;
            }
            results.push((path.clone(), result));
        }

        // Display results
        if self.detailed {
            for (path, result) in &results {
                output.kv("File", path.display());

                if result.errors.is_empty() && result.warnings.is_empty() {
                    output.success("  Valid");
                } else {
                    for error in &result.errors {
                        output.error(format!("  {}: {}", error.path, error.message));
                    }
                    for warning in &result.warnings {
                        output.warning(format!("  {}: {}", warning.path, warning.message));
                    }
                }
                println!();
            }
        } else {
            // Summary table
            let mut table = TableBuilder::new(output.colors_enabled())
                .header(["File", "Errors", "Warnings", "Status"]);

            for (path, result) in &results {
                let status = if result.valid { "Valid" } else { "Invalid" };
                let status_type = if result.valid {
                    StatusType::Success
                } else {
                    StatusType::Error
                };

                table = table.status_row(
                    [
                        path.display().to_string(),
                        result.errors.len().to_string(),
                        result.warnings.len().to_string(),
                        status.to_string(),
                    ],
                    status_type,
                );
            }
            table.print();
        }

        // Summary
        println!();
        let total = results.len();
        let valid_count = results.iter().filter(|(_, r)| r.valid).count();
        let error_count: usize = results.iter().map(|(_, r)| r.errors.len()).sum();
        let warning_count: usize = results.iter().map(|(_, r)| r.warnings.len()).sum();

        output.kv("Total files", total);
        output.kv("Valid", valid_count);
        output.kv("Errors", error_count);
        output.kv("Warnings", warning_count);

        if all_valid {
            output.success("All files are valid");
            Ok(CommandOutput::quiet_success())
        } else {
            Err(CliError::ValidationFailed {
                errors: format!("{} file(s) failed validation", total - valid_count),
            })
        }
    }
}

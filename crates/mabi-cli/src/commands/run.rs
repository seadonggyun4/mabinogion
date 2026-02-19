//! Run command implementation.
//!
//! Executes scenario files or starts protocol simulators.

use crate::context::CliContext;
use crate::error::{CliError, CliResult};
use crate::output::{StatusType, TableBuilder};
use crate::runner::{Command, CommandOutput};
use async_trait::async_trait;
use mabi_core::prelude::*;
use std::path::PathBuf;
use std::time::Duration;

/// Run command for executing scenarios.
pub struct RunCommand {
    /// Path to scenario file.
    scenario_path: PathBuf,
    /// Time scale factor (1.0 = real-time).
    time_scale: f64,
    /// Duration to run (None = until completion or Ctrl+C).
    duration: Option<Duration>,
    /// Dry run mode (validate only).
    dry_run: bool,
}

impl RunCommand {
    /// Create a new run command.
    pub fn new(scenario_path: PathBuf) -> Self {
        Self {
            scenario_path,
            time_scale: 1.0,
            duration: None,
            dry_run: false,
        }
    }

    /// Set the time scale factor.
    pub fn with_time_scale(mut self, scale: f64) -> Self {
        self.time_scale = scale;
        self
    }

    /// Set the duration.
    pub fn with_duration(mut self, duration: Duration) -> Self {
        self.duration = Some(duration);
        self
    }

    /// Enable dry run mode.
    pub fn with_dry_run(mut self, dry_run: bool) -> Self {
        self.dry_run = dry_run;
        self
    }

    /// Load and validate scenario.
    async fn load_scenario(&self, ctx: &CliContext) -> CliResult<ScenarioConfig> {
        let path = ctx.resolve_path(&self.scenario_path);

        if !path.exists() {
            return Err(CliError::ScenarioNotFound { path });
        }

        ctx.vprintln(format!("Loading scenario from: {}", path.display()));

        let content = tokio::fs::read_to_string(&path).await?;
        let config: ScenarioConfig = serde_yaml::from_str(&content)?;

        // Validate scenario
        self.validate_scenario(&config)?;

        Ok(config)
    }

    /// Validate scenario configuration.
    fn validate_scenario(&self, config: &ScenarioConfig) -> CliResult<()> {
        let mut errors: Vec<String> = Vec::new();

        if config.name.is_empty() {
            errors.push("Scenario name is required".to_string());
        }

        if config.devices.is_empty() {
            errors.push("At least one device is required".to_string());
        }

        for (idx, device) in config.devices.iter().enumerate() {
            if device.id.is_empty() {
                errors.push(format!("Device {} has empty ID", idx));
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(CliError::validation_failed(errors))
        }
    }

    /// Run the scenario.
    async fn run_scenario(
        &self,
        ctx: &mut CliContext,
        config: ScenarioConfig,
    ) -> CliResult<CommandOutput> {
        let output = ctx.output();

        // Display scenario info
        output.header("Scenario Configuration");
        output.kv("Name", &config.name);
        output.kv("Devices", config.devices.len());
        output.kv("Time Scale", format!("{}x", self.time_scale));
        if let Some(duration) = self.duration {
            output.kv("Duration", format!("{:?}", duration));
        }

        if self.dry_run {
            output.success("Dry run completed - scenario is valid");
            return Ok(CommandOutput::quiet_success());
        }

        // Create and start engine
        let engine_guard = ctx.engine().await?;
        let mut engine_lock = engine_guard.write().await;
        let engine = engine_lock.as_mut().ok_or_else(|| CliError::ExecutionFailed {
            message: "Engine not initialized".into(),
        })?;

        // Create devices from config
        let spinner = output.spinner("Creating devices...");
        for device_config in &config.devices {
            // TODO: Use actual device factory based on protocol
            ctx.dprintln(format!("Creating device: {}", device_config.id));
        }
        spinner.finish_with_message("Devices created");

        // Start engine
        let spinner = output.spinner("Starting simulator...");
        engine.start().await?;
        spinner.finish_with_message("Simulator started");

        // Display status
        output.header("Simulator Status");

        let table = TableBuilder::new(output.colors_enabled())
            .header(["Protocol", "Devices", "Points", "Status"])
            .status_row(["Modbus TCP", "0", "0", "Ready"], StatusType::Success)
            .status_row(["OPC UA", "0", "0", "Ready"], StatusType::Success)
            .status_row(["BACnet/IP", "0", "0", "Ready"], StatusType::Success)
            .status_row(["KNXnet/IP", "0", "0", "Ready"], StatusType::Success);
        table.print();

        output.info("Press Ctrl+C to stop");

        // Wait for shutdown or duration
        let shutdown = ctx.shutdown_signal();
        if let Some(duration) = self.duration {
            tokio::select! {
                _ = tokio::time::sleep(duration) => {
                    output.info("Duration reached, shutting down...");
                }
                _ = shutdown.notified() => {
                    output.info("Shutdown signal received");
                }
            }
        } else {
            shutdown.notified().await;
        }

        // Stop engine
        engine.stop().await?;
        output.success("Simulator stopped");

        Ok(CommandOutput::quiet_success())
    }
}

#[async_trait]
impl Command for RunCommand {
    fn name(&self) -> &str {
        "run"
    }

    fn description(&self) -> &str {
        "Run a simulation scenario"
    }

    fn requires_engine(&self) -> bool {
        true
    }

    fn supports_shutdown(&self) -> bool {
        true
    }

    fn validate(&self) -> CliResult<()> {
        if self.time_scale <= 0.0 {
            return Err(CliError::InvalidConfig {
                message: "Time scale must be positive".into(),
            });
        }
        Ok(())
    }

    async fn execute(&self, ctx: &mut CliContext) -> CliResult<CommandOutput> {
        let config = self.load_scenario(ctx).await?;
        self.run_scenario(ctx, config).await
    }
}

/// Scenario configuration loaded from file.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct ScenarioConfig {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub devices: Vec<ScenarioDeviceConfig>,
    #[serde(default)]
    pub events: Vec<ScenarioEventConfig>,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct ScenarioDeviceConfig {
    pub id: String,
    pub protocol: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub points: Vec<ScenarioPointConfig>,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct ScenarioPointConfig {
    pub id: String,
    #[serde(default)]
    pub data_type: String,
    #[serde(default)]
    pub initial_value: serde_yaml::Value,
    #[serde(default)]
    pub pattern: Option<String>,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct ScenarioEventConfig {
    pub name: String,
    pub trigger: serde_yaml::Value,
    pub actions: Vec<serde_yaml::Value>,
}

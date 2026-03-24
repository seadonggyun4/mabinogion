use async_trait::async_trait;
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::time::Duration;

use mabi_runtime::{RuntimeExtensions, RuntimeSession};
use mabi_scenario::prelude::{
    ExecutorConfig, ExecutorMetrics, ExecutorState, ScenarioExecutor, ScenarioValidator,
};
use mabi_scenario::{PlayerConfig, Scenario, ScenarioParser};

use crate::context::CliContext;
use crate::error::{CliError, CliResult};
use crate::output::OutputFormat;
use crate::runner::{Command, CommandOutput};
use crate::runtime_registry::workspace_protocol_registry;

#[derive(Debug, Clone, Serialize)]
pub struct ScenarioRunSummary {
    path: String,
    name: String,
    services: usize,
    devices: usize,
    state: String,
    writes_attempted: u64,
    writes_successful: u64,
    writes_failed: u64,
    success_rate: f64,
}

pub async fn run_scenario_on_session(
    ctx: &mut CliContext,
    path: &Path,
    scenario: Scenario,
    session: &RuntimeSession,
    time_scale: f64,
    duration: Option<Duration>,
) -> CliResult<(ScenarioRunSummary, ExecutorMetrics, ExecutorState)> {
    let scenario_name = scenario.name.clone();
    let player_config = PlayerConfig {
        time_scale: if (time_scale - 1.0).abs() > f64::EPSILON {
            Some(time_scale)
        } else {
            None
        },
        max_duration: duration,
    };

    let mut executor = ScenarioExecutor::new_with_player_config(
        scenario,
        ExecutorConfig::default(),
        player_config,
    );
    for (device_id, port) in session.devices().entries() {
        executor.register_device(device_id, port);
    }
    executor
        .validate_devices()
        .map_err(|error| CliError::InvalidScenario {
            message: error.to_string(),
        })?;

    let shutdown = ctx.shutdown_signal();
    let stop_signal = executor.stop_signal();
    let executor_task = tokio::spawn(async move {
        let result = executor.run().await;
        let metrics = executor.metrics();
        let state = executor.state();
        (result, metrics, state)
    });
    tokio::pin!(executor_task);

    let (result, metrics, state) = tokio::select! {
        result = &mut executor_task => result.map_err(|error| CliError::ExecutionFailed {
            message: format!("scenario executor task failed: {}", error),
        })?,
        _ = shutdown.notified() => {
            stop_signal.store(true, Ordering::SeqCst);
            executor_task.await.map_err(|error| CliError::ExecutionFailed {
                message: format!("scenario executor shutdown failed: {}", error),
            })?
        }
    };

    result.map_err(|error| CliError::ExecutionFailed {
        message: error.to_string(),
    })?;

    let summary = ScenarioRunSummary {
        path: path.display().to_string(),
        name: scenario_name,
        services: session.handles().len(),
        devices: session.devices().len(),
        state: format!("{:?}", state).to_lowercase(),
        writes_attempted: metrics.writes_attempted,
        writes_successful: metrics.writes_successful,
        writes_failed: metrics.writes_failed,
        success_rate: metrics.success_rate(),
    };

    Ok((summary, metrics, state))
}

/// Run command for executing scenarios.
pub struct RunCommand {
    scenario_path: PathBuf,
    time_scale: f64,
    duration: Option<Duration>,
    dry_run: bool,
    readiness_timeout: Duration,
}

impl RunCommand {
    pub fn new(scenario_path: PathBuf) -> Self {
        Self {
            scenario_path,
            time_scale: 1.0,
            duration: None,
            dry_run: false,
            readiness_timeout: Duration::from_secs(5),
        }
    }

    pub fn with_time_scale(mut self, scale: f64) -> Self {
        self.time_scale = scale;
        self
    }

    pub fn with_duration(mut self, duration: Duration) -> Self {
        self.duration = Some(duration);
        self
    }

    pub fn with_dry_run(mut self, dry_run: bool) -> Self {
        self.dry_run = dry_run;
        self
    }

    pub fn with_readiness_timeout(mut self, timeout: Duration) -> Self {
        self.readiness_timeout = timeout;
        self
    }

    async fn load_scenario(&self, ctx: &CliContext) -> CliResult<(PathBuf, Scenario)> {
        let path = ctx.resolve_path(&self.scenario_path);
        if !path.exists() {
            return Err(CliError::ScenarioNotFound { path });
        }

        let scenario =
            ScenarioParser::load(&path)
                .await
                .map_err(|error| CliError::InvalidScenario {
                    message: error.to_string(),
                })?;
        ScenarioParser::validate(&scenario).map_err(|error| CliError::InvalidScenario {
            message: error.to_string(),
        })?;

        let validation = ScenarioValidator::new().validate(&scenario);
        if !validation.is_valid() {
            return Err(CliError::validation_failed(validation.errors().iter().map(
                |issue| format!("{} [{:?}] {}", issue.path, issue.code, issue.message),
            )));
        }

        Ok((path, scenario))
    }

    fn render_summary(
        &self,
        ctx: &CliContext,
        summary: &ScenarioRunSummary,
        metrics: &ExecutorMetrics,
    ) -> CliResult<()> {
        if matches!(
            ctx.output().format(),
            OutputFormat::Json | OutputFormat::Yaml | OutputFormat::Compact
        ) {
            ctx.output().write(summary)?;
            return Ok(());
        }

        ctx.output().header("Scenario Run");
        ctx.output().kv("Path", &summary.path);
        ctx.output().kv("Name", &summary.name);
        ctx.output().kv("Services", summary.services);
        ctx.output().kv("Devices", summary.devices);
        ctx.output().kv("State", &summary.state);
        ctx.output()
            .kv("Writes Attempted", summary.writes_attempted);
        ctx.output()
            .kv("Writes Successful", summary.writes_successful);
        ctx.output().kv("Writes Failed", summary.writes_failed);
        ctx.output()
            .kv("Success Rate", format!("{:.2}%", summary.success_rate));
        if metrics.execution_time > Duration::ZERO {
            ctx.output()
                .kv("Execution Time", format!("{:?}", metrics.execution_time));
        }
        Ok(())
    }

    async fn run_loaded_scenario(
        &self,
        ctx: &mut CliContext,
        path: &Path,
        mut scenario: Scenario,
    ) -> CliResult<CommandOutput> {
        let session_spec = scenario
            .runtime
            .clone()
            .ok_or_else(|| CliError::InvalidScenario {
                message: "scenario execution requires a top-level runtime block".into(),
            })?;
        if session_spec.services.is_empty() {
            return Err(CliError::InvalidScenario {
                message: "scenario runtime.services must contain at least one service".into(),
            });
        }

        let registry = workspace_protocol_registry();
        let session = RuntimeSession::new(
            session_spec.clone(),
            &registry,
            RuntimeExtensions::default(),
        )
        .await?;
        session.start(self.readiness_timeout).await?;

        if !ctx.is_quiet() {
            ctx.output().info(format!(
                "Runtime session started with {} service(s)",
                session_spec.services.len()
            ));
        }

        scenario.runtime = Some(session_spec.clone());

        let run_result = run_scenario_on_session(
            ctx,
            path,
            scenario,
            &session,
            self.time_scale,
            self.duration,
        )
        .await;
        let stop_result = session.stop().await;
        let (summary, metrics, state) = run_result?;
        stop_result?;

        self.render_summary(ctx, &summary, &metrics)?;

        match state {
            ExecutorState::Completed | ExecutorState::Idle => Ok(CommandOutput::quiet_success()),
            ExecutorState::Error => Err(CliError::ExecutionFailed {
                message: "scenario executor finished in error state".into(),
            }),
            ExecutorState::Running | ExecutorState::Paused => Err(CliError::ExecutionFailed {
                message: "scenario executor did not settle into a terminal state".into(),
            }),
        }
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
        let (path, scenario) = self.load_scenario(ctx).await?;

        if self.dry_run {
            let summary = ScenarioRunSummary {
                path: path.display().to_string(),
                name: scenario.name.clone(),
                services: scenario
                    .runtime
                    .as_ref()
                    .map(|spec| spec.services.len())
                    .unwrap_or(0),
                devices: scenario.devices.len(),
                state: "validated".into(),
                writes_attempted: 0,
                writes_successful: 0,
                writes_failed: 0,
                success_rate: 100.0,
            };
            self.render_summary(ctx, &summary, &ExecutorMetrics::default())?;
            if !ctx.is_quiet() {
                ctx.output().success("Scenario validation passed");
            }
            return Ok(CommandOutput::quiet_success());
        }

        self.run_loaded_scenario(ctx, &path, scenario).await
    }
}

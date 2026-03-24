//! Command runner and execution framework.
//!
//! Provides the core abstractions for command execution.

use crate::context::CliContext;
use crate::error::{CliError, CliResult};
use async_trait::async_trait;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Trait for executable commands.
///
/// All CLI commands implement this trait for consistent execution.
#[async_trait]
pub trait Command: Send + Sync {
    /// Get the command name.
    fn name(&self) -> &str;

    /// Get the command description.
    fn description(&self) -> &str;

    /// Execute the command.
    async fn execute(&self, ctx: &mut CliContext) -> CliResult<CommandOutput>;

    /// Validate command arguments before execution.
    fn validate(&self) -> CliResult<()> {
        Ok(())
    }

    /// Check if this command requires an engine instance.
    fn requires_engine(&self) -> bool {
        false
    }

    /// Check if this command supports graceful shutdown.
    fn supports_shutdown(&self) -> bool {
        false
    }
}

/// Command output type.
#[derive(Debug, Default)]
pub struct CommandOutput {
    /// Exit code (0 = success).
    pub exit_code: i32,
    /// Optional message.
    pub message: Option<String>,
    /// Whether to suppress default success message.
    pub quiet: bool,
}

impl CommandOutput {
    /// Create a successful output.
    pub fn success() -> Self {
        Self {
            exit_code: 0,
            message: None,
            quiet: false,
        }
    }

    /// Create a successful output with message.
    pub fn success_with_message(msg: impl Into<String>) -> Self {
        Self {
            exit_code: 0,
            message: Some(msg.into()),
            quiet: false,
        }
    }

    /// Create a quiet successful output.
    pub fn quiet_success() -> Self {
        Self {
            exit_code: 0,
            message: None,
            quiet: true,
        }
    }

    /// Create a failed output.
    pub fn failure(code: i32, msg: impl Into<String>) -> Self {
        Self {
            exit_code: code,
            message: Some(msg.into()),
            quiet: false,
        }
    }
}

/// Command runner for executing commands with lifecycle management.
pub struct CommandRunner {
    ctx: Arc<RwLock<CliContext>>,
    hooks: Vec<Box<dyn CommandHook>>,
}

impl CommandRunner {
    /// Create a new command runner.
    pub fn new(ctx: CliContext) -> Self {
        Self {
            ctx: Arc::new(RwLock::new(ctx)),
            hooks: Vec::new(),
        }
    }

    /// Add a command hook.
    pub fn add_hook(&mut self, hook: impl CommandHook + 'static) {
        self.hooks.push(Box::new(hook));
    }

    /// Run a command.
    pub async fn run(&self, cmd: &dyn Command) -> CliResult<CommandOutput> {
        // Validate command
        cmd.validate()?;

        // Run pre-execution hooks
        for hook in &self.hooks {
            hook.before_execute(cmd.name()).await?;
        }

        // Execute command
        let mut ctx = self.ctx.write().await;
        let result = cmd.execute(&mut ctx).await;

        // Run post-execution hooks
        let is_success = result.is_ok();
        for hook in &self.hooks {
            hook.after_execute(cmd.name(), is_success).await?;
        }

        result
    }

    /// Run a command with graceful shutdown support.
    ///
    /// Handles both Ctrl+C (SIGINT) and Ctrl+Z (SIGTSTP) signals.
    /// Ctrl+Z triggers a graceful shutdown instead of suspending the process,
    /// which prevents the zombie-port scenario where a suspended process holds
    /// a port but never processes incoming data.
    pub async fn run_with_shutdown<C: Command>(&self, cmd: &C) -> CliResult<CommandOutput> {
        if !cmd.supports_shutdown() {
            return self.run(cmd).await;
        }

        let shutdown_signal = {
            let ctx = self.ctx.read().await;
            ctx.shutdown_signal()
        };

        // Setup Ctrl+C handler
        let signal = shutdown_signal.clone();
        ctrlc::set_handler(move || {
            signal.notify_waiters();
        })
        .map_err(|e| CliError::ExecutionFailed {
            message: format!("Failed to set Ctrl+C handler: {}", e),
        })?;

        // Setup SIGTSTP handler (Ctrl+Z) — treat as graceful shutdown instead of suspend.
        // A suspended process keeps holding the port, creating a zombie-port scenario
        // that is very difficult to diagnose: TCP connects succeed (kernel handles SYN/ACK)
        // but all application-layer reads time out indefinitely.
        #[cfg(unix)]
        {
            let sigtstp_shutdown = shutdown_signal.clone();
            let mut sigtstp = tokio::signal::unix::signal(
                tokio::signal::unix::SignalKind::from_raw(libc::SIGTSTP),
            )
            .map_err(|e| CliError::ExecutionFailed {
                message: format!("Failed to set SIGTSTP handler: {}", e),
            })?;

            tokio::spawn(async move {
                if sigtstp.recv().await.is_some() {
                    eprintln!(
                        "\n⚠ Received Ctrl+Z (SIGTSTP). Performing graceful shutdown instead of \
                         suspending to release the port.\n  \
                         Use 'kill -STOP <pid>' if you really need to suspend."
                    );
                    sigtstp_shutdown.notify_waiters();
                }
            });
        }

        // The command owns graceful shutdown: the signal handler only flips the
        // shared notify so long-running commands can stop their services cleanly.
        self.run(cmd).await
    }

    /// Get the context.
    pub fn context(&self) -> Arc<RwLock<CliContext>> {
        self.ctx.clone()
    }
}

/// Hook trait for command lifecycle events.
#[async_trait]
pub trait CommandHook: Send + Sync {
    /// Called before command execution.
    async fn before_execute(&self, _cmd_name: &str) -> CliResult<()> {
        Ok(())
    }

    /// Called after command execution.
    async fn after_execute(&self, _cmd_name: &str, _success: bool) -> CliResult<()> {
        Ok(())
    }
}

/// Logging hook for command execution.
pub struct LoggingHook;

#[async_trait]
impl CommandHook for LoggingHook {
    async fn before_execute(&self, cmd_name: &str) -> CliResult<()> {
        tracing::info!(command = cmd_name, "Executing command");
        Ok(())
    }

    async fn after_execute(&self, cmd_name: &str, success: bool) -> CliResult<()> {
        if success {
            tracing::info!(command = cmd_name, "Command completed successfully");
        } else {
            tracing::warn!(command = cmd_name, "Command failed");
        }
        Ok(())
    }
}

/// Metrics hook for command execution.
pub struct MetricsHook {
    start_time: std::sync::Mutex<Option<std::time::Instant>>,
}

impl MetricsHook {
    pub fn new() -> Self {
        Self {
            start_time: std::sync::Mutex::new(None),
        }
    }
}

impl Default for MetricsHook {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl CommandHook for MetricsHook {
    async fn before_execute(&self, _cmd_name: &str) -> CliResult<()> {
        *self.start_time.lock().unwrap() = Some(std::time::Instant::now());
        Ok(())
    }

    async fn after_execute(&self, cmd_name: &str, success: bool) -> CliResult<()> {
        if let Some(start) = self.start_time.lock().unwrap().take() {
            let duration = start.elapsed();
            tracing::debug!(
                command = cmd_name,
                success = success,
                duration_ms = duration.as_millis() as u64,
                "Command execution metrics"
            );
        }
        Ok(())
    }
}

/// Command factory for dynamic command creation.
pub trait CommandFactory: Send + Sync {
    /// Get the protocol this factory supports.
    fn protocol(&self) -> &str;

    /// Create a run command for this protocol.
    fn create_run_command(&self, args: &RunCommandArgs) -> Box<dyn Command>;

    /// Create a list command for this protocol.
    fn create_list_command(&self) -> Box<dyn Command>;

    /// Create a validate command for this protocol.
    fn create_validate_command(&self, path: std::path::PathBuf) -> Box<dyn Command>;
}

/// Arguments for run commands.
#[derive(Debug, Clone)]
pub struct RunCommandArgs {
    pub port: Option<u16>,
    pub devices: usize,
    pub points_per_device: usize,
    pub tick_interval_ms: u64,
}

impl Default for RunCommandArgs {
    fn default() -> Self {
        Self {
            port: None,
            devices: 1,
            points_per_device: 100,
            tick_interval_ms: 100,
        }
    }
}

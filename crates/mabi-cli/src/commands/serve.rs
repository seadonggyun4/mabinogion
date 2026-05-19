use async_trait::async_trait;
use mabi_runtime::{
    ProtocolLaunchSpec, RuntimeExtensions, RuntimeSession, RuntimeSessionSpec, ServiceSnapshot,
};
use tokio::time::Duration;

use crate::context::CliContext;
use crate::error::{CliError, CliResult};
use crate::output::OutputFormat;
use crate::runner::{Command, CommandOutput};
use crate::runtime_registry::workspace_protocol_registry;

/// Runtime-backed serve command for protocol services.
#[derive(Clone)]
pub struct ServeRuntimeCommand {
    launch: ProtocolLaunchSpec,
    readiness_timeout: Duration,
    extensions: RuntimeExtensions,
}

impl ServeRuntimeCommand {
    pub fn new(
        launch: ProtocolLaunchSpec,
        readiness_timeout: Duration,
        extensions: RuntimeExtensions,
    ) -> Self {
        Self {
            launch,
            readiness_timeout,
            extensions,
        }
    }

    fn render_started(&self, ctx: &CliContext, snapshot: &ServiceSnapshot) -> CliResult<()> {
        let output = ctx.output();
        if matches!(
            output.format(),
            OutputFormat::Json | OutputFormat::Yaml | OutputFormat::Compact
        ) {
            output.write(snapshot)?;
            return Ok(());
        }

        output.header(format!("{} Service", snapshot.name));
        if let Some(protocol) = snapshot.protocol {
            output.kv("Protocol", format!("{:?}", protocol));
        }
        output.kv("State", format!("{:?}", snapshot.status.state));
        for (key, value) in &snapshot.metadata {
            if key.starts_with('_') {
                continue;
            }
            match value {
                serde_json::Value::String(value) => output.kv(key, value),
                serde_json::Value::Number(value) => output.kv(key, value),
                serde_json::Value::Bool(value) => output.kv(key, value),
                _ => output.kv(key, value),
            }
        }
        Ok(())
    }
}

#[async_trait]
impl Command for ServeRuntimeCommand {
    fn name(&self) -> &str {
        "serve"
    }

    fn description(&self) -> &str {
        "Serve a protocol simulator through the shared runtime"
    }

    fn supports_shutdown(&self) -> bool {
        true
    }

    async fn execute(&self, ctx: &mut CliContext) -> CliResult<CommandOutput> {
        let registry = workspace_protocol_registry();
        let session = RuntimeSession::new(
            RuntimeSessionSpec {
                services: vec![self.launch.clone()],
                readiness_timeout: Some(self.readiness_timeout.as_millis() as u64),
            },
            &registry,
            self.extensions.clone(),
        )
        .await?;

        session.start(self.readiness_timeout).await?;
        let snapshot = session
            .snapshots()
            .await?
            .into_iter()
            .next()
            .ok_or_else(|| CliError::ExecutionFailed {
                message: "runtime session did not return a service snapshot".into(),
            })?;

        self.render_started(ctx, &snapshot)?;

        if !ctx.is_quiet() {
            ctx.output().info("Press Ctrl+C to stop");
        }

        let shutdown_signal = ctx.shutdown_signal();
        tokio::select! {
            _ = shutdown_signal.notified() => {
                session.stop().await?;
                if !ctx.is_quiet() {
                    ctx.output().success(format!("{} stopped", snapshot.name));
                }
                Ok(CommandOutput::quiet_success())
            }
            result = async {
                for handle in session.handles() {
                    handle.wait().await?;
                }
                Ok::<(), mabi_runtime::RuntimeError>(())
            } => {
                result?;
                let final_snapshot = session
                    .snapshots()
                    .await?
                    .into_iter()
                    .next()
                    .unwrap_or(snapshot);
                if final_snapshot.status.state == mabi_runtime::ServiceState::Stopped {
                    if !ctx.is_quiet() {
                        ctx.output().info(format!("{} exited cleanly", final_snapshot.name));
                    }
                    Ok(CommandOutput::quiet_success())
                } else {
                    Err(CliError::ExecutionFailed {
                        message: format!(
                            "service terminated unexpectedly: {:?}",
                            final_snapshot.status.state
                        ),
                    })
                }
            }
        }
    }
}

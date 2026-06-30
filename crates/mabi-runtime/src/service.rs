use std::collections::BTreeMap;
use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value as JsonValue};
use thiserror::Error;
use tokio::task::{AbortHandle, JoinError, JoinHandle};
use tokio::time::{timeout, Duration};
use tokio_util::sync::CancellationToken;

use mabi_core::Protocol;

use crate::device::DeviceRegistry;

/// Runtime-level result type.
pub type RuntimeResult<T> = Result<T, RuntimeError>;

/// Stable runtime contract version consumed by Imugi and Trials.
pub const RUNTIME_CONTRACT_VERSION: &str = "runtime-contract-v1";

/// Stable service snapshot metadata contract version.
pub const SNAPSHOT_METADATA_VERSION: &str = "snapshot-metadata-v1";

/// Reserved metadata key for runtime-owned service snapshot fields.
pub const RUNTIME_METADATA_KEY: &str = "_runtime";

/// Machine-readable runtime error classification.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeErrorKind {
    ProtocolError,
    ConfigError,
    BindError,
    Timeout,
    InternalError,
}

impl std::fmt::Display for RuntimeErrorKind {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::ProtocolError => "protocol_error",
            Self::ConfigError => "config_error",
            Self::BindError => "bind_error",
            Self::Timeout => "timeout",
            Self::InternalError => "internal_error",
        })
    }
}

/// Structured runtime error payload for machine consumers.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RuntimeErrorInfo {
    pub kind: RuntimeErrorKind,
    pub message: String,
}

/// Runtime-level errors.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum RuntimeError {
    #[error("service error: {message}")]
    Service { message: String },

    #[error("service task failed: {message}")]
    TaskJoin { message: String },

    #[error("service readiness timed out after {seconds}s")]
    ReadinessTimeout { seconds: u64 },

    #[error("{kind}: {message}")]
    Classified {
        kind: RuntimeErrorKind,
        message: String,
    },
}

impl RuntimeError {
    /// Convenience constructor for message-based errors.
    pub fn service(message: impl Into<String>) -> Self {
        Self::Service {
            message: message.into(),
        }
    }

    /// Creates a protocol-level runtime error.
    pub fn protocol(message: impl Into<String>) -> Self {
        Self::classified(RuntimeErrorKind::ProtocolError, message)
    }

    /// Creates a configuration-level runtime error.
    pub fn config(message: impl Into<String>) -> Self {
        Self::classified(RuntimeErrorKind::ConfigError, message)
    }

    /// Creates a bind/listen/address allocation runtime error.
    pub fn bind(message: impl Into<String>) -> Self {
        Self::classified(RuntimeErrorKind::BindError, message)
    }

    /// Creates a timeout runtime error.
    pub fn timeout(message: impl Into<String>) -> Self {
        Self::classified(RuntimeErrorKind::Timeout, message)
    }

    /// Creates an internal runtime error.
    pub fn internal(message: impl Into<String>) -> Self {
        Self::classified(RuntimeErrorKind::InternalError, message)
    }

    fn classified(kind: RuntimeErrorKind, message: impl Into<String>) -> Self {
        Self::Classified {
            kind,
            message: message.into(),
        }
    }

    /// Returns the stable machine-readable error kind.
    pub fn kind(&self) -> RuntimeErrorKind {
        match self {
            Self::Service { .. } | Self::TaskJoin { .. } => RuntimeErrorKind::InternalError,
            Self::ReadinessTimeout { .. } => RuntimeErrorKind::Timeout,
            Self::Classified { kind, .. } => *kind,
        }
    }

    /// Returns the human-readable error message without losing the stable kind.
    pub fn message(&self) -> String {
        match self {
            Self::Service { message }
            | Self::TaskJoin { message }
            | Self::Classified { message, .. } => message.clone(),
            Self::ReadinessTimeout { seconds } => {
                format!("service readiness timed out after {seconds}s")
            }
        }
    }

    /// Returns the structured runtime error payload.
    pub fn info(&self) -> RuntimeErrorInfo {
        RuntimeErrorInfo {
            kind: self.kind(),
            message: self.message(),
        }
    }
}

impl From<JoinError> for RuntimeError {
    fn from(error: JoinError) -> Self {
        Self::internal(format!("service task failed: {error}"))
    }
}

/// Shared service lifecycle states.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum ServiceState {
    #[default]
    Idle,
    Starting,
    Running,
    Stopping,
    Stopped,
    Error,
}

/// Current service status snapshot.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceStatus {
    pub name: String,
    pub protocol: Option<Protocol>,
    pub state: ServiceState,
    pub ready: bool,
    pub started_at: Option<DateTime<Utc>>,
    pub last_error: Option<String>,
}

impl ServiceStatus {
    /// Creates a fresh idle status.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            protocol: None,
            state: ServiceState::Idle,
            ready: false,
            started_at: None,
            last_error: None,
        }
    }

    /// Returns true when the service is terminal.
    pub fn is_terminal(&self) -> bool {
        matches!(self.state, ServiceState::Stopped | ServiceState::Error)
    }
}

/// Structured snapshot used by the CLI and tests.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceSnapshot {
    pub name: String,
    pub protocol: Option<Protocol>,
    pub status: ServiceStatus,
    #[serde(default)]
    pub metadata: BTreeMap<String, JsonValue>,
}

impl ServiceSnapshot {
    /// Creates an empty snapshot.
    pub fn new(name: impl Into<String>) -> Self {
        let name = name.into();
        Self {
            status: ServiceStatus::new(name.clone()),
            name,
            protocol: None,
            metadata: BTreeMap::new(),
        }
    }

    /// Adds metadata to the snapshot.
    pub fn with_metadata(mut self, key: impl Into<String>, value: JsonValue) -> Self {
        self.metadata.insert(key.into(), value);
        self
    }

    /// Adds or refreshes runtime-owned metadata under the reserved `_runtime` key.
    pub fn with_runtime_metadata(mut self) -> Self {
        self.ensure_runtime_metadata();
        self
    }

    /// Ensures runtime-owned metadata exists under the reserved `_runtime` key.
    pub fn ensure_runtime_metadata(&mut self) {
        let metadata = ServiceRuntimeMetadata::from_snapshot(self);
        self.metadata
            .insert(RUNTIME_METADATA_KEY.to_string(), json!(metadata));
    }

    /// Returns parsed runtime metadata when present.
    pub fn runtime_metadata(&self) -> Option<ServiceRuntimeMetadata> {
        self.metadata
            .get(RUNTIME_METADATA_KEY)
            .and_then(|value| serde_json::from_value(value.clone()).ok())
    }
}

/// Runtime-owned stable service snapshot metadata.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ServiceRuntimeMetadata {
    pub contract_version: String,
    pub snapshot_metadata_version: String,
    pub captured_at: DateTime<Utc>,
    pub service_name: String,
    pub protocol: Option<String>,
    pub state: ServiceState,
    pub ready: bool,
    pub started_at: Option<DateTime<Utc>>,
    pub last_error: Option<String>,
}

impl ServiceRuntimeMetadata {
    /// Builds runtime metadata from the current service snapshot.
    pub fn from_snapshot(snapshot: &ServiceSnapshot) -> Self {
        let protocol = snapshot
            .status
            .protocol
            .or(snapshot.protocol)
            .map(|protocol| protocol.to_string());
        Self {
            contract_version: RUNTIME_CONTRACT_VERSION.to_string(),
            snapshot_metadata_version: SNAPSHOT_METADATA_VERSION.to_string(),
            captured_at: Utc::now(),
            service_name: snapshot.status.name.clone(),
            protocol,
            state: snapshot.status.state,
            ready: snapshot.status.ready,
            started_at: snapshot.status.started_at,
            last_error: snapshot.status.last_error.clone(),
        }
    }
}

/// Structured readiness report for runner-facing health checks.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ServiceReadinessReport {
    pub contract_version: String,
    pub checked_at: DateTime<Utc>,
    pub service_name: String,
    pub protocol: Option<String>,
    pub state: ServiceState,
    pub ready: bool,
    pub timeout_ms: u64,
    pub error: Option<RuntimeErrorInfo>,
}

impl ServiceReadinessReport {
    /// Builds a readiness report from a status and optional error.
    pub fn from_status(
        status: ServiceStatus,
        timeout: Duration,
        error: Option<RuntimeErrorInfo>,
    ) -> Self {
        Self {
            contract_version: RUNTIME_CONTRACT_VERSION.to_string(),
            checked_at: Utc::now(),
            service_name: status.name,
            protocol: status.protocol.map(|protocol| protocol.to_string()),
            state: status.state,
            ready: status.ready,
            timeout_ms: timeout.as_millis() as u64,
            error,
        }
    }
}

/// Events emitted by the shared runtime context.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServiceEvent {
    StateChanged { state: ServiceState },
    Cancelled,
    Message { message: String },
}

#[derive(Debug, Clone)]
struct TrackedTask {
    label: String,
    abort: AbortHandle,
}

#[derive(Debug)]
struct ServiceContextInner {
    name: String,
    protocol: Option<Protocol>,
    started_at: DateTime<Utc>,
    cancellation: CancellationToken,
    event_tx: tokio::sync::broadcast::Sender<ServiceEvent>,
    tracked_tasks: Mutex<Vec<TrackedTask>>,
}

/// Shared runtime context provided to all managed services.
#[derive(Clone, Debug)]
pub struct ServiceContext {
    inner: Arc<ServiceContextInner>,
}

impl ServiceContext {
    /// Creates a new service context.
    pub fn new(name: impl Into<String>, protocol: Option<Protocol>) -> Self {
        let (event_tx, _) = tokio::sync::broadcast::channel(64);
        Self {
            inner: Arc::new(ServiceContextInner {
                name: name.into(),
                protocol,
                started_at: Utc::now(),
                cancellation: CancellationToken::new(),
                event_tx,
                tracked_tasks: Mutex::new(Vec::new()),
            }),
        }
    }

    /// Returns the service name.
    pub fn name(&self) -> &str {
        &self.inner.name
    }

    /// Returns the service protocol, if one exists.
    pub fn protocol(&self) -> Option<Protocol> {
        self.inner.protocol
    }

    /// Returns when the context was created.
    pub fn started_at(&self) -> DateTime<Utc> {
        self.inner.started_at
    }

    /// Returns the shared cancellation token.
    pub fn cancellation_token(&self) -> CancellationToken {
        self.inner.cancellation.clone()
    }

    /// Returns a child token for scoped tasks.
    pub fn child_token(&self) -> CancellationToken {
        self.inner.cancellation.child_token()
    }

    /// Cancels the context and all child scopes.
    pub fn cancel(&self) {
        self.inner.cancellation.cancel();
        let _ = self.emit(ServiceEvent::Cancelled);
    }

    /// Returns whether cancellation has been requested.
    pub fn is_cancelled(&self) -> bool {
        self.inner.cancellation.is_cancelled()
    }

    /// Subscribes to service events.
    pub fn subscribe(&self) -> tokio::sync::broadcast::Receiver<ServiceEvent> {
        self.inner.event_tx.subscribe()
    }

    /// Emits a service event.
    pub fn emit(
        &self,
        event: ServiceEvent,
    ) -> Result<usize, tokio::sync::broadcast::error::SendError<ServiceEvent>> {
        self.inner.event_tx.send(event)
    }

    /// Tracks an externally-spawned task under the context.
    pub fn track_task(&self, label: impl Into<String>, handle: &JoinHandle<()>) {
        self.inner.tracked_tasks.lock().push(TrackedTask {
            label: label.into(),
            abort: handle.abort_handle(),
        });
    }

    /// Spawns and tracks a unit-returning background task.
    pub fn spawn_task<F>(&self, label: impl Into<String>, future: F) -> JoinHandle<()>
    where
        F: std::future::Future<Output = ()> + Send + 'static,
    {
        let label = label.into();
        let handle = tokio::spawn(future);
        self.inner.tracked_tasks.lock().push(TrackedTask {
            label,
            abort: handle.abort_handle(),
        });
        handle
    }

    /// Returns the tracked task labels.
    pub fn tracked_tasks(&self) -> Vec<String> {
        self.inner
            .tracked_tasks
            .lock()
            .iter()
            .map(|task| task.label.clone())
            .collect()
    }

    /// Aborts all tracked tasks.
    pub fn abort_tracked_tasks(&self) {
        for task in self.inner.tracked_tasks.lock().iter() {
            task.abort.abort();
        }
    }
}

/// Shared lifecycle contract for protocol services.
#[async_trait]
pub trait ManagedService: Send + Sync {
    /// Performs any non-blocking startup work.
    async fn start(&self, context: &ServiceContext) -> RuntimeResult<()>;

    /// Requests a graceful stop.
    async fn stop(&self, context: &ServiceContext) -> RuntimeResult<()>;

    /// Runs the service until completion or cancellation.
    async fn serve(&self, context: ServiceContext) -> RuntimeResult<()>;

    /// Returns the current status.
    fn status(&self) -> ServiceStatus;

    /// Returns a structured snapshot.
    async fn snapshot(&self) -> RuntimeResult<ServiceSnapshot>;

    /// Publishes any controller-visible device ports exposed by this service.
    fn register_devices(&self, _registry: &DeviceRegistry) -> RuntimeResult<()> {
        Ok(())
    }
}

/// Shared handle for spawning, stopping, and inspecting managed services.
pub struct ServiceHandle {
    service: Arc<dyn ManagedService>,
    context: ServiceContext,
    task: Arc<tokio::sync::Mutex<Option<JoinHandle<RuntimeResult<()>>>>>,
}

impl ServiceHandle {
    /// Creates a new handle around a service and context.
    pub fn new(service: Arc<dyn ManagedService>, context: ServiceContext) -> Self {
        Self {
            service,
            context,
            task: Arc::new(tokio::sync::Mutex::new(None)),
        }
    }

    /// Creates a handle for a named service.
    pub fn named(
        name: impl Into<String>,
        protocol: Option<Protocol>,
        service: Arc<dyn ManagedService>,
    ) -> Self {
        Self::new(service, ServiceContext::new(name, protocol))
    }

    /// Returns the shared service context.
    pub fn context(&self) -> ServiceContext {
        self.context.clone()
    }

    /// Spawns the service task if it is not already running.
    pub async fn spawn(&self) -> RuntimeResult<()> {
        let mut guard = self.task.lock().await;
        if guard.is_some() {
            return Ok(());
        }

        self.service.start(&self.context).await?;

        let service = self.service.clone();
        let context = self.context.clone();
        *guard = Some(tokio::spawn(async move { service.serve(context).await }));
        Ok(())
    }

    /// Requests service shutdown and waits for the service task.
    pub async fn stop(&self) -> RuntimeResult<()> {
        self.context.cancel();
        self.service.stop(&self.context).await?;
        self.context.abort_tracked_tasks();

        if let Some(handle) = self.task.lock().await.take() {
            handle.await??;
        }

        Ok(())
    }

    /// Waits for the service task to finish if it was spawned.
    pub async fn wait(&self) -> RuntimeResult<()> {
        if let Some(handle) = self.task.lock().await.take() {
            handle.await??;
        }
        Ok(())
    }

    /// Waits until the service reports readiness or the timeout elapses.
    pub async fn readiness(&self, max_wait: Duration) -> RuntimeResult<ServiceStatus> {
        let service = self.service.clone();
        timeout(max_wait, async move {
            loop {
                let status = service.status();
                if status.ready || status.is_terminal() {
                    return status;
                }
                tokio::time::sleep(Duration::from_millis(25)).await;
            }
        })
        .await
        .map_err(|_| {
            RuntimeError::timeout(format!(
                "service readiness timed out after {}ms",
                max_wait.as_millis()
            ))
        })
    }

    /// Returns a structured readiness report without discarding status context.
    pub async fn readiness_report(&self, max_wait: Duration) -> ServiceReadinessReport {
        match self.readiness(max_wait).await {
            Ok(status) => ServiceReadinessReport::from_status(status, max_wait, None),
            Err(error) => {
                let status = self.status();
                ServiceReadinessReport::from_status(status, max_wait, Some(error.info()))
            }
        }
    }

    /// Returns the latest status.
    pub fn status(&self) -> ServiceStatus {
        self.service.status()
    }

    /// Returns the latest snapshot.
    pub async fn snapshot(&self) -> RuntimeResult<ServiceSnapshot> {
        Ok(self.service.snapshot().await?.with_runtime_metadata())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use async_trait::async_trait;
    use tokio::time::Duration;

    use crate::service::{
        ManagedService, RuntimeError, RuntimeErrorKind, RuntimeResult, ServiceContext,
        ServiceHandle, ServiceSnapshot, ServiceState, ServiceStatus, RUNTIME_CONTRACT_VERSION,
        RUNTIME_METADATA_KEY, SNAPSHOT_METADATA_VERSION,
    };

    struct TestService {
        status: parking_lot::RwLock<ServiceStatus>,
    }

    impl TestService {
        fn new() -> Self {
            Self {
                status: parking_lot::RwLock::new(ServiceStatus::new("test")),
            }
        }
    }

    #[async_trait]
    impl ManagedService for TestService {
        async fn start(&self, context: &ServiceContext) -> RuntimeResult<()> {
            let mut status = self.status.write();
            status.state = ServiceState::Starting;
            status.started_at = Some(context.started_at());
            Ok(())
        }

        async fn stop(&self, _context: &ServiceContext) -> RuntimeResult<()> {
            let mut status = self.status.write();
            status.state = ServiceState::Stopped;
            status.ready = false;
            Ok(())
        }

        async fn serve(&self, context: ServiceContext) -> RuntimeResult<()> {
            {
                let mut status = self.status.write();
                status.state = ServiceState::Running;
                status.ready = true;
            }
            context.cancellation_token().cancelled().await;
            let mut status = self.status.write();
            status.state = ServiceState::Stopped;
            status.ready = false;
            Ok(())
        }

        fn status(&self) -> ServiceStatus {
            self.status.read().clone()
        }

        async fn snapshot(&self) -> RuntimeResult<ServiceSnapshot> {
            let mut snapshot = ServiceSnapshot::new("test");
            snapshot.status = self.status();
            Ok(snapshot)
        }
    }

    #[tokio::test]
    async fn handle_spawns_and_stops_service() {
        let service = Arc::new(TestService::new());
        let handle = ServiceHandle::named("test", None, service);
        handle.spawn().await.unwrap();
        let status = handle.readiness(Duration::from_secs(1)).await.unwrap();
        assert!(status.ready);
        let report = handle.readiness_report(Duration::from_secs(1)).await;
        assert!(report.ready);
        assert_eq!(report.contract_version, RUNTIME_CONTRACT_VERSION);
        assert!(serde_json::to_value(&report).unwrap()["checked_at"].is_string());

        let snapshot = handle.snapshot().await.unwrap();
        assert!(snapshot.metadata.contains_key(RUNTIME_METADATA_KEY));
        let runtime = snapshot.runtime_metadata().expect("runtime metadata");
        assert_eq!(runtime.contract_version, RUNTIME_CONTRACT_VERSION);
        assert_eq!(runtime.snapshot_metadata_version, SNAPSHOT_METADATA_VERSION);
        assert_eq!(runtime.service_name, "test");
        assert!(runtime.ready);

        handle.stop().await.unwrap();
        assert_eq!(handle.status().state, ServiceState::Stopped);
    }

    #[test]
    fn runtime_error_info_uses_stable_kinds() {
        let error = RuntimeError::config("invalid launch config");
        assert_eq!(error.kind(), RuntimeErrorKind::ConfigError);
        assert_eq!(error.info().message, "invalid launch config");

        let value = serde_json::to_value(error.info()).unwrap();
        assert_eq!(value["kind"], "config_error");
        assert_eq!(value["message"], "invalid launch config");
    }
}

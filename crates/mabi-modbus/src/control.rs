//! In-process control-plane ports for DX-oriented simulator workflows.

use std::collections::VecDeque;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use parking_lot::Mutex;
use serde::Serialize;
use tokio::sync::broadcast;

use mabi_core::device::DeviceInfo;
use mabi_core::tags::Tags;
use mabi_core::types::{AccessMode, Address, DataPoint, DataPointDef, ModbusRegisterType};
use mabi_core::value::Value;
use mabi_runtime::{
    DevicePort, DevicePortLayer, DynDevicePort, ProtocolDriverRegistry, RuntimeSession,
    RuntimeSessionSpec,
};

use crate::error::{ModbusError, ModbusResult};
use crate::simulator::{
    ActionBindingSummary, BehaviorBindingSummary, CompiledModbusSession, DatastorePolicySummary,
};

/// Lifecycle-oriented control surface for a simulator session.
#[async_trait]
pub trait SessionControlPort: Send {
    async fn status(&self) -> ModbusResult<SessionStatus>;
    async fn snapshot(&self) -> ModbusResult<SessionSnapshot>;
    async fn reset(&mut self) -> ModbusResult<SessionSnapshot>;
}

/// Point catalog surface used by CLI inspection and filtering.
pub trait PointCatalogPort {
    fn list_points(&self, query: &PointCatalogQuery) -> ModbusResult<Vec<PointDescriptor>>;
}

/// Point-oriented read/write surface used by CLI control commands.
#[async_trait]
pub trait RegisterControlPort {
    async fn read(&self, target: &PointTarget) -> ModbusResult<DataPoint>;
    async fn write(&self, target: &PointTarget, value: Value) -> ModbusResult<()>;
}

/// Trace inspection surface for recent control-plane activity.
pub trait TracePort {
    fn tail(&self, limit: usize) -> Vec<TraceEntry>;
    fn clear(&self);
    fn subscribe(&self) -> broadcast::Receiver<TraceEntry>;
}

/// Named fault preset control surface.
#[async_trait]
pub trait FaultPresetPort: Send {
    fn available_fault_presets(&self) -> Vec<String>;
    fn active_fault_preset(&self) -> Option<String>;
    async fn apply_fault_preset(&mut self, name: &str) -> ModbusResult<SessionSnapshot>;
    async fn clear_fault_preset(&mut self) -> ModbusResult<SessionSnapshot>;
}

/// Named response profile control surface.
#[async_trait]
pub trait ResponseProfilePort: Send {
    fn available_response_profiles(&self) -> Vec<String>;
    fn active_response_profile(&self) -> Option<String>;
    async fn apply_response_profile(&mut self, name: &str) -> ModbusResult<SessionSnapshot>;
    async fn clear_response_profile(&mut self) -> ModbusResult<SessionSnapshot>;
}

/// Named behavior-set control surface.
#[async_trait]
pub trait BehaviorSetPort: Send {
    fn available_behavior_sets(&self) -> Vec<String>;
    fn active_behavior_set(&self) -> Option<String>;
    async fn apply_behavior_set(&mut self, name: &str) -> ModbusResult<SessionSnapshot>;
    async fn clear_behavior_set(&mut self) -> ModbusResult<SessionSnapshot>;
}

/// Static simulator metadata exposed through the control plane.
pub trait SessionMetadataPort {
    fn action_binding_summaries(&self) -> &[ActionBindingSummary];
    fn behavior_binding_summaries(&self) -> &[BehaviorBindingSummary];
    fn datastore_policy_summaries(&self) -> &[DatastorePolicySummary];
}

/// Query parameters for point catalog operations.
#[derive(Debug, Clone, Default)]
pub struct PointCatalogQuery {
    pub device_id: Option<String>,
    pub tag_filters: Vec<(String, String)>,
    pub labels: Vec<String>,
}

/// Stable point catalog record for operator-facing control flows.
#[derive(Debug, Clone, Serialize)]
pub struct PointDescriptor {
    pub device_id: String,
    pub device_name: String,
    pub unit_id: Option<u8>,
    pub point_id: String,
    pub point_name: String,
    pub register_type: Option<ModbusRegisterType>,
    pub address: Option<u16>,
    pub data_type: String,
    pub access: String,
    pub read_only: bool,
    pub invalid: bool,
    pub action_bindings: Vec<String>,
    pub behavior_bindings: Vec<String>,
    pub source_datastore: Option<String>,
    pub tags: Tags,
}

/// Point selection used by read/write commands.
#[derive(Debug, Clone, Default)]
pub struct PointTarget {
    pub device_id: Option<String>,
    pub point_id: Option<String>,
    pub unit_id: Option<u8>,
    pub register_type: Option<ModbusRegisterType>,
    pub address: Option<u16>,
}

#[derive(Debug, Clone)]
struct ResolvedPointTarget {
    device_id: String,
    point_id: String,
}

/// Human-oriented session status for CLI output.
#[derive(Debug, Clone, Serialize)]
pub struct SessionStatus {
    pub session_name: String,
    pub active_fault_preset: Option<String>,
    pub active_response_profile: Option<String>,
    pub active_behavior_set: Option<String>,
    pub trace_enabled: bool,
    pub trace_entries: usize,
    pub services: usize,
    pub devices: usize,
}

/// Snapshot surface returned by reset/fault operations.
#[derive(Debug, Clone, Serialize)]
pub struct SessionSnapshot {
    pub status: SessionStatus,
    pub services: Vec<mabi_runtime::ServiceSnapshot>,
}

/// Trace operation kind.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TraceOperation {
    Read,
    Write,
}

/// Trace entry status.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TraceStatus {
    Ok,
    Error,
}

/// In-memory control-plane trace record.
#[derive(Debug, Clone, Serialize)]
pub struct TraceEntry {
    pub device_id: String,
    pub point_id: String,
    pub operation: TraceOperation,
    pub status: TraceStatus,
    pub value: Option<serde_json::Value>,
    pub error: Option<String>,
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

#[derive(Default)]
struct TraceState {
    entries: VecDeque<TraceEntry>,
}

/// Bounded trace store with SSE-like subscription for CLI consumers.
pub struct TraceStore {
    capacity: usize,
    state: Mutex<TraceState>,
    tx: broadcast::Sender<TraceEntry>,
}

impl TraceStore {
    pub fn new(capacity: usize) -> Self {
        let (tx, _) = broadcast::channel(capacity.max(16));
        Self {
            capacity: capacity.max(1),
            state: Mutex::new(TraceState::default()),
            tx,
        }
    }

    pub fn record(&self, entry: TraceEntry) {
        let mut state = self.state.lock();
        if state.entries.len() == self.capacity {
            state.entries.pop_front();
        }
        state.entries.push_back(entry.clone());
        let _ = self.tx.send(entry);
    }

    pub fn tail(&self, limit: usize) -> Vec<TraceEntry> {
        let state = self.state.lock();
        let take = limit.max(1).min(state.entries.len());
        state
            .entries
            .iter()
            .rev()
            .take(take)
            .cloned()
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect()
    }

    pub fn clear(&self) {
        self.state.lock().entries.clear();
    }

    pub fn len(&self) -> usize {
        self.state.lock().entries.len()
    }

    pub fn subscribe(&self) -> broadcast::Receiver<TraceEntry> {
        self.tx.subscribe()
    }
}

/// Device-layer decorator that records point reads and writes.
pub struct TraceLayer {
    store: Arc<TraceStore>,
}

impl TraceLayer {
    pub fn new(store: Arc<TraceStore>) -> Self {
        Self { store }
    }
}

impl DevicePortLayer for TraceLayer {
    fn decorate(
        &self,
        _protocol: Option<mabi_core::Protocol>,
        port: DynDevicePort,
    ) -> DynDevicePort {
        Arc::new(TracedDevicePort {
            inner: port,
            store: Arc::clone(&self.store),
        })
    }
}

struct TracedDevicePort {
    inner: DynDevicePort,
    store: Arc<TraceStore>,
}

#[async_trait]
impl DevicePort for TracedDevicePort {
    fn info(&self) -> DeviceInfo {
        self.inner.info()
    }

    async fn start(&self) -> mabi_core::Result<()> {
        self.inner.start().await
    }

    async fn stop(&self) -> mabi_core::Result<()> {
        self.inner.stop().await
    }

    async fn read(&self, point_id: &str) -> mabi_core::Result<DataPoint> {
        let result = self.inner.read(point_id).await;
        self.store.record(match &result {
            Ok(point) => TraceEntry {
                device_id: point.id.device_id.clone(),
                point_id: point.id.point_id.clone(),
                operation: TraceOperation::Read,
                status: TraceStatus::Ok,
                value: serde_json::to_value(&point.value).ok(),
                error: None,
                timestamp: chrono::Utc::now(),
            },
            Err(error) => TraceEntry {
                device_id: self.inner.id(),
                point_id: point_id.to_string(),
                operation: TraceOperation::Read,
                status: TraceStatus::Error,
                value: None,
                error: Some(error.to_string()),
                timestamp: chrono::Utc::now(),
            },
        });
        result
    }

    async fn write(&self, point_id: &str, value: Value) -> mabi_core::Result<()> {
        let json_value = serde_json::to_value(&value).ok();
        let result = self.inner.write(point_id, value).await;
        self.store.record(match &result {
            Ok(()) => TraceEntry {
                device_id: self.inner.id(),
                point_id: point_id.to_string(),
                operation: TraceOperation::Write,
                status: TraceStatus::Ok,
                value: json_value,
                error: None,
                timestamp: chrono::Utc::now(),
            },
            Err(error) => TraceEntry {
                device_id: self.inner.id(),
                point_id: point_id.to_string(),
                operation: TraceOperation::Write,
                status: TraceStatus::Error,
                value: json_value,
                error: Some(error.to_string()),
                timestamp: chrono::Utc::now(),
            },
        });
        result
    }

    fn point_definitions(&self) -> Vec<DataPointDef> {
        self.inner.point_definitions()
    }
}

/// In-process control session over a compiled simulator session.
pub struct ModbusControlSession {
    registry: ProtocolDriverRegistry,
    compiled: CompiledModbusSession,
    fallback_readiness_timeout: Duration,
    trace_store: Arc<TraceStore>,
    runtime_session: RuntimeSession,
}

impl ModbusControlSession {
    pub async fn new(
        registry: ProtocolDriverRegistry,
        compiled: CompiledModbusSession,
        fallback_readiness_timeout: Duration,
    ) -> ModbusResult<Self> {
        let trace_store = Arc::new(TraceStore::new(compiled.trace.buffer_capacity()));
        let runtime_session = Self::start_runtime(
            &registry,
            &compiled,
            Arc::clone(&trace_store),
            fallback_readiness_timeout,
        )
        .await?;

        Ok(Self {
            registry,
            compiled,
            fallback_readiness_timeout,
            trace_store,
            runtime_session,
        })
    }

    async fn start_runtime(
        registry: &ProtocolDriverRegistry,
        compiled: &CompiledModbusSession,
        trace_store: Arc<TraceStore>,
        fallback_readiness_timeout: Duration,
    ) -> ModbusResult<RuntimeSession> {
        let mut extensions = compiled.runtime_extensions();
        if compiled.trace.enabled {
            extensions.add_device_layer(Arc::new(TraceLayer::new(trace_store)));
        }

        let session = RuntimeSession::new(
            RuntimeSessionSpec {
                services: vec![compiled.launch.clone()],
                readiness_timeout: compiled.readiness_timeout_ms,
            },
            registry,
            extensions,
        )
        .await
        .map_err(|error| ModbusError::Server(error.to_string()))?;
        session
            .start(fallback_readiness_timeout)
            .await
            .map_err(|error| ModbusError::Server(error.to_string()))?;
        Ok(session)
    }

    async fn rebuild(
        &mut self,
        compiled: CompiledModbusSession,
        clear_trace: bool,
    ) -> ModbusResult<()> {
        self.runtime_session
            .stop()
            .await
            .map_err(|error| ModbusError::Server(error.to_string()))?;
        if clear_trace {
            self.trace_store.clear();
        }

        let runtime_session = Self::start_runtime(
            &self.registry,
            &compiled,
            Arc::clone(&self.trace_store),
            self.fallback_readiness_timeout,
        )
        .await?;
        self.compiled = compiled;
        self.runtime_session = runtime_session;
        Ok(())
    }

    pub async fn stop(&self) -> ModbusResult<()> {
        self.runtime_session
            .stop()
            .await
            .map_err(|error| ModbusError::Server(error.to_string()))
    }

    fn resolve_target(&self, target: &PointTarget) -> ModbusResult<ResolvedPointTarget> {
        if let (Some(device_id), Some(point_id)) = (&target.device_id, &target.point_id) {
            return Ok(ResolvedPointTarget {
                device_id: device_id.clone(),
                point_id: point_id.clone(),
            });
        }

        let descriptors = self.list_points(&PointCatalogQuery {
            device_id: target.device_id.clone(),
            ..Default::default()
        })?;

        let mut matches = descriptors
            .into_iter()
            .filter(|descriptor| {
                let point_match = target
                    .point_id
                    .as_ref()
                    .map(|point_id| descriptor.point_id == *point_id)
                    .unwrap_or(true);
                let unit_match = target
                    .unit_id
                    .map(|unit_id| descriptor.unit_id == Some(unit_id))
                    .unwrap_or(true);
                let register_type_match = target
                    .register_type
                    .map(|register_type| descriptor.register_type == Some(register_type))
                    .unwrap_or(true);
                let address_match = target
                    .address
                    .map(|address| descriptor.address == Some(address))
                    .unwrap_or(true);
                point_match && unit_match && register_type_match && address_match
            })
            .map(|descriptor| ResolvedPointTarget {
                device_id: descriptor.device_id,
                point_id: descriptor.point_id,
            })
            .collect::<Vec<_>>();

        if matches.is_empty() {
            return Err(ModbusError::Config(
                "no point matched the supplied selector".into(),
            ));
        }
        if matches.len() > 1 {
            return Err(ModbusError::Config(
                "point selector matched more than one point; add --device or --unit".into(),
            ));
        }
        Ok(matches.remove(0))
    }

    fn matches_query(info: &DeviceInfo, query: &PointCatalogQuery) -> bool {
        if let Some(device_id) = &query.device_id {
            if &info.id != device_id {
                return false;
            }
        }

        if !query
            .tag_filters
            .iter()
            .all(|(key, value)| info.tags.get(key) == Some(value.as_str()))
        {
            return false;
        }

        if !query
            .labels
            .iter()
            .all(|label| info.tags.has_label(label.as_str()))
        {
            return false;
        }

        true
    }
}

#[async_trait]
impl SessionControlPort for ModbusControlSession {
    async fn status(&self) -> ModbusResult<SessionStatus> {
        Ok(SessionStatus {
            session_name: self.compiled.session_name.clone(),
            active_fault_preset: self.compiled.active_fault_preset.clone(),
            active_response_profile: self.compiled.active_response_profile.clone(),
            active_behavior_set: self.compiled.active_behavior_set.clone(),
            trace_enabled: self.compiled.trace.enabled,
            trace_entries: self.trace_store.len(),
            services: self.runtime_session.handles().len(),
            devices: self.runtime_session.devices().len(),
        })
    }

    async fn snapshot(&self) -> ModbusResult<SessionSnapshot> {
        let status = self.status().await?;
        let services = self
            .runtime_session
            .snapshots()
            .await
            .map_err(|error| ModbusError::Server(error.to_string()))?;
        Ok(SessionSnapshot { status, services })
    }

    async fn reset(&mut self) -> ModbusResult<SessionSnapshot> {
        let mut compiled = self.compiled.clone();
        if self.compiled.reset.clear_fault_preset {
            compiled = compiled.with_active_fault_preset(None)?;
        }
        if self.compiled.reset.clear_response_profile {
            compiled = compiled.with_active_response_profile(None)?;
        }
        if self.compiled.reset.clear_behavior_set {
            compiled = compiled.with_active_behavior_set(None)?;
        }
        self.rebuild(compiled, self.compiled.reset.clear_trace_buffer)
            .await?;
        self.snapshot().await
    }
}

impl PointCatalogPort for ModbusControlSession {
    fn list_points(&self, query: &PointCatalogQuery) -> ModbusResult<Vec<PointDescriptor>> {
        let mut points = Vec::new();

        for (_device_id, port) in self.runtime_session.devices().entries() {
            let info = port.info();
            if !Self::matches_query(&info, query) {
                continue;
            }

            let unit_id = info
                .metadata
                .get("unit_id")
                .and_then(|value| value.parse::<u8>().ok());
            for point in port.point_definitions() {
                let (register_type, address) = match point.address.clone() {
                    Some(Address::Modbus(address)) => {
                        (Some(address.register_type), Some(address.address))
                    }
                    _ => (None, None),
                };
                points.push(PointDescriptor {
                    device_id: info.id.clone(),
                    device_name: info.name.clone(),
                    unit_id,
                    point_id: point.id.clone(),
                    point_name: point.name.clone(),
                    register_type,
                    address,
                    data_type: format!("{:?}", point.data_type),
                    access: access_mode_name(point.access),
                    read_only: self
                        .compiled
                        .point_metadata(&info.id, &point.id)
                        .map(|metadata| metadata.read_only)
                        .unwrap_or(matches!(point.access, AccessMode::ReadOnly)),
                    invalid: self
                        .compiled
                        .point_metadata(&info.id, &point.id)
                        .map(|metadata| metadata.invalid)
                        .unwrap_or(false),
                    action_bindings: self
                        .compiled
                        .point_metadata(&info.id, &point.id)
                        .map(|metadata| metadata.action_bindings.clone())
                        .unwrap_or_default(),
                    behavior_bindings: self
                        .compiled
                        .point_metadata(&info.id, &point.id)
                        .map(|metadata| metadata.behavior_bindings.clone())
                        .unwrap_or_default(),
                    source_datastore: self
                        .compiled
                        .point_metadata(&info.id, &point.id)
                        .and_then(|metadata| metadata.source_datastore.clone()),
                    tags: info.tags.clone(),
                });
            }
        }

        points.sort_by(|left, right| {
            left.device_id
                .cmp(&right.device_id)
                .then(left.point_id.cmp(&right.point_id))
        });
        Ok(points)
    }
}

#[async_trait]
impl RegisterControlPort for ModbusControlSession {
    async fn read(&self, target: &PointTarget) -> ModbusResult<DataPoint> {
        let resolved = self.resolve_target(target)?;
        let port = self
            .runtime_session
            .devices()
            .get(&resolved.device_id)
            .ok_or_else(|| {
                ModbusError::Config(format!("unknown device '{}'", resolved.device_id))
            })?;
        port.read(&resolved.point_id)
            .await
            .map_err(ModbusError::from)
    }

    async fn write(&self, target: &PointTarget, value: Value) -> ModbusResult<()> {
        let resolved = self.resolve_target(target)?;
        let port = self
            .runtime_session
            .devices()
            .get(&resolved.device_id)
            .ok_or_else(|| {
                ModbusError::Config(format!("unknown device '{}'", resolved.device_id))
            })?;
        port.write(&resolved.point_id, value)
            .await
            .map_err(ModbusError::from)
    }
}

impl TracePort for ModbusControlSession {
    fn tail(&self, limit: usize) -> Vec<TraceEntry> {
        self.trace_store.tail(limit)
    }

    fn clear(&self) {
        self.trace_store.clear();
    }

    fn subscribe(&self) -> broadcast::Receiver<TraceEntry> {
        self.trace_store.subscribe()
    }
}

#[async_trait]
impl FaultPresetPort for ModbusControlSession {
    fn available_fault_presets(&self) -> Vec<String> {
        self.compiled.fault_presets.keys().cloned().collect()
    }

    fn active_fault_preset(&self) -> Option<String> {
        self.compiled.active_fault_preset.clone()
    }

    async fn apply_fault_preset(&mut self, name: &str) -> ModbusResult<SessionSnapshot> {
        let compiled = self.compiled.with_active_fault_preset(Some(name))?;
        self.rebuild(compiled, false).await?;
        self.snapshot().await
    }

    async fn clear_fault_preset(&mut self) -> ModbusResult<SessionSnapshot> {
        let compiled = self.compiled.with_active_fault_preset(None)?;
        self.rebuild(compiled, false).await?;
        self.snapshot().await
    }
}

#[async_trait]
impl ResponseProfilePort for ModbusControlSession {
    fn available_response_profiles(&self) -> Vec<String> {
        self.compiled.response_profiles.keys().cloned().collect()
    }

    fn active_response_profile(&self) -> Option<String> {
        self.compiled.active_response_profile.clone()
    }

    async fn apply_response_profile(&mut self, name: &str) -> ModbusResult<SessionSnapshot> {
        let compiled = self.compiled.with_active_response_profile(Some(name))?;
        self.rebuild(compiled, false).await?;
        self.snapshot().await
    }

    async fn clear_response_profile(&mut self) -> ModbusResult<SessionSnapshot> {
        let compiled = self.compiled.with_active_response_profile(None)?;
        self.rebuild(compiled, false).await?;
        self.snapshot().await
    }
}

impl SessionMetadataPort for ModbusControlSession {
    fn action_binding_summaries(&self) -> &[ActionBindingSummary] {
        &self.compiled.action_binding_summaries
    }

    fn behavior_binding_summaries(&self) -> &[BehaviorBindingSummary] {
        &self.compiled.behavior_binding_summaries
    }

    fn datastore_policy_summaries(&self) -> &[DatastorePolicySummary] {
        &self.compiled.datastore_policies
    }
}

#[async_trait]
impl BehaviorSetPort for ModbusControlSession {
    fn available_behavior_sets(&self) -> Vec<String> {
        self.compiled.behavior_sets.keys().cloned().collect()
    }

    fn active_behavior_set(&self) -> Option<String> {
        self.compiled.active_behavior_set.clone()
    }

    async fn apply_behavior_set(&mut self, name: &str) -> ModbusResult<SessionSnapshot> {
        let compiled = self.compiled.with_active_behavior_set(Some(name))?;
        self.rebuild(compiled, false).await?;
        self.snapshot().await
    }

    async fn clear_behavior_set(&mut self) -> ModbusResult<SessionSnapshot> {
        let compiled = self.compiled.with_active_behavior_set(None)?;
        self.rebuild(compiled, false).await?;
        self.snapshot().await
    }
}

fn access_mode_name(mode: AccessMode) -> String {
    match mode {
        AccessMode::ReadOnly => "read_only",
        AccessMode::WriteOnly => "write_only",
        AccessMode::ReadWrite => "read_write",
    }
    .to_string()
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::time::Duration;

    use super::{
        BehaviorSetPort, FaultPresetPort, ModbusControlSession, PointCatalogPort, PointTarget,
        RegisterControlPort, ResponseProfilePort, SessionControlPort, TracePort,
    };
    use crate::fault_injection::FaultInjectionConfig;
    use crate::profile::{PointProfile, SimulatorProfile, UnitProfile};
    use crate::rtu::RtuServerConfig;
    use crate::simulator::{
        CompiledModbusSession, CompiledPointMetadata, CompiledTransportKind,
        ModbusServiceLaunchConfig, ModbusTransportLaunch, ResponseProfileDefinition,
        SessionControlConfig, SessionResetPolicy, SessionTraceConfig,
    };
    use mabi_core::types::{DataType, ModbusRegisterType};
    use mabi_core::value::Value;
    use mabi_runtime::ProtocolDriverRegistry;

    fn registry() -> ProtocolDriverRegistry {
        let mut registry = ProtocolDriverRegistry::new();
        registry.register(crate::driver());
        registry
    }

    fn compiled_session() -> CompiledModbusSession {
        let profile = SimulatorProfile::new().with_unit(UnitProfile::new(1, "Pump").with_point(
            PointProfile::new(
                "temperature",
                "Temperature",
                ModbusRegisterType::HoldingRegister,
                0,
                DataType::UInt16,
            ),
        ));

        CompiledModbusSession {
            session_name: "demo".into(),
            launch: mabi_runtime::ProtocolLaunchSpec {
                protocol: "modbus".into(),
                name: Some("demo".into()),
                config: serde_json::to_value(ModbusServiceLaunchConfig {
                    transport: ModbusTransportLaunch::Rtu {
                        config: RtuServerConfig::for_testing(),
                    },
                    profile: Some(profile.clone()),
                    devices: None,
                    points_per_device: None,
                })
                .unwrap(),
            },
            transport_kind: CompiledTransportKind::Rtu,
            profile,
            trace: SessionTraceConfig {
                enabled: true,
                capacity: Some(32),
            },
            reset: SessionResetPolicy::default(),
            control: SessionControlConfig::default(),
            fault_presets: BTreeMap::from([("drop".to_string(), FaultInjectionConfig::default())]),
            active_fault_preset: None,
            response_profiles: BTreeMap::from([(
                "slow".to_string(),
                ResponseProfileDefinition {
                    delay_ms: Some(10),
                    ..Default::default()
                },
            )]),
            active_response_profile: None,
            actions: BTreeMap::new(),
            behaviors: BTreeMap::new(),
            behavior_sets: BTreeMap::from([(
                "maintenance".to_string(),
                crate::simulator::BehaviorSetDefinition {
                    behaviors: vec!["temperature_guard".into()],
                },
            )]),
            active_behavior_set: None,
            point_catalog: BTreeMap::from([(
                "modbus-1/temperature".to_string(),
                CompiledPointMetadata {
                    device_id: "modbus-1".into(),
                    point_id: "temperature".into(),
                    source_datastore: Some("inline".into()),
                    read_only: false,
                    invalid: false,
                    action_bindings: vec!["clamp_temp@on_write".into()],
                    behavior_bindings: vec!["temperature_guard@maintenance".into()],
                },
            )]),
            datastore_policies: Vec::new(),
            action_binding_summaries: Vec::new(),
            behavior_binding_summaries: Vec::new(),
            compiled_behavior_bindings: Vec::new(),
            readiness_timeout_ms: Some(500),
        }
    }

    #[tokio::test]
    async fn control_session_lists_points_and_reads_back_writes() {
        let session =
            ModbusControlSession::new(registry(), compiled_session(), Duration::from_secs(1))
                .await
                .unwrap();

        let points = session.list_points(&Default::default()).unwrap();
        assert_eq!(points.len(), 1);
        assert_eq!(points[0].action_bindings, vec!["clamp_temp@on_write"]);
        assert_eq!(
            points[0].behavior_bindings,
            vec!["temperature_guard@maintenance"]
        );
        assert_eq!(points[0].source_datastore.as_deref(), Some("inline"));

        session
            .write(
                &PointTarget {
                    point_id: Some("temperature".into()),
                    ..Default::default()
                },
                Value::U16(42),
            )
            .await
            .unwrap();

        let point = session
            .read(&PointTarget {
                point_id: Some("temperature".into()),
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(point.value, Value::U16(42));
        assert_eq!(session.tail(10).len(), 2);

        session.stop().await.unwrap();
    }

    #[tokio::test]
    async fn control_session_can_apply_and_clear_fault_presets() {
        let mut session =
            ModbusControlSession::new(registry(), compiled_session(), Duration::from_secs(1))
                .await
                .unwrap();

        let snapshot = session.apply_fault_preset("drop").await.unwrap();
        assert_eq!(snapshot.status.active_fault_preset.as_deref(), Some("drop"));

        let snapshot = session.clear_fault_preset().await.unwrap();
        assert!(snapshot.status.active_fault_preset.is_none());

        session.stop().await.unwrap();
    }

    #[tokio::test]
    async fn session_reset_clears_traces_and_fault_preset() {
        let mut compiled = compiled_session();
        compiled.active_fault_preset = Some("drop".into());
        compiled.active_response_profile = Some("slow".into());
        compiled.active_behavior_set = Some("maintenance".into());

        let mut session = ModbusControlSession::new(registry(), compiled, Duration::from_secs(1))
            .await
            .unwrap();
        session
            .write(
                &PointTarget {
                    point_id: Some("temperature".into()),
                    ..Default::default()
                },
                Value::U16(7),
            )
            .await
            .unwrap();
        assert_eq!(session.tail(10).len(), 1);

        let snapshot = session.reset().await.unwrap();
        assert!(snapshot.status.active_fault_preset.is_none());
        assert!(snapshot.status.active_response_profile.is_none());
        assert!(snapshot.status.active_behavior_set.is_none());
        assert_eq!(snapshot.status.trace_entries, 0);

        session.stop().await.unwrap();
    }

    #[tokio::test]
    async fn control_session_can_apply_and_clear_response_profiles() {
        let mut session =
            ModbusControlSession::new(registry(), compiled_session(), Duration::from_secs(1))
                .await
                .unwrap();

        let snapshot = session.apply_response_profile("slow").await.unwrap();
        assert_eq!(
            snapshot.status.active_response_profile.as_deref(),
            Some("slow")
        );

        let snapshot = session.clear_response_profile().await.unwrap();
        assert!(snapshot.status.active_response_profile.is_none());

        session.stop().await.unwrap();
    }

    #[tokio::test]
    async fn control_session_can_apply_and_clear_behavior_sets() {
        let mut session =
            ModbusControlSession::new(registry(), compiled_session(), Duration::from_secs(1))
                .await
                .unwrap();

        let snapshot = session.apply_behavior_set("maintenance").await.unwrap();
        assert_eq!(
            snapshot.status.active_behavior_set.as_deref(),
            Some("maintenance")
        );

        let snapshot = session.clear_behavior_set().await.unwrap();
        assert!(snapshot.status.active_behavior_set.is_none());

        session.stop().await.unwrap();
    }
}

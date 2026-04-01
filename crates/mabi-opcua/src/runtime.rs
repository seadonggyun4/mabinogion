use std::collections::BTreeMap;
use std::sync::Arc;

use async_trait::async_trait;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use serde_json::json;

use mabi_core::device::{DeviceInfo, DeviceState};
use mabi_core::types::{DataPoint, DataPointDef, DataPointId};
use mabi_core::{Protocol, Value};
use mabi_runtime::{
    DevicePort, DeviceRegistry, ManagedService, ProtocolDescriptor, ProtocolDriver,
    ProtocolLaunchSpec, RuntimeExtensions, RuntimeResult, ServiceContext, ServiceSnapshot,
    ServiceState, ServiceStatus,
};

use crate::modeling::{
    CompiledOpcUaSession, CompiledPointBinding, OpcUaCompiledLaunchConfig, OpcUaSimulatorConfig,
    PresetDefinition, SessionControlConfig, SessionDefinition, SessionRuntimeConfig,
    SimulatorDefaults, TransportDefinition,
};
use crate::server_runtime::ServerBuildSpec;
use crate::TransportProtocol;
use crate::{NodeId, OpcUaError, OpcUaResult, OpcUaServer, Variant};

#[derive(Debug, Clone, Serialize, Deserialize)]
struct LegacyOpcUaLaunchConfig {
    bind_addr: std::net::SocketAddr,
    endpoint_path: String,
    nodes: usize,
    security_mode: String,
}

fn runtime_error(message: impl Into<String>) -> mabi_runtime::RuntimeError {
    mabi_runtime::RuntimeError::service(message)
}

fn new_status(name: &str) -> ServiceStatus {
    let mut status = ServiceStatus::new(name);
    status.protocol = Some(Protocol::OpcUa);
    status
}

fn snapshot_with_metadata(
    status: &ServiceStatus,
    metadata: BTreeMap<String, serde_json::Value>,
) -> ServiceSnapshot {
    let mut snapshot = ServiceSnapshot::new(status.name.clone());
    snapshot.protocol = status.protocol;
    snapshot.status = status.clone();
    snapshot.metadata = metadata;
    snapshot
}

fn server_state_to_device_state(state: crate::ServerState) -> DeviceState {
    match state {
        crate::ServerState::Stopped => DeviceState::Offline,
        crate::ServerState::Starting => DeviceState::Initializing,
        crate::ServerState::Running => DeviceState::Online,
        crate::ServerState::Stopping => DeviceState::ShuttingDown,
        crate::ServerState::Error => DeviceState::Error,
        crate::ServerState::Uninitialized => DeviceState::Offline,
    }
}

fn parse_node_selector(point_id: &str) -> mabi_core::Result<NodeId> {
    point_id
        .parse::<NodeId>()
        .map_err(|error| mabi_core::Error::Protocol(error.to_string()))
}

struct OpcUaDevicePort {
    server: Arc<OpcUaServer>,
    info: DeviceInfo,
    points: BTreeMap<String, CompiledPointBinding>,
    allow_raw_node_access: bool,
}

impl OpcUaDevicePort {
    fn new(
        server: Arc<OpcUaServer>,
        device_id: String,
        name: String,
        points: Vec<CompiledPointBinding>,
        tags: mabi_core::tags::Tags,
        allow_raw_node_access: bool,
    ) -> Self {
        let mut info = DeviceInfo::new(device_id, name, Protocol::OpcUa).with_tags(tags);
        info.point_count = points.len();
        let points = points
            .into_iter()
            .map(|binding| (binding.point_id.clone(), binding))
            .collect();
        Self {
            server,
            info,
            points,
            allow_raw_node_access,
        }
    }

    fn resolve_binding(&self, point_id: &str) -> Option<&CompiledPointBinding> {
        self.points.get(point_id)
    }

    fn resolve_node_id(&self, point_id: &str) -> mabi_core::Result<NodeId> {
        if let Some(binding) = self.resolve_binding(point_id) {
            return Ok(binding.node_id.clone());
        }
        if self.allow_raw_node_access {
            return parse_node_selector(point_id);
        }
        Err(mabi_core::Error::Protocol(format!(
            "unknown OPC UA point '{}'",
            point_id
        )))
    }
}

#[async_trait]
impl DevicePort for OpcUaDevicePort {
    fn info(&self) -> DeviceInfo {
        let mut info = self.info.clone();
        info.state = server_state_to_device_state(self.server.state());
        info
    }

    async fn start(&self) -> mabi_core::Result<()> {
        Ok(())
    }

    async fn stop(&self) -> mabi_core::Result<()> {
        Ok(())
    }

    async fn read(&self, point_id: &str) -> mabi_core::Result<DataPoint> {
        let node_id = self.resolve_node_id(point_id)?;
        let value = self
            .server
            .read_value(&node_id)
            .value()
            .cloned()
            .map(Value::from)
            .unwrap_or(Value::Null);
        Ok(DataPoint::new(
            DataPointId::new(&self.info.id, point_id),
            value,
        ))
    }

    async fn write(&self, point_id: &str, value: Value) -> mabi_core::Result<()> {
        if let Some(binding) = self.resolve_binding(point_id) {
            if !binding.writable {
                return Err(mabi_core::Error::Protocol(format!(
                    "point '{}' is not writable",
                    point_id
                )));
            }
        }
        let node_id = self.resolve_node_id(point_id)?;
        self.server
            .write_value(&node_id, Variant::from(value))
            .map_err(|error| mabi_core::Error::Protocol(error.to_string()))
    }

    fn point_definitions(&self) -> Vec<DataPointDef> {
        self.points
            .values()
            .map(|binding| binding.point_def.clone())
            .collect()
    }
}

struct OpcUaManagedService {
    server: Arc<OpcUaServer>,
    launch: OpcUaCompiledLaunchConfig,
    status: RwLock<ServiceStatus>,
}

impl OpcUaManagedService {
    fn new(server: Arc<OpcUaServer>, name: String, launch: OpcUaCompiledLaunchConfig) -> Self {
        Self {
            server,
            launch,
            status: RwLock::new(new_status(&name)),
        }
    }

    fn register_compiled_devices(&self, registry: &DeviceRegistry) {
        if self.launch.devices.is_empty() {
            let fallback_id = format!("opcua-{}", self.status().name);
            registry.register(
                fallback_id.clone(),
                Arc::new(OpcUaDevicePort::new(
                    Arc::clone(&self.server),
                    fallback_id,
                    self.status().name,
                    Vec::new(),
                    mabi_core::tags::Tags::new(),
                    self.launch.control.allow_raw_node_access,
                )),
            );
            return;
        }

        for device in &self.launch.devices {
            registry.register(
                device.device_id.clone(),
                Arc::new(OpcUaDevicePort::new(
                    Arc::clone(&self.server),
                    device.device_id.clone(),
                    device.name.clone(),
                    device.points.clone(),
                    device.tags.clone(),
                    self.launch.control.allow_raw_node_access,
                )),
            );
        }
    }
}

#[async_trait]
impl ManagedService for OpcUaManagedService {
    async fn start(&self, context: &ServiceContext) -> RuntimeResult<()> {
        let mut status = self.status.write();
        status.state = ServiceState::Starting;
        status.ready = false;
        status.started_at = Some(context.started_at());
        Ok(())
    }

    async fn stop(&self, _context: &ServiceContext) -> RuntimeResult<()> {
        {
            let mut status = self.status.write();
            status.state = ServiceState::Stopping;
            status.ready = false;
        }
        self.server
            .stop()
            .await
            .map_err(|error| runtime_error(format!("opcua stop failed: {}", error)))?;
        let mut status = self.status.write();
        status.state = ServiceState::Stopped;
        status.ready = false;
        Ok(())
    }

    async fn serve(&self, context: ServiceContext) -> RuntimeResult<()> {
        self.server
            .start()
            .await
            .map_err(|error| runtime_error(format!("opcua start failed: {}", error)))?;
        {
            let mut status = self.status.write();
            status.state = ServiceState::Running;
            status.ready = true;
        }
        context.cancellation_token().cancelled().await;
        Ok(())
    }

    fn status(&self) -> ServiceStatus {
        self.status.read().clone()
    }

    async fn snapshot(&self) -> RuntimeResult<ServiceSnapshot> {
        let stats = self.server.stats();
        let mut metadata = BTreeMap::new();
        metadata.insert(
            "endpoint".to_string(),
            serde_json::to_value(&self.launch.server_config.endpoint_url)
                .map_err(|error| runtime_error(error.to_string()))?,
        );
        metadata.insert(
            "transport_protocol".to_string(),
            json!(self.launch.server_config.endpoint_protocol.scheme()),
        );
        metadata.insert("nodes".to_string(), json!(self.launch.catalog.nodes.len()));
        metadata.insert("devices".to_string(), json!(self.launch.devices.len()));
        metadata.insert(
            "namespaces".to_string(),
            serde_json::to_value(&self.launch.catalog.namespace_table)
                .map_err(|error| runtime_error(error.to_string()))?,
        );
        metadata.insert(
            "security_profile".to_string(),
            json!(self.launch.security.name.clone()),
        );
        metadata.insert(
            "durability_mode".to_string(),
            json!(format!("{:?}", self.launch.runtime.durability.mode)),
        );
        metadata.insert(
            "restored_subscriptions".to_string(),
            json!(self
                .server
                .subscription_manager()
                .restored_subscription_count()),
        );
        metadata.insert(
            "detached_restored_subscriptions".to_string(),
            json!(self
                .server
                .subscription_manager()
                .detached_subscription_count()),
        );
        metadata.insert(
            "generated_types".to_string(),
            json!({
                "module": self.launch.generated_types.module_name,
                "entries": self.launch.generated_types.entries.len(),
            }),
        );
        metadata.insert(
            "stats".to_string(),
            json!({
                "total_requests": stats.total_requests,
                "successful_requests": stats.successful_requests,
                "failed_requests": stats.failed_requests,
                "active_sessions": stats.active_sessions,
                "active_subscriptions": stats.active_subscriptions,
                "total_nodes": stats.total_nodes,
                "cache_hit_rate": stats.cache_hit_rate,
                "uptime_seconds": stats.uptime_seconds,
            }),
        );
        Ok(snapshot_with_metadata(&self.status(), metadata))
    }

    fn register_devices(&self, registry: &DeviceRegistry) -> RuntimeResult<()> {
        self.register_compiled_devices(registry);
        Ok(())
    }
}

#[derive(Debug, Clone, Copy)]
pub struct OpcUaDriver;

#[async_trait]
impl ProtocolDriver for OpcUaDriver {
    fn descriptor(&self) -> ProtocolDescriptor {
        ProtocolDescriptor {
            key: "opcua",
            display_name: "OPC UA",
            protocol: Protocol::OpcUa,
            default_port: 4840,
            description: "Serve OPC UA endpoints through the shared runtime",
        }
    }

    fn features(&self) -> &'static [&'static str] {
        &[
            "address space simulation",
            "subscriptions",
            "controller-visible node port",
            "nodeset-driven canonical modeling",
        ]
    }

    async fn build(
        &self,
        spec: ProtocolLaunchSpec,
        _extensions: RuntimeExtensions,
    ) -> RuntimeResult<Arc<dyn ManagedService>> {
        let launch = decode_launch_config(&spec)
            .map_err(|error| runtime_error(format!("invalid opcua launch config: {}", error)))?;
        let mut build_spec = ServerBuildSpec::from_server_config(launch.server_config.clone())
            .with_generated_catalog(launch.catalog.clone());
        build_spec.defaults.subscription.durability = launch.runtime.durability.clone();
        build_spec.defaults.security = launch.security.manager_config.clone();
        let server = Arc::new(OpcUaServer::from_build_spec(build_spec).map_err(
            |error: OpcUaError| runtime_error(format!("failed to create opcua server: {}", error)),
        )?);
        Ok(Arc::new(OpcUaManagedService::new(
            server,
            spec.service_name(&self.descriptor()),
            launch,
        )))
    }
}

fn decode_launch_config(spec: &ProtocolLaunchSpec) -> OpcUaResult<OpcUaCompiledLaunchConfig> {
    if let Ok(compiled) = serde_json::from_value::<OpcUaCompiledLaunchConfig>(spec.config.clone()) {
        return Ok(compiled);
    }

    let legacy: LegacyOpcUaLaunchConfig = serde_json::from_value(spec.config.clone())
        .map_err(|error| OpcUaError::Config(format!("invalid legacy OPC UA config: {}", error)))?;
    let legacy_service_name = spec.name.clone().unwrap_or_else(|| "opcua".to_string());

    let config = OpcUaSimulatorConfig {
        defaults: SimulatorDefaults::default(),
        transports: BTreeMap::from([(
            "legacy".into(),
            TransportDefinition {
                protocol: TransportProtocol::OpcTcp,
                connection_mode: crate::TransportConnectionMode::Listener,
                bind: legacy.bind_addr.ip().to_string(),
                port: legacy.bind_addr.port(),
                endpoint_path: legacy.endpoint_path.clone(),
                reverse_connect_target: None,
                retry_interval_ms: 5_000,
                security_profile: Some(legacy.security_mode.clone()),
                server_name: Some(DEFAULT_SERVER_NAME.into()),
                certificate_path: None,
                private_key_path: None,
            },
        )]),
        sessions: BTreeMap::from([(
            "legacy".into(),
            SessionDefinition {
                transport: "legacy".into(),
                models: Vec::new(),
                devices: Vec::new(),
                preset: Some("legacy".into()),
                service_name: Some(legacy_service_name),
                readiness_timeout_ms: Some(5_000),
                control: SessionControlConfig::default(),
                runtime: SessionRuntimeConfig::default(),
            },
        )]),
        presets: BTreeMap::from([(
            "legacy".into(),
            PresetDefinition {
                nodes: legacy.nodes,
                writable: true,
                historizing: false,
                ..Default::default()
            },
        )]),
        ..Default::default()
    };
    let compiled: CompiledOpcUaSession = config.compile_session("legacy", None)?;
    serde_json::from_value(compiled.launch.config)
        .map_err(|error| OpcUaError::Config(format!("invalid compiled OPC UA config: {}", error)))
}

pub fn descriptor() -> ProtocolDescriptor {
    OpcUaDriver.descriptor()
}

pub fn driver() -> OpcUaDriver {
    OpcUaDriver
}

const DEFAULT_SERVER_NAME: &str = "Mabinogion OPC UA Simulator";

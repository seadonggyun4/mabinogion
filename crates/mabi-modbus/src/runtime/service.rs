use std::collections::BTreeMap;
use std::sync::Arc;

use async_trait::async_trait;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use serde_json::{json, to_value};

use mabi_core::device::{Device, DeviceInfo};
use mabi_core::types::DataPointDef;
use mabi_core::Protocol;
use mabi_runtime::{
    DevicePort, DeviceRegistry, ManagedService, ProtocolDescriptor, ProtocolDriver,
    ProtocolLaunchSpec, RuntimeError, RuntimeExtensions, RuntimeResult, ServiceContext,
    ServiceSnapshot, ServiceState, ServiceStatus,
};

use crate::fault_injection::{FaultInjectionConfig, FaultPipeline};
use crate::simulator::{schema_summary, ModbusServiceLaunchConfig, ModbusTransportLaunch};
use crate::{
    Builder, ConnectionDisruptionConfig, GeneratedProfilePreset, ModbusDevice, ModbusRtuServer,
    ModbusTcpServerV2, Profile,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
struct LegacyModbusLaunchConfig {
    bind_addr: std::net::SocketAddr,
    #[serde(default)]
    devices: Option<usize>,
    #[serde(default)]
    points_per_device: Option<usize>,
    #[serde(default)]
    profile: Option<Profile>,
}

impl LegacyModbusLaunchConfig {
    fn into_service_launch(self) -> ModbusServiceLaunchConfig {
        ModbusServiceLaunchConfig {
            transport: ModbusTransportLaunch::Tcp {
                bind_addr: self.bind_addr,
                performance_preset: crate::tcp::PerformancePreset::Default,
            },
            profile: self.profile,
            devices: self.devices,
            points_per_device: self.points_per_device,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct ModbusProtocolRuntimeConfig {
    #[serde(default)]
    fault_injection: Option<FaultInjectionConfig>,
    #[serde(default)]
    connection_disruption: Option<ConnectionDisruptionConfig>,
}

fn config_error(message: impl Into<String>) -> RuntimeError {
    RuntimeError::config(message)
}

fn protocol_error(message: impl Into<String>) -> RuntimeError {
    RuntimeError::protocol(message)
}

fn internal_error(message: impl Into<String>) -> RuntimeError {
    RuntimeError::internal(message)
}

fn bind_or_protocol_error(message: impl Into<String>) -> RuntimeError {
    let message = message.into();
    let lower = message.to_ascii_lowercase();
    if lower.contains("bind")
        || lower.contains("listen")
        || lower.contains("address already in use")
        || lower.contains("address not available")
    {
        RuntimeError::bind(message)
    } else {
        RuntimeError::protocol(message)
    }
}

fn new_status(name: &str, protocol: Protocol) -> ServiceStatus {
    let mut status = ServiceStatus::new(name);
    status.protocol = Some(protocol);
    status
}

fn mark_starting(status: &RwLock<ServiceStatus>, context: &ServiceContext) {
    let mut current = status.write();
    current.state = ServiceState::Starting;
    current.ready = false;
    current.started_at = Some(context.started_at());
    current.last_error = None;
}

fn mark_running(status: &RwLock<ServiceStatus>) {
    let mut current = status.write();
    current.state = ServiceState::Running;
    current.ready = true;
}

fn mark_stopping(status: &RwLock<ServiceStatus>) {
    let mut current = status.write();
    current.state = ServiceState::Stopping;
    current.ready = false;
}

fn mark_stopped(status: &RwLock<ServiceStatus>) {
    let mut current = status.write();
    current.state = ServiceState::Stopped;
    current.ready = false;
}

fn mark_error(status: &RwLock<ServiceStatus>, message: impl Into<String>) {
    let mut current = status.write();
    current.state = ServiceState::Error;
    current.ready = false;
    current.last_error = Some(message.into());
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

struct ModbusDevicePort {
    device: Arc<ModbusDevice>,
}

impl ModbusDevicePort {
    fn new(device: Arc<ModbusDevice>) -> Self {
        Self { device }
    }
}

#[async_trait]
impl DevicePort for ModbusDevicePort {
    fn info(&self) -> DeviceInfo {
        self.device.info().clone()
    }

    async fn start(&self) -> mabi_core::Result<()> {
        Ok(())
    }

    async fn stop(&self) -> mabi_core::Result<()> {
        Ok(())
    }

    async fn read(&self, point_id: &str) -> mabi_core::Result<mabi_core::types::DataPoint> {
        self.device.read_point(point_id).await
    }

    async fn write(&self, point_id: &str, value: mabi_core::Value) -> mabi_core::Result<()> {
        self.device.write_point(point_id, value).await
    }

    fn point_definitions(&self) -> Vec<DataPointDef> {
        self.device.point_definitions_owned()
    }
}

enum ModbusRuntimeServer {
    Tcp(Arc<ModbusTcpServerV2>),
    Rtu(Arc<ModbusRtuServer>),
}

impl ModbusRuntimeServer {
    fn protocol(&self) -> Protocol {
        match self {
            Self::Tcp(_) => Protocol::ModbusTcp,
            Self::Rtu(_) => Protocol::ModbusRtu,
        }
    }

    fn transport_name(&self) -> &'static str {
        match self {
            Self::Tcp(_) => "tcp",
            Self::Rtu(_) => "rtu",
        }
    }

    fn shutdown(&self) {
        match self {
            Self::Tcp(server) => server.shutdown(),
            Self::Rtu(server) => server.shutdown(),
        }
    }

    async fn run(&self) -> RuntimeResult<()> {
        match self {
            Self::Tcp(server) => server.run().await.map_err(|error| {
                bind_or_protocol_error(format!("modbus tcp server failed: {}", error))
            }),
            Self::Rtu(server) => server
                .run()
                .await
                .map_err(|error| protocol_error(format!("modbus rtu server failed: {}", error))),
        }
    }

    fn register_devices(&self, registry: &DeviceRegistry) {
        match self {
            Self::Tcp(server) => {
                for unit_id in server.device_ids() {
                    if let Some(device) = server.device(unit_id) {
                        registry.register(
                            device.id().to_string(),
                            Arc::new(ModbusDevicePort::new(device)),
                        );
                    }
                }
            }
            Self::Rtu(server) => {
                for unit_id in server.device_ids() {
                    if let Some(device) = server.device(unit_id) {
                        registry.register(
                            device.id().to_string(),
                            Arc::new(ModbusDevicePort::new(device)),
                        );
                    }
                }
            }
        }
    }

    fn metrics_metadata(&self) -> serde_json::Value {
        match self {
            Self::Tcp(server) => {
                let metrics = server.metrics().snapshot();
                json!({
                    "connections_total": metrics.connections_total,
                    "connections_active": metrics.connections_active,
                    "connections_rejected": metrics.connections_rejected,
                    "requests_total": metrics.requests_total,
                    "responses_success": metrics.responses_success,
                    "responses_exception": metrics.responses_exception,
                    "errors_total": metrics.errors_total,
                    "frame_errors": metrics.frame_errors,
                    "timeout_errors": metrics.timeout_errors,
                    "bytes_received": metrics.bytes_received,
                    "bytes_sent": metrics.bytes_sent,
                    "uptime_secs": metrics.uptime_secs,
                    "requests_per_second": metrics.requests_per_second,
                    "avg_latency_us": metrics.avg_latency_us,
                    "p50_latency_us": metrics.p50_latency_us,
                    "p95_latency_us": metrics.p95_latency_us,
                    "p99_latency_us": metrics.p99_latency_us,
                })
            }
            Self::Rtu(server) => {
                let stats = server.stats();
                let transport = server.transport_metrics();
                json!({
                    "requests_processed": stats.requests_processed,
                    "requests_success": stats.requests_success,
                    "requests_exception": stats.requests_exception,
                    "crc_errors": stats.crc_errors,
                    "framing_errors": stats.framing_errors,
                    "timeouts": stats.timeouts,
                    "avg_latency_us": stats.avg_latency_us,
                    "bytes_received": transport.bytes_received,
                    "bytes_sent": transport.bytes_sent,
                    "frames_received": transport.frames_received,
                    "frames_sent": transport.frames_sent,
                    "crc_errors_total": transport.crc_errors,
                    "framing_errors_total": transport.framing_errors,
                })
            }
        }
    }
}

struct ModbusManagedService {
    server: ModbusRuntimeServer,
    launch: ModbusServiceLaunchConfig,
    profile: Profile,
    status: RwLock<ServiceStatus>,
}

impl ModbusManagedService {
    fn new(
        server: ModbusRuntimeServer,
        name: String,
        launch: ModbusServiceLaunchConfig,
        profile: Profile,
    ) -> Self {
        Self {
            status: RwLock::new(new_status(&name, server.protocol())),
            server,
            launch,
            profile,
        }
    }
}

#[async_trait]
impl ManagedService for ModbusManagedService {
    async fn start(&self, context: &ServiceContext) -> RuntimeResult<()> {
        mark_starting(&self.status, context);
        Ok(())
    }

    async fn stop(&self, _context: &ServiceContext) -> RuntimeResult<()> {
        mark_stopping(&self.status);
        self.server.shutdown();
        Ok(())
    }

    async fn serve(&self, _context: ServiceContext) -> RuntimeResult<()> {
        mark_running(&self.status);
        match self.server.run().await {
            Ok(()) => {
                mark_stopped(&self.status);
                Ok(())
            }
            Err(error) => {
                mark_error(&self.status, error.to_string());
                Err(error)
            }
        }
    }

    fn status(&self) -> ServiceStatus {
        self.status.read().clone()
    }

    async fn snapshot(&self) -> RuntimeResult<ServiceSnapshot> {
        let mut metadata = BTreeMap::new();
        metadata.insert(
            "transport".to_string(),
            to_value(self.server.transport_name())
                .map_err(|error| internal_error(error.to_string()))?,
        );
        metadata.insert(
            "devices".to_string(),
            to_value(self.profile.units.len())
                .map_err(|error| internal_error(error.to_string()))?,
        );
        metadata.insert(
            "points".to_string(),
            to_value(
                self.profile
                    .units
                    .iter()
                    .map(|unit| unit.points.len())
                    .sum::<usize>(),
            )
            .map_err(|error| internal_error(error.to_string()))?,
        );
        match &self.launch.transport {
            ModbusTransportLaunch::Tcp { bind_addr, .. } => {
                metadata.insert(
                    "bind_address".to_string(),
                    to_value(bind_addr.to_string())
                        .map_err(|error| internal_error(error.to_string()))?,
                );
            }
            ModbusTransportLaunch::Rtu { config } => {
                metadata.insert(
                    "rtu_transport".to_string(),
                    to_value(&config.transport)
                        .map_err(|error| internal_error(error.to_string()))?,
                );
            }
        }
        metadata.insert("metrics".to_string(), self.server.metrics_metadata());
        Ok(snapshot_with_metadata(&self.status(), metadata))
    }

    fn register_devices(&self, registry: &DeviceRegistry) -> RuntimeResult<()> {
        self.server.register_devices(registry);
        Ok(())
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ModbusDriver;

impl ModbusDriver {
    fn protocol_runtime_config(
        extensions: &RuntimeExtensions,
    ) -> RuntimeResult<ModbusProtocolRuntimeConfig> {
        match extensions.protocol_config("modbus") {
            Some(config) => serde_json::from_value(config.clone())
                .map_err(|error| config_error(format!("invalid modbus runtime config: {}", error))),
            None => Ok(ModbusProtocolRuntimeConfig::default()),
        }
    }

    fn parse_launch_config(config: serde_json::Value) -> RuntimeResult<ModbusServiceLaunchConfig> {
        serde_json::from_value::<ModbusServiceLaunchConfig>(config.clone())
            .or_else(|_| {
                serde_json::from_value::<LegacyModbusLaunchConfig>(config)
                    .map(LegacyModbusLaunchConfig::into_service_launch)
            })
            .map_err(|error| config_error(format!("invalid modbus launch config: {}", error)))
    }

    fn resolved_profile(launch: &ModbusServiceLaunchConfig) -> Profile {
        launch.profile.clone().unwrap_or_else(|| {
            GeneratedProfilePreset::new(
                launch.devices.unwrap_or(1),
                launch.points_per_device.unwrap_or(4),
            )
            .build()
        })
    }
}

#[async_trait]
impl ProtocolDriver for ModbusDriver {
    fn descriptor(&self) -> ProtocolDescriptor {
        ProtocolDescriptor {
            key: "modbus",
            display_name: "Modbus",
            protocol: Protocol::ModbusTcp,
            default_port: 502,
            description: "Serve Modbus TCP or RTU devices through the shared runtime",
        }
    }

    fn features(&self) -> &'static [&'static str] {
        &[
            "tcp and rtu transports",
            "session-centric config",
            "controller-visible device ports",
            "typed config inspection",
        ]
    }

    fn schema(&self) -> Option<serde_json::Value> {
        serde_json::to_value(schema_summary()).ok()
    }

    async fn build(
        &self,
        spec: ProtocolLaunchSpec,
        extensions: RuntimeExtensions,
    ) -> RuntimeResult<Arc<dyn ManagedService>> {
        let launch = Self::parse_launch_config(spec.config.clone())?;
        let runtime_config = Self::protocol_runtime_config(&extensions)?;
        let profile = Self::resolved_profile(&launch);

        let server = match &launch.transport {
            ModbusTransportLaunch::Tcp {
                bind_addr,
                performance_preset,
            } => {
                let mut server = Builder::new()
                    .config(crate::tcp::ServerConfigV2 {
                        bind_address: *bind_addr,
                        performance_preset: *performance_preset,
                        ..Default::default()
                    })
                    .profile(profile.clone())
                    .build()
                    .map_err(|error| {
                        config_error(format!("failed to build modbus tcp server: {}", error))
                    })?;

                if let Some(fault_injection) = runtime_config.fault_injection.clone() {
                    if fault_injection.enabled {
                        server = server
                            .with_fault_pipeline(FaultPipeline::from_config(&fault_injection));
                    }
                }
                if let Some(connection_disruption) = runtime_config.connection_disruption.clone() {
                    server = server.with_connection_disruption(connection_disruption);
                }

                ModbusRuntimeServer::Tcp(Arc::new(server))
            }
            ModbusTransportLaunch::Rtu { config } => {
                let mut server = ModbusRtuServer::new(config.clone());
                server.set_broadcast_enabled(profile.broadcast_enabled);
                for unit in profile.units.iter().cloned() {
                    let device = ModbusDevice::from_profile(&unit).map_err(|error| {
                        config_error(format!("failed to build modbus rtu device: {}", error))
                    })?;
                    server.add_device(device);
                }
                if let Some(fault_injection) = runtime_config.fault_injection.clone() {
                    if fault_injection.enabled {
                        server = server
                            .with_fault_pipeline(FaultPipeline::from_config(&fault_injection));
                    }
                }
                ModbusRuntimeServer::Rtu(Arc::new(server))
            }
        };

        Ok(Arc::new(ModbusManagedService::new(
            server,
            spec.service_name(&self.descriptor()),
            launch,
            profile,
        )))
    }
}

pub fn descriptor() -> ProtocolDescriptor {
    ModbusDriver.descriptor()
}

pub fn driver() -> ModbusDriver {
    ModbusDriver
}

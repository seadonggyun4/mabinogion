use std::collections::BTreeMap;
use std::sync::Arc;

use async_trait::async_trait;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use serde_json::{json, to_value};

use mabi_core::device::{Device, DeviceInfo};
use mabi_core::types::DataType;
use mabi_core::Protocol;
use mabi_runtime::{
    DevicePort, DeviceRegistry, ManagedService, ProtocolDescriptor, ProtocolDriver,
    ProtocolLaunchSpec, RuntimeError, RuntimeExtensions, RuntimeResult, ServiceContext,
    ServiceSnapshot, ServiceState, ServiceStatus,
};

use crate::fault_injection::{FaultInjectionConfig, FaultPipeline};
use crate::{ConnectionDisruptionConfig, ModbusDevice, ModbusDeviceConfig, ModbusTcpServerV2};

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ModbusLaunchConfig {
    bind_addr: std::net::SocketAddr,
    devices: usize,
    points_per_device: usize,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct ModbusProtocolRuntimeConfig {
    #[serde(default)]
    fault_injection: Option<FaultInjectionConfig>,
    #[serde(default)]
    connection_disruption: Option<ConnectionDisruptionConfig>,
}

fn runtime_error(message: impl Into<String>) -> RuntimeError {
    RuntimeError::service(message)
}

fn new_status(name: &str) -> ServiceStatus {
    let mut status = ServiceStatus::new(name);
    status.protocol = Some(Protocol::ModbusTcp);
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
}

struct ModbusManagedService {
    server: Arc<ModbusTcpServerV2>,
    launch: ModbusLaunchConfig,
    status: RwLock<ServiceStatus>,
}

impl ModbusManagedService {
    fn new(server: Arc<ModbusTcpServerV2>, name: String, launch: ModbusLaunchConfig) -> Self {
        Self {
            server,
            launch,
            status: RwLock::new(new_status(&name)),
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
                Err(runtime_error(format!("modbus server failed: {}", error)))
            }
        }
    }

    fn status(&self) -> ServiceStatus {
        self.status.read().clone()
    }

    async fn snapshot(&self) -> RuntimeResult<ServiceSnapshot> {
        let metrics = self.server.metrics().snapshot();
        let mut metadata = BTreeMap::new();
        metadata.insert(
            "bind_address".to_string(),
            to_value(self.launch.bind_addr.to_string())
                .map_err(|error| runtime_error(error.to_string()))?,
        );
        metadata.insert(
            "devices".to_string(),
            to_value(self.launch.devices).map_err(|error| runtime_error(error.to_string()))?,
        );
        metadata.insert(
            "points_per_device".to_string(),
            to_value(self.launch.points_per_device)
                .map_err(|error| runtime_error(error.to_string()))?,
        );
        metadata.insert(
            "metrics".to_string(),
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
            }),
        );
        Ok(snapshot_with_metadata(&self.status(), metadata))
    }

    fn register_devices(&self, registry: &DeviceRegistry) -> RuntimeResult<()> {
        for unit_id in 1..=self.launch.devices {
            if let Some(device) = self.server.device(unit_id as u8) {
                let device_id = device.id().to_string();
                registry.register(device_id, Arc::new(ModbusDevicePort::new(device)));
            }
        }
        Ok(())
    }
}

fn populate_default_points(device: &mut ModbusDevice, requested_points: usize) {
    let family_points = std::cmp::max(1, requested_points / 4);
    for index in 0..family_points {
        let address = index as u16;
        device.add_holding_register(
            format!("holding_{}", index),
            format!("Holding Register {}", index),
            address,
            DataType::UInt16,
        );
        device.add_input_register(
            format!("input_{}", index),
            format!("Input Register {}", index),
            address,
            DataType::UInt16,
        );
        device.add_coil(
            format!("coil_{}", index),
            format!("Coil {}", index),
            address,
        );
        device.add_discrete_input(
            format!("discrete_{}", index),
            format!("Discrete Input {}", index),
            address,
        );
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ModbusDriver;

impl ModbusDriver {
    fn protocol_runtime_config(
        extensions: &RuntimeExtensions,
    ) -> RuntimeResult<ModbusProtocolRuntimeConfig> {
        match extensions.protocol_config("modbus") {
            Some(config) => serde_json::from_value(config.clone()).map_err(|error| {
                runtime_error(format!("invalid modbus runtime config: {}", error))
            }),
            None => Ok(ModbusProtocolRuntimeConfig::default()),
        }
    }
}

#[async_trait]
impl ProtocolDriver for ModbusDriver {
    fn descriptor(&self) -> ProtocolDescriptor {
        ProtocolDescriptor {
            key: "modbus",
            display_name: "Modbus TCP",
            protocol: Protocol::ModbusTcp,
            default_port: 502,
            description: "Serve Modbus TCP devices through the shared runtime",
        }
    }

    fn features(&self) -> &'static [&'static str] {
        &[
            "multi-unit devices",
            "register families",
            "controller-visible device ports",
        ]
    }

    async fn build(
        &self,
        spec: ProtocolLaunchSpec,
        extensions: RuntimeExtensions,
    ) -> RuntimeResult<Arc<dyn ManagedService>> {
        let launch: ModbusLaunchConfig = serde_json::from_value(spec.config.clone())
            .map_err(|error| runtime_error(format!("invalid modbus launch config: {}", error)))?;
        let runtime_config = Self::protocol_runtime_config(&extensions)?;

        let mut server = ModbusTcpServerV2::new(crate::tcp::ServerConfigV2 {
            bind_address: launch.bind_addr,
            ..Default::default()
        });

        if let Some(fault_injection) = runtime_config.fault_injection {
            if fault_injection.enabled {
                server = server.with_fault_pipeline(FaultPipeline::from_config(&fault_injection));
            }
        }
        if let Some(connection_disruption) = runtime_config.connection_disruption {
            server = server.with_connection_disruption(connection_disruption);
        }

        for index in 0..launch.devices {
            let unit_id = (index + 1) as u8;
            let points_per_family = std::cmp::max(1, launch.points_per_device / 4) as u16;
            let mut device = ModbusDevice::new(ModbusDeviceConfig {
                unit_id,
                name: format!("Device-{}", unit_id),
                holding_registers: points_per_family,
                input_registers: points_per_family,
                coils: points_per_family,
                discrete_inputs: points_per_family,
                response_delay_ms: 0,
                tags: mabi_core::tags::Tags::new(),
            });
            populate_default_points(&mut device, launch.points_per_device);
            server.add_device(device);
        }

        Ok(Arc::new(ModbusManagedService::new(
            Arc::new(server),
            spec.service_name(&self.descriptor()),
            launch,
        )))
    }
}

pub fn descriptor() -> ProtocolDescriptor {
    ModbusDriver.descriptor()
}

pub fn driver() -> ModbusDriver {
    ModbusDriver
}

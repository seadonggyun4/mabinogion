use std::collections::BTreeMap;
use std::sync::Arc;

use async_trait::async_trait;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use serde_json::{json, to_value};

use mabi_core::device::{DeviceInfo, DeviceState};
use mabi_core::types::{DataPoint, DataPointId};
use mabi_core::{Protocol, Value};
use mabi_runtime::{
    DevicePort, DeviceRegistry, ManagedService, ProtocolDescriptor, ProtocolDriver,
    ProtocolLaunchSpec, RuntimeExtensions, RuntimeResult, ServiceContext, ServiceSnapshot,
    ServiceState, ServiceStatus,
};

use crate::{NodeId, OpcUaError, OpcUaServer, OpcUaServerConfig, Variant};

#[derive(Debug, Clone, Serialize, Deserialize)]
struct OpcUaLaunchConfig {
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

struct OpcUaDevicePort {
    server: Arc<OpcUaServer>,
    info: DeviceInfo,
}

impl OpcUaDevicePort {
    fn new(server: Arc<OpcUaServer>, device_id: String, name: String) -> Self {
        Self {
            server,
            info: DeviceInfo::new(device_id, name, Protocol::OpcUa),
        }
    }
}

#[async_trait]
impl DevicePort for OpcUaDevicePort {
    fn info(&self) -> DeviceInfo {
        let mut info = self.info.clone();
        info.state = match self.server.state() {
            crate::ServerState::Stopped => DeviceState::Offline,
            crate::ServerState::Starting => DeviceState::Initializing,
            crate::ServerState::Running => DeviceState::Online,
            crate::ServerState::Stopping => DeviceState::ShuttingDown,
            crate::ServerState::Error => DeviceState::Error,
            crate::ServerState::Uninitialized => DeviceState::Offline,
        };
        info
    }

    async fn start(&self) -> mabi_core::Result<()> {
        Ok(())
    }

    async fn stop(&self) -> mabi_core::Result<()> {
        Ok(())
    }

    async fn read(&self, point_id: &str) -> mabi_core::Result<DataPoint> {
        let node_id = point_id
            .parse::<NodeId>()
            .map_err(|error| mabi_core::Error::Protocol(error.to_string()))?;
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
        let node_id = point_id
            .parse::<NodeId>()
            .map_err(|error| mabi_core::Error::Protocol(error.to_string()))?;
        self.server
            .write_value(&node_id, Variant::from(value))
            .map_err(|error| mabi_core::Error::Protocol(error.to_string()))
    }
}

struct OpcUaManagedService {
    server: Arc<OpcUaServer>,
    launch: OpcUaLaunchConfig,
    status: RwLock<ServiceStatus>,
}

impl OpcUaManagedService {
    fn new(server: Arc<OpcUaServer>, name: String, launch: OpcUaLaunchConfig) -> Self {
        Self {
            server,
            launch,
            status: RwLock::new(new_status(&name)),
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
            to_value(format!(
                "opc.tcp://{}{}",
                self.launch.bind_addr, self.launch.endpoint_path
            ))
            .map_err(|error| runtime_error(error.to_string()))?,
        );
        metadata.insert(
            "nodes".to_string(),
            to_value(self.launch.nodes).map_err(|error| runtime_error(error.to_string()))?,
        );
        metadata.insert(
            "security_mode".to_string(),
            to_value(self.launch.security_mode.clone())
                .map_err(|error| runtime_error(error.to_string()))?,
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
        let device_id = format!("opcua-{}", self.status().name);
        registry.register(
            device_id.clone(),
            Arc::new(OpcUaDevicePort::new(
                Arc::clone(&self.server),
                device_id,
                self.status().name,
            )),
        );
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
        ]
    }

    async fn build(
        &self,
        spec: ProtocolLaunchSpec,
        _extensions: RuntimeExtensions,
    ) -> RuntimeResult<Arc<dyn ManagedService>> {
        let launch: OpcUaLaunchConfig = serde_json::from_value(spec.config.clone())
            .map_err(|error| runtime_error(format!("invalid opcua launch config: {}", error)))?;
        let config = OpcUaServerConfig {
            endpoint_url: format!("opc.tcp://{}{}", launch.bind_addr, launch.endpoint_path),
            server_name: "Mabinogion OPC UA Simulator".to_string(),
            max_subscriptions: 1000,
            max_monitored_items: 10_000,
            ..Default::default()
        };
        let server = Arc::new(OpcUaServer::new(config).map_err(|error: OpcUaError| {
            runtime_error(format!("failed to create opcua server: {}", error))
        })?);
        for index in 0..launch.nodes.min(100) {
            let node_id = format!("ns=2;i={}", 1000 + index);
            let name = format!("Variable_{}", index);
            let value = (index as f64) * 0.1;
            if index % 2 == 0 {
                let _ = server.add_writable_variable(node_id, name, value);
            } else {
                let _ = server.add_variable(node_id, name, value);
            }
        }
        Ok(Arc::new(OpcUaManagedService::new(
            server,
            spec.service_name(&self.descriptor()),
            launch,
        )))
    }
}

pub fn descriptor() -> ProtocolDescriptor {
    OpcUaDriver.descriptor()
}

pub fn driver() -> OpcUaDriver {
    OpcUaDriver
}

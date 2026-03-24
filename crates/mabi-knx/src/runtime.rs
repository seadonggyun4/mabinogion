use std::collections::BTreeMap;
use std::sync::Arc;

use async_trait::async_trait;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use serde_json::to_value;

use mabi_core::device::DeviceInfo;
use mabi_core::types::{DataPoint, DataPointId};
use mabi_core::{Protocol, Value};
use mabi_runtime::{
    DevicePort, DeviceRegistry, ManagedService, ProtocolDescriptor, ProtocolDriver,
    ProtocolLaunchSpec, RuntimeExtensions, RuntimeResult, ServiceContext, ServiceSnapshot,
    ServiceState, ServiceStatus,
};

use crate::{
    DptId, DptValue, GroupAddress, GroupObjectTable, IndividualAddress, KnxServer, KnxServerConfig,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
struct KnxLaunchConfig {
    bind_addr: std::net::SocketAddr,
    individual_address: String,
    group_objects: usize,
}

fn runtime_error(message: impl Into<String>) -> mabi_runtime::RuntimeError {
    mabi_runtime::RuntimeError::service(message)
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

struct KnxDevicePort {
    info: DeviceInfo,
    group_objects: Arc<GroupObjectTable>,
}

impl KnxDevicePort {
    fn new(device_id: String, name: String, group_objects: Arc<GroupObjectTable>) -> Self {
        Self {
            info: DeviceInfo::new(device_id, name, Protocol::KnxIp),
            group_objects,
        }
    }

    fn dpt_to_value(dpt_value: &DptValue) -> Value {
        match dpt_value {
            DptValue::Bool(value) => Value::Bool(*value),
            DptValue::U8(value) => Value::U8(*value),
            DptValue::I8(value) => Value::I8(*value),
            DptValue::U16(value) => Value::U16(*value),
            DptValue::I16(value) => Value::I16(*value),
            DptValue::U32(value) => Value::U32(*value),
            DptValue::I32(value) => Value::I32(*value),
            DptValue::F16(value) => Value::F32(*value),
            DptValue::F32(value) => Value::F32(*value),
            DptValue::String(value) => Value::String(value.clone()),
            DptValue::Raw(value) => Value::Bytes(value.clone()),
            _ => Value::Null,
        }
    }

    fn value_to_dpt(value: &Value) -> DptValue {
        match value {
            Value::Bool(value) => DptValue::Bool(*value),
            Value::I8(value) => DptValue::I8(*value),
            Value::U8(value) => DptValue::U8(*value),
            Value::I16(value) => DptValue::I16(*value),
            Value::U16(value) => DptValue::U16(*value),
            Value::I32(value) => DptValue::I32(*value),
            Value::U32(value) => DptValue::U32(*value),
            Value::I64(value) => DptValue::I32(*value as i32),
            Value::U64(value) => DptValue::U32(*value as u32),
            Value::F32(value) => DptValue::F32(*value),
            Value::F64(value) => DptValue::F32(*value as f32),
            Value::String(value) => DptValue::String(value.clone()),
            Value::Bytes(value) => DptValue::Raw(value.clone()),
            _ => DptValue::Raw(vec![]),
        }
    }
}

#[async_trait]
impl DevicePort for KnxDevicePort {
    fn info(&self) -> DeviceInfo {
        self.info.clone()
    }

    async fn start(&self) -> mabi_core::Result<()> {
        Ok(())
    }

    async fn stop(&self) -> mabi_core::Result<()> {
        Ok(())
    }

    async fn read(&self, point_id: &str) -> mabi_core::Result<DataPoint> {
        let address_str = point_id.split('_').next().unwrap_or(point_id);
        let address = address_str
            .parse::<GroupAddress>()
            .map_err(|error| mabi_core::Error::Protocol(error.to_string()))?;
        let value = self
            .group_objects
            .read_value(&address)
            .map(|value| Self::dpt_to_value(&value))
            .map_err(|error| mabi_core::Error::Protocol(error.to_string()))?;
        Ok(DataPoint::new(
            DataPointId::new(&self.info.id, point_id),
            value,
        ))
    }

    async fn write(&self, point_id: &str, value: Value) -> mabi_core::Result<()> {
        let address_str = point_id.split('_').next().unwrap_or(point_id);
        let address = address_str
            .parse::<GroupAddress>()
            .map_err(|error| mabi_core::Error::Protocol(error.to_string()))?;
        self.group_objects
            .write_value(
                &address,
                &Self::value_to_dpt(&value),
                Some(self.info.id.clone()),
            )
            .map_err(|error| mabi_core::Error::Protocol(error.to_string()))
    }
}

struct KnxManagedService {
    server: Arc<KnxServer>,
    launch: KnxLaunchConfig,
    status: RwLock<ServiceStatus>,
}

impl KnxManagedService {
    fn new(server: Arc<KnxServer>, name: String, launch: KnxLaunchConfig) -> Self {
        let mut status = ServiceStatus::new(name);
        status.protocol = Some(Protocol::KnxIp);
        Self {
            server,
            launch,
            status: RwLock::new(status),
        }
    }
}

#[async_trait]
impl ManagedService for KnxManagedService {
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
            .map_err(|error| runtime_error(format!("knx stop failed: {}", error)))?;
        let mut status = self.status.write();
        status.state = ServiceState::Stopped;
        status.ready = false;
        Ok(())
    }

    async fn serve(&self, _context: ServiceContext) -> RuntimeResult<()> {
        {
            let mut status = self.status.write();
            status.state = ServiceState::Running;
            status.ready = true;
        }
        match self.server.start().await {
            Ok(()) => {
                let mut status = self.status.write();
                status.state = ServiceState::Stopped;
                status.ready = false;
                Ok(())
            }
            Err(error) => {
                let mut status = self.status.write();
                status.state = ServiceState::Error;
                status.ready = false;
                status.last_error = Some(error.to_string());
                Err(runtime_error(format!("knx server failed: {}", error)))
            }
        }
    }

    fn status(&self) -> ServiceStatus {
        self.status.read().clone()
    }

    async fn snapshot(&self) -> RuntimeResult<ServiceSnapshot> {
        let mut metadata = BTreeMap::new();
        metadata.insert(
            "bind_address".to_string(),
            to_value(self.launch.bind_addr.to_string())
                .map_err(|error| runtime_error(error.to_string()))?,
        );
        metadata.insert(
            "individual_address".to_string(),
            to_value(self.launch.individual_address.clone())
                .map_err(|error| runtime_error(error.to_string()))?,
        );
        metadata.insert(
            "group_objects".to_string(),
            to_value(self.launch.group_objects)
                .map_err(|error| runtime_error(error.to_string()))?,
        );
        metadata.insert(
            "metrics".to_string(),
            to_value(self.server.metrics_snapshot())
                .map_err(|error| runtime_error(error.to_string()))?,
        );
        Ok(snapshot_with_metadata(&self.status(), metadata))
    }

    fn register_devices(&self, registry: &DeviceRegistry) -> RuntimeResult<()> {
        let device_id = format!("knx-{}", self.launch.individual_address.replace('.', "-"));
        registry.register(
            device_id.clone(),
            Arc::new(KnxDevicePort::new(
                device_id,
                self.status().name,
                self.server.group_objects(),
            )),
        );
        Ok(())
    }
}

#[derive(Debug, Clone, Copy)]
pub struct KnxDriver;

#[async_trait]
impl ProtocolDriver for KnxDriver {
    fn descriptor(&self) -> ProtocolDescriptor {
        ProtocolDescriptor {
            key: "knx",
            display_name: "KNXnet/IP",
            protocol: Protocol::KnxIp,
            default_port: 3671,
            description: "Serve KNXnet/IP group objects through the shared runtime",
        }
    }

    fn features(&self) -> &'static [&'static str] {
        &[
            "group object table",
            "tunneling services",
            "controller-visible group port",
        ]
    }

    async fn build(
        &self,
        spec: ProtocolLaunchSpec,
        _extensions: RuntimeExtensions,
    ) -> RuntimeResult<Arc<dyn ManagedService>> {
        let launch: KnxLaunchConfig = serde_json::from_value(spec.config.clone())
            .map_err(|error| runtime_error(format!("invalid knx launch config: {}", error)))?;
        let individual_address: IndividualAddress = launch
            .individual_address
            .parse()
            .map_err(|error| runtime_error(format!("invalid KNX individual address: {}", error)))?;
        let config = KnxServerConfig {
            bind_addr: launch.bind_addr,
            individual_address,
            max_connections: 256,
            ..Default::default()
        };
        let table = Arc::new(GroupObjectTable::new());
        let dpt_types = [
            DptId::new(1, 1),
            DptId::new(5, 1),
            DptId::new(9, 1),
            DptId::new(9, 4),
            DptId::new(9, 7),
            DptId::new(12, 1),
            DptId::new(13, 1),
            DptId::new(14, 56),
        ];
        let dpt_names = [
            "Switch",
            "Scaling",
            "Temperature",
            "Lux",
            "Humidity",
            "Counter",
            "SignedCounter",
            "Float",
        ];
        for index in 0..launch.group_objects {
            let main = ((index / 256) + 1) as u8;
            let middle = ((index / 8) % 8) as u8;
            let sub = (index % 256) as u8;
            let address = GroupAddress::three_level(main, middle, sub);
            let dpt_idx = index % dpt_types.len();
            let name = format!("{}_{}", dpt_names[dpt_idx], index);
            let _ = table.create(address, &name, &dpt_types[dpt_idx]);
        }
        let server = Arc::new(KnxServer::new(config).with_group_objects(table));
        Ok(Arc::new(KnxManagedService::new(
            server,
            spec.service_name(&self.descriptor()),
            launch,
        )))
    }
}

pub fn descriptor() -> ProtocolDescriptor {
    KnxDriver.descriptor()
}

pub fn driver() -> KnxDriver {
    KnxDriver
}

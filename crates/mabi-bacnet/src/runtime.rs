use std::collections::BTreeMap;
use std::sync::Arc;

use async_trait::async_trait;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use serde_json::{json, to_value, Value as JsonValue};

use mabi_core::device::DeviceInfo;
use mabi_core::types::{DataPoint, DataPointId};
use mabi_core::{Protocol, Value};
use mabi_runtime::{
    DevicePort, DeviceRegistry, ManagedService, ProtocolDescriptor, ProtocolDriver,
    ProtocolLaunchSpec, RuntimeExtensions, RuntimeResult, ServiceContext, ServiceSnapshot,
    ServiceState, ServiceStatus,
};

use crate::object::property::{BACnetValue, PropertyId};
use crate::object::registry::default_object_descriptors;
use crate::object::types::{ObjectId, ObjectType};
use crate::prelude::{BACnetServer, ObjectRegistry, ServerConfig};

#[derive(Debug, Clone, Serialize, Deserialize)]
struct BacnetLaunchConfig {
    bind_addr: std::net::SocketAddr,
    device_instance: u32,
    objects: usize,
    bbmd_enabled: bool,
}

fn runtime_error(message: impl Into<String>) -> mabi_runtime::RuntimeError {
    mabi_runtime::RuntimeError::service(message)
}

fn snapshot_with_metadata(
    status: &ServiceStatus,
    metadata: BTreeMap<String, JsonValue>,
) -> ServiceSnapshot {
    let mut snapshot = ServiceSnapshot::new(status.name.clone());
    snapshot.protocol = status.protocol;
    snapshot.status = status.clone();
    snapshot.metadata = metadata;
    snapshot
}

struct BacnetDevicePort {
    info: DeviceInfo,
    objects: Arc<ObjectRegistry>,
}

impl BacnetDevicePort {
    fn new(
        device_id: String,
        name: String,
        device_instance: u32,
        objects: Arc<ObjectRegistry>,
    ) -> Self {
        let info = DeviceInfo::new(device_id, name, Protocol::BacnetIp)
            .with_metadata("device_instance", device_instance.to_string());
        Self { info, objects }
    }

    fn point_id_to_object_id(point_id: &str) -> Option<ObjectId> {
        let parts: Vec<&str> = point_id.split(':').collect();
        if parts.len() != 2 {
            return None;
        }
        let type_code = parts[0].parse().ok()?;
        let instance = parts[1].parse().ok()?;
        Some(ObjectId::new(ObjectType::from_u16(type_code)?, instance))
    }

    fn bacnet_to_core_value(bacnet: &BACnetValue) -> Option<Value> {
        match bacnet {
            BACnetValue::Null => Some(Value::Null),
            BACnetValue::Boolean(value) => Some(Value::Bool(*value)),
            BACnetValue::Unsigned(value) => Some(Value::U32(*value)),
            BACnetValue::Signed(value) => Some(Value::I32(*value)),
            BACnetValue::Real(value) => Some(Value::F32(*value)),
            BACnetValue::Double(value) => Some(Value::F64(*value)),
            BACnetValue::CharacterString(value) => Some(Value::String(value.clone())),
            BACnetValue::Enumerated(value) => Some(Value::U32(*value)),
            _ => None,
        }
    }

    fn core_to_bacnet_value(value: &Value) -> Option<BACnetValue> {
        match value {
            Value::Null => Some(BACnetValue::Null),
            Value::Bool(value) => Some(BACnetValue::Boolean(*value)),
            Value::U8(value) => Some(BACnetValue::Unsigned(*value as u32)),
            Value::U16(value) => Some(BACnetValue::Unsigned(*value as u32)),
            Value::U32(value) => Some(BACnetValue::Unsigned(*value)),
            Value::U64(value) => Some(BACnetValue::Unsigned(*value as u32)),
            Value::I8(value) => Some(BACnetValue::Signed(*value as i32)),
            Value::I16(value) => Some(BACnetValue::Signed(*value as i32)),
            Value::I32(value) => Some(BACnetValue::Signed(*value)),
            Value::I64(value) => Some(BACnetValue::Signed(*value as i32)),
            Value::F32(value) => Some(BACnetValue::Real(*value)),
            Value::F64(value) => Some(BACnetValue::Double(*value)),
            Value::String(value) => Some(BACnetValue::CharacterString(value.clone())),
            _ => None,
        }
    }
}

#[async_trait]
impl DevicePort for BacnetDevicePort {
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
        let object_id = Self::point_id_to_object_id(point_id)
            .ok_or_else(|| mabi_core::Error::point_not_found(&self.info.id, point_id))?;
        let value = self
            .objects
            .read_property(&object_id, PropertyId::PresentValue)
            .map_err(|error| mabi_core::Error::Protocol(error.to_string()))
            .and_then(|value| {
                Self::bacnet_to_core_value(&value)
                    .ok_or_else(|| mabi_core::Error::Protocol("cannot convert BACnet value".into()))
            })?;
        Ok(DataPoint::new(
            DataPointId::new(&self.info.id, point_id),
            value,
        ))
    }

    async fn write(&self, point_id: &str, value: Value) -> mabi_core::Result<()> {
        let object_id = Self::point_id_to_object_id(point_id)
            .ok_or_else(|| mabi_core::Error::point_not_found(&self.info.id, point_id))?;
        let bacnet_value = Self::core_to_bacnet_value(&value)
            .ok_or_else(|| mabi_core::Error::Protocol("cannot convert BACnet value".into()))?;
        self.objects
            .write_property(&object_id, PropertyId::PresentValue, bacnet_value)
            .map_err(|error| mabi_core::Error::Protocol(error.to_string()))
    }
}

struct BacnetManagedService {
    server: Arc<BACnetServer>,
    launch: BacnetLaunchConfig,
    status: RwLock<ServiceStatus>,
}

impl BacnetManagedService {
    fn new(server: Arc<BACnetServer>, name: String, launch: BacnetLaunchConfig) -> Self {
        let mut status = ServiceStatus::new(name);
        status.protocol = Some(Protocol::BacnetIp);
        Self {
            server,
            launch,
            status: RwLock::new(status),
        }
    }
}

#[async_trait]
impl ManagedService for BacnetManagedService {
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
        self.server.shutdown();
        Ok(())
    }

    async fn serve(&self, _context: ServiceContext) -> RuntimeResult<()> {
        {
            let mut status = self.status.write();
            status.state = ServiceState::Running;
            status.ready = true;
        }
        match self.server.run().await {
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
                Err(runtime_error(format!("bacnet server failed: {}", error)))
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
                .map_err(|error: serde_json::Error| runtime_error(error.to_string()))?,
        );
        metadata.insert(
            "device_instance".to_string(),
            to_value(self.launch.device_instance)
                .map_err(|error: serde_json::Error| runtime_error(error.to_string()))?,
        );
        metadata.insert(
            "objects".to_string(),
            to_value(self.launch.objects)
                .map_err(|error: serde_json::Error| runtime_error(error.to_string()))?,
        );
        metadata.insert(
            "bbmd_enabled".to_string(),
            to_value(self.launch.bbmd_enabled)
                .map_err(|error: serde_json::Error| runtime_error(error.to_string()))?,
        );
        metadata.insert(
            "metrics".to_string(),
            json!({
                "requests_received": metrics.requests_received,
                "requests_success": metrics.requests_success,
                "requests_error": metrics.requests_error,
                "who_is_requests": metrics.who_is_requests,
                "i_am_sent": metrics.i_am_sent,
                "read_property_requests": metrics.read_property_requests,
                "write_property_requests": metrics.write_property_requests,
                "cov_subscriptions": metrics.cov_subscriptions,
                "cov_notifications_sent": metrics.cov_notifications_sent,
                "bytes_received": metrics.bytes_received,
                "bytes_sent": metrics.bytes_sent,
                "average_latency_us": metrics.average_latency_us,
                "uptime_secs": metrics.uptime_secs,
            }),
        );
        Ok(snapshot_with_metadata(&self.status(), metadata))
    }

    fn register_devices(&self, registry: &DeviceRegistry) -> RuntimeResult<()> {
        let device_id = format!("bacnet-{}", self.launch.device_instance);
        registry.register(
            device_id.clone(),
            Arc::new(BacnetDevicePort::new(
                device_id,
                self.status().name,
                self.launch.device_instance,
                Arc::clone(self.server.objects()),
            )),
        );
        Ok(())
    }
}

#[derive(Debug, Clone, Copy)]
pub struct BacnetDriver;

#[async_trait]
impl ProtocolDriver for BacnetDriver {
    fn descriptor(&self) -> ProtocolDescriptor {
        ProtocolDescriptor {
            key: "bacnet",
            display_name: "BACnet/IP",
            protocol: Protocol::BacnetIp,
            default_port: 47_808,
            description: "Serve BACnet/IP objects through the shared runtime",
        }
    }

    fn features(&self) -> &'static [&'static str] {
        &[
            "standard object registry",
            "COV workflows",
            "controller-visible object port",
        ]
    }

    async fn build(
        &self,
        spec: ProtocolLaunchSpec,
        _extensions: RuntimeExtensions,
    ) -> RuntimeResult<Arc<dyn ManagedService>> {
        let launch: BacnetLaunchConfig = serde_json::from_value(spec.config.clone())
            .map_err(|error| runtime_error(format!("invalid bacnet launch config: {}", error)))?;
        let config = ServerConfig::new(launch.device_instance)
            .with_bind_addr(launch.bind_addr)
            .with_device_name("Mabinogion BACnet Simulator");
        let registry = ObjectRegistry::new();
        if launch.objects > 0 {
            let descriptors = default_object_descriptors();
            let objects_per_type = std::cmp::max(1, launch.objects / descriptors.len());
            registry.populate_standard_objects(&descriptors, objects_per_type);
        }
        let server = Arc::new(BACnetServer::new(config, registry));
        Ok(Arc::new(BacnetManagedService::new(
            server,
            spec.service_name(&self.descriptor()),
            launch,
        )))
    }
}

pub fn descriptor() -> ProtocolDescriptor {
    BacnetDriver.descriptor()
}

pub fn driver() -> BacnetDriver {
    BacnetDriver
}

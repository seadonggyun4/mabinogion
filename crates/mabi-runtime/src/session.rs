use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use tokio::time::Duration;

use mabi_core::Protocol;

use crate::device::{DeviceRegistry, DynDevicePort};
use crate::driver::{ProtocolDriverRegistry, ProtocolLaunchSpec};
use crate::service::{RuntimeError, RuntimeResult, ServiceHandle, ServiceSnapshot};

/// Decorates controller-visible device ports at runtime.
pub trait DevicePortLayer: Send + Sync {
    fn decorate(&self, protocol: Option<Protocol>, port: DynDevicePort) -> DynDevicePort;
}

/// Shared runtime extensions consumed by sessions and protocol drivers.
#[derive(Clone, Default)]
pub struct RuntimeExtensions {
    device_layers: Vec<Arc<dyn DevicePortLayer>>,
    protocol_configs: std::collections::BTreeMap<String, JsonValue>,
}

impl RuntimeExtensions {
    /// Creates an empty extension set.
    pub fn new() -> Self {
        Self::default()
    }

    /// Appends a device-layer decorator.
    pub fn add_device_layer(&mut self, layer: Arc<dyn DevicePortLayer>) {
        self.device_layers.push(layer);
    }

    /// Inserts a protocol-scoped configuration payload.
    pub fn insert_protocol_config(&mut self, protocol: impl Into<String>, value: JsonValue) {
        self.protocol_configs.insert(protocol.into(), value);
    }

    /// Returns a protocol-scoped configuration payload.
    pub fn protocol_config(&self, protocol: &str) -> Option<&JsonValue> {
        self.protocol_configs.get(protocol)
    }

    /// Applies all registered device layers in insertion order.
    pub fn decorate_device_port(
        &self,
        protocol: Option<Protocol>,
        mut port: DynDevicePort,
    ) -> DynDevicePort {
        for layer in &self.device_layers {
            port = layer.decorate(protocol, port);
        }
        port
    }
}

/// Runtime session configuration shared by CLI controllers.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct RuntimeSessionSpec {
    /// Protocol services that should be launched in the session.
    #[serde(default)]
    pub services: Vec<ProtocolLaunchSpec>,
    /// Optional readiness timeout override in milliseconds.
    #[serde(default)]
    pub readiness_timeout: Option<u64>,
}

impl RuntimeSessionSpec {
    /// Returns the configured readiness timeout or a fallback duration.
    pub fn readiness_duration(&self, fallback: Duration) -> Duration {
        self.readiness_timeout
            .map(Duration::from_millis)
            .unwrap_or(fallback)
    }
}

/// Same-process runtime session containing multiple managed services.
pub struct RuntimeSession {
    spec: RuntimeSessionSpec,
    devices: DeviceRegistry,
    handles: Vec<ServiceHandle>,
}

impl RuntimeSession {
    /// Builds a runtime session from the provided spec and registry.
    pub async fn new(
        spec: RuntimeSessionSpec,
        registry: &ProtocolDriverRegistry,
        extensions: RuntimeExtensions,
    ) -> RuntimeResult<Self> {
        if spec.services.is_empty() {
            return Err(RuntimeError::service(
                "runtime session requires at least one service",
            ));
        }

        let devices = DeviceRegistry::new();
        let mut handles = Vec::with_capacity(spec.services.len());

        for launch in &spec.services {
            let driver = registry.get(launch.key()).ok_or_else(|| {
                RuntimeError::service(format!("unknown protocol driver: {}", launch.key()))
            })?;
            let descriptor = driver.descriptor();
            let service = driver.build(launch.clone(), extensions.clone()).await?;

            let service_devices = DeviceRegistry::new();
            service.register_devices(&service_devices)?;
            for (device_id, port) in service_devices.entries() {
                devices.register(
                    device_id,
                    extensions.decorate_device_port(Some(descriptor.protocol), port),
                );
            }

            handles.push(ServiceHandle::named(
                launch.service_name(&descriptor),
                Some(descriptor.protocol),
                service,
            ));
        }

        Ok(Self {
            spec,
            devices,
            handles,
        })
    }

    /// Starts all managed services and waits for readiness.
    pub async fn start(&self, fallback_readiness_timeout: Duration) -> RuntimeResult<()> {
        let readiness_timeout = self.spec.readiness_duration(fallback_readiness_timeout);
        let mut started = Vec::new();

        for handle in &self.handles {
            if let Err(error) = handle.spawn().await {
                self.stop_started(&started).await;
                return Err(error);
            }

            match handle.readiness(readiness_timeout).await {
                Ok(status) if status.ready && !status.is_terminal() => started.push(handle),
                Ok(status) => {
                    self.stop_started(&started).await;
                    return Err(RuntimeError::service(format!(
                        "service failed to become ready: {} ({:?})",
                        status.name, status.state
                    )));
                }
                Err(error) => {
                    self.stop_started(&started).await;
                    return Err(error);
                }
            }
        }

        Ok(())
    }

    async fn stop_started(&self, started: &[&ServiceHandle]) {
        for handle in started.iter().rev() {
            let _ = handle.stop().await;
        }
    }

    /// Stops all managed services in reverse order.
    pub async fn stop(&self) -> RuntimeResult<()> {
        let mut errors = Vec::new();
        for handle in self.handles.iter().rev() {
            if let Err(error) = handle.stop().await {
                errors.push(error.to_string());
            }
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(RuntimeError::service(errors.join("; ")))
        }
    }

    /// Returns the shared controller-visible device registry.
    pub fn devices(&self) -> DeviceRegistry {
        self.devices.clone()
    }

    /// Returns the current service snapshots.
    pub async fn snapshots(&self) -> RuntimeResult<Vec<ServiceSnapshot>> {
        let mut snapshots = Vec::with_capacity(self.handles.len());
        for handle in &self.handles {
            snapshots.push(handle.snapshot().await?);
        }
        Ok(snapshots)
    }

    /// Returns the managed handles.
    pub fn handles(&self) -> &[ServiceHandle] {
        &self.handles
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use async_trait::async_trait;
    use tokio::time::Duration;

    use mabi_core::Protocol;

    use crate::device::DeviceRegistry;
    use crate::driver::{
        ProtocolDescriptor, ProtocolDriver, ProtocolDriverRegistry, ProtocolLaunchSpec,
    };
    use crate::service::{
        ManagedService, RuntimeResult, ServiceContext, ServiceSnapshot, ServiceState, ServiceStatus,
    };
    use crate::session::{RuntimeExtensions, RuntimeSession, RuntimeSessionSpec};

    struct NullService {
        status: parking_lot::RwLock<ServiceStatus>,
    }

    impl NullService {
        fn new() -> Self {
            Self {
                status: parking_lot::RwLock::new(ServiceStatus::new("null")),
            }
        }
    }

    #[async_trait]
    impl ManagedService for NullService {
        async fn start(&self, _context: &ServiceContext) -> RuntimeResult<()> {
            let mut status = self.status.write();
            status.state = ServiceState::Starting;
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
            let mut snapshot = ServiceSnapshot::new("null");
            snapshot.status = self.status();
            Ok(snapshot)
        }

        fn register_devices(&self, _registry: &DeviceRegistry) -> RuntimeResult<()> {
            Ok(())
        }
    }

    struct NullDriver;

    #[async_trait]
    impl ProtocolDriver for NullDriver {
        fn descriptor(&self) -> ProtocolDescriptor {
            ProtocolDescriptor {
                key: "null",
                display_name: "Null",
                protocol: Protocol::ModbusTcp,
                default_port: 0,
                description: "null driver",
            }
        }

        async fn build(
            &self,
            _spec: ProtocolLaunchSpec,
            _extensions: RuntimeExtensions,
        ) -> RuntimeResult<Arc<dyn ManagedService>> {
            Ok(Arc::new(NullService::new()))
        }
    }

    #[tokio::test]
    async fn session_starts_and_stops_services() {
        let mut registry = ProtocolDriverRegistry::new();
        registry.register(NullDriver);

        let spec = RuntimeSessionSpec {
            services: vec![ProtocolLaunchSpec {
                protocol: "null".into(),
                name: Some("test-null".into()),
                config: serde_json::json!({}),
            }],
            readiness_timeout: Some(1_000),
        };

        let session = RuntimeSession::new(spec, &registry, RuntimeExtensions::default())
            .await
            .unwrap();
        session.start(Duration::from_secs(1)).await.unwrap();
        assert_eq!(session.snapshots().await.unwrap().len(), 1);
        session.stop().await.unwrap();
    }
}

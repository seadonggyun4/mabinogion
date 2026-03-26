use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use parking_lot::RwLock;

use mabi_core::device::{DeviceHandle, DeviceInfo, DeviceState};
use mabi_core::types::{DataPoint, DataPointDef};
use mabi_core::value::Value;
use mabi_core::Result as CoreResult;

/// Shared protocol-agnostic device surface for controllers.
#[async_trait]
pub trait DevicePort: Send + Sync {
    /// Returns the current device metadata snapshot.
    fn info(&self) -> DeviceInfo;

    /// Returns the stable device identifier.
    fn id(&self) -> String {
        self.info().id
    }

    /// Returns the current device state.
    fn state(&self) -> DeviceState {
        self.info().state
    }

    /// Starts the device port if supported.
    async fn start(&self) -> CoreResult<()>;

    /// Stops the device port if supported.
    async fn stop(&self) -> CoreResult<()>;

    /// Reads a point value.
    async fn read(&self, point_id: &str) -> CoreResult<DataPoint>;

    /// Writes a point value.
    async fn write(&self, point_id: &str, value: Value) -> CoreResult<()>;

    /// Returns the point definitions exposed by the device, if available.
    fn point_definitions(&self) -> Vec<DataPointDef> {
        Vec::new()
    }
}

/// Shared trait-object type for device controllers.
pub type DynDevicePort = Arc<dyn DevicePort>;

/// Adapter from the legacy `mabi-core` device handle into the shared runtime surface.
#[derive(Clone)]
pub struct CoreDevicePort {
    handle: DeviceHandle,
}

impl CoreDevicePort {
    /// Wraps an existing device handle.
    pub fn new(handle: DeviceHandle) -> Self {
        Self { handle }
    }

    /// Returns the wrapped handle.
    pub fn handle(&self) -> &DeviceHandle {
        &self.handle
    }

    /// Converts the adapter into a shared device port.
    pub fn into_shared(self) -> DynDevicePort {
        Arc::new(self)
    }
}

impl From<DeviceHandle> for CoreDevicePort {
    fn from(handle: DeviceHandle) -> Self {
        Self::new(handle)
    }
}

#[async_trait]
impl DevicePort for CoreDevicePort {
    fn info(&self) -> DeviceInfo {
        self.handle.info()
    }

    async fn start(&self) -> CoreResult<()> {
        self.handle.start().await
    }

    async fn stop(&self) -> CoreResult<()> {
        self.handle.stop().await
    }

    async fn read(&self, point_id: &str) -> CoreResult<DataPoint> {
        self.handle.read(point_id).await
    }

    async fn write(&self, point_id: &str, value: Value) -> CoreResult<()> {
        self.handle.write(point_id, value).await
    }
}

/// Shared device registry used by controllers such as scenarios and chaos layers.
#[derive(Clone, Default)]
pub struct DeviceRegistry {
    inner: Arc<RwLock<HashMap<String, DynDevicePort>>>,
}

impl DeviceRegistry {
    /// Creates an empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers a device port under the provided identifier.
    pub fn register(&self, device_id: impl Into<String>, port: DynDevicePort) {
        self.inner.write().insert(device_id.into(), port);
    }

    /// Registers a legacy device handle via the runtime adapter.
    pub fn register_handle(&self, device_id: impl Into<String>, handle: DeviceHandle) {
        self.register(device_id, CoreDevicePort::new(handle).into_shared());
    }

    /// Returns a device port if present.
    pub fn get(&self, device_id: &str) -> Option<DynDevicePort> {
        self.inner.read().get(device_id).cloned()
    }

    /// Returns whether a device exists.
    pub fn contains(&self, device_id: &str) -> bool {
        self.inner.read().contains_key(device_id)
    }

    /// Returns all registered device identifiers.
    pub fn device_ids(&self) -> Vec<String> {
        self.inner.read().keys().cloned().collect()
    }

    /// Returns a snapshot of all registered device ports.
    pub fn entries(&self) -> Vec<(String, DynDevicePort)> {
        self.inner
            .read()
            .iter()
            .map(|(device_id, port)| (device_id.clone(), Arc::clone(port)))
            .collect()
    }

    /// Returns the device count.
    pub fn len(&self) -> usize {
        self.inner.read().len()
    }

    /// Returns true if the registry is empty.
    pub fn is_empty(&self) -> bool {
        self.inner.read().is_empty()
    }

    /// Removes a device by identifier.
    pub fn remove(&self, device_id: &str) -> Option<DynDevicePort> {
        self.inner.write().remove(device_id)
    }

    /// Clears the registry.
    pub fn clear(&self) {
        self.inner.write().clear();
    }
}

#[cfg(test)]
mod tests {
    use super::DeviceRegistry;

    #[test]
    fn registry_starts_empty() {
        let registry = DeviceRegistry::new();
        assert!(registry.is_empty());
        assert_eq!(registry.len(), 0);
    }
}

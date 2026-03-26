use std::collections::BTreeMap;
use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;

use mabi_core::Protocol;

use crate::service::{ManagedService, RuntimeResult};
use crate::session::RuntimeExtensions;

/// Descriptor for a protocol driver registered with the runtime.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProtocolDescriptor {
    /// Stable identifier used by the CLI and docs.
    pub key: &'static str,
    /// Human-readable protocol name.
    pub display_name: &'static str,
    /// Shared protocol enum.
    pub protocol: Protocol,
    /// Default listening port for the protocol.
    pub default_port: u16,
    /// Short description shown in help and inspection output.
    pub description: &'static str,
}

/// Generic launch request used by runtime sessions and the CLI.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProtocolLaunchSpec {
    /// Stable protocol key matching a registered descriptor.
    pub protocol: String,
    /// Optional service name override.
    #[serde(default)]
    pub name: Option<String>,
    /// Protocol-specific configuration payload.
    #[serde(default)]
    pub config: JsonValue,
}

impl ProtocolLaunchSpec {
    /// Returns the stable protocol key.
    pub fn key(&self) -> &str {
        &self.protocol
    }

    /// Returns the runtime service name using the descriptor as fallback.
    pub fn service_name(&self, descriptor: &ProtocolDescriptor) -> String {
        self.name
            .clone()
            .unwrap_or_else(|| descriptor.key.to_string())
    }
}

/// Registry-facing catalog entry.
#[derive(Debug, Clone, Serialize)]
pub struct ProtocolCatalogEntry {
    pub descriptor: ProtocolDescriptor,
    pub features: Vec<&'static str>,
}

/// Compile-time extensibility point for protocol service creation.
#[async_trait]
pub trait ProtocolDriver: Send + Sync {
    /// Returns the driver descriptor.
    fn descriptor(&self) -> ProtocolDescriptor;

    /// Returns a short feature list for CLI inspection surfaces.
    fn features(&self) -> &'static [&'static str] {
        &[]
    }

    /// Returns an optional schema summary for CLI inspection surfaces.
    fn schema(&self) -> Option<JsonValue> {
        None
    }

    /// Builds a managed service from the generic launch request.
    async fn build(
        &self,
        spec: ProtocolLaunchSpec,
        extensions: RuntimeExtensions,
    ) -> RuntimeResult<Arc<dyn ManagedService>>;
}

/// Shared registry for protocol drivers.
#[derive(Default, Clone)]
pub struct ProtocolDriverRegistry {
    drivers: BTreeMap<String, Arc<dyn ProtocolDriver>>,
}

impl ProtocolDriverRegistry {
    /// Creates an empty registry.
    pub fn new() -> Self {
        Self {
            drivers: BTreeMap::new(),
        }
    }

    /// Registers a driver by its descriptor key.
    pub fn register(&mut self, driver: impl ProtocolDriver + 'static) {
        let descriptor = driver.descriptor();
        self.drivers
            .insert(descriptor.key.to_string(), Arc::new(driver));
    }

    /// Extends the registry with all entries from another registry.
    pub fn extend(&mut self, other: &Self) {
        for (key, driver) in &other.drivers {
            self.drivers.insert(key.clone(), Arc::clone(driver));
        }
    }

    /// Returns a driver by key.
    pub fn get(&self, key: &str) -> Option<Arc<dyn ProtocolDriver>> {
        self.drivers.get(key).cloned()
    }

    /// Returns whether the registry contains a driver.
    pub fn contains(&self, key: &str) -> bool {
        self.drivers.contains_key(key)
    }

    /// Returns the registered descriptors in stable order.
    pub fn descriptors(&self) -> Vec<ProtocolDescriptor> {
        self.drivers
            .values()
            .map(|driver| driver.descriptor())
            .collect()
    }

    /// Returns catalog entries with driver features in stable order.
    pub fn catalog(&self) -> Vec<ProtocolCatalogEntry> {
        self.drivers
            .values()
            .map(|driver| ProtocolCatalogEntry {
                descriptor: driver.descriptor(),
                features: driver.features().to_vec(),
            })
            .collect()
    }

    /// Returns a schema summary for the provided protocol key, if available.
    pub fn schema(&self, key: &str) -> Option<JsonValue> {
        self.get(key).and_then(|driver| driver.schema())
    }

    /// Returns the number of registered drivers.
    pub fn len(&self) -> usize {
        self.drivers.len()
    }

    /// Returns true when the registry is empty.
    pub fn is_empty(&self) -> bool {
        self.drivers.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use async_trait::async_trait;
    use serde_json::json;

    use mabi_core::Protocol;

    use crate::driver::{
        ProtocolDescriptor, ProtocolDriver, ProtocolDriverRegistry, ProtocolLaunchSpec,
    };
    use crate::service::{
        ManagedService, RuntimeResult, ServiceContext, ServiceSnapshot, ServiceStatus,
    };
    use crate::session::RuntimeExtensions;

    struct NullService;

    #[async_trait]
    impl ManagedService for NullService {
        async fn start(&self, _context: &ServiceContext) -> RuntimeResult<()> {
            Ok(())
        }

        async fn stop(&self, _context: &ServiceContext) -> RuntimeResult<()> {
            Ok(())
        }

        async fn serve(&self, _context: ServiceContext) -> RuntimeResult<()> {
            Ok(())
        }

        fn status(&self) -> ServiceStatus {
            ServiceStatus::new("null")
        }

        async fn snapshot(&self) -> RuntimeResult<ServiceSnapshot> {
            Ok(ServiceSnapshot::new("null"))
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
                description: "test driver",
            }
        }

        fn features(&self) -> &'static [&'static str] {
            &["feature-a"]
        }

        async fn build(
            &self,
            _spec: ProtocolLaunchSpec,
            _extensions: RuntimeExtensions,
        ) -> RuntimeResult<Arc<dyn ManagedService>> {
            Ok(Arc::new(NullService))
        }
    }

    #[test]
    fn registry_returns_descriptors() {
        let mut registry = ProtocolDriverRegistry::new();
        registry.register(NullDriver);
        assert!(registry.contains("null"));
        assert_eq!(registry.len(), 1);
        assert_eq!(registry.descriptors()[0].key, "null");
    }

    #[test]
    fn registry_returns_catalog_entries() {
        let mut registry = ProtocolDriverRegistry::new();
        registry.register(NullDriver);
        let catalog = registry.catalog();
        assert_eq!(catalog.len(), 1);
        assert_eq!(catalog[0].descriptor.key, "null");
        assert_eq!(catalog[0].features, vec!["feature-a"]);
    }

    #[tokio::test]
    async fn launch_spec_keeps_service_name_override() {
        let spec = ProtocolLaunchSpec {
            protocol: "null".into(),
            name: Some("custom".into()),
            config: json!({"ok": true}),
        };
        let descriptor = NullDriver.descriptor();
        assert_eq!(spec.service_name(&descriptor), "custom");
    }
}

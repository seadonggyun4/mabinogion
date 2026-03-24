//! Device factory and plugin system for extensibility.
//!
//! This module provides abstractions for creating devices dynamically
//! and registering protocol plugins at runtime.

use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::RwLock;

use crate::config::DeviceConfig;
use crate::device::BoxedDevice;
use crate::error::{Error, Result};
use crate::protocol::Protocol;

/// A factory trait for creating devices.
///
/// Implement this trait to add support for new protocols or device types.
///
/// # Example
///
/// ```ignore
/// struct ModbusDeviceFactory;
///
/// impl DeviceFactory for ModbusDeviceFactory {
///     fn protocol(&self) -> Protocol {
///         Protocol::ModbusTcp
///     }
///
///     fn create(&self, config: DeviceConfig) -> Result<BoxedDevice> {
///         Ok(Box::new(ModbusDevice::new(config)?))
///     }
/// }
/// ```
pub trait DeviceFactory: Send + Sync {
    /// Get the protocol this factory handles.
    fn protocol(&self) -> Protocol;

    /// Create a device from configuration.
    fn create(&self, config: DeviceConfig) -> Result<BoxedDevice>;

    /// Create multiple devices from configurations.
    fn create_batch(&self, configs: Vec<DeviceConfig>) -> Result<Vec<BoxedDevice>> {
        configs.into_iter().map(|c| self.create(c)).collect()
    }

    /// Validate device configuration without creating the device.
    fn validate(&self, config: &DeviceConfig) -> Result<()> {
        if config.id.is_empty() {
            return Err(Error::Config("Device ID cannot be empty".into()));
        }
        if config.name.is_empty() {
            return Err(Error::Config("Device name cannot be empty".into()));
        }
        if config.protocol != self.protocol() {
            return Err(Error::Config(format!(
                "Protocol mismatch: expected {:?}, got {:?}",
                self.protocol(),
                config.protocol
            )));
        }
        Ok(())
    }

    /// Get factory metadata.
    fn metadata(&self) -> FactoryMetadata {
        FactoryMetadata {
            protocol: self.protocol(),
            version: "1.0.0".to_string(),
            description: format!("{:?} device factory", self.protocol()),
            capabilities: Vec::new(),
        }
    }
}

/// Metadata about a device factory.
#[derive(Debug, Clone)]
pub struct FactoryMetadata {
    /// Protocol this factory handles.
    pub protocol: Protocol,
    /// Factory version.
    pub version: String,
    /// Description.
    pub description: String,
    /// Supported capabilities.
    pub capabilities: Vec<String>,
}

/// A type-erased device factory.
pub type BoxedFactory = Box<dyn DeviceFactory>;

/// Registry for device factories.
///
/// This allows registering factories for different protocols at runtime,
/// enabling a plugin-like architecture.
///
/// # Example
///
/// ```ignore
/// let mut registry = FactoryRegistry::new();
/// registry.register(ModbusDeviceFactory)?;
///
/// let config = DeviceConfig { /* ... */ };
/// let device = registry.create_device(config)?;
/// ```
pub struct FactoryRegistry {
    factories: RwLock<HashMap<Protocol, Arc<BoxedFactory>>>,
}

impl FactoryRegistry {
    /// Create a new empty registry.
    pub fn new() -> Self {
        Self {
            factories: RwLock::new(HashMap::new()),
        }
    }

    /// Register a device factory.
    pub fn register<F: DeviceFactory + 'static>(&self, factory: F) -> Result<()> {
        let protocol = factory.protocol();
        let mut factories = self.factories.write();

        if factories.contains_key(&protocol) {
            return Err(Error::Config(format!(
                "Factory already registered for protocol {:?}",
                protocol
            )));
        }

        factories.insert(protocol, Arc::new(Box::new(factory)));
        Ok(())
    }

    /// Register a boxed factory.
    pub fn register_boxed(&self, factory: BoxedFactory) -> Result<()> {
        let protocol = factory.protocol();
        let mut factories = self.factories.write();

        if factories.contains_key(&protocol) {
            return Err(Error::Config(format!(
                "Factory already registered for protocol {:?}",
                protocol
            )));
        }

        factories.insert(protocol, Arc::new(factory));
        Ok(())
    }

    /// Unregister a factory for a protocol.
    pub fn unregister(&self, protocol: Protocol) -> bool {
        self.factories.write().remove(&protocol).is_some()
    }

    /// Get a factory for a protocol.
    pub fn get(&self, protocol: Protocol) -> Option<Arc<BoxedFactory>> {
        self.factories.read().get(&protocol).cloned()
    }

    /// Check if a factory is registered for a protocol.
    pub fn has(&self, protocol: Protocol) -> bool {
        self.factories.read().contains_key(&protocol)
    }

    /// Get all registered protocols.
    pub fn protocols(&self) -> Vec<Protocol> {
        self.factories.read().keys().copied().collect()
    }

    /// Get metadata for all registered factories.
    pub fn all_metadata(&self) -> Vec<FactoryMetadata> {
        self.factories
            .read()
            .values()
            .map(|f| f.metadata())
            .collect()
    }

    /// Create a device using the appropriate factory.
    pub fn create_device(&self, config: DeviceConfig) -> Result<BoxedDevice> {
        let factory = self
            .get(config.protocol)
            .ok_or_else(|| Error::NotSupported(format!("Protocol {:?}", config.protocol)))?;

        factory.validate(&config)?;
        factory.create(config)
    }

    /// Create multiple devices.
    pub fn create_devices(&self, configs: Vec<DeviceConfig>) -> Result<Vec<BoxedDevice>> {
        configs.into_iter().map(|c| self.create_device(c)).collect()
    }
}

impl Default for FactoryRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Plugin trait for extending the simulator.
///
/// Plugins can register factories, add event handlers, and customize behavior.
pub trait Plugin: Send + Sync {
    /// Get plugin name.
    fn name(&self) -> &str;

    /// Get plugin version.
    fn version(&self) -> &str {
        "1.0.0"
    }

    /// Get plugin description.
    fn description(&self) -> &str {
        ""
    }

    /// Initialize the plugin (called when loaded).
    fn initialize(&mut self) -> Result<()> {
        Ok(())
    }

    /// Register device factories with the registry.
    fn register_factories(&self, _registry: &FactoryRegistry) -> Result<()> {
        Ok(())
    }

    /// Shutdown the plugin (called when unloaded).
    fn shutdown(&mut self) -> Result<()> {
        Ok(())
    }
}

/// A boxed plugin.
pub type BoxedPlugin = Box<dyn Plugin>;

/// Plugin manager for loading and managing plugins.
pub struct PluginManager {
    plugins: RwLock<Vec<BoxedPlugin>>,
    registry: Arc<FactoryRegistry>,
}

impl PluginManager {
    /// Create a new plugin manager.
    pub fn new(registry: Arc<FactoryRegistry>) -> Self {
        Self {
            plugins: RwLock::new(Vec::new()),
            registry,
        }
    }

    /// Load a plugin.
    pub fn load<P: Plugin + 'static>(&self, mut plugin: P) -> Result<()> {
        plugin.initialize()?;
        plugin.register_factories(&self.registry)?;

        self.plugins.write().push(Box::new(plugin));
        Ok(())
    }

    /// Load a boxed plugin.
    pub fn load_boxed(&self, mut plugin: BoxedPlugin) -> Result<()> {
        plugin.initialize()?;
        plugin.register_factories(&self.registry)?;

        self.plugins.write().push(plugin);
        Ok(())
    }

    /// Get the number of loaded plugins.
    pub fn plugin_count(&self) -> usize {
        self.plugins.read().len()
    }

    /// Get information about loaded plugins.
    pub fn plugin_info(&self) -> Vec<PluginInfo> {
        self.plugins
            .read()
            .iter()
            .map(|p| PluginInfo {
                name: p.name().to_string(),
                version: p.version().to_string(),
                description: p.description().to_string(),
            })
            .collect()
    }

    /// Get the factory registry.
    pub fn registry(&self) -> &Arc<FactoryRegistry> {
        &self.registry
    }

    /// Shutdown all plugins.
    pub fn shutdown_all(&self) -> Result<()> {
        for plugin in self.plugins.write().iter_mut() {
            plugin.shutdown()?;
        }
        Ok(())
    }
}

/// Information about a loaded plugin.
#[derive(Debug, Clone)]
pub struct PluginInfo {
    /// Plugin name.
    pub name: String,
    /// Plugin version.
    pub version: String,
    /// Plugin description.
    pub description: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::device::{Device, DeviceInfo, DeviceStatistics};
    use crate::tags::Tags;
    use crate::types::{DataPoint, DataPointDef, DataPointId};
    use crate::value::Value;
    use async_trait::async_trait;

    // Mock device for testing
    struct MockDevice {
        info: DeviceInfo,
    }

    impl MockDevice {
        fn new(id: &str, name: &str) -> Self {
            Self {
                info: DeviceInfo::new(id, name, Protocol::ModbusTcp),
            }
        }
    }

    #[async_trait]
    impl Device for MockDevice {
        fn info(&self) -> &DeviceInfo {
            &self.info
        }

        async fn initialize(&mut self) -> Result<()> {
            Ok(())
        }

        async fn start(&mut self) -> Result<()> {
            Ok(())
        }

        async fn stop(&mut self) -> Result<()> {
            Ok(())
        }

        async fn tick(&mut self) -> Result<()> {
            Ok(())
        }

        fn point_definitions(&self) -> Vec<&DataPointDef> {
            vec![]
        }

        fn point_definition(&self, _point_id: &str) -> Option<&DataPointDef> {
            None
        }

        async fn read(&self, point_id: &str) -> Result<DataPoint> {
            Ok(DataPoint::new(
                DataPointId::new(&self.info.id, point_id),
                Value::F64(0.0),
            ))
        }

        async fn write(&mut self, _point_id: &str, _value: Value) -> Result<()> {
            Ok(())
        }

        fn statistics(&self) -> DeviceStatistics {
            DeviceStatistics::default()
        }
    }

    // Mock factory for testing
    struct MockFactory;

    impl DeviceFactory for MockFactory {
        fn protocol(&self) -> Protocol {
            Protocol::ModbusTcp
        }

        fn create(&self, config: DeviceConfig) -> Result<BoxedDevice> {
            Ok(Box::new(MockDevice::new(&config.id, &config.name)))
        }
    }

    // Mock plugin for testing
    struct MockPlugin {
        name: String,
    }

    impl MockPlugin {
        fn new(name: &str) -> Self {
            Self {
                name: name.to_string(),
            }
        }
    }

    impl Plugin for MockPlugin {
        fn name(&self) -> &str {
            &self.name
        }

        fn description(&self) -> &str {
            "Mock plugin for testing"
        }

        fn register_factories(&self, registry: &FactoryRegistry) -> Result<()> {
            registry.register(MockFactory)
        }
    }

    #[test]
    fn test_factory_registry() {
        let registry = FactoryRegistry::new();

        assert!(!registry.has(Protocol::ModbusTcp));

        registry.register(MockFactory).unwrap();

        assert!(registry.has(Protocol::ModbusTcp));
        assert!(registry.protocols().contains(&Protocol::ModbusTcp));
    }

    #[test]
    fn test_factory_create_device() {
        let registry = FactoryRegistry::new();
        registry.register(MockFactory).unwrap();

        let config = DeviceConfig {
            id: "test-001".to_string(),
            name: "Test Device".to_string(),
            description: String::new(),
            protocol: Protocol::ModbusTcp,
            address: None,
            points: vec![],
            metadata: Default::default(),
            tags: Tags::new(),
        };

        let device = registry.create_device(config).unwrap();
        assert_eq!(device.id(), "test-001");
    }

    #[test]
    fn test_factory_validation() {
        let factory = MockFactory;

        // Empty ID should fail
        let config = DeviceConfig {
            id: String::new(),
            name: "Test".to_string(),
            description: String::new(),
            protocol: Protocol::ModbusTcp,
            address: None,
            points: vec![],
            metadata: Default::default(),
            tags: Tags::new(),
        };
        assert!(factory.validate(&config).is_err());

        // Wrong protocol should fail
        let config = DeviceConfig {
            id: "test".to_string(),
            name: "Test".to_string(),
            description: String::new(),
            protocol: Protocol::OpcUa,
            address: None,
            points: vec![],
            metadata: Default::default(),
            tags: Tags::new(),
        };
        assert!(factory.validate(&config).is_err());
    }

    #[test]
    fn test_plugin_manager() {
        let registry = Arc::new(FactoryRegistry::new());
        let manager = PluginManager::new(registry.clone());

        assert_eq!(manager.plugin_count(), 0);

        manager.load(MockPlugin::new("test-plugin")).unwrap();

        assert_eq!(manager.plugin_count(), 1);
        assert!(registry.has(Protocol::ModbusTcp));

        let info = manager.plugin_info();
        assert_eq!(info[0].name, "test-plugin");
    }

    #[test]
    fn test_duplicate_factory_registration() {
        let registry = FactoryRegistry::new();

        registry.register(MockFactory).unwrap();
        let result = registry.register(MockFactory);

        assert!(result.is_err());
    }
}

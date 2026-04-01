#![cfg(feature = "experimental-namespace-api")]

use std::sync::Arc;

use parking_lot::RwLock;

use mabi_opcua::{
    AddressSpace, AddressSpaceConfig, AttributeId, BrowseDirection, DataValue, NamespaceDiagnostics,
    NamespaceManagerPlugin, NamespaceOperation, NamespaceRegistration, NamespaceRuntimeSnapshot,
    NodeId, OpcUaServer, Variant,
};

#[derive(Default)]
struct DemoNamespaceManager {
    registration: RwLock<Option<NamespaceRegistration>>,
    started: RwLock<bool>,
}

impl DemoNamespaceManager {
    fn namespace_index(&self) -> u16 {
        self.registration
            .read()
            .as_ref()
            .and_then(|registration| registration.namespace_index)
            .unwrap()
    }
}

impl NamespaceManagerPlugin for DemoNamespaceManager {
    fn kind(&self) -> &'static str {
        "demo-plugin"
    }

    fn namespace_uri(&self) -> Option<&str> {
        Some("urn:mabinogion:test:experimental-plugin")
    }

    fn on_registered(&self, registration: &NamespaceRegistration) {
        *self.registration.write() = Some(registration.clone());
    }

    fn materialize(&self, address_space: &AddressSpace, registration: &NamespaceRegistration) {
        let namespace_index = registration.namespace_index.unwrap();
        let folder = NodeId::string(namespace_index, "PluginFolder");
        let value = NodeId::string(namespace_index, "PluginValue");
        address_space
            .add_folder(
                folder.clone(),
                "PluginFolder",
                "PluginFolder",
                &NodeId::objects_folder(),
            )
            .unwrap();
        address_space
            .add_writable_variable(
                value,
                "PluginValue",
                "PluginValue",
                NodeId::numeric(0, 11),
                Variant::Double(12.5),
                &folder,
            )
            .unwrap();
    }

    fn on_runtime_start(
        &self,
        _address_space: &AddressSpace,
        _registration: &NamespaceRegistration,
        _snapshot: &NamespaceRuntimeSnapshot,
    ) {
        *self.started.write() = true;
    }

    fn diagnostics_snapshot(
        &self,
        _address_space: &AddressSpace,
        registration: &NamespaceRegistration,
    ) -> Option<NamespaceDiagnostics> {
        Some(NamespaceDiagnostics {
            summary: format!(
                "manager=demo-plugin namespace={}",
                registration.namespace_index.unwrap_or_default()
            ),
        })
    }

    fn read_attribute(
        &self,
        _address_space: &AddressSpace,
        registration: &NamespaceRegistration,
        node_id: &NodeId,
        attribute_id: AttributeId,
    ) -> NamespaceOperation<DataValue> {
        let namespace_index = registration.namespace_index.unwrap_or_default();
        if *node_id == NodeId::string(namespace_index, "Virtual.Temperature")
            && attribute_id == AttributeId::Value
        {
            return NamespaceOperation::handled(DataValue::new(Variant::Double(42.0)));
        }

        NamespaceOperation::not_handled()
    }
}

#[test]
fn experimental_namespace_manager_materializes_and_falls_back_to_canonical_runtime() {
    let manager = Arc::new(DemoNamespaceManager::default());
    let address_space = AddressSpace::new_with_namespace_managers(
        AddressSpaceConfig::default(),
        vec![manager.clone()],
    );

    let namespace_index = manager.namespace_index();
    let folder = NodeId::string(namespace_index, "PluginFolder");
    let value = NodeId::string(namespace_index, "PluginValue");

    assert!(address_space.contains_node(&folder));
    assert_eq!(
        address_space
            .read_value(&NodeId::string(namespace_index, "Virtual.Temperature"))
            .value()
            .and_then(|value| value.as_f64()),
        Some(42.0)
    );
    assert_eq!(
        address_space.read_value(&value).value().and_then(|value| value.as_f64()),
        Some(12.5)
    );

    let browse = address_space.browse(
        &NodeId::objects_folder(),
        BrowseDirection::Forward,
        None,
        false,
        None,
        100,
    );
    assert!(browse
        .references
        .iter()
        .any(|reference| reference.node_id == folder));
}

#[tokio::test]
async fn experimental_namespace_manager_registers_via_server_builder_and_updates_diagnostics() {
    let manager = Arc::new(DemoNamespaceManager::default());
    let server = mabi_opcua::OpcUaServer::builder()
        .endpoint_url("opc.tcp://127.0.0.1:0")
        .with_namespace_manager(DemoNamespaceManager::default())
        .build()
        .unwrap();

    server.start().await.unwrap();

    let namespace_index = server
        .address_space()
        .get_namespace_index("urn:mabinogion:opcua:diagnostics")
        .unwrap();
    let summary = server.address_space().read_value(&NodeId::string(
        namespace_index,
        "Diagnostics.ManagerOwnershipSummary",
    ));
    assert!(summary
        .value()
        .and_then(|value| value.as_str())
        .is_some_and(|summary| summary.contains("manager=demo-plugin")));

    server.stop().await.unwrap();

    // Separate server builder registration should not mutate this detached Arc.
    assert!(!*manager.started.read());

    let server = OpcUaServer::builder()
        .endpoint_url("opc.tcp://127.0.0.1:0")
        .with_namespace_manager(ArcDemoNamespaceManager(manager.clone()))
        .build()
        .unwrap();
    server.start().await.unwrap();
    assert!(*manager.started.read());
    server.stop().await.unwrap();
}

struct ArcDemoNamespaceManager(Arc<DemoNamespaceManager>);

impl NamespaceManagerPlugin for ArcDemoNamespaceManager {
    fn kind(&self) -> &'static str {
        self.0.kind()
    }

    fn namespace_uri(&self) -> Option<&str> {
        self.0.namespace_uri()
    }

    fn on_registered(&self, registration: &NamespaceRegistration) {
        self.0.on_registered(registration);
    }

    fn materialize(&self, address_space: &AddressSpace, registration: &NamespaceRegistration) {
        self.0.materialize(address_space, registration);
    }

    fn on_runtime_start(
        &self,
        address_space: &AddressSpace,
        registration: &NamespaceRegistration,
        snapshot: &NamespaceRuntimeSnapshot,
    ) {
        self.0.on_runtime_start(address_space, registration, snapshot);
    }

    fn diagnostics_snapshot(
        &self,
        address_space: &AddressSpace,
        registration: &NamespaceRegistration,
    ) -> Option<NamespaceDiagnostics> {
        self.0.diagnostics_snapshot(address_space, registration)
    }

    fn read_attribute(
        &self,
        address_space: &AddressSpace,
        registration: &NamespaceRegistration,
        node_id: &NodeId,
        attribute_id: AttributeId,
    ) -> NamespaceOperation<DataValue> {
        self.0
            .read_attribute(address_space, registration, node_id, attribute_id)
    }
}

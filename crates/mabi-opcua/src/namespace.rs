use std::sync::Arc;

use parking_lot::RwLock;

use crate::nodes::{
    AddressSpace, BrowseDirection, BrowsePathResult, ReferenceDescription, ReferenceTypeId,
    RelativePathElement,
};
use crate::sdk::address_space::{DiagnosticsSnapshot, ManagedOperation, NodeManager};
use crate::types::{AttributeId, DataValue, NodeId, StatusCode};

/// Public experimental outcome for namespace operation overrides.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NamespaceOperation<T> {
    Handled(T),
    NotHandled,
}

impl<T> NamespaceOperation<T> {
    pub fn handled(value: T) -> Self {
        Self::Handled(value)
    }

    pub fn not_handled() -> Self {
        Self::NotHandled
    }
}

/// Public experimental type-query contract for namespace managers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NamespaceTypeQuery {
    ReferenceSubtype {
        candidate: ReferenceTypeId,
        parent: ReferenceTypeId,
    },
    NodeSubtype {
        candidate: NodeId,
        parent: NodeId,
    },
}

/// Public experimental namespace registration metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NamespaceRegistration {
    pub namespace_index: Option<u16>,
    pub namespace_uri: Option<String>,
    pub kind: &'static str,
    pub fallback_manager: bool,
}

/// Public experimental namespace diagnostics contribution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NamespaceDiagnostics {
    pub summary: String,
}

/// Public experimental runtime diagnostics snapshot for namespace managers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NamespaceRuntimeSnapshot {
    pub current_sessions: u32,
    pub current_subscriptions: u32,
    pub total_nodes: u32,
    pub namespace_count: u32,
    pub security_profile_summary: String,
    pub durable_restore_summary: String,
    pub manager_ownership_summary: String,
}

impl NamespaceRuntimeSnapshot {
    pub(crate) fn from_diagnostics_snapshot(snapshot: &DiagnosticsSnapshot) -> Self {
        Self {
            current_sessions: snapshot.current_sessions,
            current_subscriptions: snapshot.current_subscriptions,
            total_nodes: snapshot.total_nodes,
            namespace_count: snapshot.namespace_count,
            security_profile_summary: snapshot.security_profile_summary.clone(),
            durable_restore_summary: snapshot.durable_restore_summary.clone(),
            manager_ownership_summary: snapshot.manager_ownership_summary.clone(),
        }
    }
}

/// Experimental in-process namespace manager plugin contract.
///
/// This is intentionally feature-gated and should be treated as an evolving
/// extension seam rather than a stable plugin ABI.
pub trait NamespaceManagerPlugin: Send + Sync {
    fn kind(&self) -> &'static str {
        "plugin"
    }

    fn namespace_uri(&self) -> Option<&str> {
        None
    }

    fn is_fallback_manager(&self) -> bool {
        false
    }

    fn on_registered(&self, _registration: &NamespaceRegistration) {}

    fn materialize(&self, _address_space: &AddressSpace, _registration: &NamespaceRegistration) {}

    fn on_runtime_start(
        &self,
        _address_space: &AddressSpace,
        _registration: &NamespaceRegistration,
        _snapshot: &NamespaceRuntimeSnapshot,
    ) {
    }

    fn on_runtime_stop(
        &self,
        _address_space: &AddressSpace,
        _registration: &NamespaceRegistration,
        _snapshot: &NamespaceRuntimeSnapshot,
    ) {
    }

    fn diagnostics_snapshot(
        &self,
        _address_space: &AddressSpace,
        _registration: &NamespaceRegistration,
    ) -> Option<NamespaceDiagnostics> {
        None
    }

    fn read_attribute(
        &self,
        _address_space: &AddressSpace,
        _registration: &NamespaceRegistration,
        _node_id: &NodeId,
        _attribute_id: AttributeId,
    ) -> NamespaceOperation<DataValue> {
        NamespaceOperation::NotHandled
    }

    fn write_attribute(
        &self,
        _address_space: &AddressSpace,
        _registration: &NamespaceRegistration,
        _node_id: &NodeId,
        _attribute_id: AttributeId,
        _value: &DataValue,
    ) -> NamespaceOperation<StatusCode> {
        NamespaceOperation::NotHandled
    }

    fn browse(
        &self,
        _address_space: &AddressSpace,
        _registration: &NamespaceRegistration,
        _node_id: &NodeId,
        _direction: BrowseDirection,
        _reference_type_filter: Option<ReferenceTypeId>,
        _include_subtypes: bool,
        _node_class_mask: Option<u32>,
    ) -> NamespaceOperation<Vec<ReferenceDescription>> {
        NamespaceOperation::NotHandled
    }

    fn resolve_browse_path(
        &self,
        _address_space: &AddressSpace,
        _registration: &NamespaceRegistration,
        _starting_node: &NodeId,
        _elements: &[RelativePathElement],
    ) -> NamespaceOperation<BrowsePathResult> {
        NamespaceOperation::NotHandled
    }

    fn type_query(
        &self,
        _address_space: &AddressSpace,
        _registration: &NamespaceRegistration,
        _query: &NamespaceTypeQuery,
    ) -> NamespaceOperation<bool> {
        NamespaceOperation::NotHandled
    }
}

pub(crate) fn adapt_namespace_manager_plugin(
    plugin: Arc<dyn NamespaceManagerPlugin>,
) -> Arc<dyn NodeManager> {
    Arc::new(NamespaceManagerAdapter::new(plugin))
}

struct NamespaceManagerAdapter {
    plugin: Arc<dyn NamespaceManagerPlugin>,
    registration: RwLock<NamespaceRegistration>,
}

impl NamespaceManagerAdapter {
    fn new(plugin: Arc<dyn NamespaceManagerPlugin>) -> Self {
        Self {
            registration: RwLock::new(NamespaceRegistration {
                namespace_index: None,
                namespace_uri: plugin.namespace_uri().map(ToString::to_string),
                kind: plugin.kind(),
                fallback_manager: plugin.is_fallback_manager(),
            }),
            plugin,
        }
    }

    fn registration(&self) -> NamespaceRegistration {
        self.registration.read().clone()
    }
}

impl NodeManager for NamespaceManagerAdapter {
    fn kind(&self) -> &'static str {
        self.plugin.kind()
    }

    fn namespace_uri(&self) -> Option<&str> {
        self.plugin.namespace_uri()
    }

    fn is_fallback_manager(&self) -> bool {
        self.plugin.is_fallback_manager()
    }

    fn on_registered(&self, namespace_index: Option<u16>, namespace_uri: Option<&str>) {
        let registration = NamespaceRegistration {
            namespace_index,
            namespace_uri: namespace_uri.map(ToString::to_string),
            kind: self.plugin.kind(),
            fallback_manager: self.plugin.is_fallback_manager(),
        };
        *self.registration.write() = registration.clone();
        self.plugin.on_registered(&registration);
    }

    fn owns_namespace(&self, namespace_index: u16) -> bool {
        let registration = self.registration();
        registration
            .namespace_index
            .is_some_and(|index| index == namespace_index)
            || (registration.fallback_manager && registration.namespace_index.is_none())
    }

    fn materialize(&self, address_space: &AddressSpace, _namespace_index: Option<u16>) {
        self.plugin.materialize(address_space, &self.registration());
    }

    fn on_runtime_start(
        &self,
        address_space: &AddressSpace,
        _namespace_index: Option<u16>,
        snapshot: &DiagnosticsSnapshot,
    ) {
        self.plugin.on_runtime_start(
            address_space,
            &self.registration(),
            &NamespaceRuntimeSnapshot::from_diagnostics_snapshot(snapshot),
        );
    }

    fn on_runtime_stop(
        &self,
        address_space: &AddressSpace,
        _namespace_index: Option<u16>,
        snapshot: &DiagnosticsSnapshot,
    ) {
        self.plugin.on_runtime_stop(
            address_space,
            &self.registration(),
            &NamespaceRuntimeSnapshot::from_diagnostics_snapshot(snapshot),
        );
    }

    fn diagnostics_snapshot(
        &self,
        address_space: &AddressSpace,
        _namespace_index: Option<u16>,
    ) -> Option<String> {
        self.plugin
            .diagnostics_snapshot(address_space, &self.registration())
            .map(|diagnostics| diagnostics.summary)
    }

    fn read_attribute(
        &self,
        address_space: &AddressSpace,
        node_id: &NodeId,
        attribute_id: AttributeId,
    ) -> ManagedOperation<DataValue> {
        self.plugin
            .read_attribute(address_space, &self.registration(), node_id, attribute_id)
            .into()
    }

    fn write_attribute(
        &self,
        address_space: &AddressSpace,
        node_id: &NodeId,
        attribute_id: AttributeId,
        value: &DataValue,
    ) -> ManagedOperation<StatusCode> {
        self.plugin
            .write_attribute(
                address_space,
                &self.registration(),
                node_id,
                attribute_id,
                value,
            )
            .into()
    }

    fn browse(
        &self,
        address_space: &AddressSpace,
        node_id: &NodeId,
        direction: BrowseDirection,
        reference_type_filter: Option<ReferenceTypeId>,
        include_subtypes: bool,
        node_class_mask: Option<u32>,
    ) -> ManagedOperation<Vec<ReferenceDescription>> {
        self.plugin
            .browse(
                address_space,
                &self.registration(),
                node_id,
                direction,
                reference_type_filter,
                include_subtypes,
                node_class_mask,
            )
            .into()
    }

    fn resolve_browse_path(
        &self,
        address_space: &AddressSpace,
        starting_node: &NodeId,
        elements: &[RelativePathElement],
    ) -> ManagedOperation<BrowsePathResult> {
        self.plugin
            .resolve_browse_path(address_space, &self.registration(), starting_node, elements)
            .into()
    }

    fn is_reference_subtype(
        &self,
        address_space: &AddressSpace,
        candidate: &ReferenceTypeId,
        parent: &ReferenceTypeId,
    ) -> ManagedOperation<bool> {
        self.plugin
            .type_query(
                address_space,
                &self.registration(),
                &NamespaceTypeQuery::ReferenceSubtype {
                    candidate: *candidate,
                    parent: *parent,
                },
            )
            .into()
    }

    fn is_node_subtype_of(
        &self,
        address_space: &AddressSpace,
        candidate: &NodeId,
        parent: &NodeId,
    ) -> ManagedOperation<bool> {
        self.plugin
            .type_query(
                address_space,
                &self.registration(),
                &NamespaceTypeQuery::NodeSubtype {
                    candidate: candidate.clone(),
                    parent: parent.clone(),
                },
            )
            .into()
    }
}

impl<T> From<NamespaceOperation<T>> for ManagedOperation<T> {
    fn from(value: NamespaceOperation<T>) -> Self {
        match value {
            NamespaceOperation::Handled(value) => ManagedOperation::Handled(value),
            NamespaceOperation::NotHandled => ManagedOperation::NotHandled,
        }
    }
}

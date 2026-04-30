use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use chrono::{DateTime, Utc};
use dashmap::DashMap;
use parking_lot::RwLock;

use crate::nodes::base::{shared_node, Node, NodeClass, SharedNode};
use crate::nodes::reference::{
    BrowseDirection, BrowseResult, Reference, ReferenceDescription, ReferenceDirection,
    ReferenceTypeId,
};
use crate::nodes::store::{BrowsePathResult, BrowsePathTarget, RelativePathElement};
use crate::nodes::AddressSpace;
use crate::types::{AttributeId, DataValue, NodeId, StatusCode};

pub(crate) enum ManagedOperation<T> {
    Handled(T),
    NotHandled,
}

#[allow(dead_code)]
pub(crate) trait AttributeAccessPort: Send + Sync {
    fn read_attribute(&self, node_id: &NodeId, attribute_id: AttributeId) -> DataValue;
    fn write_attribute(
        &self,
        node_id: &NodeId,
        attribute_id: AttributeId,
        value: DataValue,
    ) -> StatusCode;
}

#[allow(dead_code)]
pub(crate) trait BrowsePort: Send + Sync {
    fn get_references(&self, node_id: &NodeId, direction: BrowseDirection) -> Vec<Reference>;
    fn browse(
        &self,
        node_id: &NodeId,
        direction: BrowseDirection,
        reference_type_filter: Option<ReferenceTypeId>,
        include_subtypes: bool,
        node_class_mask: Option<u32>,
        max_results: usize,
    ) -> BrowseResult;
    fn browse_next(
        &self,
        continuation_point: &[u8],
        release: bool,
        max_results: usize,
    ) -> BrowseResult;
    fn release_continuation_point(&self, continuation_point: &[u8]);
}

#[allow(dead_code)]
pub(crate) trait BrowsePathPort: Send + Sync {
    fn resolve_browse_path(
        &self,
        starting_node: &NodeId,
        elements: &[RelativePathElement],
    ) -> BrowsePathResult;
}

#[allow(dead_code)]
pub(crate) trait TypeHierarchyPort: Send + Sync {
    fn is_reference_subtype(&self, candidate: &ReferenceTypeId, parent: &ReferenceTypeId) -> bool;
    fn is_node_subtype_of(&self, candidate: &NodeId, parent: &NodeId) -> bool;
}

pub(crate) trait NodeManager: Send + Sync {
    fn kind(&self) -> &'static str {
        "default"
    }
    fn namespace_uri(&self) -> Option<&str> {
        None
    }
    fn is_fallback_manager(&self) -> bool {
        false
    }
    fn on_registered(&self, _namespace_index: Option<u16>, _namespace_uri: Option<&str>) {}
    fn owns_namespace(&self, namespace_index: u16) -> bool;
    fn materialize(&self, _address_space: &AddressSpace, _namespace_index: Option<u16>) {}
    fn on_runtime_start(
        &self,
        _address_space: &AddressSpace,
        _namespace_index: Option<u16>,
        _snapshot: &DiagnosticsSnapshot,
    ) {
    }
    fn on_runtime_stop(
        &self,
        _address_space: &AddressSpace,
        _namespace_index: Option<u16>,
        _snapshot: &DiagnosticsSnapshot,
    ) {
    }
    fn diagnostics_snapshot(
        &self,
        _address_space: &AddressSpace,
        _namespace_index: Option<u16>,
    ) -> Option<String> {
        None
    }
    fn read_attribute(
        &self,
        _address_space: &AddressSpace,
        _node_id: &NodeId,
        _attribute_id: AttributeId,
    ) -> ManagedOperation<DataValue> {
        ManagedOperation::NotHandled
    }
    fn write_attribute(
        &self,
        _address_space: &AddressSpace,
        _node_id: &NodeId,
        _attribute_id: AttributeId,
        _value: &DataValue,
    ) -> ManagedOperation<StatusCode> {
        ManagedOperation::NotHandled
    }
    fn browse(
        &self,
        _address_space: &AddressSpace,
        _node_id: &NodeId,
        _direction: BrowseDirection,
        _reference_type_filter: Option<ReferenceTypeId>,
        _include_subtypes: bool,
        _node_class_mask: Option<u32>,
    ) -> ManagedOperation<Vec<ReferenceDescription>> {
        ManagedOperation::NotHandled
    }
    fn resolve_browse_path(
        &self,
        _address_space: &AddressSpace,
        _starting_node: &NodeId,
        _elements: &[RelativePathElement],
    ) -> ManagedOperation<BrowsePathResult> {
        ManagedOperation::NotHandled
    }
    fn is_reference_subtype(
        &self,
        _address_space: &AddressSpace,
        _candidate: &ReferenceTypeId,
        _parent: &ReferenceTypeId,
    ) -> ManagedOperation<bool> {
        ManagedOperation::NotHandled
    }
    fn is_node_subtype_of(
        &self,
        _address_space: &AddressSpace,
        _candidate: &NodeId,
        _parent: &NodeId,
    ) -> ManagedOperation<bool> {
        ManagedOperation::NotHandled
    }
}

#[derive(Default)]
pub(crate) struct DefaultNodeManager;

impl NodeManager for DefaultNodeManager {
    fn kind(&self) -> &'static str {
        "default"
    }

    fn is_fallback_manager(&self) -> bool {
        true
    }

    fn owns_namespace(&self, _namespace_index: u16) -> bool {
        true
    }

    fn diagnostics_snapshot(
        &self,
        _address_space: &AddressSpace,
        namespace_index: Option<u16>,
    ) -> Option<String> {
        Some(format!(
            "manager=default namespace={}",
            namespace_index
                .map(|index| index.to_string())
                .unwrap_or_else(|| "dynamic".to_string())
        ))
    }
}

#[derive(Default)]
pub(crate) struct DiagnosticsNodeManager {
    assigned_namespace_index: RwLock<Option<u16>>,
}

impl DiagnosticsNodeManager {
    pub(crate) const NAMESPACE_URI: &'static str = "urn:mabinogion:opcua:diagnostics";

    fn root_node_id(namespace_index: u16) -> NodeId {
        NodeId::string(namespace_index, "Diagnostics")
    }

    fn metric_node_id(namespace_index: u16, name: &str) -> NodeId {
        NodeId::string(namespace_index, format!("Diagnostics.{}", name))
    }
}

pub(crate) struct CatalogNodeManager {
    namespace_uri: String,
    assigned_namespace_index: RwLock<Option<u16>>,
}

impl CatalogNodeManager {
    pub(crate) fn new(namespace_uri: impl Into<String>) -> Self {
        Self {
            namespace_uri: namespace_uri.into(),
            assigned_namespace_index: RwLock::new(None),
        }
    }
}

impl NodeManager for CatalogNodeManager {
    fn kind(&self) -> &'static str {
        "catalog"
    }

    fn namespace_uri(&self) -> Option<&str> {
        Some(&self.namespace_uri)
    }

    fn on_registered(&self, namespace_index: Option<u16>, _namespace_uri: Option<&str>) {
        *self.assigned_namespace_index.write() = namespace_index;
    }

    fn owns_namespace(&self, namespace_index: u16) -> bool {
        self.assigned_namespace_index
            .read()
            .is_some_and(|index| index == namespace_index)
    }

    fn diagnostics_snapshot(
        &self,
        _address_space: &AddressSpace,
        namespace_index: Option<u16>,
    ) -> Option<String> {
        Some(format!(
            "manager=catalog namespace={} uri={}",
            namespace_index
                .map(|index| index.to_string())
                .unwrap_or_else(|| "unassigned".to_string()),
            self.namespace_uri
        ))
    }
}

#[derive(Debug, Clone)]
pub(crate) struct DiagnosticsSnapshot {
    pub(crate) current_sessions: u32,
    pub(crate) current_subscriptions: u32,
    pub(crate) total_nodes: u32,
    pub(crate) namespace_count: u32,
    pub(crate) security_profile_summary: String,
    pub(crate) durable_restore_summary: String,
    pub(crate) manager_ownership_summary: String,
}

impl NodeManager for DiagnosticsNodeManager {
    fn kind(&self) -> &'static str {
        "diagnostics"
    }

    fn namespace_uri(&self) -> Option<&str> {
        Some(Self::NAMESPACE_URI)
    }

    fn on_registered(&self, namespace_index: Option<u16>, _namespace_uri: Option<&str>) {
        *self.assigned_namespace_index.write() = namespace_index;
    }

    fn owns_namespace(&self, namespace_index: u16) -> bool {
        self.assigned_namespace_index
            .read()
            .is_some_and(|index| index == namespace_index)
    }

    fn materialize(&self, address_space: &AddressSpace, namespace_index: Option<u16>) {
        let Some(namespace_index) = namespace_index else {
            return;
        };

        let diagnostics_root = Self::root_node_id(namespace_index);
        let _ = address_space.add_folder(
            diagnostics_root.clone(),
            "Diagnostics",
            "Diagnostics",
            &NodeId::server(),
        );

        let diagnostics = [
            ("CurrentSessionCount", 0u32),
            ("CurrentSubscriptionCount", 0u32),
            ("TotalNodes", 0u32),
            ("NamespaceCount", 0u32),
        ];

        for (name, value) in diagnostics {
            let _ = address_space.add_writable_variable(
                Self::metric_node_id(namespace_index, name),
                name,
                name,
                NodeId::numeric(0, 7),
                crate::types::Variant::UInt32(value),
                &diagnostics_root,
            );
        }

        for name in ["SecurityProfileSummary", "DurableRestoreSummary"] {
            let _ = address_space.add_writable_variable(
                Self::metric_node_id(namespace_index, name),
                name,
                name,
                NodeId::numeric(0, 12),
                crate::types::Variant::String(String::new()),
                &diagnostics_root,
            );
        }

        let _ = address_space.add_writable_variable(
            Self::metric_node_id(namespace_index, "ManagerOwnershipSummary"),
            "ManagerOwnershipSummary",
            "ManagerOwnershipSummary",
            NodeId::numeric(0, 12),
            crate::types::Variant::String(String::new()),
            &diagnostics_root,
        );
    }

    fn diagnostics_snapshot(
        &self,
        _address_space: &AddressSpace,
        namespace_index: Option<u16>,
    ) -> Option<String> {
        Some(format!(
            "manager=diagnostics namespace={}",
            namespace_index
                .map(|index| index.to_string())
                .unwrap_or_else(|| "unassigned".to_string())
        ))
    }
}

#[derive(Debug)]
struct StoredBrowseContinuation {
    remaining_references: Vec<ReferenceDescription>,
    created_at: DateTime<Utc>,
}

pub(crate) struct NamespaceManager {
    namespaces: RwLock<Vec<String>>,
}

impl NamespaceManager {
    pub(crate) fn new(namespaces: Vec<String>) -> Self {
        Self {
            namespaces: RwLock::new(namespaces),
        }
    }

    pub(crate) fn register_namespace(&self, uri: &str) -> u16 {
        let mut namespaces = self.namespaces.write();
        if let Some(index) = namespaces.iter().position(|existing| existing == uri) {
            return index as u16;
        }

        let index = namespaces.len() as u16;
        namespaces.push(uri.to_string());
        index
    }

    pub(crate) fn get_namespace_uri(&self, index: u16) -> Option<String> {
        self.namespaces.read().get(index as usize).cloned()
    }

    pub(crate) fn get_namespace_index(&self, uri: &str) -> Option<u16> {
        self.namespaces
            .read()
            .iter()
            .position(|existing| existing == uri)
            .map(|index| index as u16)
    }

    pub(crate) fn namespaces(&self) -> Vec<String> {
        self.namespaces.read().clone()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NamespaceLifecycleState {
    Registered,
    Running,
    Stopped,
}

#[derive(Debug, Clone)]
pub(crate) struct NamespaceDiagnosticsState {
    pub(crate) lifecycle: NamespaceLifecycleState,
    pub(crate) manager_kind: &'static str,
    pub(crate) namespace_index: Option<u16>,
    pub(crate) namespace_uri: Option<String>,
    pub(crate) fallback_manager: bool,
    pub(crate) last_summary: Option<String>,
}

pub(crate) struct NamespaceRuntime {
    namespace_index: Option<u16>,
    manager: Arc<dyn NodeManager>,
    state: RwLock<NamespaceDiagnosticsState>,
}

impl NamespaceRuntime {
    fn new(
        namespace_index: Option<u16>,
        namespace_uri: Option<String>,
        manager: Arc<dyn NodeManager>,
    ) -> Self {
        Self {
            namespace_index,
            state: RwLock::new(NamespaceDiagnosticsState {
                lifecycle: NamespaceLifecycleState::Registered,
                manager_kind: manager.kind(),
                namespace_index,
                namespace_uri,
                fallback_manager: manager.is_fallback_manager(),
                last_summary: None,
            }),
            manager,
        }
    }

    fn on_runtime_start(&self, address_space: &AddressSpace, snapshot: &DiagnosticsSnapshot) {
        self.manager
            .on_runtime_start(address_space, self.namespace_index, snapshot);
        let mut state = self.state.write();
        state.lifecycle = NamespaceLifecycleState::Running;
        state.last_summary = self
            .manager
            .diagnostics_snapshot(address_space, self.namespace_index);
    }

    fn on_runtime_stop(&self, address_space: &AddressSpace, snapshot: &DiagnosticsSnapshot) {
        self.manager
            .on_runtime_stop(address_space, self.namespace_index, snapshot);
        let mut state = self.state.write();
        state.lifecycle = NamespaceLifecycleState::Stopped;
        state.last_summary = self
            .manager
            .diagnostics_snapshot(address_space, self.namespace_index);
    }

    fn diagnostics_state(&self) -> NamespaceDiagnosticsState {
        self.state.read().clone()
    }
}

pub(crate) struct NamespaceRegistry {
    namespaces: NamespaceManager,
    runtimes: RwLock<Vec<NamespaceRuntime>>,
}

impl NamespaceRegistry {
    pub(crate) fn new(namespaces: Vec<String>, managers: Vec<Arc<dyn NodeManager>>) -> Self {
        let registry = Self {
            namespaces: NamespaceManager::new(namespaces),
            runtimes: RwLock::new(Vec::new()),
        };
        registry.register_manager(Arc::new(DefaultNodeManager));
        registry.register_manager(Arc::new(DiagnosticsNodeManager::default()));
        for manager in managers {
            registry.register_manager(manager);
        }
        registry
    }

    pub(crate) fn register_manager(&self, manager: Arc<dyn NodeManager>) {
        let namespace_index = manager
            .namespace_uri()
            .map(|uri| self.namespaces.register_namespace(uri));
        let namespace_uri = namespace_index
            .and_then(|index| self.namespaces.get_namespace_uri(index))
            .or_else(|| manager.namespace_uri().map(ToString::to_string));
        manager.on_registered(namespace_index, namespace_uri.as_deref());
        self.runtimes.write().push(NamespaceRuntime::new(
            namespace_index,
            namespace_uri,
            manager,
        ));
    }

    pub(crate) fn register_namespace(&self, uri: &str) -> u16 {
        self.namespaces.register_namespace(uri)
    }

    pub(crate) fn get_namespace_uri(&self, index: u16) -> Option<String> {
        self.namespaces.get_namespace_uri(index)
    }

    pub(crate) fn get_namespace_index(&self, uri: &str) -> Option<u16> {
        self.namespaces.get_namespace_index(uri)
    }

    pub(crate) fn namespaces(&self) -> Vec<String> {
        self.namespaces.namespaces()
    }

    pub(crate) fn owns_namespace(&self, namespace_index: u16) -> bool {
        self.resolve_manager(namespace_index).is_some()
    }

    pub(crate) fn ownership_summary(&self) -> Vec<String> {
        self.namespaces
            .namespaces()
            .into_iter()
            .enumerate()
            .filter_map(|(index, uri)| {
                let index = index as u16;
                self.resolve_manager(index)
                    .map(|state| format!("ns={} uri={} manager={}", index, uri, state.manager_kind))
            })
            .collect()
    }

    fn resolve_manager_handle(&self, namespace_index: u16) -> Option<Arc<dyn NodeManager>> {
        let runtimes = self.runtimes.read();
        if let Some(runtime) = runtimes.iter().find(|runtime| {
            !runtime.manager.is_fallback_manager()
                && runtime.manager.owns_namespace(namespace_index)
        }) {
            return Some(runtime.manager.clone());
        }

        runtimes
            .iter()
            .find(|runtime| {
                runtime.manager.is_fallback_manager()
                    && runtime.manager.owns_namespace(namespace_index)
            })
            .map(|runtime| runtime.manager.clone())
    }

    fn managers(&self) -> Vec<Arc<dyn NodeManager>> {
        self.runtimes
            .read()
            .iter()
            .map(|runtime| runtime.manager.clone())
            .collect()
    }

    fn resolve_manager(&self, namespace_index: u16) -> Option<NamespaceDiagnosticsState> {
        let runtimes = self.runtimes.read();
        if let Some(runtime) = runtimes.iter().find(|runtime| {
            !runtime.manager.is_fallback_manager()
                && runtime.manager.owns_namespace(namespace_index)
        }) {
            return Some(runtime.diagnostics_state());
        }

        runtimes
            .iter()
            .find(|runtime| {
                runtime.manager.is_fallback_manager()
                    && runtime.manager.owns_namespace(namespace_index)
            })
            .map(NamespaceRuntime::diagnostics_state)
    }

    pub(crate) fn materialize_managers(&self, address_space: &AddressSpace) {
        for runtime in self.runtimes.read().iter() {
            runtime
                .manager
                .materialize(address_space, runtime.namespace_index);
        }
    }

    pub(crate) fn on_runtime_start(
        &self,
        address_space: &AddressSpace,
        snapshot: &DiagnosticsSnapshot,
    ) {
        for runtime in self.runtimes.read().iter() {
            runtime.on_runtime_start(address_space, snapshot);
        }
    }

    pub(crate) fn on_runtime_stop(
        &self,
        address_space: &AddressSpace,
        snapshot: &DiagnosticsSnapshot,
    ) {
        for runtime in self.runtimes.read().iter() {
            runtime.on_runtime_stop(address_space, snapshot);
        }
    }

    pub(crate) fn diagnostics_state(&self) -> Vec<NamespaceDiagnosticsState> {
        self.runtimes
            .read()
            .iter()
            .map(NamespaceRuntime::diagnostics_state)
            .collect()
    }
}

pub(crate) struct NodeStore {
    nodes: DashMap<NodeId, SharedNode>,
    max_nodes: usize,
}

impl NodeStore {
    pub(crate) fn new(max_nodes: usize) -> Self {
        Self {
            nodes: DashMap::new(),
            max_nodes,
        }
    }

    pub(crate) fn insert_node<N: Node + 'static>(&self, node: N) -> bool {
        if self.nodes.len() >= self.max_nodes {
            return false;
        }

        let node_id = node.node_id().clone();
        if self.nodes.contains_key(&node_id) {
            return false;
        }

        self.nodes.insert(node_id, shared_node(node));
        true
    }

    pub(crate) fn insert_boxed_node(&self, node: Box<dyn Node>) -> bool {
        if self.nodes.len() >= self.max_nodes {
            return false;
        }

        let node_id = node.node_id().clone();
        if self.nodes.contains_key(&node_id) {
            return false;
        }

        self.nodes
            .insert(node_id, Arc::new(parking_lot::RwLock::new(node)));
        true
    }

    pub(crate) fn get(&self, node_id: &NodeId) -> Option<SharedNode> {
        self.nodes.get(node_id).map(|node| node.clone())
    }

    pub(crate) fn contains(&self, node_id: &NodeId) -> bool {
        self.nodes.contains_key(node_id)
    }

    pub(crate) fn remove(&self, node_id: &NodeId) -> bool {
        self.nodes.remove(node_id).is_some()
    }

    pub(crate) fn len(&self) -> usize {
        self.nodes.len()
    }

    pub(crate) fn node_ids(&self) -> Vec<NodeId> {
        self.nodes.iter().map(|entry| entry.key().clone()).collect()
    }

    pub(crate) fn counts_by_class(&self) -> (u64, u64, u64) {
        let mut variable_nodes = 0;
        let mut object_nodes = 0;
        let mut method_nodes = 0;

        for entry in self.nodes.iter() {
            match entry.value().read().node_class() {
                NodeClass::Variable => variable_nodes += 1,
                NodeClass::Object => object_nodes += 1,
                NodeClass::Method => method_nodes += 1,
                _ => {}
            }
        }

        (variable_nodes, object_nodes, method_nodes)
    }
}

pub(crate) struct ReferenceIndex {
    forward_references: DashMap<NodeId, Vec<Reference>>,
    inverse_references: DashMap<NodeId, Vec<Reference>>,
}

impl ReferenceIndex {
    pub(crate) fn new() -> Self {
        Self {
            forward_references: DashMap::new(),
            inverse_references: DashMap::new(),
        }
    }

    pub(crate) fn add_reference(&self, reference: Reference) {
        self.forward_references
            .entry(reference.source_node_id.clone())
            .or_insert_with(Vec::new)
            .push(reference.clone());

        let inverse = reference.inverse_ref();
        self.inverse_references
            .entry(inverse.source_node_id.clone())
            .or_insert_with(Vec::new)
            .push(inverse);
    }

    pub(crate) fn remove_reference(
        &self,
        source: &NodeId,
        reference_type: ReferenceTypeId,
        target: &NodeId,
    ) -> bool {
        let mut removed = false;

        if let Some(mut refs) = self.forward_references.get_mut(source) {
            refs.retain(|reference| {
                let matches = reference.reference_type_id == reference_type
                    && &reference.target_node_id == target;
                if matches {
                    removed = true;
                }
                !matches
            });
        }

        if let Some(mut refs) = self.inverse_references.get_mut(target) {
            refs.retain(|reference| {
                !(reference.reference_type_id == reference_type
                    && &reference.target_node_id == source)
            });
        }

        removed
    }

    pub(crate) fn get_references(
        &self,
        node_id: &NodeId,
        direction: BrowseDirection,
    ) -> Vec<Reference> {
        let mut references = Vec::new();

        if matches!(direction, BrowseDirection::Forward | BrowseDirection::Both) {
            if let Some(forward) = self.forward_references.get(node_id) {
                references.extend(forward.iter().cloned());
            }
        }

        if matches!(direction, BrowseDirection::Inverse | BrowseDirection::Both) {
            if let Some(inverse) = self.inverse_references.get(node_id) {
                references.extend(inverse.iter().cloned());
            }
        }

        references
    }

    pub(crate) fn remove_node(&self, node_id: &NodeId) {
        self.forward_references.remove(node_id);
        self.inverse_references.remove(node_id);
    }

    pub(crate) fn total_references(&self) -> u64 {
        self.forward_references
            .iter()
            .map(|entry| entry.len() as u64)
            .sum()
    }
}

pub(crate) struct ContinuationStore {
    browse_continuations: RwLock<HashMap<u64, StoredBrowseContinuation>>,
    next_browse_continuation_id: AtomicU64,
}

impl ContinuationStore {
    pub(crate) fn new() -> Self {
        Self {
            browse_continuations: RwLock::new(HashMap::new()),
            next_browse_continuation_id: AtomicU64::new(1),
        }
    }

    pub(crate) fn create(&self, remaining_references: Vec<ReferenceDescription>) -> Vec<u8> {
        let continuation_id = self
            .next_browse_continuation_id
            .fetch_add(1, Ordering::Relaxed);
        let continuation = StoredBrowseContinuation {
            remaining_references,
            created_at: Utc::now(),
        };
        self.browse_continuations
            .write()
            .insert(continuation_id, continuation);
        continuation_id.to_le_bytes().to_vec()
    }

    pub(crate) fn browse_next(
        &self,
        continuation_point: &[u8],
        release: bool,
        max_results: usize,
    ) -> BrowseResult {
        if continuation_point.len() != 8 {
            return BrowseResult::default();
        }

        let continuation_id = u64::from_le_bytes([
            continuation_point[0],
            continuation_point[1],
            continuation_point[2],
            continuation_point[3],
            continuation_point[4],
            continuation_point[5],
            continuation_point[6],
            continuation_point[7],
        ]);

        if release {
            self.browse_continuations.write().remove(&continuation_id);
            return BrowseResult::new(Vec::new());
        }

        let continuation = self.browse_continuations.write().remove(&continuation_id);
        let Some(mut continuation) = continuation else {
            return BrowseResult::default();
        };

        if Utc::now()
            .signed_duration_since(continuation.created_at)
            .num_seconds()
            > 300
        {
            return BrowseResult::default();
        }

        let max = if max_results == 0 { 1000 } else { max_results };
        if continuation.remaining_references.len() > max {
            let returned: Vec<_> = continuation.remaining_references.drain(..max).collect();
            let continuation_point = self.create(continuation.remaining_references);
            BrowseResult::with_continuation(returned, continuation_point)
        } else {
            BrowseResult::new(continuation.remaining_references)
        }
    }

    pub(crate) fn release(&self, continuation_point: &[u8]) {
        if continuation_point.len() != 8 {
            return;
        }

        let continuation_id = u64::from_le_bytes([
            continuation_point[0],
            continuation_point[1],
            continuation_point[2],
            continuation_point[3],
            continuation_point[4],
            continuation_point[5],
            continuation_point[6],
            continuation_point[7],
        ]);
        self.browse_continuations.write().remove(&continuation_id);
    }
}

pub(crate) struct TypeTree {
    reference_index: Arc<ReferenceIndex>,
}

impl TypeTree {
    pub(crate) fn new(reference_index: Arc<ReferenceIndex>) -> Self {
        Self { reference_index }
    }

    pub(crate) fn is_reference_subtype(
        &self,
        candidate: &ReferenceTypeId,
        parent: &ReferenceTypeId,
    ) -> bool {
        if *parent == ReferenceTypeId::HierarchicalReferences {
            return candidate.is_hierarchical();
        }
        if *parent == ReferenceTypeId::HasChild {
            return matches!(
                candidate,
                ReferenceTypeId::HasComponent
                    | ReferenceTypeId::HasProperty
                    | ReferenceTypeId::HasSubtype
                    | ReferenceTypeId::HasOrderedComponent
                    | ReferenceTypeId::Aggregates
            );
        }
        if *parent == ReferenceTypeId::NonHierarchicalReferences {
            return matches!(
                candidate,
                ReferenceTypeId::HasTypeDefinition
                    | ReferenceTypeId::HasEncoding
                    | ReferenceTypeId::HasDescription
                    | ReferenceTypeId::HasModellingRule
                    | ReferenceTypeId::GeneratesEvent
                    | ReferenceTypeId::AlwaysGeneratesEvent
            );
        }

        false
    }

    pub(crate) fn is_node_subtype_of(&self, candidate: &NodeId, parent: &NodeId) -> bool {
        if candidate == parent {
            return true;
        }

        let mut current = candidate.clone();
        let mut depth = 0;
        const MAX_DEPTH: usize = 50;

        while depth < MAX_DEPTH {
            let references = self
                .reference_index
                .get_references(&current, BrowseDirection::Inverse);
            let next_parent = references.iter().find_map(|reference| {
                if reference.reference_type_id == ReferenceTypeId::HasSubtype {
                    Some(reference.target_node_id.clone())
                } else {
                    None
                }
            });

            match next_parent {
                Some(parent_id) => {
                    if parent_id == *parent {
                        return true;
                    }
                    current = parent_id;
                    depth += 1;
                }
                None => break,
            }
        }

        false
    }
}

pub(crate) struct AddressSpaceRuntime {
    namespace_registry: NamespaceRegistry,
    node_store: NodeStore,
    reference_index: Arc<ReferenceIndex>,
    continuation_store: ContinuationStore,
    type_tree: TypeTree,
}

impl AddressSpaceRuntime {
    pub(crate) fn new(default_namespace_uri: String, max_nodes: usize) -> Self {
        Self::with_managers(default_namespace_uri, max_nodes, Vec::new())
    }

    pub(crate) fn with_managers(
        default_namespace_uri: String,
        max_nodes: usize,
        managers: Vec<Arc<dyn NodeManager>>,
    ) -> Self {
        let reference_index = Arc::new(ReferenceIndex::new());
        Self {
            namespace_registry: NamespaceRegistry::new(
                vec![
                    "http://opcfoundation.org/UA/".to_string(),
                    default_namespace_uri,
                ],
                managers,
            ),
            node_store: NodeStore::new(max_nodes),
            reference_index: reference_index.clone(),
            continuation_store: ContinuationStore::new(),
            type_tree: TypeTree::new(reference_index),
        }
    }

    pub(crate) fn register_namespace(&self, uri: &str) -> u16 {
        self.namespace_registry.register_namespace(uri)
    }

    pub(crate) fn get_namespace_uri(&self, index: u16) -> Option<String> {
        self.namespace_registry.get_namespace_uri(index)
    }

    pub(crate) fn get_namespace_index(&self, uri: &str) -> Option<u16> {
        self.namespace_registry.get_namespace_index(uri)
    }

    pub(crate) fn namespaces(&self) -> Vec<String> {
        self.namespace_registry.namespaces()
    }

    pub(crate) fn owns_namespace(&self, namespace_index: u16) -> bool {
        self.namespace_registry.owns_namespace(namespace_index)
    }

    pub(crate) fn materialize_managers(&self, address_space: &AddressSpace) {
        self.namespace_registry.materialize_managers(address_space);
    }

    pub(crate) fn on_runtime_start(
        &self,
        address_space: &AddressSpace,
        snapshot: &DiagnosticsSnapshot,
    ) {
        self.namespace_registry
            .on_runtime_start(address_space, snapshot);
    }

    pub(crate) fn on_runtime_stop(
        &self,
        address_space: &AddressSpace,
        snapshot: &DiagnosticsSnapshot,
    ) {
        self.namespace_registry
            .on_runtime_stop(address_space, snapshot);
    }

    pub(crate) fn diagnostics_namespace_index(&self) -> Option<u16> {
        self.namespace_registry
            .get_namespace_index(DiagnosticsNodeManager::NAMESPACE_URI)
    }

    pub(crate) fn manager_ownership_summary(&self) -> Vec<String> {
        self.namespace_registry.ownership_summary()
    }

    fn manager_for_namespace(&self, namespace_index: u16) -> Option<Arc<dyn NodeManager>> {
        self.namespace_registry
            .resolve_manager_handle(namespace_index)
    }

    fn paginate_descriptions(
        &self,
        mut descriptions: Vec<ReferenceDescription>,
        max_results: usize,
    ) -> BrowseResult {
        let max = if max_results == 0 { 1000 } else { max_results };
        if descriptions.len() > max {
            let returned: Vec<_> = descriptions.drain(..max).collect();
            let continuation = self.continuation_store.create(descriptions);
            BrowseResult::with_continuation(returned, continuation)
        } else {
            BrowseResult::new(descriptions)
        }
    }

    pub(crate) fn insert_node<N: Node + 'static>(&self, node: N) -> bool {
        self.node_store.insert_node(node)
    }

    pub(crate) fn insert_boxed_node(&self, node: Box<dyn Node>) -> bool {
        self.node_store.insert_boxed_node(node)
    }

    pub(crate) fn get_node(&self, node_id: &NodeId) -> Option<SharedNode> {
        self.node_store.get(node_id)
    }

    pub(crate) fn contains_node(&self, node_id: &NodeId) -> bool {
        self.node_store.contains(node_id)
    }

    pub(crate) fn remove_node(&self, node_id: &NodeId) -> bool {
        let removed = self.node_store.remove(node_id);
        if removed {
            self.reference_index.remove_node(node_id);
        }
        removed
    }

    pub(crate) fn node_count(&self) -> usize {
        self.node_store.len()
    }

    pub(crate) fn node_ids(&self) -> Vec<NodeId> {
        self.node_store.node_ids()
    }

    pub(crate) fn add_reference(&self, reference: Reference) {
        self.reference_index.add_reference(reference);
    }

    pub(crate) fn remove_reference(
        &self,
        source: &NodeId,
        reference_type: ReferenceTypeId,
        target: &NodeId,
    ) -> bool {
        self.reference_index
            .remove_reference(source, reference_type, target)
    }

    pub(crate) fn get_references(
        &self,
        node_id: &NodeId,
        direction: BrowseDirection,
    ) -> Vec<Reference> {
        self.reference_index.get_references(node_id, direction)
    }

    pub(crate) fn browse(
        &self,
        address_space: &AddressSpace,
        node_id: &NodeId,
        direction: BrowseDirection,
        reference_type_filter: Option<ReferenceTypeId>,
        include_subtypes: bool,
        node_class_mask: Option<u32>,
        max_results: usize,
    ) -> BrowseResult {
        if !self.contains_node(node_id) {
            return BrowseResult::default();
        }

        if let Some(manager) = self.manager_for_namespace(node_id.namespace()) {
            if let ManagedOperation::Handled(descriptions) = manager.browse(
                address_space,
                node_id,
                direction,
                reference_type_filter,
                include_subtypes,
                node_class_mask,
            ) {
                return self.paginate_descriptions(descriptions, max_results);
            }
        }

        let references = self.get_references(node_id, direction);
        let mut descriptions = Vec::new();

        for reference in references {
            if let Some(filter) = reference_type_filter {
                if reference.reference_type_id != filter {
                    if !include_subtypes
                        || !self
                            .type_tree
                            .is_reference_subtype(&reference.reference_type_id, &filter)
                    {
                        continue;
                    }
                }
            }

            if let Some(target_node) = self.get_node(&reference.target_node_id) {
                let target = target_node.read();
                if let Some(mask) = node_class_mask {
                    if mask != 0 && (target.node_class() as u32 & mask) == 0 {
                        continue;
                    }
                }

                descriptions.push(ReferenceDescription::new(
                    reference.reference_type_id,
                    matches!(reference.direction, ReferenceDirection::Forward),
                    reference.target_node_id.clone(),
                    target.browse_name().clone(),
                    target.display_name().clone(),
                    target.node_class(),
                ));
            }
        }

        self.paginate_descriptions(descriptions, max_results)
    }

    pub(crate) fn browse_next(
        &self,
        continuation_point: &[u8],
        release: bool,
        max_results: usize,
    ) -> BrowseResult {
        self.continuation_store
            .browse_next(continuation_point, release, max_results)
    }

    pub(crate) fn release_continuation_point(&self, continuation_point: &[u8]) {
        self.continuation_store.release(continuation_point);
    }

    pub(crate) fn resolve_browse_path(
        &self,
        address_space: &AddressSpace,
        starting_node: &NodeId,
        elements: &[RelativePathElement],
    ) -> BrowsePathResult {
        if !self.contains_node(starting_node) {
            return BrowsePathResult {
                status: StatusCode::BAD_NODE_ID_UNKNOWN,
                targets: Vec::new(),
            };
        }

        if let Some(manager) = self.manager_for_namespace(starting_node.namespace()) {
            if let ManagedOperation::Handled(result) =
                manager.resolve_browse_path(address_space, starting_node, elements)
            {
                return result;
            }
        }

        if elements.is_empty() {
            return BrowsePathResult {
                status: StatusCode::GOOD,
                targets: vec![BrowsePathTarget {
                    target_id: starting_node.clone(),
                    remaining_path_index: 0,
                }],
            };
        }

        let mut current_nodes = vec![starting_node.clone()];
        for element in elements {
            let mut next_nodes = Vec::new();
            for current_node in &current_nodes {
                let direction = if element.is_inverse {
                    BrowseDirection::Inverse
                } else {
                    BrowseDirection::Forward
                };

                for reference in self.get_references(current_node, direction) {
                    if reference.reference_type_id.node_id() != NodeId::numeric(0, 0) {
                        if let Some(filter) =
                            ReferenceTypeId::from_node_id(&element.reference_type_id)
                        {
                            if reference.reference_type_id != filter {
                                if !element.include_subtypes
                                    || !self
                                        .type_tree
                                        .is_reference_subtype(&reference.reference_type_id, &filter)
                                {
                                    continue;
                                }
                            }
                        }
                    }

                    if let Some(target_node) = self.get_node(&reference.target_node_id) {
                        let target = target_node.read();
                        let browse_name = target.browse_name();
                        if browse_name.name == element.target_name.name {
                            let namespace_matches = element.target_name.namespace_index == 0
                                || element.target_name.namespace_index
                                    == browse_name.namespace_index;
                            if namespace_matches {
                                next_nodes.push(reference.target_node_id.clone());
                            }
                        }
                    }
                }
            }

            if next_nodes.is_empty() {
                return BrowsePathResult {
                    status: StatusCode::BAD_NO_MATCH,
                    targets: Vec::new(),
                };
            }

            current_nodes = next_nodes;
        }

        BrowsePathResult {
            status: StatusCode::GOOD,
            targets: current_nodes
                .into_iter()
                .map(|node_id| BrowsePathTarget {
                    target_id: node_id,
                    remaining_path_index: 0,
                })
                .collect(),
        }
    }

    pub(crate) fn read(
        &self,
        address_space: &AddressSpace,
        node_id: &NodeId,
        attribute_id: AttributeId,
    ) -> DataValue {
        if let Some(manager) = self.manager_for_namespace(node_id.namespace()) {
            if let ManagedOperation::Handled(value) =
                manager.read_attribute(address_space, node_id, attribute_id)
            {
                return value;
            }
        }

        match self.get_node(node_id) {
            Some(node) => node.read().read_attribute(attribute_id),
            None => DataValue::bad(StatusCode::BAD_NODE_ID_UNKNOWN),
        }
    }

    pub(crate) fn write(
        &self,
        address_space: &AddressSpace,
        node_id: &NodeId,
        attribute_id: AttributeId,
        value: DataValue,
    ) -> StatusCode {
        if let Some(manager) = self.manager_for_namespace(node_id.namespace()) {
            if let ManagedOperation::Handled(status) =
                manager.write_attribute(address_space, node_id, attribute_id, &value)
            {
                return status;
            }
        }

        match self.get_node(node_id) {
            Some(node) => node.write().write_attribute(attribute_id, value),
            None => StatusCode::BAD_NODE_ID_UNKNOWN,
        }
    }

    pub(crate) fn total_references(&self) -> u64 {
        self.reference_index.total_references()
    }

    pub(crate) fn node_class_counts(&self) -> (u64, u64, u64) {
        self.node_store.counts_by_class()
    }

    #[allow(dead_code)]
    pub(crate) fn is_reference_subtype(
        &self,
        address_space: &AddressSpace,
        candidate: &ReferenceTypeId,
        parent: &ReferenceTypeId,
    ) -> bool {
        for manager in self.namespace_registry.managers() {
            if let ManagedOperation::Handled(result) =
                manager.is_reference_subtype(address_space, candidate, parent)
            {
                return result;
            }
        }

        self.type_tree.is_reference_subtype(candidate, parent)
    }

    pub(crate) fn is_node_subtype_of(
        &self,
        address_space: &AddressSpace,
        candidate: &NodeId,
        parent: &NodeId,
    ) -> bool {
        if let Some(manager) = self.manager_for_namespace(candidate.namespace()) {
            if let ManagedOperation::Handled(result) =
                manager.is_node_subtype_of(address_space, candidate, parent)
            {
                return result;
            }
        }

        self.type_tree.is_node_subtype_of(candidate, parent)
    }

    pub(crate) fn refresh_diagnostics(
        &self,
        address_space: &AddressSpace,
        snapshot: &DiagnosticsSnapshot,
    ) {
        let Some(namespace_index) = self.diagnostics_namespace_index() else {
            return;
        };

        let numeric_metrics = [
            ("CurrentSessionCount", snapshot.current_sessions),
            ("CurrentSubscriptionCount", snapshot.current_subscriptions),
            ("TotalNodes", snapshot.total_nodes),
            ("NamespaceCount", snapshot.namespace_count),
        ];
        for (name, value) in numeric_metrics {
            let _ = address_space.write_value(
                &DiagnosticsNodeManager::metric_node_id(namespace_index, name),
                crate::types::Variant::UInt32(value),
            );
        }

        let string_metrics = [
            (
                "SecurityProfileSummary",
                snapshot.security_profile_summary.as_str(),
            ),
            (
                "DurableRestoreSummary",
                snapshot.durable_restore_summary.as_str(),
            ),
            (
                "ManagerOwnershipSummary",
                snapshot.manager_ownership_summary.as_str(),
            ),
        ];
        for (name, value) in string_metrics {
            let _ = address_space.write_value(
                &DiagnosticsNodeManager::metric_node_id(namespace_index, name),
                crate::types::Variant::String(value.to_string()),
            );
        }
    }

    pub(crate) fn namespace_diagnostics_state(&self) -> Vec<NamespaceDiagnosticsState> {
        self.namespace_registry.diagnostics_state()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use parking_lot::RwLock;

    use super::{
        CatalogNodeManager, DiagnosticsNodeManager, DiagnosticsSnapshot, NamespaceLifecycleState,
        NodeManager,
    };
    use crate::nodes::{AddressSpace, AddressSpaceConfig};
    use crate::types::Variant;

    #[derive(Default)]
    struct TestManager {
        started: RwLock<bool>,
        stopped: RwLock<bool>,
        namespace_index: RwLock<Option<u16>>,
    }

    impl NodeManager for TestManager {
        fn kind(&self) -> &'static str {
            "test"
        }

        fn namespace_uri(&self) -> Option<&str> {
            Some("urn:mabinogion:test:manager")
        }

        fn on_registered(&self, namespace_index: Option<u16>, _namespace_uri: Option<&str>) {
            *self.namespace_index.write() = namespace_index;
        }

        fn owns_namespace(&self, namespace_index: u16) -> bool {
            self.namespace_index
                .read()
                .is_some_and(|index| index == namespace_index)
        }

        fn on_runtime_start(
            &self,
            _address_space: &AddressSpace,
            _namespace_index: Option<u16>,
            _snapshot: &DiagnosticsSnapshot,
        ) {
            *self.started.write() = true;
        }

        fn on_runtime_stop(
            &self,
            _address_space: &AddressSpace,
            _namespace_index: Option<u16>,
            _snapshot: &DiagnosticsSnapshot,
        ) {
            *self.stopped.write() = true;
        }

        fn diagnostics_snapshot(
            &self,
            _address_space: &AddressSpace,
            namespace_index: Option<u16>,
        ) -> Option<String> {
            Some(format!(
                "manager=test namespace={}",
                namespace_index.unwrap_or_default()
            ))
        }
    }

    #[test]
    fn diagnostics_manager_materializes_runtime_namespace_and_metrics() {
        let address_space = AddressSpace::new(AddressSpaceConfig::default());
        let namespace_index = address_space.diagnostics_namespace_index().unwrap();
        let snapshot = DiagnosticsSnapshot {
            current_sessions: 3,
            current_subscriptions: 7,
            total_nodes: 42,
            namespace_count: 4,
            security_profile_summary: "None".into(),
            durable_restore_summary: "restored=1 detached=1".into(),
            manager_ownership_summary: "ns=0 uri=http://opcfoundation.org/UA/ manager=default"
                .into(),
        };

        address_space.on_runtime_start(&snapshot);
        address_space.refresh_diagnostics(&snapshot);

        let states = address_space.namespace_diagnostics_state();
        let diagnostics = states
            .iter()
            .find(|state| state.manager_kind == "diagnostics")
            .unwrap();
        assert_eq!(diagnostics.lifecycle, NamespaceLifecycleState::Running);
        assert_eq!(diagnostics.namespace_index, Some(namespace_index));
        assert!(diagnostics
            .last_summary
            .as_deref()
            .unwrap_or_default()
            .contains("manager=diagnostics"));

        let value = address_space.read_value(&DiagnosticsNodeManager::metric_node_id(
            namespace_index,
            "CurrentSessionCount",
        ));
        assert_eq!(value.value(), Some(&Variant::UInt32(3)));

        let summary = address_space.read_value(&DiagnosticsNodeManager::metric_node_id(
            namespace_index,
            "ManagerOwnershipSummary",
        ));
        assert!(matches!(
            summary.value(),
            Some(Variant::String(summary)) if summary.contains("manager=default")
        ));
    }

    #[test]
    fn namespace_registry_reports_default_and_diagnostics_managers() {
        let address_space = AddressSpace::new(AddressSpaceConfig::default());
        let snapshot = DiagnosticsSnapshot {
            current_sessions: 0,
            current_subscriptions: 0,
            total_nodes: 0,
            namespace_count: address_space.namespaces().len() as u32,
            security_profile_summary: "None".into(),
            durable_restore_summary: "restored=0 detached=0".into(),
            manager_ownership_summary: "ns=0 uri=http://opcfoundation.org/UA/ manager=default"
                .into(),
        };

        address_space.on_runtime_start(&snapshot);
        let states = address_space.namespace_diagnostics_state();
        assert!(states.iter().any(|state| state.manager_kind == "default"));
        assert!(states
            .iter()
            .any(|state| state.manager_kind == "diagnostics"));
    }

    #[test]
    fn internal_custom_manager_registers_lifecycle_and_namespace_summary() {
        let manager = Arc::new(TestManager::default());
        let address_space = AddressSpace::new_with_internal_managers(
            AddressSpaceConfig::default(),
            vec![manager.clone()],
        );
        let snapshot = DiagnosticsSnapshot {
            current_sessions: 1,
            current_subscriptions: 2,
            total_nodes: 3,
            namespace_count: address_space.namespaces().len() as u32,
            security_profile_summary: "None".into(),
            durable_restore_summary: "restored=0 detached=0".into(),
            manager_ownership_summary: address_space.manager_ownership_summary().join("; "),
        };

        address_space.on_runtime_start(&snapshot);
        address_space.on_runtime_stop(&snapshot);

        let namespace_index = *manager.namespace_index.read();
        assert!(namespace_index.is_some_and(|index| index > 1));
        assert!(*manager.started.read());
        assert!(*manager.stopped.read());
        assert!(address_space
            .manager_ownership_summary()
            .iter()
            .any(|summary| summary.contains("manager=test")));
    }

    #[test]
    fn catalog_manager_reports_owned_namespace_in_summary() {
        let address_space = AddressSpace::new_with_internal_managers(
            AddressSpaceConfig::default(),
            vec![Arc::new(CatalogNodeManager::new(
                "urn:mabinogion:test:catalog",
            ))],
        );
        assert!(address_space
            .manager_ownership_summary()
            .iter()
            .any(|summary| summary.contains("manager=catalog")));
    }
}

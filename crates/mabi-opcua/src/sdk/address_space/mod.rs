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
use crate::types::{AttributeId, DataValue, NodeId, StatusCode};

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
    fn owns_namespace(&self, namespace_index: u16) -> bool;
}

#[derive(Default)]
pub(crate) struct DefaultNodeManager;

impl NodeManager for DefaultNodeManager {
    fn owns_namespace(&self, _namespace_index: u16) -> bool {
        true
    }
}

#[derive(Debug)]
struct StoredBrowseContinuation {
    remaining_references: Vec<ReferenceDescription>,
    created_at: DateTime<Utc>,
}

pub(crate) struct NamespaceManager {
    namespaces: RwLock<Vec<String>>,
    node_manager: Arc<dyn NodeManager>,
}

impl NamespaceManager {
    pub(crate) fn new(namespaces: Vec<String>) -> Self {
        Self {
            namespaces: RwLock::new(namespaces),
            node_manager: Arc::new(DefaultNodeManager),
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

    #[allow(dead_code)]
    pub(crate) fn owns_namespace(&self, namespace_index: u16) -> bool {
        self.node_manager.owns_namespace(namespace_index)
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
    namespace_manager: NamespaceManager,
    node_store: NodeStore,
    reference_index: Arc<ReferenceIndex>,
    continuation_store: ContinuationStore,
    type_tree: TypeTree,
}

impl AddressSpaceRuntime {
    pub(crate) fn new(default_namespace_uri: String, max_nodes: usize) -> Self {
        let reference_index = Arc::new(ReferenceIndex::new());
        Self {
            namespace_manager: NamespaceManager::new(vec![
                "http://opcfoundation.org/UA/".to_string(),
                default_namespace_uri,
            ]),
            node_store: NodeStore::new(max_nodes),
            reference_index: reference_index.clone(),
            continuation_store: ContinuationStore::new(),
            type_tree: TypeTree::new(reference_index),
        }
    }

    pub(crate) fn register_namespace(&self, uri: &str) -> u16 {
        self.namespace_manager.register_namespace(uri)
    }

    pub(crate) fn get_namespace_uri(&self, index: u16) -> Option<String> {
        self.namespace_manager.get_namespace_uri(index)
    }

    pub(crate) fn get_namespace_index(&self, uri: &str) -> Option<u16> {
        self.namespace_manager.get_namespace_index(uri)
    }

    pub(crate) fn namespaces(&self) -> Vec<String> {
        self.namespace_manager.namespaces()
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

        if descriptions.len() > max_results {
            let returned: Vec<_> = descriptions.drain(..max_results).collect();
            let continuation = self.continuation_store.create(descriptions);
            BrowseResult::with_continuation(returned, continuation)
        } else {
            BrowseResult::new(descriptions)
        }
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
        starting_node: &NodeId,
        elements: &[RelativePathElement],
    ) -> BrowsePathResult {
        if !self.contains_node(starting_node) {
            return BrowsePathResult {
                status: StatusCode::BAD_NODE_ID_UNKNOWN,
                targets: Vec::new(),
            };
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

    pub(crate) fn read(&self, node_id: &NodeId, attribute_id: AttributeId) -> DataValue {
        match self.get_node(node_id) {
            Some(node) => node.read().read_attribute(attribute_id),
            None => DataValue::bad(StatusCode::BAD_NODE_ID_UNKNOWN),
        }
    }

    pub(crate) fn write(
        &self,
        node_id: &NodeId,
        attribute_id: AttributeId,
        value: DataValue,
    ) -> StatusCode {
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
        candidate: &ReferenceTypeId,
        parent: &ReferenceTypeId,
    ) -> bool {
        self.type_tree.is_reference_subtype(candidate, parent)
    }

    pub(crate) fn is_node_subtype_of(&self, candidate: &NodeId, parent: &NodeId) -> bool {
        self.type_tree.is_node_subtype_of(candidate, parent)
    }
}

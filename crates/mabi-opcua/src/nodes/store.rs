//! Address Space - the node store for OPC UA servers.
//!
//! The address space is the primary container for all nodes and references.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use dashmap::DashMap;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use tracing::{debug, info, instrument, warn};

use crate::types::{NodeId, AttributeId, DataValue, StatusCode, Variant};
use crate::error::{OpcUaError, OpcUaResult};
use super::base::{Node, NodeClass, QualifiedName, LocalizedText, SharedNode, shared_node};
use super::classes::{ObjectNode, VariableNode};
use super::reference::{
    Reference, ReferenceDescription, ReferenceTypeId, ReferenceDirection,
    BrowseDirection, BrowseResult,
};

/// Address space configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AddressSpaceConfig {
    /// Default namespace URI.
    pub default_namespace_uri: String,
    /// Maximum nodes allowed.
    pub max_nodes: usize,
    /// Maximum references per node.
    pub max_references_per_node: usize,
    /// Enable standard namespace (namespace 0).
    pub enable_standard_namespace: bool,
}

impl Default for AddressSpaceConfig {
    fn default() -> Self {
        Self {
            default_namespace_uri: "urn:trap:simulator".to_string(),
            max_nodes: 1_000_000,
            max_references_per_node: 10_000,
            enable_standard_namespace: true,
        }
    }
}

/// Node store statistics.
#[derive(Debug, Clone, Default)]
pub struct NodeStoreStats {
    /// Total nodes in the store.
    pub total_nodes: u64,
    /// Total references.
    pub total_references: u64,
    /// Variable nodes count.
    pub variable_nodes: u64,
    /// Object nodes count.
    pub object_nodes: u64,
    /// Method nodes count.
    pub method_nodes: u64,
    /// Read operations count.
    pub reads: u64,
    /// Write operations count.
    pub writes: u64,
    /// Browse operations count.
    pub browses: u64,
}

/// OPC UA Address Space.
///
/// The address space contains all nodes and references that make up the
/// OPC UA server's information model.
///
/// # Thread Safety
///
/// The address space is designed for concurrent access using DashMap
/// for the node store and a separate structure for references.
///
/// # Examples
///
/// ```
/// use mabi_opcua::nodes::{AddressSpace, AddressSpaceConfig};
/// use mabi_opcua::types::NodeId;
///
/// let address_space = AddressSpace::new(AddressSpaceConfig::default());
///
/// // Add nodes
/// address_space.add_folder(
///     NodeId::numeric(2, 1000),
///     "MyFolder",
///     "My Folder",
///     &NodeId::objects_folder(),
/// );
/// ```
pub struct AddressSpace {
    /// Configuration.
    config: AddressSpaceConfig,
    /// Node store (NodeId -> Node).
    nodes: DashMap<NodeId, SharedNode>,
    /// Forward references (source NodeId -> References).
    forward_references: DashMap<NodeId, Vec<Reference>>,
    /// Inverse references (target NodeId -> References).
    inverse_references: DashMap<NodeId, Vec<Reference>>,
    /// Namespace array (index -> URI).
    namespaces: RwLock<Vec<String>>,
    /// Statistics.
    stats: NodeStoreStats,
    /// Atomic counters for stats.
    read_counter: AtomicU64,
    write_counter: AtomicU64,
    browse_counter: AtomicU64,
}

impl AddressSpace {
    /// Create a new address space.
    pub fn new(config: AddressSpaceConfig) -> Self {
        let mut namespaces = vec![
            "http://opcfoundation.org/UA/".to_string(), // Namespace 0 (standard)
            config.default_namespace_uri.clone(),       // Namespace 1 (server)
        ];

        let address_space = Self {
            config,
            nodes: DashMap::new(),
            forward_references: DashMap::new(),
            inverse_references: DashMap::new(),
            namespaces: RwLock::new(namespaces),
            stats: NodeStoreStats::default(),
            read_counter: AtomicU64::new(0),
            write_counter: AtomicU64::new(0),
            browse_counter: AtomicU64::new(0),
        };

        // Initialize standard nodes if enabled
        if address_space.config.enable_standard_namespace {
            address_space.init_standard_nodes();
        }

        address_space
    }

    /// Initialize standard OPC UA nodes.
    fn init_standard_nodes(&self) {
        // Root folder
        let root = ObjectNode::new(
            NodeId::root_folder(),
            QualifiedName::null("Root"),
            "Root",
        );
        self.insert_node(root);

        // Objects folder
        let objects = ObjectNode::new(
            NodeId::objects_folder(),
            QualifiedName::null("Objects"),
            "Objects",
        );
        self.insert_node(objects);
        self.add_reference(Reference::organizes(
            NodeId::root_folder(),
            NodeId::objects_folder(),
        ));

        // Types folder
        let types = ObjectNode::new(
            NodeId::types_folder(),
            QualifiedName::null("Types"),
            "Types",
        );
        self.insert_node(types);
        self.add_reference(Reference::organizes(
            NodeId::root_folder(),
            NodeId::types_folder(),
        ));

        // Views folder
        let views = ObjectNode::new(
            NodeId::views_folder(),
            QualifiedName::null("Views"),
            "Views",
        );
        self.insert_node(views);
        self.add_reference(Reference::organizes(
            NodeId::root_folder(),
            NodeId::views_folder(),
        ));

        // Server node
        let server = ObjectNode::new(
            NodeId::server(),
            QualifiedName::null("Server"),
            "Server",
        );
        self.insert_node(server);
        self.add_reference(Reference::organizes(
            NodeId::objects_folder(),
            NodeId::server(),
        ));

        info!("Initialized standard OPC UA address space nodes");
    }

    // =========================================================================
    // Namespace Management
    // =========================================================================

    /// Register a new namespace.
    pub fn register_namespace(&self, uri: &str) -> u16 {
        let mut namespaces = self.namespaces.write();

        // Check if already exists
        if let Some(index) = namespaces.iter().position(|u| u == uri) {
            return index as u16;
        }

        // Add new namespace
        let index = namespaces.len() as u16;
        namespaces.push(uri.to_string());
        index
    }

    /// Get namespace URI by index.
    pub fn get_namespace_uri(&self, index: u16) -> Option<String> {
        let namespaces = self.namespaces.read();
        namespaces.get(index as usize).cloned()
    }

    /// Get namespace index by URI.
    pub fn get_namespace_index(&self, uri: &str) -> Option<u16> {
        let namespaces = self.namespaces.read();
        namespaces.iter().position(|u| u == uri).map(|i| i as u16)
    }

    /// Get all namespace URIs.
    pub fn namespaces(&self) -> Vec<String> {
        self.namespaces.read().clone()
    }

    // =========================================================================
    // Node Operations
    // =========================================================================

    /// Insert a node into the address space.
    pub fn insert_node<N: Node + 'static>(&self, node: N) -> bool {
        if self.nodes.len() >= self.config.max_nodes {
            warn!("Address space is full, cannot add more nodes");
            return false;
        }

        let node_id = node.node_id().clone();
        if self.nodes.contains_key(&node_id) {
            return false;
        }

        self.nodes.insert(node_id, shared_node(node));
        true
    }

    /// Insert a boxed node.
    pub fn insert_boxed_node(&self, node: Box<dyn Node>) -> bool {
        if self.nodes.len() >= self.config.max_nodes {
            return false;
        }

        let node_id = node.node_id().clone();
        if self.nodes.contains_key(&node_id) {
            return false;
        }

        self.nodes.insert(node_id, Arc::new(parking_lot::RwLock::new(node)));
        true
    }

    /// Get a node by ID.
    pub fn get_node(&self, node_id: &NodeId) -> Option<SharedNode> {
        self.nodes.get(node_id).map(|n| n.clone())
    }

    /// Check if a node exists.
    pub fn contains_node(&self, node_id: &NodeId) -> bool {
        self.nodes.contains_key(node_id)
    }

    /// Remove a node.
    pub fn remove_node(&self, node_id: &NodeId) -> bool {
        if let Some(_) = self.nodes.remove(node_id) {
            // Remove all references involving this node
            self.forward_references.remove(node_id);
            self.inverse_references.remove(node_id);
            true
        } else {
            false
        }
    }

    /// Get the number of nodes.
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    /// Get all node IDs.
    pub fn node_ids(&self) -> Vec<NodeId> {
        self.nodes.iter().map(|e| e.key().clone()).collect()
    }

    // =========================================================================
    // Convenience Methods for Adding Nodes
    // =========================================================================

    /// Add a folder node.
    #[instrument(skip_all, fields(node_id = %node_id, parent_id = %parent_id))]
    pub fn add_folder(
        &self,
        node_id: NodeId,
        browse_name: impl Into<QualifiedName>,
        display_name: impl Into<LocalizedText>,
        parent_id: &NodeId,
    ) -> OpcUaResult<NodeId> {
        let browse_name = browse_name.into();
        let folder = ObjectNode::new(node_id.clone(), browse_name, display_name);

        if !self.insert_node(folder) {
            return Err(OpcUaError::Server(format!(
                "Failed to insert folder node: {}",
                node_id
            )));
        }

        self.add_reference(Reference::organizes(parent_id.clone(), node_id.clone()));

        debug!(node_id = %node_id, "Added folder node");
        Ok(node_id)
    }

    /// Add a variable node.
    #[instrument(skip_all, fields(node_id = %node_id, parent_id = %parent_id))]
    pub fn add_variable(
        &self,
        node_id: NodeId,
        browse_name: impl Into<QualifiedName>,
        display_name: impl Into<LocalizedText>,
        data_type: NodeId,
        value: impl Into<Variant>,
        parent_id: &NodeId,
    ) -> OpcUaResult<NodeId> {
        let browse_name = browse_name.into();
        let variable = VariableNode::new(
            node_id.clone(),
            browse_name,
            display_name,
            data_type,
            value,
        );

        if !self.insert_node(variable) {
            return Err(OpcUaError::Server(format!(
                "Failed to insert variable node: {}",
                node_id
            )));
        }

        self.add_reference(Reference::has_component(parent_id.clone(), node_id.clone()));

        debug!(node_id = %node_id, "Added variable node");
        Ok(node_id)
    }

    /// Add a writable variable node.
    pub fn add_writable_variable(
        &self,
        node_id: NodeId,
        browse_name: impl Into<QualifiedName>,
        display_name: impl Into<LocalizedText>,
        data_type: NodeId,
        value: impl Into<Variant>,
        parent_id: &NodeId,
    ) -> OpcUaResult<NodeId> {
        let browse_name = browse_name.into();
        let variable = VariableNode::new(
            node_id.clone(),
            browse_name,
            display_name,
            data_type,
            value,
        )
        .writable();

        if !self.insert_node(variable) {
            return Err(OpcUaError::Server(format!(
                "Failed to insert variable node: {}",
                node_id
            )));
        }

        self.add_reference(Reference::has_component(parent_id.clone(), node_id.clone()));
        Ok(node_id)
    }

    /// Add an object node.
    pub fn add_object(
        &self,
        node_id: NodeId,
        browse_name: impl Into<QualifiedName>,
        display_name: impl Into<LocalizedText>,
        parent_id: &NodeId,
    ) -> OpcUaResult<NodeId> {
        let object = ObjectNode::new(node_id.clone(), browse_name, display_name);

        if !self.insert_node(object) {
            return Err(OpcUaError::Server(format!(
                "Failed to insert object node: {}",
                node_id
            )));
        }

        self.add_reference(Reference::has_component(parent_id.clone(), node_id.clone()));
        Ok(node_id)
    }

    // =========================================================================
    // Reference Operations
    // =========================================================================

    /// Add a reference.
    pub fn add_reference(&self, reference: Reference) {
        // Add forward reference
        self.forward_references
            .entry(reference.source_node_id.clone())
            .or_insert_with(Vec::new)
            .push(reference.clone());

        // Add inverse reference
        let inverse = reference.inverse_ref();
        self.inverse_references
            .entry(inverse.source_node_id.clone())
            .or_insert_with(Vec::new)
            .push(inverse);
    }

    /// Remove a reference.
    pub fn remove_reference(&self, source: &NodeId, reference_type: ReferenceTypeId, target: &NodeId) -> bool {
        let mut removed = false;

        // Remove forward reference
        if let Some(mut refs) = self.forward_references.get_mut(source) {
            refs.retain(|r| {
                let matches = r.reference_type_id == reference_type && &r.target_node_id == target;
                if matches {
                    removed = true;
                }
                !matches
            });
        }

        // Remove inverse reference
        if let Some(mut refs) = self.inverse_references.get_mut(target) {
            refs.retain(|r| {
                !(r.reference_type_id == reference_type && &r.target_node_id == source)
            });
        }

        removed
    }

    /// Get references from a node.
    pub fn get_references(&self, node_id: &NodeId, direction: BrowseDirection) -> Vec<Reference> {
        let mut refs = Vec::new();

        if matches!(direction, BrowseDirection::Forward | BrowseDirection::Both) {
            if let Some(forward) = self.forward_references.get(node_id) {
                refs.extend(forward.iter().cloned());
            }
        }

        if matches!(direction, BrowseDirection::Inverse | BrowseDirection::Both) {
            if let Some(inverse) = self.inverse_references.get(node_id) {
                refs.extend(inverse.iter().cloned());
            }
        }

        refs
    }

    /// Browse node references.
    #[instrument(skip(self))]
    pub fn browse(
        &self,
        node_id: &NodeId,
        direction: BrowseDirection,
        reference_type_filter: Option<ReferenceTypeId>,
        include_subtypes: bool,
        node_class_mask: Option<u32>,
        max_results: usize,
    ) -> BrowseResult {
        self.browse_counter.fetch_add(1, Ordering::Relaxed);

        if !self.contains_node(node_id) {
            return BrowseResult::default();
        }

        let refs = self.get_references(node_id, direction);
        let mut descriptions = Vec::new();

        for reference in refs {
            // Filter by reference type
            if let Some(ref filter) = reference_type_filter {
                if reference.reference_type_id != *filter {
                    if !include_subtypes {
                        continue;
                    }
                    // TODO: Check if reference type is subtype of filter
                }
            }

            // Get target node
            if let Some(target_node) = self.get_node(&reference.target_node_id) {
                let target = target_node.read();

                // Filter by node class
                if let Some(mask) = node_class_mask {
                    if mask != 0 && (target.node_class() as u32 & mask) == 0 {
                        continue;
                    }
                }

                let desc = ReferenceDescription::new(
                    reference.reference_type_id,
                    matches!(reference.direction, ReferenceDirection::Forward),
                    reference.target_node_id.clone(),
                    target.browse_name().clone(),
                    target.display_name().clone(),
                    target.node_class(),
                );

                descriptions.push(desc);

                if descriptions.len() >= max_results {
                    // TODO: Create continuation point
                    break;
                }
            }
        }

        BrowseResult::new(descriptions)
    }

    // =========================================================================
    // Read/Write Operations
    // =========================================================================

    /// Read an attribute from a node.
    pub fn read(&self, node_id: &NodeId, attribute_id: AttributeId) -> DataValue {
        self.read_counter.fetch_add(1, Ordering::Relaxed);

        match self.get_node(node_id) {
            Some(node) => {
                let node = node.read();
                node.read_attribute(attribute_id)
            }
            None => DataValue::bad(StatusCode::BAD_NODE_ID_UNKNOWN),
        }
    }

    /// Read the value attribute from a variable node.
    pub fn read_value(&self, node_id: &NodeId) -> DataValue {
        self.read(node_id, AttributeId::Value)
    }

    /// Write an attribute to a node.
    pub fn write(&self, node_id: &NodeId, attribute_id: AttributeId, value: DataValue) -> StatusCode {
        self.write_counter.fetch_add(1, Ordering::Relaxed);

        match self.get_node(node_id) {
            Some(node) => {
                let mut node = node.write();
                node.write_attribute(attribute_id, value)
            }
            None => StatusCode::BAD_NODE_ID_UNKNOWN,
        }
    }

    /// Write the value attribute to a variable node.
    pub fn write_value(&self, node_id: &NodeId, value: impl Into<Variant>) -> StatusCode {
        self.write(node_id, AttributeId::Value, DataValue::new(value.into()))
    }

    // =========================================================================
    // Batch Operations
    // =========================================================================

    /// Read multiple values.
    pub fn read_values(&self, node_ids: &[NodeId]) -> Vec<DataValue> {
        node_ids
            .iter()
            .map(|id| self.read_value(id))
            .collect()
    }

    /// Write multiple values.
    pub fn write_values(&self, values: &[(NodeId, Variant)]) -> Vec<StatusCode> {
        values
            .iter()
            .map(|(id, v)| self.write_value(id, v.clone()))
            .collect()
    }

    // =========================================================================
    // Statistics
    // =========================================================================

    /// Get statistics.
    pub fn stats(&self) -> NodeStoreStats {
        let mut stats = NodeStoreStats::default();
        stats.total_nodes = self.nodes.len() as u64;

        // Count references
        for refs in self.forward_references.iter() {
            stats.total_references += refs.len() as u64;
        }

        // Count by node class
        for entry in self.nodes.iter() {
            let node = entry.value().read();
            match node.node_class() {
                NodeClass::Variable => stats.variable_nodes += 1,
                NodeClass::Object => stats.object_nodes += 1,
                NodeClass::Method => stats.method_nodes += 1,
                _ => {}
            }
        }

        stats.reads = self.read_counter.load(Ordering::Relaxed);
        stats.writes = self.write_counter.load(Ordering::Relaxed);
        stats.browses = self.browse_counter.load(Ordering::Relaxed);

        stats
    }
}

impl Default for AddressSpace {
    fn default() -> Self {
        Self::new(AddressSpaceConfig::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::variant::DataTypeId;

    #[test]
    fn test_address_space_creation() {
        let address_space = AddressSpace::default();

        // Standard nodes should exist
        assert!(address_space.contains_node(&NodeId::root_folder()));
        assert!(address_space.contains_node(&NodeId::objects_folder()));
        assert!(address_space.contains_node(&NodeId::types_folder()));
        assert!(address_space.contains_node(&NodeId::views_folder()));
        assert!(address_space.contains_node(&NodeId::server()));
    }

    #[test]
    fn test_add_folder() {
        let address_space = AddressSpace::default();

        let folder_id = address_space
            .add_folder(
                NodeId::numeric(2, 1000),
                QualifiedName::new(2, "MyFolder"),
                "My Folder",
                &NodeId::objects_folder(),
            )
            .unwrap();

        assert!(address_space.contains_node(&folder_id));

        // Check reference
        let refs = address_space.get_references(&NodeId::objects_folder(), BrowseDirection::Forward);
        assert!(refs.iter().any(|r| r.target_node_id == folder_id));
    }

    #[test]
    fn test_add_variable() {
        let address_space = AddressSpace::default();

        let folder_id = address_space
            .add_folder(
                NodeId::numeric(2, 1000),
                "MyFolder",
                "My Folder",
                &NodeId::objects_folder(),
            )
            .unwrap();

        let var_id = address_space
            .add_variable(
                NodeId::numeric(2, 1001),
                "Temperature",
                "Temperature",
                NodeId::numeric(0, DataTypeId::Double as u32),
                25.5f64,
                &folder_id,
            )
            .unwrap();

        assert!(address_space.contains_node(&var_id));

        // Read value
        let value = address_space.read_value(&var_id);
        assert!(value.is_good());
        assert_eq!(value.value().unwrap().as_f64(), Some(25.5));
    }

    #[test]
    fn test_write_value() {
        let address_space = AddressSpace::default();

        let var_id = address_space
            .add_writable_variable(
                NodeId::numeric(2, 1001),
                "Temperature",
                "Temperature",
                NodeId::numeric(0, DataTypeId::Double as u32),
                25.5f64,
                &NodeId::objects_folder(),
            )
            .unwrap();

        // Write new value
        let status = address_space.write_value(&var_id, 30.0f64);
        assert!(status.is_good());

        // Verify
        let value = address_space.read_value(&var_id);
        assert_eq!(value.value().unwrap().as_f64(), Some(30.0));
    }

    #[test]
    fn test_browse() {
        let address_space = AddressSpace::default();

        // Add some nodes
        let folder_id = address_space
            .add_folder(
                NodeId::numeric(2, 1000),
                "TestFolder",
                "Test Folder",
                &NodeId::objects_folder(),
            )
            .unwrap();

        for i in 0..5 {
            address_space
                .add_variable(
                    NodeId::numeric(2, 1001 + i),
                    format!("Var{}", i),
                    format!("Variable {}", i),
                    NodeId::numeric(0, DataTypeId::Double as u32),
                    i as f64,
                    &folder_id,
                )
                .unwrap();
        }

        // Browse the folder
        let result = address_space.browse(
            &folder_id,
            BrowseDirection::Forward,
            None,
            false,
            None,
            100,
        );

        assert_eq!(result.len(), 5);
    }

    #[test]
    fn test_namespaces() {
        let address_space = AddressSpace::default();

        // Standard namespace
        assert_eq!(
            address_space.get_namespace_uri(0),
            Some("http://opcfoundation.org/UA/".to_string())
        );

        // Register custom namespace
        let idx = address_space.register_namespace("http://example.com/test");
        assert!(idx >= 2);
        assert_eq!(
            address_space.get_namespace_index("http://example.com/test"),
            Some(idx)
        );
    }

    #[test]
    fn test_stats() {
        let address_space = AddressSpace::default();

        // Add some nodes
        for i in 0..10 {
            address_space
                .add_variable(
                    NodeId::numeric(2, 1000 + i),
                    format!("Var{}", i),
                    format!("Variable {}", i),
                    NodeId::numeric(0, DataTypeId::Int32 as u32),
                    i,
                    &NodeId::objects_folder(),
                )
                .unwrap();
        }

        let stats = address_space.stats();
        assert!(stats.total_nodes > 10);
        assert!(stats.variable_nodes >= 10);
    }
}

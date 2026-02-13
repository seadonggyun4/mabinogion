//! Address Space - the node store for OPC UA servers.
//!
//! The address space is the primary container for all nodes and references.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use chrono::{DateTime, Utc};
use dashmap::DashMap;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use tracing::{debug, info, instrument, warn};

use crate::types::{NodeId, AttributeId, DataValue, StatusCode, Variant};
use crate::error::{OpcUaError, OpcUaResult};
use super::base::{Node, NodeClass, QualifiedName, LocalizedText, SharedNode, shared_node};
use super::classes::{ObjectNode, VariableNode, DataTypeNode, ObjectTypeNode};
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
/// A stored browse continuation for BrowseNext pagination.
pub struct BrowseContinuation {
    /// Unique identifier.
    pub id: u64,
    /// Remaining references not yet returned.
    pub remaining_references: Vec<ReferenceDescription>,
    /// Creation time for expiry check.
    pub created_at: DateTime<Utc>,
}

/// Result of a browse path resolution.
pub struct BrowsePathResult {
    /// Status of the resolution.
    pub status: StatusCode,
    /// Resolved targets.
    pub targets: Vec<BrowsePathTarget>,
}

/// A resolved browse path target.
pub struct BrowsePathTarget {
    /// The resolved node ID.
    pub target_id: NodeId,
    /// Index of the first unprocessed element (0 = fully resolved).
    pub remaining_path_index: u32,
}

/// A single element in a relative browse path.
pub struct RelativePathElement {
    /// Reference type to follow.
    pub reference_type_id: NodeId,
    /// Whether to follow inverse references.
    pub is_inverse: bool,
    /// Whether to include subtypes of the reference type.
    pub include_subtypes: bool,
    /// Target browse name to match.
    pub target_name: QualifiedName,
}

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
    /// Browse continuation points for BrowseNext pagination.
    browse_continuations: RwLock<HashMap<u64, BrowseContinuation>>,
    /// Next continuation point ID.
    next_browse_continuation_id: AtomicU64,
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
            browse_continuations: RwLock::new(HashMap::new()),
            next_browse_continuation_id: AtomicU64::new(1),
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

        // Server node (with event_notifier=1 to support event subscriptions)
        let server = ObjectNode::new(
            NodeId::server(),
            QualifiedName::null("Server"),
            "Server",
        ).with_event_notifier(1);
        self.insert_node(server);
        self.add_reference(Reference::organizes(
            NodeId::objects_folder(),
            NodeId::server(),
        ));

        // =====================================================================
        // Server/ServerCapabilities/OperationLimits
        // =====================================================================
        self.init_server_capabilities();

        // =====================================================================
        // DataType hierarchy under Types folder
        // =====================================================================
        self.init_data_type_hierarchy();

        // =====================================================================
        // Event type hierarchy under Types folder
        // =====================================================================
        self.init_event_type_hierarchy();

        info!("Initialized standard OPC UA address space nodes");
    }

    /// Initialize Server/ServerCapabilities/OperationLimits nodes.
    ///
    /// These nodes are read by TRAP's OperationLimitsInitTask to discover
    /// server capacity and automatically partition batch operations.
    fn init_server_capabilities(&self) {
        // ServerCapabilities (i=2268)
        let capabilities = ObjectNode::new(
            NodeId::numeric(0, 2268),
            QualifiedName::null("ServerCapabilities"),
            "ServerCapabilities",
        );
        self.insert_node(capabilities);
        self.add_reference(Reference::has_component(
            NodeId::server(), // i=2253
            NodeId::numeric(0, 2268),
        ));

        // OperationLimits (i=11704)
        let op_limits = ObjectNode::new(
            NodeId::numeric(0, 11704),
            QualifiedName::null("OperationLimits"),
            "OperationLimits",
        );
        self.insert_node(op_limits);
        self.add_reference(Reference::has_component(
            NodeId::numeric(0, 2268),
            NodeId::numeric(0, 11704),
        ));

        // Individual limit variables (UInt32)
        let limits: &[(u32, &str, u32)] = &[
            (11705, "MaxNodesPerRead", 10_000),
            (11707, "MaxNodesPerWrite", 10_000),
            (11710, "MaxNodesPerBrowse", 10_000),
            (11711, "MaxNodesPerRegisterNodes", 10_000),
            (11709, "MaxNodesPerMethodCall", 1_000),
            (11714, "MaxMonitoredItemsPerCall", 10_000),
            (12165, "MaxNodesPerHistoryReadData", 1_000),
            (11712, "MaxNodesPerTranslateBrowsePathsToNodeIds", 10_000),
        ];

        for &(id, name, value) in limits {
            let var = VariableNode::new(
                NodeId::numeric(0, id),
                QualifiedName::null(name),
                name,
                NodeId::numeric(0, 7), // UInt32 data type
                Variant::UInt32(value),
            );
            self.insert_node(var);
            self.add_reference(Reference::has_component(
                NodeId::numeric(0, 11704), // OperationLimits
                NodeId::numeric(0, id),
            ));
        }

        // HistoryServerCapabilities (i=2330) — indicates HDA support
        let history_cap = ObjectNode::new(
            NodeId::numeric(0, 2330),
            QualifiedName::null("HistoryServerCapabilities"),
            "HistoryServerCapabilities",
        );
        self.insert_node(history_cap);
        self.add_reference(Reference::has_component(
            NodeId::numeric(0, 2268), // ServerCapabilities
            NodeId::numeric(0, 2330),
        ));

        // AccessHistoryDataCapability (i=11192) = true
        let access_hda = VariableNode::new(
            NodeId::numeric(0, 11192),
            QualifiedName::null("AccessHistoryDataCapability"),
            "AccessHistoryDataCapability",
            NodeId::numeric(0, 1), // Boolean
            Variant::Boolean(true),
        );
        self.insert_node(access_hda);
        self.add_reference(Reference::has_property(
            NodeId::numeric(0, 2330),
            NodeId::numeric(0, 11192),
        ));
    }

    /// Initialize the OPC UA DataType hierarchy.
    ///
    /// This creates the standard built-in type tree under BaseDataType (i=24)
    /// so that TRAP's DataTypeTree can discover types via Browse(HasSubtype).
    fn init_data_type_hierarchy(&self) {
        // BaseDataType (i=24) — abstract root of all data types
        let base = DataTypeNode::new(NodeId::numeric(0, 24), "BaseDataType", "BaseDataType");
        self.insert_node(base);
        self.add_reference(Reference::organizes(
            NodeId::types_folder(), // i=86
            NodeId::numeric(0, 24),
        ));

        // Helper closure to add a data type node with HasSubtype from parent
        let add_type = |store: &AddressSpace, id: u32, name: &str, parent_id: u32, is_abstract: bool| {
            let mut node = DataTypeNode::new(NodeId::numeric(0, id), name, name);
            node.is_abstract = is_abstract;
            store.insert_node(node);
            store.add_reference(Reference::has_subtype(
                NodeId::numeric(0, parent_id),
                NodeId::numeric(0, id),
            ));
        };

        // Direct children of BaseDataType (i=24)
        add_type(self, 1, "Boolean", 24, false);
        add_type(self, 12, "String", 24, false);
        add_type(self, 13, "DateTime", 24, false);
        add_type(self, 14, "Guid", 24, false);
        add_type(self, 15, "ByteString", 24, false);
        add_type(self, 16, "XmlElement", 24, false);
        add_type(self, 17, "NodeId", 24, false);
        add_type(self, 18, "ExpandedNodeId", 24, false);
        add_type(self, 19, "StatusCode", 24, false);
        add_type(self, 20, "QualifiedName", 24, false);
        add_type(self, 21, "LocalizedText", 24, false);
        add_type(self, 22, "Structure", 24, true);   // abstract
        add_type(self, 29, "Enumeration", 24, true);  // abstract
        add_type(self, 26, "Number", 24, true);       // abstract

        // Number subtypes
        add_type(self, 27, "Integer", 26, true);   // abstract
        add_type(self, 28, "UInteger", 26, true);  // abstract
        add_type(self, 10, "Float", 26, false);
        add_type(self, 11, "Double", 26, false);

        // Integer subtypes
        add_type(self, 2, "SByte", 27, false);
        add_type(self, 4, "Int16", 27, false);
        add_type(self, 6, "Int32", 27, false);
        add_type(self, 8, "Int64", 27, false);

        // UInteger subtypes
        add_type(self, 3, "Byte", 28, false);
        add_type(self, 5, "UInt16", 28, false);
        add_type(self, 7, "UInt32", 28, false);
        add_type(self, 9, "UInt64", 28, false);
    }

    /// Initialize event type hierarchy for event subscription support.
    ///
    /// Creates BaseEventType (i=2041) with standard event property nodes
    /// so TRAP's EventFilter can discover and subscribe to events.
    fn init_event_type_hierarchy(&self) {
        // BaseEventType (i=2041) — abstract root of all event types
        let base_event = ObjectTypeNode::new(
            NodeId::numeric(0, 2041),
            "BaseEventType",
            "BaseEventType",
        ).with_is_abstract(true);
        self.insert_node(base_event);
        self.add_reference(Reference::organizes(
            NodeId::types_folder(), // i=86
            NodeId::numeric(0, 2041),
        ));

        // Standard event properties (HasProperty from BaseEventType)
        let event_properties: &[(u32, &str, u32)] = &[
            (2042, "EventId", 15),      // ByteString
            (2043, "EventType", 17),     // NodeId
            (2044, "SourceNode", 17),    // NodeId
            (2045, "SourceName", 12),    // String
            (2046, "Time", 13),          // DateTime
            (2047, "ReceiveTime", 13),   // DateTime
            (2050, "Message", 21),       // LocalizedText
            (2051, "Severity", 5),       // UInt16
        ];

        for &(id, name, data_type_id) in event_properties {
            let var = VariableNode::new(
                NodeId::numeric(0, id),
                QualifiedName::null(name),
                name,
                NodeId::numeric(0, data_type_id),
                Variant::Null,
            );
            self.insert_node(var);
            self.add_reference(Reference::has_property(
                NodeId::numeric(0, 2041), // BaseEventType
                NodeId::numeric(0, id),
            ));
        }

        // Common event subtypes
        let event_subtypes: &[(u32, &str)] = &[
            (2052, "AuditEventType"),
            (2130, "SystemEventType"),
            (2132, "DeviceFailureEventType"),
            (2133, "BaseModelChangeEventType"),
        ];

        for &(id, name) in event_subtypes {
            let event_type = ObjectTypeNode::new(
                NodeId::numeric(0, id),
                name,
                name,
            );
            self.insert_node(event_type);
            self.add_reference(Reference::has_subtype(
                NodeId::numeric(0, 2041), // BaseEventType
                NodeId::numeric(0, id),
            ));
        }
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

    /// Browse node references with continuation point support.
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
        let mut all_descriptions = Vec::new();

        for reference in refs {
            // Filter by reference type
            if let Some(ref filter) = reference_type_filter {
                if reference.reference_type_id != *filter {
                    if !include_subtypes {
                        continue;
                    }
                    // Check if reference type is a subtype of the filter.
                    // For HierarchicalReferences (i=33), accept all hierarchical types.
                    if !self.is_reference_subtype(&reference.reference_type_id, filter) {
                        continue;
                    }
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

                all_descriptions.push(desc);
            }
        }

        // If we have more references than max_results, create a continuation point
        if all_descriptions.len() > max_results {
            let returned: Vec<_> = all_descriptions.drain(..max_results).collect();
            let remaining = all_descriptions; // remaining after drain

            let cp_id = self.next_browse_continuation_id.fetch_add(1, Ordering::Relaxed);
            let continuation = BrowseContinuation {
                id: cp_id,
                remaining_references: remaining,
                created_at: Utc::now(),
            };

            self.browse_continuations.write().insert(cp_id, continuation);

            // Encode continuation point ID as 8-byte LE ByteString
            let cp_bytes = cp_id.to_le_bytes().to_vec();
            BrowseResult::with_continuation(returned, cp_bytes)
        } else {
            BrowseResult::new(all_descriptions)
        }
    }

    /// Continue a previous browse operation using a continuation point.
    ///
    /// Returns the next batch of references, or creates a new continuation
    /// point if more results remain.
    pub fn browse_next(
        &self,
        continuation_point: &[u8],
        release: bool,
        max_results: usize,
    ) -> BrowseResult {
        // Decode continuation point ID from 8-byte LE
        if continuation_point.len() != 8 {
            return BrowseResult::default();
        }
        let cp_id = u64::from_le_bytes([
            continuation_point[0], continuation_point[1],
            continuation_point[2], continuation_point[3],
            continuation_point[4], continuation_point[5],
            continuation_point[6], continuation_point[7],
        ]);

        // If releasing, just remove and return empty
        if release {
            self.browse_continuations.write().remove(&cp_id);
            return BrowseResult::new(Vec::new());
        }

        // Remove the old continuation point
        let continuation = self.browse_continuations.write().remove(&cp_id);
        let Some(mut continuation) = continuation else {
            return BrowseResult::default();
        };

        // Check expiry (5 minutes)
        if Utc::now().signed_duration_since(continuation.created_at).num_seconds() > 300 {
            return BrowseResult::default();
        }

        let max = if max_results == 0 { 1000 } else { max_results };

        if continuation.remaining_references.len() > max {
            let returned: Vec<_> = continuation.remaining_references.drain(..max).collect();
            let remaining = continuation.remaining_references;

            let new_cp_id = self.next_browse_continuation_id.fetch_add(1, Ordering::Relaxed);
            let new_continuation = BrowseContinuation {
                id: new_cp_id,
                remaining_references: remaining,
                created_at: Utc::now(),
            };
            self.browse_continuations.write().insert(new_cp_id, new_continuation);
            let cp_bytes = new_cp_id.to_le_bytes().to_vec();
            BrowseResult::with_continuation(returned, cp_bytes)
        } else {
            BrowseResult::new(continuation.remaining_references)
        }
    }

    /// Release a browse continuation point without returning results.
    pub fn release_continuation_point(&self, continuation_point: &[u8]) {
        if continuation_point.len() == 8 {
            let cp_id = u64::from_le_bytes([
                continuation_point[0], continuation_point[1],
                continuation_point[2], continuation_point[3],
                continuation_point[4], continuation_point[5],
                continuation_point[6], continuation_point[7],
            ]);
            self.browse_continuations.write().remove(&cp_id);
        }
    }

    /// Check if a reference type is a subtype of another.
    ///
    /// Handles the common hierarchical reference type hierarchy:
    /// HierarchicalReferences(33) → HasChild(34) → HasSubtype(45), HasComponent(47), HasProperty(46), Organizes(35)
    fn is_reference_subtype(&self, candidate: &ReferenceTypeId, parent: &ReferenceTypeId) -> bool {
        // All hierarchical types are subtypes of HierarchicalReferences
        if *parent == ReferenceTypeId::HierarchicalReferences {
            return candidate.is_hierarchical();
        }
        // HasChild subtypes
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
        // NonHierarchicalReferences subtypes
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

    /// Resolve a browse path starting from a node, following relative path elements.
    ///
    /// Used by the TranslateBrowsePathsToNodeIds service. Walks the address
    /// space from `starting_node`, matching references by browse name at each hop.
    pub fn resolve_browse_path(
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

        for (idx, element) in elements.iter().enumerate() {
            let mut next_nodes = Vec::new();

            for current_node in &current_nodes {
                let direction = if element.is_inverse {
                    BrowseDirection::Inverse
                } else {
                    BrowseDirection::Forward
                };

                let refs = self.get_references(current_node, direction);

                for reference in &refs {
                    // Filter by reference type (if specified, i.e., not null node i=0)
                    if reference.reference_type_id.node_id() != NodeId::numeric(0, 0) {
                        let ref_type_filter = ReferenceTypeId::from_node_id(&element.reference_type_id);
                        if let Some(filter) = ref_type_filter {
                            if reference.reference_type_id != filter {
                                if element.include_subtypes {
                                    if !self.is_reference_subtype(&reference.reference_type_id, &filter) {
                                        continue;
                                    }
                                } else {
                                    continue;
                                }
                            }
                        }
                    }

                    // Match browse name
                    if let Some(target_node) = self.get_node(&reference.target_node_id) {
                        let target = target_node.read();
                        let browse_name = target.browse_name();

                        // Match by name (and namespace if specified)
                        if browse_name.name == element.target_name.name {
                            let ns_match = element.target_name.namespace_index == 0
                                || element.target_name.namespace_index == browse_name.namespace_index;
                            if ns_match {
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
            targets: current_nodes.into_iter().map(|id| BrowsePathTarget {
                target_id: id,
                remaining_path_index: 0,
            }).collect(),
        }
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

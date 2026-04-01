//! Address Space - the node store for OPC UA servers.
//!
//! The address space is the primary container for all nodes and references.

use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, Ordering};
use tracing::{debug, info, instrument, warn};

use super::base::{LocalizedText, Node, QualifiedName, SharedNode};
use super::classes::{DataTypeNode, ObjectNode, ObjectTypeNode, VariableNode};
use super::reference::{BrowseDirection, BrowseResult, Reference, ReferenceTypeId};
use crate::error::{OpcUaError, OpcUaResult};
#[cfg(feature = "experimental-namespace-api")]
use crate::namespace::{adapt_namespace_manager_plugin, NamespaceManagerPlugin};
use crate::sdk::address_space::{
    AddressSpaceRuntime, AttributeAccessPort, BrowsePathPort, BrowsePort, DiagnosticsSnapshot,
    NamespaceDiagnosticsState, NodeManager, TypeHierarchyPort,
};
use crate::types::{AttributeId, DataValue, NodeId, StatusCode, Variant};

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
    /// Internal canonical runtime components.
    runtime: AddressSpaceRuntime,
    /// Atomic counters for stats.
    read_counter: AtomicU64,
    write_counter: AtomicU64,
    browse_counter: AtomicU64,
}

impl AddressSpace {
    /// Create a new address space.
    pub fn new(config: AddressSpaceConfig) -> Self {
        Self::new_with_internal_managers(config, Vec::new())
    }

    #[cfg(feature = "experimental-namespace-api")]
    pub fn new_with_namespace_managers(
        config: AddressSpaceConfig,
        managers: Vec<std::sync::Arc<dyn NamespaceManagerPlugin>>,
    ) -> Self {
        Self::new_with_internal_managers(
            config,
            managers
                .into_iter()
                .map(adapt_namespace_manager_plugin)
                .collect(),
        )
    }

    pub(crate) fn new_with_internal_managers(
        config: AddressSpaceConfig,
        managers: Vec<std::sync::Arc<dyn NodeManager>>,
    ) -> Self {
        let address_space = Self {
            runtime: AddressSpaceRuntime::with_managers(
                config.default_namespace_uri.clone(),
                config.max_nodes,
                managers,
            ),
            config,
            read_counter: AtomicU64::new(0),
            write_counter: AtomicU64::new(0),
            browse_counter: AtomicU64::new(0),
        };

        // Initialize standard nodes if enabled
        if address_space.config.enable_standard_namespace {
            address_space.init_standard_nodes();
        }

        address_space.runtime.materialize_managers(&address_space);

        address_space
    }

    /// Initialize standard OPC UA nodes.
    fn init_standard_nodes(&self) {
        // Root folder
        let root = ObjectNode::new(NodeId::root_folder(), QualifiedName::null("Root"), "Root");
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
        let server = ObjectNode::new(NodeId::server(), QualifiedName::null("Server"), "Server")
            .with_event_notifier(1);
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
        let add_type =
            |store: &AddressSpace, id: u32, name: &str, parent_id: u32, is_abstract: bool| {
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
        add_type(self, 22, "Structure", 24, true); // abstract
        add_type(self, 29, "Enumeration", 24, true); // abstract
        add_type(self, 26, "Number", 24, true); // abstract

        // Number subtypes
        add_type(self, 27, "Integer", 26, true); // abstract
        add_type(self, 28, "UInteger", 26, true); // abstract
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
        let base_event =
            ObjectTypeNode::new(NodeId::numeric(0, 2041), "BaseEventType", "BaseEventType")
                .with_is_abstract(true);
        self.insert_node(base_event);
        self.add_reference(Reference::organizes(
            NodeId::types_folder(), // i=86
            NodeId::numeric(0, 2041),
        ));

        // Standard event properties (HasProperty from BaseEventType)
        let event_properties: &[(u32, &str, u32)] = &[
            (2042, "EventId", 15),     // ByteString
            (2043, "EventType", 17),   // NodeId
            (2044, "SourceNode", 17),  // NodeId
            (2045, "SourceName", 12),  // String
            (2046, "Time", 13),        // DateTime
            (2047, "ReceiveTime", 13), // DateTime
            (2050, "Message", 21),     // LocalizedText
            (2051, "Severity", 5),     // UInt16
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
            let event_type = ObjectTypeNode::new(NodeId::numeric(0, id), name, name);
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
        self.runtime.register_namespace(uri)
    }

    /// Get namespace URI by index.
    pub fn get_namespace_uri(&self, index: u16) -> Option<String> {
        self.runtime.get_namespace_uri(index)
    }

    /// Get namespace index by URI.
    pub fn get_namespace_index(&self, uri: &str) -> Option<u16> {
        self.runtime.get_namespace_index(uri)
    }

    /// Get all namespace URIs.
    pub fn namespaces(&self) -> Vec<String> {
        self.runtime.namespaces()
    }

    pub(crate) fn refresh_diagnostics(&self, snapshot: &DiagnosticsSnapshot) {
        self.runtime.refresh_diagnostics(self, snapshot);
    }

    pub(crate) fn on_runtime_start(&self, snapshot: &DiagnosticsSnapshot) {
        self.runtime.on_runtime_start(self, snapshot);
    }

    pub(crate) fn on_runtime_stop(&self, snapshot: &DiagnosticsSnapshot) {
        self.runtime.on_runtime_stop(self, snapshot);
    }

    pub(crate) fn namespace_diagnostics_state(&self) -> Vec<NamespaceDiagnosticsState> {
        self.runtime.namespace_diagnostics_state()
    }

    pub(crate) fn diagnostics_namespace_index(&self) -> Option<u16> {
        self.runtime.diagnostics_namespace_index()
    }

    pub(crate) fn manager_ownership_summary(&self) -> Vec<String> {
        self.runtime.manager_ownership_summary()
    }

    // =========================================================================
    // Node Operations
    // =========================================================================

    /// Insert a node into the address space.
    pub fn insert_node<N: Node + 'static>(&self, node: N) -> bool {
        let inserted = self.runtime.insert_node(node);
        if !inserted {
            warn!("Address space is full or node already exists, cannot insert node");
        }
        inserted
    }

    /// Insert a boxed node.
    pub fn insert_boxed_node(&self, node: Box<dyn Node>) -> bool {
        self.runtime.insert_boxed_node(node)
    }

    /// Get a node by ID.
    pub fn get_node(&self, node_id: &NodeId) -> Option<SharedNode> {
        self.runtime.get_node(node_id)
    }

    /// Check if a node exists.
    pub fn contains_node(&self, node_id: &NodeId) -> bool {
        self.runtime.contains_node(node_id)
    }

    /// Remove a node.
    pub fn remove_node(&self, node_id: &NodeId) -> bool {
        self.runtime.remove_node(node_id)
    }

    /// Get the number of nodes.
    pub fn node_count(&self) -> usize {
        self.runtime.node_count()
    }

    /// Get all node IDs.
    pub fn node_ids(&self) -> Vec<NodeId> {
        self.runtime.node_ids()
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
        let variable =
            VariableNode::new(node_id.clone(), browse_name, display_name, data_type, value);

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
        let variable =
            VariableNode::new(node_id.clone(), browse_name, display_name, data_type, value)
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
        self.runtime.add_reference(reference);
    }

    /// Remove a reference.
    pub fn remove_reference(
        &self,
        source: &NodeId,
        reference_type: ReferenceTypeId,
        target: &NodeId,
    ) -> bool {
        self.runtime
            .remove_reference(source, reference_type, target)
    }

    /// Get references from a node.
    pub fn get_references(&self, node_id: &NodeId, direction: BrowseDirection) -> Vec<Reference> {
        self.runtime.get_references(node_id, direction)
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
        self.runtime.browse(
            self,
            node_id,
            direction,
            reference_type_filter,
            include_subtypes,
            node_class_mask,
            max_results,
        )
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
        self.runtime
            .browse_next(continuation_point, release, max_results)
    }

    /// Release a browse continuation point without returning results.
    pub fn release_continuation_point(&self, continuation_point: &[u8]) {
        self.runtime.release_continuation_point(continuation_point);
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
        self.runtime.resolve_browse_path(self, starting_node, elements)
    }

    // =========================================================================
    // Read/Write Operations
    // =========================================================================

    /// Read an attribute from a node.
    pub fn read(&self, node_id: &NodeId, attribute_id: AttributeId) -> DataValue {
        self.read_counter.fetch_add(1, Ordering::Relaxed);
        self.runtime.read(self, node_id, attribute_id)
    }

    /// Read the value attribute from a variable node.
    pub fn read_value(&self, node_id: &NodeId) -> DataValue {
        self.read(node_id, AttributeId::Value)
    }

    /// Write an attribute to a node.
    pub fn write(
        &self,
        node_id: &NodeId,
        attribute_id: AttributeId,
        value: DataValue,
    ) -> StatusCode {
        self.write_counter.fetch_add(1, Ordering::Relaxed);
        self.runtime.write(self, node_id, attribute_id, value)
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
        node_ids.iter().map(|id| self.read_value(id)).collect()
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
        stats.total_nodes = self.runtime.node_count() as u64;
        stats.total_references = self.runtime.total_references();
        let (variable_nodes, object_nodes, method_nodes) = self.runtime.node_class_counts();
        stats.variable_nodes = variable_nodes;
        stats.object_nodes = object_nodes;
        stats.method_nodes = method_nodes;

        stats.reads = self.read_counter.load(Ordering::Relaxed);
        stats.writes = self.write_counter.load(Ordering::Relaxed);
        stats.browses = self.browse_counter.load(Ordering::Relaxed);

        stats
    }
}

impl AttributeAccessPort for AddressSpace {
    fn read_attribute(&self, node_id: &NodeId, attribute_id: AttributeId) -> DataValue {
        AddressSpace::read(self, node_id, attribute_id)
    }

    fn write_attribute(
        &self,
        node_id: &NodeId,
        attribute_id: AttributeId,
        value: DataValue,
    ) -> StatusCode {
        AddressSpace::write(self, node_id, attribute_id, value)
    }
}

impl BrowsePort for AddressSpace {
    fn get_references(&self, node_id: &NodeId, direction: BrowseDirection) -> Vec<Reference> {
        AddressSpace::get_references(self, node_id, direction)
    }

    fn browse(
        &self,
        node_id: &NodeId,
        direction: BrowseDirection,
        reference_type_filter: Option<ReferenceTypeId>,
        include_subtypes: bool,
        node_class_mask: Option<u32>,
        max_results: usize,
    ) -> BrowseResult {
        AddressSpace::browse(
            self,
            node_id,
            direction,
            reference_type_filter,
            include_subtypes,
            node_class_mask,
            max_results,
        )
    }

    fn browse_next(
        &self,
        continuation_point: &[u8],
        release: bool,
        max_results: usize,
    ) -> BrowseResult {
        AddressSpace::browse_next(self, continuation_point, release, max_results)
    }

    fn release_continuation_point(&self, continuation_point: &[u8]) {
        AddressSpace::release_continuation_point(self, continuation_point)
    }
}

impl BrowsePathPort for AddressSpace {
    fn resolve_browse_path(
        &self,
        starting_node: &NodeId,
        elements: &[RelativePathElement],
    ) -> BrowsePathResult {
        AddressSpace::resolve_browse_path(self, starting_node, elements)
    }
}

impl TypeHierarchyPort for AddressSpace {
    fn is_reference_subtype(&self, candidate: &ReferenceTypeId, parent: &ReferenceTypeId) -> bool {
        self.runtime.is_reference_subtype(self, candidate, parent)
    }

    fn is_node_subtype_of(&self, candidate: &NodeId, parent: &NodeId) -> bool {
        self.runtime.is_node_subtype_of(self, candidate, parent)
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
        let refs =
            address_space.get_references(&NodeId::objects_folder(), BrowseDirection::Forward);
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
        let result =
            address_space.browse(&folder_id, BrowseDirection::Forward, None, false, None, 100);

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

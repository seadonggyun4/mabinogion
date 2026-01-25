//! Node builders for fluent API node creation.
//!
//! These builders provide a convenient way to construct nodes with
//! all their attributes configured properly.

use crate::types::{NodeId, AccessLevel, Variant, variant::DataTypeId};
use super::base::{QualifiedName, LocalizedText};
use super::base::Node;
use super::classes::{ObjectNode, VariableNode};
use super::reference::{Reference, ReferenceTypeId};
use super::store::AddressSpace;
use crate::error::OpcUaResult;

/// Generic node builder trait.
pub trait NodeBuilder {
    /// The node type being built.
    type Node;

    /// Build the node.
    fn build(self) -> Self::Node;
}

/// Builder for variable nodes.
///
/// # Examples
///
/// ```ignore
/// let variable = VariableBuilder::new(NodeId::numeric(2, 1001))
///     .browse_name(2, "Temperature")
///     .display_name("Temperature Sensor")
///     .description("Ambient temperature measurement")
///     .data_type(DataTypeId::Double)
///     .value(25.5f64)
///     .writable()
///     .historizing()
///     .build();
/// ```
#[derive(Debug)]
pub struct VariableBuilder {
    node_id: NodeId,
    browse_name: Option<QualifiedName>,
    display_name: Option<LocalizedText>,
    description: Option<LocalizedText>,
    data_type: NodeId,
    value: Variant,
    value_rank: i32,
    array_dimensions: Option<Vec<u32>>,
    access_level: AccessLevel,
    minimum_sampling_interval: f64,
    historizing: bool,
}

impl VariableBuilder {
    /// Create a new variable builder.
    pub fn new(node_id: NodeId) -> Self {
        Self {
            node_id,
            browse_name: None,
            display_name: None,
            description: None,
            data_type: NodeId::numeric(0, DataTypeId::Double as u32),
            value: Variant::null(),
            value_rank: -1,
            array_dimensions: None,
            access_level: AccessLevel::CURRENT_READ,
            minimum_sampling_interval: 0.0,
            historizing: false,
        }
    }

    /// Set the browse name.
    pub fn browse_name(mut self, namespace: u16, name: impl Into<String>) -> Self {
        self.browse_name = Some(QualifiedName::new(namespace, name));
        self
    }

    /// Set the display name.
    pub fn display_name(mut self, name: impl Into<String>) -> Self {
        self.display_name = Some(LocalizedText::invariant(name));
        self
    }

    /// Set the description.
    pub fn description(mut self, desc: impl Into<String>) -> Self {
        self.description = Some(LocalizedText::invariant(desc));
        self
    }

    /// Set the data type using DataTypeId enum.
    pub fn data_type(mut self, data_type: DataTypeId) -> Self {
        self.data_type = NodeId::numeric(0, data_type as u32);
        self
    }

    /// Set the data type using a NodeId.
    pub fn data_type_node(mut self, data_type: NodeId) -> Self {
        self.data_type = data_type;
        self
    }

    /// Set the initial value.
    pub fn value(mut self, value: impl Into<Variant>) -> Self {
        self.value = value.into();
        self
    }

    /// Configure as an array.
    pub fn array(mut self, value_rank: i32, dimensions: Option<Vec<u32>>) -> Self {
        self.value_rank = value_rank;
        self.array_dimensions = dimensions;
        self
    }

    /// Set access level.
    pub fn access_level(mut self, level: AccessLevel) -> Self {
        self.access_level = level;
        self
    }

    /// Make writable.
    pub fn writable(mut self) -> Self {
        self.access_level = AccessLevel::READ_WRITE;
        self
    }

    /// Enable historizing.
    pub fn historizing(mut self) -> Self {
        self.historizing = true;
        self.access_level = self.access_level | AccessLevel::HISTORY_READ;
        self
    }

    /// Set minimum sampling interval.
    pub fn sampling_interval(mut self, interval_ms: f64) -> Self {
        self.minimum_sampling_interval = interval_ms;
        self
    }
}

impl NodeBuilder for VariableBuilder {
    type Node = VariableNode;

    fn build(self) -> VariableNode {
        let browse_name = self.browse_name.unwrap_or_else(|| {
            QualifiedName::new(self.node_id.namespace(), "Variable")
        });
        let display_name = self.display_name.unwrap_or_else(|| {
            LocalizedText::invariant(&browse_name.name)
        });

        let mut variable = VariableNode::new(
            self.node_id,
            browse_name,
            display_name,
            self.data_type,
            self.value,
        )
        .with_access_level(self.access_level)
        .with_minimum_sampling_interval(self.minimum_sampling_interval)
        .with_historizing(self.historizing)
        .with_array(self.value_rank, self.array_dimensions);

        if let Some(desc) = self.description {
            variable = variable.with_description(desc);
        }

        variable
    }
}

/// Builder for object nodes.
#[derive(Debug)]
pub struct ObjectBuilder {
    node_id: NodeId,
    browse_name: Option<QualifiedName>,
    display_name: Option<LocalizedText>,
    description: Option<LocalizedText>,
    event_notifier: u8,
}

impl ObjectBuilder {
    /// Create a new object builder.
    pub fn new(node_id: NodeId) -> Self {
        Self {
            node_id,
            browse_name: None,
            display_name: None,
            description: None,
            event_notifier: 0,
        }
    }

    /// Set the browse name.
    pub fn browse_name(mut self, namespace: u16, name: impl Into<String>) -> Self {
        self.browse_name = Some(QualifiedName::new(namespace, name));
        self
    }

    /// Set the display name.
    pub fn display_name(mut self, name: impl Into<String>) -> Self {
        self.display_name = Some(LocalizedText::invariant(name));
        self
    }

    /// Set the description.
    pub fn description(mut self, desc: impl Into<String>) -> Self {
        self.description = Some(LocalizedText::invariant(desc));
        self
    }

    /// Set event notifier.
    pub fn event_notifier(mut self, notifier: u8) -> Self {
        self.event_notifier = notifier;
        self
    }
}

impl NodeBuilder for ObjectBuilder {
    type Node = ObjectNode;

    fn build(self) -> ObjectNode {
        let browse_name = self.browse_name.unwrap_or_else(|| {
            QualifiedName::new(self.node_id.namespace(), "Object")
        });
        let display_name = self.display_name.unwrap_or_else(|| {
            LocalizedText::invariant(&browse_name.name)
        });

        let mut object = ObjectNode::new(self.node_id, browse_name, display_name)
            .with_event_notifier(self.event_notifier);

        if let Some(desc) = self.description {
            object = object.with_description(desc);
        }

        object
    }
}

/// Builder for folder nodes (specialized object).
#[derive(Debug)]
pub struct FolderBuilder {
    inner: ObjectBuilder,
}

impl FolderBuilder {
    /// Create a new folder builder.
    pub fn new(node_id: NodeId) -> Self {
        Self {
            inner: ObjectBuilder::new(node_id),
        }
    }

    /// Set the browse name.
    pub fn browse_name(mut self, namespace: u16, name: impl Into<String>) -> Self {
        self.inner = self.inner.browse_name(namespace, name);
        self
    }

    /// Set the display name.
    pub fn display_name(mut self, name: impl Into<String>) -> Self {
        self.inner = self.inner.display_name(name);
        self
    }

    /// Set the description.
    pub fn description(mut self, desc: impl Into<String>) -> Self {
        self.inner = self.inner.description(desc);
        self
    }
}

impl NodeBuilder for FolderBuilder {
    type Node = ObjectNode;

    fn build(self) -> ObjectNode {
        self.inner.build()
    }
}

/// Batch variable builder for creating many variables efficiently.
///
/// # Examples
///
/// ```ignore
/// let variables = BatchVariableBuilder::new(2)
///     .add("Temperature", DataTypeId::Double, 25.5f64)
///     .add("Pressure", DataTypeId::Double, 101.3f64)
///     .add("Humidity", DataTypeId::Double, 65.0f64)
///     .build_all();
/// ```
#[derive(Debug)]
pub struct BatchVariableBuilder {
    namespace: u16,
    start_id: u32,
    variables: Vec<VariableSpec>,
}

#[derive(Debug)]
struct VariableSpec {
    name: String,
    data_type: DataTypeId,
    value: Variant,
    writable: bool,
}

impl BatchVariableBuilder {
    /// Create a new batch builder.
    pub fn new(namespace: u16) -> Self {
        Self {
            namespace,
            start_id: 1000,
            variables: Vec::new(),
        }
    }

    /// Set the starting node ID.
    pub fn start_id(mut self, id: u32) -> Self {
        self.start_id = id;
        self
    }

    /// Add a variable.
    pub fn add(
        mut self,
        name: impl Into<String>,
        data_type: DataTypeId,
        value: impl Into<Variant>,
    ) -> Self {
        self.variables.push(VariableSpec {
            name: name.into(),
            data_type,
            value: value.into(),
            writable: false,
        });
        self
    }

    /// Add a writable variable.
    pub fn add_writable(
        mut self,
        name: impl Into<String>,
        data_type: DataTypeId,
        value: impl Into<Variant>,
    ) -> Self {
        self.variables.push(VariableSpec {
            name: name.into(),
            data_type,
            value: value.into(),
            writable: true,
        });
        self
    }

    /// Build all variables.
    pub fn build_all(self) -> Vec<VariableNode> {
        self.variables
            .into_iter()
            .enumerate()
            .map(|(i, spec)| {
                let node_id = NodeId::numeric(self.namespace, self.start_id + i as u32);
                let mut builder = VariableBuilder::new(node_id)
                    .browse_name(self.namespace, &spec.name)
                    .display_name(&spec.name)
                    .data_type(spec.data_type)
                    .value(spec.value);

                if spec.writable {
                    builder = builder.writable();
                }

                builder.build()
            })
            .collect()
    }

    /// Build and add to address space.
    pub fn build_into(
        self,
        address_space: &AddressSpace,
        parent_id: &NodeId,
    ) -> OpcUaResult<Vec<NodeId>> {
        let variables = self.build_all();
        let mut node_ids = Vec::with_capacity(variables.len());

        for variable in variables {
            let node_id = variable.base.node_id.clone();
            address_space.insert_node(variable);
            address_space.add_reference(Reference::has_component(
                parent_id.clone(),
                node_id.clone(),
            ));
            node_ids.push(node_id);
        }

        Ok(node_ids)
    }
}

/// Helper trait for adding nodes to address space.
pub trait AddToAddressSpace {
    /// Add this node to the address space with a reference from the parent.
    fn add_to(
        self,
        address_space: &AddressSpace,
        parent_id: &NodeId,
        reference_type: ReferenceTypeId,
    ) -> OpcUaResult<NodeId>;
}

impl<B: NodeBuilder> AddToAddressSpace for B
where
    B::Node: super::base::Node + 'static,
{
    fn add_to(
        self,
        address_space: &AddressSpace,
        parent_id: &NodeId,
        reference_type: ReferenceTypeId,
    ) -> OpcUaResult<NodeId> {
        let node = self.build();
        let node_id = node.node_id().clone();

        if !address_space.insert_node(node) {
            return Err(crate::error::OpcUaError::Server(format!(
                "Failed to add node: {}",
                node_id
            )));
        }

        address_space.add_reference(Reference::forward(
            parent_id.clone(),
            reference_type,
            node_id.clone(),
        ));

        Ok(node_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_variable_builder() {
        let variable = VariableBuilder::new(NodeId::numeric(2, 1001))
            .browse_name(2, "Temperature")
            .display_name("Temperature Sensor")
            .description("Ambient temperature")
            .data_type(DataTypeId::Double)
            .value(25.5f64)
            .writable()
            .sampling_interval(100.0)
            .build();

        assert_eq!(variable.base.browse_name.name, "Temperature");
        assert!(variable.is_writable());
        assert_eq!(variable.minimum_sampling_interval, 100.0);
    }

    #[test]
    fn test_object_builder() {
        let object = ObjectBuilder::new(NodeId::numeric(2, 1000))
            .browse_name(2, "Device")
            .display_name("My Device")
            .description("A test device")
            .build();

        assert_eq!(object.base.browse_name.name, "Device");
    }

    #[test]
    fn test_batch_variable_builder() {
        let variables = BatchVariableBuilder::new(2)
            .start_id(1000)
            .add("Temp1", DataTypeId::Double, 25.0f64)
            .add("Temp2", DataTypeId::Double, 26.0f64)
            .add_writable("Setpoint", DataTypeId::Double, 22.0f64)
            .build_all();

        assert_eq!(variables.len(), 3);
        assert!(!variables[0].is_writable());
        assert!(!variables[1].is_writable());
        assert!(variables[2].is_writable());
    }
}

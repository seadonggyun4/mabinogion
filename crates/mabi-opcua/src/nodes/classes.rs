//! Concrete node class implementations.
//!
//! This module provides implementations for all OPC UA node classes.

use serde::{Deserialize, Serialize};

use super::base::{LocalizedText, Node, NodeBase, NodeClass, QualifiedName};
use crate::types::{
    variant::DataTypeId, AccessLevel, AttributeId, DataValue, NodeId, StatusCode, Variant,
    WriteMask,
};

// =============================================================================
// Object Node
// =============================================================================

/// Object node - container for variables and methods.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ObjectNode {
    /// Base node attributes.
    pub base: NodeBase,
    /// Event notifier (indicates event capabilities).
    pub event_notifier: u8,
}

impl ObjectNode {
    /// Create a new object node.
    pub fn new(
        node_id: NodeId,
        browse_name: impl Into<QualifiedName>,
        display_name: impl Into<LocalizedText>,
    ) -> Self {
        Self {
            base: NodeBase::new(node_id, NodeClass::Object, browse_name, display_name),
            event_notifier: 0,
        }
    }

    /// Set event notifier.
    pub fn with_event_notifier(mut self, event_notifier: u8) -> Self {
        self.event_notifier = event_notifier;
        self
    }

    /// Set description.
    pub fn with_description(mut self, description: impl Into<LocalizedText>) -> Self {
        self.base.description = description.into();
        self
    }
}

impl Node for ObjectNode {
    fn node_id(&self) -> &NodeId {
        &self.base.node_id
    }

    fn node_class(&self) -> NodeClass {
        NodeClass::Object
    }

    fn browse_name(&self) -> &QualifiedName {
        &self.base.browse_name
    }

    fn display_name(&self) -> &LocalizedText {
        &self.base.display_name
    }

    fn description(&self) -> &LocalizedText {
        &self.base.description
    }

    fn write_mask(&self) -> WriteMask {
        self.base.write_mask
    }

    fn user_write_mask(&self) -> WriteMask {
        self.base.user_write_mask
    }

    fn read_attribute(&self, attribute_id: AttributeId) -> DataValue {
        match attribute_id {
            AttributeId::EventNotifier => DataValue::new(Variant::byte(self.event_notifier)),
            _ => Node::read_attribute(self, attribute_id),
        }
    }

    fn clone_box(&self) -> Box<dyn Node> {
        Box::new(self.clone())
    }
}

// =============================================================================
// Variable Node
// =============================================================================

/// Variable node - holds a value that can be read/written.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VariableNode {
    /// Base node attributes.
    pub base: NodeBase,
    /// The current value.
    pub value: DataValue,
    /// Data type of the value.
    pub data_type: NodeId,
    /// Value rank (-1 = scalar, 0+ = array dimensions).
    pub value_rank: i32,
    /// Array dimensions (for arrays).
    pub array_dimensions: Option<Vec<u32>>,
    /// Access level (read/write permissions).
    pub access_level: AccessLevel,
    /// User access level.
    pub user_access_level: AccessLevel,
    /// Minimum sampling interval in milliseconds.
    pub minimum_sampling_interval: f64,
    /// Whether the variable is historizing.
    pub historizing: bool,
}

impl VariableNode {
    /// Create a new variable node.
    pub fn new(
        node_id: NodeId,
        browse_name: impl Into<QualifiedName>,
        display_name: impl Into<LocalizedText>,
        data_type: NodeId,
        value: impl Into<Variant>,
    ) -> Self {
        Self {
            base: NodeBase::new(node_id, NodeClass::Variable, browse_name, display_name),
            value: DataValue::new(value.into()),
            data_type,
            value_rank: -1, // Scalar by default
            array_dimensions: None,
            access_level: AccessLevel::CURRENT_READ,
            user_access_level: AccessLevel::CURRENT_READ,
            minimum_sampling_interval: 0.0,
            historizing: false,
        }
    }

    /// Create a new variable node with a specific data type ID.
    pub fn with_type_id(
        node_id: NodeId,
        browse_name: impl Into<QualifiedName>,
        display_name: impl Into<LocalizedText>,
        data_type_id: DataTypeId,
        value: impl Into<Variant>,
    ) -> Self {
        // Map DataTypeId to NodeId (namespace 0)
        let data_type = NodeId::numeric(0, data_type_id as u32);
        Self::new(node_id, browse_name, display_name, data_type, value)
    }

    /// Set value.
    pub fn with_value(mut self, value: impl Into<Variant>) -> Self {
        self.value = DataValue::new(value.into());
        self
    }

    /// Set description.
    pub fn with_description(mut self, description: impl Into<LocalizedText>) -> Self {
        self.base.description = description.into();
        self
    }

    /// Set access level.
    pub fn with_access_level(mut self, access_level: AccessLevel) -> Self {
        self.access_level = access_level;
        self.user_access_level = access_level;
        self
    }

    /// Make writable.
    pub fn writable(mut self) -> Self {
        self.access_level = AccessLevel::READ_WRITE;
        self.user_access_level = AccessLevel::READ_WRITE;
        self
    }

    /// Enable historizing.
    pub fn with_historizing(mut self, historizing: bool) -> Self {
        self.historizing = historizing;
        if historizing {
            self.access_level = self.access_level | AccessLevel::HISTORY_READ;
            self.user_access_level = self.user_access_level | AccessLevel::HISTORY_READ;
        }
        self
    }

    /// Set minimum sampling interval.
    pub fn with_minimum_sampling_interval(mut self, interval_ms: f64) -> Self {
        self.minimum_sampling_interval = interval_ms;
        self
    }

    /// Set value rank for arrays.
    pub fn with_array(mut self, value_rank: i32, dimensions: Option<Vec<u32>>) -> Self {
        self.value_rank = value_rank;
        self.array_dimensions = dimensions;
        self
    }

    /// Get the current value.
    pub fn get_value(&self) -> &DataValue {
        &self.value
    }

    /// Set the current value.
    pub fn set_value(&mut self, value: impl Into<Variant>) {
        self.value = DataValue::new(value.into());
    }

    /// Update the value with a DataValue.
    pub fn update_value(&mut self, data_value: DataValue) {
        self.value = data_value;
    }

    /// Check if the variable is writable.
    pub fn is_writable(&self) -> bool {
        self.access_level.can_write()
    }

    /// Check if the variable is readable.
    pub fn is_readable(&self) -> bool {
        self.access_level.can_read()
    }
}

impl Node for VariableNode {
    fn node_id(&self) -> &NodeId {
        &self.base.node_id
    }

    fn node_class(&self) -> NodeClass {
        NodeClass::Variable
    }

    fn browse_name(&self) -> &QualifiedName {
        &self.base.browse_name
    }

    fn display_name(&self) -> &LocalizedText {
        &self.base.display_name
    }

    fn description(&self) -> &LocalizedText {
        &self.base.description
    }

    fn write_mask(&self) -> WriteMask {
        self.base.write_mask
    }

    fn user_write_mask(&self) -> WriteMask {
        self.base.user_write_mask
    }

    fn read_attribute(&self, attribute_id: AttributeId) -> DataValue {
        match attribute_id {
            AttributeId::Value => self.value.clone(),
            AttributeId::DataType => DataValue::new(Variant::node_id(self.data_type.clone())),
            AttributeId::ValueRank => DataValue::new(Variant::int32(self.value_rank)),
            AttributeId::ArrayDimensions => match &self.array_dimensions {
                Some(dims) => DataValue::new(Variant::uint32_array(dims.clone())),
                None => DataValue::null(),
            },
            AttributeId::AccessLevel => DataValue::new(Variant::byte(self.access_level.raw())),
            AttributeId::UserAccessLevel => {
                DataValue::new(Variant::byte(self.user_access_level.raw()))
            }
            AttributeId::MinimumSamplingInterval => {
                DataValue::new(Variant::double(self.minimum_sampling_interval))
            }
            AttributeId::Historizing => DataValue::new(Variant::boolean(self.historizing)),
            _ => Node::read_attribute(self, attribute_id),
        }
    }

    fn write_attribute(&mut self, attribute_id: AttributeId, value: DataValue) -> StatusCode {
        match attribute_id {
            AttributeId::Value => {
                if !self.access_level.can_write() {
                    return StatusCode::BAD_NOT_WRITABLE;
                }
                if let Some(v) = value.value() {
                    self.value = DataValue::new(v.clone());
                    self.value.update_timestamps();
                    StatusCode::GOOD
                } else {
                    StatusCode::BAD_INVALID_ARGUMENT
                }
            }
            _ => StatusCode::BAD_NOT_WRITABLE,
        }
    }

    fn clone_box(&self) -> Box<dyn Node> {
        Box::new(self.clone())
    }
}

// =============================================================================
// Method Node
// =============================================================================

/// Method node - callable function on an object.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MethodNode {
    /// Base node attributes.
    pub base: NodeBase,
    /// Whether the method is executable.
    pub executable: bool,
    /// Whether the user can execute the method.
    pub user_executable: bool,
}

impl MethodNode {
    /// Create a new method node.
    pub fn new(
        node_id: NodeId,
        browse_name: impl Into<QualifiedName>,
        display_name: impl Into<LocalizedText>,
    ) -> Self {
        Self {
            base: NodeBase::new(node_id, NodeClass::Method, browse_name, display_name),
            executable: true,
            user_executable: true,
        }
    }

    /// Set executable state.
    pub fn with_executable(mut self, executable: bool) -> Self {
        self.executable = executable;
        self.user_executable = executable;
        self
    }

    /// Set description.
    pub fn with_description(mut self, description: impl Into<LocalizedText>) -> Self {
        self.base.description = description.into();
        self
    }
}

impl Node for MethodNode {
    fn node_id(&self) -> &NodeId {
        &self.base.node_id
    }

    fn node_class(&self) -> NodeClass {
        NodeClass::Method
    }

    fn browse_name(&self) -> &QualifiedName {
        &self.base.browse_name
    }

    fn display_name(&self) -> &LocalizedText {
        &self.base.display_name
    }

    fn description(&self) -> &LocalizedText {
        &self.base.description
    }

    fn write_mask(&self) -> WriteMask {
        self.base.write_mask
    }

    fn user_write_mask(&self) -> WriteMask {
        self.base.user_write_mask
    }

    fn read_attribute(&self, attribute_id: AttributeId) -> DataValue {
        match attribute_id {
            AttributeId::Executable => DataValue::new(Variant::boolean(self.executable)),
            AttributeId::UserExecutable => DataValue::new(Variant::boolean(self.user_executable)),
            _ => Node::read_attribute(self, attribute_id),
        }
    }

    fn clone_box(&self) -> Box<dyn Node> {
        Box::new(self.clone())
    }
}

// =============================================================================
// Type Definition Nodes
// =============================================================================

/// ObjectType node - type definition for objects.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ObjectTypeNode {
    /// Base node attributes.
    pub base: NodeBase,
    /// Whether the type is abstract.
    pub is_abstract: bool,
}

impl ObjectTypeNode {
    /// Create a new object type node.
    pub fn new(
        node_id: NodeId,
        browse_name: impl Into<QualifiedName>,
        display_name: impl Into<LocalizedText>,
    ) -> Self {
        Self {
            base: NodeBase::new(node_id, NodeClass::ObjectType, browse_name, display_name),
            is_abstract: false,
        }
    }

    /// Set abstract state.
    pub fn with_is_abstract(mut self, is_abstract: bool) -> Self {
        self.is_abstract = is_abstract;
        self
    }
}

impl Node for ObjectTypeNode {
    fn node_id(&self) -> &NodeId {
        &self.base.node_id
    }

    fn node_class(&self) -> NodeClass {
        NodeClass::ObjectType
    }

    fn browse_name(&self) -> &QualifiedName {
        &self.base.browse_name
    }

    fn display_name(&self) -> &LocalizedText {
        &self.base.display_name
    }

    fn description(&self) -> &LocalizedText {
        &self.base.description
    }

    fn write_mask(&self) -> WriteMask {
        self.base.write_mask
    }

    fn user_write_mask(&self) -> WriteMask {
        self.base.user_write_mask
    }

    fn read_attribute(&self, attribute_id: AttributeId) -> DataValue {
        match attribute_id {
            AttributeId::IsAbstract => DataValue::new(Variant::boolean(self.is_abstract)),
            _ => Node::read_attribute(self, attribute_id),
        }
    }

    fn clone_box(&self) -> Box<dyn Node> {
        Box::new(self.clone())
    }
}

/// VariableType node - type definition for variables.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VariableTypeNode {
    /// Base node attributes.
    pub base: NodeBase,
    /// Default value for variables of this type.
    pub value: Option<DataValue>,
    /// Data type.
    pub data_type: NodeId,
    /// Value rank.
    pub value_rank: i32,
    /// Array dimensions.
    pub array_dimensions: Option<Vec<u32>>,
    /// Whether the type is abstract.
    pub is_abstract: bool,
}

impl VariableTypeNode {
    /// Create a new variable type node.
    pub fn new(
        node_id: NodeId,
        browse_name: impl Into<QualifiedName>,
        display_name: impl Into<LocalizedText>,
        data_type: NodeId,
    ) -> Self {
        Self {
            base: NodeBase::new(node_id, NodeClass::VariableType, browse_name, display_name),
            value: None,
            data_type,
            value_rank: -1,
            array_dimensions: None,
            is_abstract: false,
        }
    }
}

impl Node for VariableTypeNode {
    fn node_id(&self) -> &NodeId {
        &self.base.node_id
    }

    fn node_class(&self) -> NodeClass {
        NodeClass::VariableType
    }

    fn browse_name(&self) -> &QualifiedName {
        &self.base.browse_name
    }

    fn display_name(&self) -> &LocalizedText {
        &self.base.display_name
    }

    fn description(&self) -> &LocalizedText {
        &self.base.description
    }

    fn write_mask(&self) -> WriteMask {
        self.base.write_mask
    }

    fn user_write_mask(&self) -> WriteMask {
        self.base.user_write_mask
    }

    fn clone_box(&self) -> Box<dyn Node> {
        Box::new(self.clone())
    }
}

/// ReferenceType node - defines relationship types.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReferenceTypeNode {
    /// Base node attributes.
    pub base: NodeBase,
    /// Whether the reference type is abstract.
    pub is_abstract: bool,
    /// Whether the reference is symmetric.
    pub symmetric: bool,
    /// Inverse name (for non-symmetric references).
    pub inverse_name: Option<LocalizedText>,
}

impl ReferenceTypeNode {
    /// Create a new reference type node.
    pub fn new(
        node_id: NodeId,
        browse_name: impl Into<QualifiedName>,
        display_name: impl Into<LocalizedText>,
    ) -> Self {
        Self {
            base: NodeBase::new(node_id, NodeClass::ReferenceType, browse_name, display_name),
            is_abstract: false,
            symmetric: false,
            inverse_name: None,
        }
    }

    /// Set symmetric.
    pub fn with_symmetric(mut self, symmetric: bool) -> Self {
        self.symmetric = symmetric;
        self
    }

    /// Set inverse name.
    pub fn with_inverse_name(mut self, inverse_name: impl Into<LocalizedText>) -> Self {
        self.inverse_name = Some(inverse_name.into());
        self
    }
}

impl Node for ReferenceTypeNode {
    fn node_id(&self) -> &NodeId {
        &self.base.node_id
    }

    fn node_class(&self) -> NodeClass {
        NodeClass::ReferenceType
    }

    fn browse_name(&self) -> &QualifiedName {
        &self.base.browse_name
    }

    fn display_name(&self) -> &LocalizedText {
        &self.base.display_name
    }

    fn description(&self) -> &LocalizedText {
        &self.base.description
    }

    fn write_mask(&self) -> WriteMask {
        self.base.write_mask
    }

    fn user_write_mask(&self) -> WriteMask {
        self.base.user_write_mask
    }

    fn clone_box(&self) -> Box<dyn Node> {
        Box::new(self.clone())
    }
}

/// DataType node - defines data types.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataTypeNode {
    /// Base node attributes.
    pub base: NodeBase,
    /// Whether the data type is abstract.
    pub is_abstract: bool,
}

impl DataTypeNode {
    /// Create a new data type node.
    pub fn new(
        node_id: NodeId,
        browse_name: impl Into<QualifiedName>,
        display_name: impl Into<LocalizedText>,
    ) -> Self {
        Self {
            base: NodeBase::new(node_id, NodeClass::DataType, browse_name, display_name),
            is_abstract: false,
        }
    }
}

impl Node for DataTypeNode {
    fn node_id(&self) -> &NodeId {
        &self.base.node_id
    }

    fn node_class(&self) -> NodeClass {
        NodeClass::DataType
    }

    fn browse_name(&self) -> &QualifiedName {
        &self.base.browse_name
    }

    fn display_name(&self) -> &LocalizedText {
        &self.base.display_name
    }

    fn description(&self) -> &LocalizedText {
        &self.base.description
    }

    fn write_mask(&self) -> WriteMask {
        self.base.write_mask
    }

    fn user_write_mask(&self) -> WriteMask {
        self.base.user_write_mask
    }

    fn clone_box(&self) -> Box<dyn Node> {
        Box::new(self.clone())
    }
}

/// View node - subset of address space.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ViewNode {
    /// Base node attributes.
    pub base: NodeBase,
    /// Whether the view contains no loops.
    pub contains_no_loops: bool,
    /// Event notifier.
    pub event_notifier: u8,
}

impl ViewNode {
    /// Create a new view node.
    pub fn new(
        node_id: NodeId,
        browse_name: impl Into<QualifiedName>,
        display_name: impl Into<LocalizedText>,
    ) -> Self {
        Self {
            base: NodeBase::new(node_id, NodeClass::View, browse_name, display_name),
            contains_no_loops: true,
            event_notifier: 0,
        }
    }
}

impl Node for ViewNode {
    fn node_id(&self) -> &NodeId {
        &self.base.node_id
    }

    fn node_class(&self) -> NodeClass {
        NodeClass::View
    }

    fn browse_name(&self) -> &QualifiedName {
        &self.base.browse_name
    }

    fn display_name(&self) -> &LocalizedText {
        &self.base.display_name
    }

    fn description(&self) -> &LocalizedText {
        &self.base.description
    }

    fn write_mask(&self) -> WriteMask {
        self.base.write_mask
    }

    fn user_write_mask(&self) -> WriteMask {
        self.base.user_write_mask
    }

    fn clone_box(&self) -> Box<dyn Node> {
        Box::new(self.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_object_node() {
        let node = ObjectNode::new(
            NodeId::numeric(2, 1001),
            QualifiedName::new(2, "MyObject"),
            "My Object",
        );

        assert_eq!(node.node_class(), NodeClass::Object);
        assert_eq!(node.browse_name().name, "MyObject");
    }

    #[test]
    fn test_variable_node() {
        let node = VariableNode::with_type_id(
            NodeId::numeric(2, 1002),
            QualifiedName::new(2, "Temperature"),
            "Temperature",
            DataTypeId::Double,
            25.5f64,
        )
        .writable();

        assert_eq!(node.node_class(), NodeClass::Variable);
        assert!(node.is_writable());
        assert!(node.is_readable());

        let value = node.read_attribute(AttributeId::Value);
        assert!(value.is_good());
    }

    #[test]
    fn test_variable_write() {
        let mut node = VariableNode::with_type_id(
            NodeId::numeric(2, 1002),
            "Temperature",
            "Temperature",
            DataTypeId::Double,
            25.5f64,
        )
        .writable();

        let new_value = DataValue::new(Variant::double(30.0));
        let status = node.write_attribute(AttributeId::Value, new_value);
        assert!(status.is_good());

        assert_eq!(node.value.value().unwrap().as_f64(), Some(30.0));
    }

    #[test]
    fn test_method_node() {
        let node = MethodNode::new(NodeId::numeric(2, 1003), "StartProcess", "Start Process");

        assert_eq!(node.node_class(), NodeClass::Method);
        assert!(node.executable);
    }
}

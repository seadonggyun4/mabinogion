//! Base node types and common traits.
//!
//! This module defines the fundamental node structures that all OPC UA nodes share.

use std::fmt;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::types::{NodeId, AttributeId, WriteMask, Variant, DataValue, StatusCode};

/// Node class enumeration matching OPC UA specification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[repr(u8)]
pub enum NodeClass {
    /// Unspecified node class.
    Unspecified = 0,
    /// Object node - container for variables and methods.
    Object = 1,
    /// Variable node - holds a value.
    Variable = 2,
    /// Method node - callable function.
    Method = 4,
    /// ObjectType node - type definition for objects.
    ObjectType = 8,
    /// VariableType node - type definition for variables.
    VariableType = 16,
    /// ReferenceType node - defines relationship types.
    ReferenceType = 32,
    /// DataType node - defines data types.
    DataType = 64,
    /// View node - subset of address space.
    View = 128,
}

impl NodeClass {
    /// Get from numeric value.
    pub fn from_u8(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::Unspecified),
            1 => Some(Self::Object),
            2 => Some(Self::Variable),
            4 => Some(Self::Method),
            8 => Some(Self::ObjectType),
            16 => Some(Self::VariableType),
            32 => Some(Self::ReferenceType),
            64 => Some(Self::DataType),
            128 => Some(Self::View),
            _ => None,
        }
    }

    /// Get display name.
    pub fn name(&self) -> &'static str {
        match self {
            Self::Unspecified => "Unspecified",
            Self::Object => "Object",
            Self::Variable => "Variable",
            Self::Method => "Method",
            Self::ObjectType => "ObjectType",
            Self::VariableType => "VariableType",
            Self::ReferenceType => "ReferenceType",
            Self::DataType => "DataType",
            Self::View => "View",
        }
    }
}

impl fmt::Display for NodeClass {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.name())
    }
}

/// Qualified name for browsing.
///
/// A qualified name consists of a namespace index and a name string.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct QualifiedName {
    /// Namespace index.
    pub namespace_index: u16,
    /// The name.
    pub name: String,
}

impl QualifiedName {
    /// Create a new qualified name.
    pub fn new(namespace_index: u16, name: impl Into<String>) -> Self {
        Self {
            namespace_index,
            name: name.into(),
        }
    }

    /// Create a qualified name in namespace 0.
    pub fn null(name: impl Into<String>) -> Self {
        Self::new(0, name)
    }

    /// Check if the name is empty.
    pub fn is_null(&self) -> bool {
        self.name.is_empty()
    }
}

impl fmt::Display for QualifiedName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.namespace_index == 0 {
            write!(f, "{}", self.name)
        } else {
            write!(f, "{}:{}", self.namespace_index, self.name)
        }
    }
}

impl<S: Into<String>> From<S> for QualifiedName {
    fn from(s: S) -> Self {
        Self::null(s)
    }
}

/// Localized text for display.
///
/// Contains a locale identifier and the text in that locale.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct LocalizedText {
    /// Locale identifier (e.g., "en-US", "ko-KR").
    pub locale: String,
    /// The text in the specified locale.
    pub text: String,
}

impl LocalizedText {
    /// Create a new localized text.
    pub fn new(locale: impl Into<String>, text: impl Into<String>) -> Self {
        Self {
            locale: locale.into(),
            text: text.into(),
        }
    }

    /// Create a localized text with empty locale (invariant).
    pub fn invariant(text: impl Into<String>) -> Self {
        Self::new("", text)
    }

    /// Check if the text is null/empty.
    pub fn is_null(&self) -> bool {
        self.text.is_empty()
    }
}

impl fmt::Display for LocalizedText {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.text)
    }
}

impl<S: Into<String>> From<S> for LocalizedText {
    fn from(s: S) -> Self {
        Self::invariant(s)
    }
}

/// Base attributes shared by all nodes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeBase {
    /// Node ID (unique identifier).
    pub node_id: NodeId,
    /// Node class.
    pub node_class: NodeClass,
    /// Browse name (qualified name for navigation).
    pub browse_name: QualifiedName,
    /// Display name (human-readable name).
    pub display_name: LocalizedText,
    /// Description.
    pub description: LocalizedText,
    /// Write mask (which attributes can be written).
    pub write_mask: WriteMask,
    /// User write mask (user-specific write permissions).
    pub user_write_mask: WriteMask,
}

impl NodeBase {
    /// Create a new node base with required attributes.
    pub fn new(
        node_id: NodeId,
        node_class: NodeClass,
        browse_name: impl Into<QualifiedName>,
        display_name: impl Into<LocalizedText>,
    ) -> Self {
        let browse_name = browse_name.into();
        let display_name = display_name.into();

        Self {
            node_id,
            node_class,
            browse_name,
            display_name: display_name.clone(),
            description: LocalizedText::invariant(""),
            write_mask: WriteMask::NONE,
            user_write_mask: WriteMask::NONE,
        }
    }

    /// Set description.
    pub fn with_description(mut self, description: impl Into<LocalizedText>) -> Self {
        self.description = description.into();
        self
    }

    /// Set write mask.
    pub fn with_write_mask(mut self, write_mask: WriteMask) -> Self {
        self.write_mask = write_mask;
        self
    }
}

/// Trait for all OPC UA nodes.
///
/// This trait provides common functionality for accessing node attributes.
pub trait Node: Send + Sync {
    /// Get the node ID.
    fn node_id(&self) -> &NodeId;

    /// Get the node class.
    fn node_class(&self) -> NodeClass;

    /// Get the browse name.
    fn browse_name(&self) -> &QualifiedName;

    /// Get the display name.
    fn display_name(&self) -> &LocalizedText;

    /// Get the description.
    fn description(&self) -> &LocalizedText;

    /// Get the write mask.
    fn write_mask(&self) -> WriteMask;

    /// Get the user write mask.
    fn user_write_mask(&self) -> WriteMask;

    /// Read an attribute value.
    fn read_attribute(&self, attribute_id: AttributeId) -> DataValue {
        match attribute_id {
            AttributeId::NodeId => DataValue::new(Variant::node_id(self.node_id().clone())),
            AttributeId::NodeClass => DataValue::new(Variant::uint32(self.node_class() as u32)),
            AttributeId::BrowseName => DataValue::new(Variant::string(self.browse_name().to_string())),
            AttributeId::DisplayName => DataValue::new(Variant::string(self.display_name().to_string())),
            AttributeId::Description => DataValue::new(Variant::string(self.description().to_string())),
            AttributeId::WriteMask => DataValue::new(Variant::uint32(self.write_mask().raw())),
            AttributeId::UserWriteMask => DataValue::new(Variant::uint32(self.user_write_mask().raw())),
            _ => DataValue::bad(StatusCode::BAD_ATTRIBUTE_ID_INVALID),
        }
    }

    /// Write an attribute value.
    fn write_attribute(&mut self, _attribute_id: AttributeId, _value: DataValue) -> StatusCode {
        // Default implementation: not writable
        StatusCode::BAD_NOT_WRITABLE
    }

    /// Clone the node as a boxed trait object.
    fn clone_box(&self) -> Box<dyn Node>;
}

impl Clone for Box<dyn Node> {
    fn clone(&self) -> Self {
        self.clone_box()
    }
}

/// Type alias for a shared node reference.
pub type SharedNode = Arc<parking_lot::RwLock<Box<dyn Node>>>;

/// Create a shared node from a node implementation.
pub fn shared_node<N: Node + 'static>(node: N) -> SharedNode {
    Arc::new(parking_lot::RwLock::new(Box::new(node)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_node_class() {
        assert_eq!(NodeClass::Object as u8, 1);
        assert_eq!(NodeClass::Variable as u8, 2);
        assert_eq!(NodeClass::from_u8(1), Some(NodeClass::Object));
        assert_eq!(NodeClass::from_u8(99), None);
    }

    #[test]
    fn test_qualified_name() {
        let name = QualifiedName::new(2, "Temperature");
        assert_eq!(name.namespace_index, 2);
        assert_eq!(name.to_string(), "2:Temperature");

        let name = QualifiedName::null("Test");
        assert_eq!(name.to_string(), "Test");
    }

    #[test]
    fn test_localized_text() {
        let text = LocalizedText::new("en-US", "Hello");
        assert_eq!(text.locale, "en-US");
        assert_eq!(text.to_string(), "Hello");

        let text = LocalizedText::invariant("World");
        assert!(text.locale.is_empty());
    }

    #[test]
    fn test_node_base() {
        let base = NodeBase::new(
            NodeId::numeric(2, 1001),
            NodeClass::Variable,
            QualifiedName::new(2, "Temperature"),
            "Temperature Sensor",
        )
        .with_description("Ambient temperature measurement");

        assert_eq!(base.node_class, NodeClass::Variable);
        assert_eq!(base.browse_name.name, "Temperature");
        assert!(!base.description.is_null());
    }
}

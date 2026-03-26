//! OPC UA reference system.
//!
//! References define relationships between nodes in the address space.

use serde::{Deserialize, Serialize};
use std::fmt;

use super::base::{LocalizedText, NodeClass, QualifiedName};
use crate::types::NodeId;

/// Standard OPC UA reference type IDs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ReferenceTypeId {
    /// References (base type).
    References,
    /// Non-hierarchical references.
    NonHierarchicalReferences,
    /// Hierarchical references.
    HierarchicalReferences,
    /// HasChild reference.
    HasChild,
    /// Organizes reference (folder contains item).
    Organizes,
    /// HasComponent reference (object has component).
    HasComponent,
    /// HasProperty reference (node has property).
    HasProperty,
    /// HasSubtype reference (type has subtype).
    HasSubtype,
    /// HasTypeDefinition reference (instance has type).
    HasTypeDefinition,
    /// HasModellingRule reference.
    HasModellingRule,
    /// HasEncoding reference.
    HasEncoding,
    /// HasDescription reference.
    HasDescription,
    /// GeneratesEvent reference.
    GeneratesEvent,
    /// AlwaysGeneratesEvent reference.
    AlwaysGeneratesEvent,
    /// HasEventSource reference.
    HasEventSource,
    /// HasNotifier reference.
    HasNotifier,
    /// Aggregates reference.
    Aggregates,
    /// HasOrderedComponent reference.
    HasOrderedComponent,
    /// FromState reference.
    FromState,
    /// ToState reference.
    ToState,
    /// HasCause reference.
    HasCause,
    /// HasEffect reference.
    HasEffect,
    /// HasHistoricalConfiguration reference.
    HasHistoricalConfiguration,
    /// Custom reference type.
    Custom(u32),
}

impl ReferenceTypeId {
    /// Get the NodeId for this reference type.
    pub fn node_id(&self) -> NodeId {
        let id = match self {
            Self::References => 31,
            Self::NonHierarchicalReferences => 32,
            Self::HierarchicalReferences => 33,
            Self::HasChild => 34,
            Self::Organizes => 35,
            Self::HasComponent => 47,
            Self::HasProperty => 46,
            Self::HasSubtype => 45,
            Self::HasTypeDefinition => 40,
            Self::HasModellingRule => 37,
            Self::HasEncoding => 38,
            Self::HasDescription => 39,
            Self::GeneratesEvent => 41,
            Self::AlwaysGeneratesEvent => 3065,
            Self::HasEventSource => 36,
            Self::HasNotifier => 48,
            Self::Aggregates => 44,
            Self::HasOrderedComponent => 49,
            Self::FromState => 51,
            Self::ToState => 52,
            Self::HasCause => 53,
            Self::HasEffect => 54,
            Self::HasHistoricalConfiguration => 56,
            Self::Custom(id) => *id,
        };
        NodeId::numeric(0, id)
    }

    /// Get from NodeId.
    pub fn from_node_id(node_id: &NodeId) -> Option<Self> {
        if node_id.namespace() != 0 {
            return None;
        }
        let id = node_id.as_numeric()?;
        Some(match id {
            31 => Self::References,
            32 => Self::NonHierarchicalReferences,
            33 => Self::HierarchicalReferences,
            34 => Self::HasChild,
            35 => Self::Organizes,
            47 => Self::HasComponent,
            46 => Self::HasProperty,
            45 => Self::HasSubtype,
            40 => Self::HasTypeDefinition,
            37 => Self::HasModellingRule,
            38 => Self::HasEncoding,
            39 => Self::HasDescription,
            41 => Self::GeneratesEvent,
            3065 => Self::AlwaysGeneratesEvent,
            36 => Self::HasEventSource,
            48 => Self::HasNotifier,
            44 => Self::Aggregates,
            49 => Self::HasOrderedComponent,
            51 => Self::FromState,
            52 => Self::ToState,
            53 => Self::HasCause,
            54 => Self::HasEffect,
            56 => Self::HasHistoricalConfiguration,
            _ => Self::Custom(id),
        })
    }

    /// Check if this is a hierarchical reference type.
    pub fn is_hierarchical(&self) -> bool {
        matches!(
            self,
            Self::HierarchicalReferences
                | Self::HasChild
                | Self::Organizes
                | Self::HasComponent
                | Self::HasProperty
                | Self::HasSubtype
                | Self::Aggregates
                | Self::HasOrderedComponent
                | Self::HasEventSource
                | Self::HasNotifier
        )
    }

    /// Get the display name.
    pub fn name(&self) -> &'static str {
        match self {
            Self::References => "References",
            Self::NonHierarchicalReferences => "NonHierarchicalReferences",
            Self::HierarchicalReferences => "HierarchicalReferences",
            Self::HasChild => "HasChild",
            Self::Organizes => "Organizes",
            Self::HasComponent => "HasComponent",
            Self::HasProperty => "HasProperty",
            Self::HasSubtype => "HasSubtype",
            Self::HasTypeDefinition => "HasTypeDefinition",
            Self::HasModellingRule => "HasModellingRule",
            Self::HasEncoding => "HasEncoding",
            Self::HasDescription => "HasDescription",
            Self::GeneratesEvent => "GeneratesEvent",
            Self::AlwaysGeneratesEvent => "AlwaysGeneratesEvent",
            Self::HasEventSource => "HasEventSource",
            Self::HasNotifier => "HasNotifier",
            Self::Aggregates => "Aggregates",
            Self::HasOrderedComponent => "HasOrderedComponent",
            Self::FromState => "FromState",
            Self::ToState => "ToState",
            Self::HasCause => "HasCause",
            Self::HasEffect => "HasEffect",
            Self::HasHistoricalConfiguration => "HasHistoricalConfiguration",
            Self::Custom(_) => "Custom",
        }
    }
}

impl fmt::Display for ReferenceTypeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.name())
    }
}

/// Reference direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ReferenceDirection {
    /// Forward reference (from source to target).
    Forward,
    /// Inverse reference (from target to source).
    Inverse,
}

/// Browse direction for browsing operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum BrowseDirection {
    /// Browse forward references only.
    Forward,
    /// Browse inverse references only.
    Inverse,
    /// Browse both directions.
    Both,
}

impl Default for BrowseDirection {
    fn default() -> Self {
        Self::Forward
    }
}

/// A reference between two nodes.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Reference {
    /// Source node ID.
    pub source_node_id: NodeId,
    /// Reference type ID.
    pub reference_type_id: ReferenceTypeId,
    /// Target node ID.
    pub target_node_id: NodeId,
    /// Direction of the reference.
    pub direction: ReferenceDirection,
}

impl Reference {
    /// Create a new forward reference.
    pub fn forward(
        source_node_id: NodeId,
        reference_type_id: ReferenceTypeId,
        target_node_id: NodeId,
    ) -> Self {
        Self {
            source_node_id,
            reference_type_id,
            target_node_id,
            direction: ReferenceDirection::Forward,
        }
    }

    /// Create a new inverse reference.
    pub fn inverse(
        source_node_id: NodeId,
        reference_type_id: ReferenceTypeId,
        target_node_id: NodeId,
    ) -> Self {
        Self {
            source_node_id,
            reference_type_id,
            target_node_id,
            direction: ReferenceDirection::Inverse,
        }
    }

    /// Create an Organizes reference (folder organization).
    pub fn organizes(parent: NodeId, child: NodeId) -> Self {
        Self::forward(parent, ReferenceTypeId::Organizes, child)
    }

    /// Create a HasComponent reference.
    pub fn has_component(parent: NodeId, component: NodeId) -> Self {
        Self::forward(parent, ReferenceTypeId::HasComponent, component)
    }

    /// Create a HasProperty reference.
    pub fn has_property(parent: NodeId, property: NodeId) -> Self {
        Self::forward(parent, ReferenceTypeId::HasProperty, property)
    }

    /// Create a HasTypeDefinition reference.
    pub fn has_type_definition(instance: NodeId, type_definition: NodeId) -> Self {
        Self::forward(
            instance,
            ReferenceTypeId::HasTypeDefinition,
            type_definition,
        )
    }

    /// Create a HasSubtype reference.
    pub fn has_subtype(parent_type: NodeId, subtype: NodeId) -> Self {
        Self::forward(parent_type, ReferenceTypeId::HasSubtype, subtype)
    }

    /// Check if this is a hierarchical reference.
    pub fn is_hierarchical(&self) -> bool {
        self.reference_type_id.is_hierarchical()
    }

    /// Get the inverse of this reference.
    pub fn inverse_ref(&self) -> Self {
        Self {
            source_node_id: self.target_node_id.clone(),
            reference_type_id: self.reference_type_id,
            target_node_id: self.source_node_id.clone(),
            direction: match self.direction {
                ReferenceDirection::Forward => ReferenceDirection::Inverse,
                ReferenceDirection::Inverse => ReferenceDirection::Forward,
            },
        }
    }
}

/// Reference description returned from browse operations.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReferenceDescription {
    /// Reference type ID.
    pub reference_type_id: NodeId,
    /// Whether this is a forward reference.
    pub is_forward: bool,
    /// Target node ID.
    pub node_id: NodeId,
    /// Browse name of the target node.
    pub browse_name: QualifiedName,
    /// Display name of the target node.
    pub display_name: LocalizedText,
    /// Node class of the target node.
    pub node_class: NodeClass,
    /// Type definition node ID (for objects and variables).
    pub type_definition: Option<NodeId>,
}

impl ReferenceDescription {
    /// Create a new reference description.
    pub fn new(
        reference_type_id: ReferenceTypeId,
        is_forward: bool,
        node_id: NodeId,
        browse_name: impl Into<QualifiedName>,
        display_name: impl Into<LocalizedText>,
        node_class: NodeClass,
    ) -> Self {
        Self {
            reference_type_id: reference_type_id.node_id(),
            is_forward,
            node_id,
            browse_name: browse_name.into(),
            display_name: display_name.into(),
            node_class,
            type_definition: None,
        }
    }

    /// Set type definition.
    pub fn with_type_definition(mut self, type_definition: NodeId) -> Self {
        self.type_definition = Some(type_definition);
        self
    }
}

/// Result of a browse operation.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BrowseResult {
    /// References found.
    pub references: Vec<ReferenceDescription>,
    /// Continuation point for paging (if more results available).
    pub continuation_point: Option<Vec<u8>>,
}

impl BrowseResult {
    /// Create a new browse result.
    pub fn new(references: Vec<ReferenceDescription>) -> Self {
        Self {
            references,
            continuation_point: None,
        }
    }

    /// Create with continuation point.
    pub fn with_continuation(references: Vec<ReferenceDescription>, continuation: Vec<u8>) -> Self {
        Self {
            references,
            continuation_point: Some(continuation),
        }
    }

    /// Check if there are more results.
    pub fn has_more(&self) -> bool {
        self.continuation_point.is_some()
    }

    /// Get the number of references.
    pub fn len(&self) -> usize {
        self.references.len()
    }

    /// Check if empty.
    pub fn is_empty(&self) -> bool {
        self.references.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_reference_type_id() {
        let ref_type = ReferenceTypeId::Organizes;
        assert!(ref_type.is_hierarchical());
        assert_eq!(ref_type.node_id(), NodeId::numeric(0, 35));

        let ref_type = ReferenceTypeId::HasTypeDefinition;
        assert!(!ref_type.is_hierarchical());
    }

    #[test]
    fn test_reference() {
        let parent = NodeId::numeric(2, 1000);
        let child = NodeId::numeric(2, 1001);

        let ref_ = Reference::organizes(parent.clone(), child.clone());
        assert_eq!(ref_.source_node_id, parent);
        assert_eq!(ref_.target_node_id, child);
        assert!(ref_.is_hierarchical());

        let inverse = ref_.inverse_ref();
        assert_eq!(inverse.source_node_id, child);
        assert_eq!(inverse.target_node_id, parent);
    }

    #[test]
    fn test_browse_result() {
        let result = BrowseResult::default();
        assert!(result.is_empty());
        assert!(!result.has_more());

        let result = BrowseResult::with_continuation(vec![], vec![1, 2, 3]);
        assert!(result.has_more());
    }
}

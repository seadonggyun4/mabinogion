//! OPC UA event system implementation.
//!
//! Provides event type definitions, EventFilter parsing, and event generation
//! for the OPC UA Publish service. Used by TRAP Phase 4's EventFilter and
//! EventNotification handling.
//!
//! ## Architecture
//!
//! ```text
//! EventManager
//!     ├── fire_event(source, type, severity, message)
//!     │       │
//!     │       ├── Iterates all subscriptions
//!     │       ├── Finds event-monitoring items (attribute_id == EventNotifier)
//!     │       ├── Evaluates SelectClauses → builds EventFieldList
//!     │       └── Pushes EventNotification to subscription queue
//!     │
//!     └── EventFilter types
//!             ├── SimpleAttributeOperand
//!             ├── ContentFilterElement
//!             └── EventFieldList
//! ```

use std::sync::Arc;

use chrono::{DateTime, Utc};
use tracing::{debug, trace};
use uuid::Uuid;

use crate::nodes::{AddressSpace, BrowseDirection, QualifiedName};
use crate::nodes::reference::ReferenceTypeId;
use crate::types::{AttributeId, NodeId, Variant};

// =========================================================================
// EventFilter types (OPC UA Part 4, Section 7.17)
// =========================================================================

/// Simple attribute operand — specifies a path to an attribute of an event
/// field relative to a type definition.
///
/// OPC UA Part 4, Section 7.4.4.5.
#[derive(Debug, Clone)]
pub struct SimpleAttributeOperand {
    /// The NodeId of the event type this operand applies to.
    /// Typically `BaseEventType` (i=2041) or a subtype.
    pub type_definition_id: NodeId,
    /// Browse path from the type definition node to the target node.
    /// Each element is a QualifiedName for the BrowseName of the next node.
    pub browse_path: Vec<QualifiedName>,
    /// The attribute of the target node to read.
    pub attribute_id: AttributeId,
    /// Index range for array values (empty string for no range).
    pub index_range: String,
}

/// Filter operand — used in ContentFilter where clauses.
///
/// Simplified representation supporting the most common operand types.
#[derive(Debug, Clone)]
pub enum FilterOperand {
    /// Literal value operand.
    Literal(Variant),
    /// Simple attribute operand (refers to an event field).
    SimpleAttribute(SimpleAttributeOperand),
    /// Element operand (refers to another element in the ContentFilter by index).
    Element(u32),
}

/// Content filter element — a single clause in the where clause.
///
/// OPC UA Part 4, Section 7.4.1.
#[derive(Debug, Clone)]
pub struct ContentFilterElement {
    /// The filter operator (e.g., Equals, GreaterThan, And, Or, OfType).
    pub filter_operator: FilterOperator,
    /// Operands for the operator.
    pub filter_operands: Vec<FilterOperand>,
}

/// Filter operators for ContentFilter elements.
///
/// OPC UA Part 4, Section 7.4.2.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum FilterOperator {
    Equals = 0,
    IsNull = 1,
    GreaterThan = 2,
    LessThan = 3,
    GreaterThanOrEqual = 4,
    LessThanOrEqual = 5,
    Like = 6,
    Not = 7,
    Between = 8,
    InList = 9,
    And = 10,
    Or = 11,
    Cast = 12,
    InView = 13,
    OfType = 14,
    RelatedTo = 15,
    BitwiseAnd = 16,
    BitwiseOr = 17,
}

impl FilterOperator {
    /// Create from u32 value.
    pub fn from_u32(value: u32) -> Option<Self> {
        match value {
            0 => Some(Self::Equals),
            1 => Some(Self::IsNull),
            2 => Some(Self::GreaterThan),
            3 => Some(Self::LessThan),
            4 => Some(Self::GreaterThanOrEqual),
            5 => Some(Self::LessThanOrEqual),
            6 => Some(Self::Like),
            7 => Some(Self::Not),
            8 => Some(Self::Between),
            9 => Some(Self::InList),
            10 => Some(Self::And),
            11 => Some(Self::Or),
            12 => Some(Self::Cast),
            13 => Some(Self::InView),
            14 => Some(Self::OfType),
            15 => Some(Self::RelatedTo),
            16 => Some(Self::BitwiseAnd),
            17 => Some(Self::BitwiseOr),
            _ => None,
        }
    }
}

/// EventFilter — selects which event fields to return and filters events.
///
/// OPC UA Part 4, Section 7.17.3.
#[derive(Debug, Clone)]
pub struct EventFilter {
    /// Select clauses — specifies which event fields to include in notifications.
    /// Each clause maps to one field in the EventFieldList.
    pub select_clauses: Vec<SimpleAttributeOperand>,
    /// Where clause — content filter for event filtering.
    /// Empty means accept all events of the subscribed type.
    pub where_clause: Vec<ContentFilterElement>,
}

impl EventFilter {
    /// Create an empty EventFilter (accepts all events, returns no fields).
    pub fn new() -> Self {
        Self {
            select_clauses: Vec::new(),
            where_clause: Vec::new(),
        }
    }

    /// Create an EventFilter with the standard BaseEventType fields.
    ///
    /// Returns: EventId, EventType, SourceNode, SourceName, Time, ReceiveTime, Message, Severity
    pub fn base_event_type_filter() -> Self {
        let base_type = NodeId::numeric(0, 2041); // BaseEventType

        let field_names = [
            "EventId", "EventType", "SourceNode", "SourceName",
            "Time", "ReceiveTime", "Message", "Severity",
        ];

        let select_clauses = field_names
            .iter()
            .map(|name| SimpleAttributeOperand {
                type_definition_id: base_type.clone(),
                browse_path: vec![QualifiedName::new(0, *name)],
                attribute_id: AttributeId::Value,
                index_range: String::new(),
            })
            .collect();

        Self {
            select_clauses,
            where_clause: Vec::new(),
        }
    }
}

impl Default for EventFilter {
    fn default() -> Self {
        Self::new()
    }
}

// =========================================================================
// Event notification types
// =========================================================================

/// An event field list — the set of values for one event occurrence,
/// corresponding 1:1 with the SelectClauses of the EventFilter.
///
/// OPC UA Part 4, Section 7.21.2.
#[derive(Debug, Clone)]
pub struct EventFieldList {
    /// Client handle of the monitored item that generated this event notification.
    pub client_handle: u32,
    /// Event field values, one per SelectClause in the EventFilter.
    pub event_fields: Vec<Variant>,
}

/// An event notification queued for delivery via Publish.
#[derive(Debug, Clone)]
pub struct EventNotification {
    /// The subscription that this notification belongs to.
    pub subscription_id: u32,
    /// The event field list for this event.
    pub field_list: EventFieldList,
}

/// Parameters for firing an event.
#[derive(Debug, Clone)]
pub struct EventData {
    /// Unique event ID (ByteString).
    pub event_id: Vec<u8>,
    /// The event type NodeId (e.g., BaseEventType i=2041 or a subtype).
    pub event_type: NodeId,
    /// The source node that generated the event.
    pub source_node: NodeId,
    /// Human-readable source name.
    pub source_name: String,
    /// Time the event occurred.
    pub time: DateTime<Utc>,
    /// Time the server received the event.
    pub receive_time: DateTime<Utc>,
    /// Human-readable event message.
    pub message: String,
    /// Severity (0-1000, with 1000 being most severe).
    pub severity: u16,
    /// Additional custom fields (QualifiedName browse path → Variant value).
    pub custom_fields: Vec<(Vec<QualifiedName>, Variant)>,
}

impl EventData {
    /// Create a new EventData with required fields and auto-generated EventId.
    pub fn new(
        event_type: NodeId,
        source_node: NodeId,
        source_name: impl Into<String>,
        severity: u16,
        message: impl Into<String>,
    ) -> Self {
        let event_id = Uuid::new_v4().as_bytes().to_vec();
        let now = Utc::now();

        Self {
            event_id,
            event_type,
            source_node,
            source_name: source_name.into(),
            time: now,
            receive_time: now,
            message: message.into(),
            severity,
            custom_fields: Vec::new(),
        }
    }

    /// Add a custom event field.
    pub fn with_field(
        mut self,
        browse_path: Vec<QualifiedName>,
        value: Variant,
    ) -> Self {
        self.custom_fields.push((browse_path, value));
        self
    }

    /// Resolve a SimpleAttributeOperand to a Variant value from this event.
    ///
    /// Matches the browse_path of the operand against the standard
    /// BaseEventType properties and custom fields.
    pub fn resolve_field(&self, operand: &SimpleAttributeOperand) -> Variant {
        if operand.browse_path.is_empty() {
            return Variant::Null;
        }

        // Match single-element browse paths against standard BaseEventType fields
        if operand.browse_path.len() == 1 {
            let field_name = &operand.browse_path[0].name;
            match field_name.as_str() {
                "EventId" => return Variant::ByteString(self.event_id.clone()),
                "EventType" => return Variant::NodeId(self.event_type.clone()),
                "SourceNode" => return Variant::NodeId(self.source_node.clone()),
                "SourceName" => return Variant::String(self.source_name.clone()),
                "Time" => return Variant::DateTime(self.time),
                "ReceiveTime" => return Variant::DateTime(self.receive_time),
                "Message" => return Variant::String(self.message.clone()),
                "Severity" => return Variant::UInt16(self.severity),
                _ => {}
            }
        }

        // Check custom fields
        for (path, value) in &self.custom_fields {
            if paths_match(path, &operand.browse_path) {
                return value.clone();
            }
        }

        // Field not found
        Variant::Null
    }
}

/// Check if two browse paths match (case-sensitive name comparison).
fn paths_match(a: &[QualifiedName], b: &[QualifiedName]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b.iter()).all(|(qa, qb)| {
        qa.namespace_index == qb.namespace_index && qa.name == qb.name
    })
}

// =========================================================================
// EventManager
// =========================================================================

/// Well-known NodeId for BaseEventType.
pub const BASE_EVENT_TYPE_ID: u32 = 2041;

/// Manages event generation and distribution to subscriptions.
///
/// When an event is fired, the EventManager:
/// 1. Iterates all subscriptions
/// 2. Finds monitored items with EventFilter (event monitoring)
/// 3. Evaluates the where clause (if any) to decide if the event passes
/// 4. Builds an EventFieldList from the select clauses
/// 5. Pushes the EventNotification into the subscription's pending queue
pub struct EventManager {
    /// Reference to the subscription manager for distributing events.
    subscription_manager: Arc<super::subscription::SubscriptionManager>,
    /// Reference to the address space for type hierarchy checks.
    address_space: Arc<AddressSpace>,
}

impl EventManager {
    /// Create a new EventManager.
    pub fn new(
        subscription_manager: Arc<super::subscription::SubscriptionManager>,
        address_space: Arc<AddressSpace>,
    ) -> Self {
        Self {
            subscription_manager,
            address_space,
        }
    }

    /// Fire an event and distribute it to all interested subscriptions.
    ///
    /// This is the primary entry point for event generation.
    ///
    /// # Arguments
    /// * `source_node` — The node that generated the event.
    /// * `event_type` — The NodeId of the event type (must be a subtype of BaseEventType).
    /// * `severity` — Event severity (0-1000).
    /// * `message` — Human-readable event message.
    pub fn fire_event(
        &self,
        source_node: &NodeId,
        event_type: &NodeId,
        severity: u16,
        message: &str,
    ) {
        let event_data = EventData::new(
            event_type.clone(),
            source_node.clone(),
            source_node.to_string(),
            severity,
            message,
        );

        self.fire_event_with_data(&event_data);
    }

    /// Fire an event with full EventData (including custom fields).
    pub fn fire_event_with_data(&self, event_data: &EventData) {
        debug!(
            source_node = %event_data.source_node,
            event_type = %event_data.event_type,
            severity = event_data.severity,
            message = %event_data.message,
            "Firing event"
        );

        let sub_ids = self.subscription_manager.subscription_ids();

        for sub_id in &sub_ids {
            self.distribute_to_subscription(*sub_id, event_data);
        }
    }

    /// Distribute an event to a single subscription's event-monitoring items.
    fn distribute_to_subscription(
        &self,
        subscription_id: u32,
        event_data: &EventData,
    ) {
        // Get event-monitoring items and their filters for this subscription
        let event_items = self.subscription_manager
            .get_event_monitored_items(subscription_id);

        for (client_handle, filter) in &event_items {
            // Evaluate where clause
            if !self.evaluate_where_clause(&filter.where_clause, event_data) {
                trace!(
                    subscription_id,
                    client_handle,
                    "Event filtered out by where clause"
                );
                continue;
            }

            // Build EventFieldList from select clauses
            let event_fields: Vec<Variant> = filter
                .select_clauses
                .iter()
                .map(|clause| event_data.resolve_field(clause))
                .collect();

            let field_list = EventFieldList {
                client_handle: *client_handle,
                event_fields,
            };

            // Push to subscription's event notification queue
            self.subscription_manager
                .push_event_notification(subscription_id, field_list);
        }
    }

    /// Evaluate a ContentFilter where clause against an event.
    ///
    /// Returns `true` if the event passes the filter (or if the filter is empty).
    fn evaluate_where_clause(
        &self,
        where_clause: &[ContentFilterElement],
        event_data: &EventData,
    ) -> bool {
        if where_clause.is_empty() {
            return true; // No filter = accept all
        }

        // Evaluate the first element (root of the filter tree)
        self.evaluate_element(where_clause, 0, event_data)
    }

    /// Evaluate a single ContentFilter element recursively.
    fn evaluate_element(
        &self,
        elements: &[ContentFilterElement],
        index: usize,
        event_data: &EventData,
    ) -> bool {
        let Some(element) = elements.get(index) else {
            return true; // Out of bounds = pass
        };

        match element.filter_operator {
            FilterOperator::OfType => {
                // OfType: check if the event type is the given type or a subtype
                if let Some(operand) = element.filter_operands.first() {
                    if let Some(type_id) = self.resolve_operand_as_node_id(operand, event_data) {
                        return self.is_subtype_of(&event_data.event_type, &type_id);
                    }
                }
                true // If we can't resolve, pass
            }
            FilterOperator::Equals => {
                self.evaluate_comparison(elements, element, event_data, |a, b| a == b)
            }
            FilterOperator::GreaterThan => {
                self.evaluate_comparison(elements, element, event_data, |a, b| {
                    compare_variants(a, b).map_or(false, |ord| ord == std::cmp::Ordering::Greater)
                })
            }
            FilterOperator::LessThan => {
                self.evaluate_comparison(elements, element, event_data, |a, b| {
                    compare_variants(a, b).map_or(false, |ord| ord == std::cmp::Ordering::Less)
                })
            }
            FilterOperator::GreaterThanOrEqual => {
                self.evaluate_comparison(elements, element, event_data, |a, b| {
                    compare_variants(a, b).map_or(false, |ord| ord != std::cmp::Ordering::Less)
                })
            }
            FilterOperator::LessThanOrEqual => {
                self.evaluate_comparison(elements, element, event_data, |a, b| {
                    compare_variants(a, b).map_or(false, |ord| ord != std::cmp::Ordering::Greater)
                })
            }
            FilterOperator::And => {
                // All operands must evaluate to true (they should be Element references)
                element.filter_operands.iter().all(|op| {
                    if let FilterOperand::Element(idx) = op {
                        self.evaluate_element(elements, *idx as usize, event_data)
                    } else {
                        true
                    }
                })
            }
            FilterOperator::Or => {
                // At least one operand must evaluate to true
                element.filter_operands.iter().any(|op| {
                    if let FilterOperand::Element(idx) = op {
                        self.evaluate_element(elements, *idx as usize, event_data)
                    } else {
                        false
                    }
                })
            }
            FilterOperator::Not => {
                // Negate the first operand
                if let Some(FilterOperand::Element(idx)) = element.filter_operands.first() {
                    !self.evaluate_element(elements, *idx as usize, event_data)
                } else {
                    true
                }
            }
            FilterOperator::IsNull => {
                if let Some(operand) = element.filter_operands.first() {
                    let value = self.resolve_operand_as_variant(operand, event_data);
                    matches!(value, Variant::Null)
                } else {
                    true
                }
            }
            // For unsupported operators, pass the event through
            _ => true,
        }
    }

    /// Evaluate a binary comparison operator.
    fn evaluate_comparison<F>(
        &self,
        elements: &[ContentFilterElement],
        element: &ContentFilterElement,
        event_data: &EventData,
        cmp_fn: F,
    ) -> bool
    where
        F: Fn(&Variant, &Variant) -> bool,
    {
        if element.filter_operands.len() < 2 {
            return true; // Malformed, pass through
        }
        let left = self.resolve_operand_as_variant(&element.filter_operands[0], event_data);
        let right = self.resolve_operand_as_variant(&element.filter_operands[1], event_data);
        cmp_fn(&left, &right)
    }

    /// Resolve a filter operand to a Variant value.
    fn resolve_operand_as_variant(
        &self,
        operand: &FilterOperand,
        event_data: &EventData,
    ) -> Variant {
        match operand {
            FilterOperand::Literal(v) => v.clone(),
            FilterOperand::SimpleAttribute(attr) => event_data.resolve_field(attr),
            FilterOperand::Element(_) => Variant::Null, // Element operands are for boolean composition
        }
    }

    /// Resolve a filter operand to a NodeId (for OfType operator).
    fn resolve_operand_as_node_id(
        &self,
        operand: &FilterOperand,
        event_data: &EventData,
    ) -> Option<NodeId> {
        match operand {
            FilterOperand::Literal(Variant::NodeId(nid)) => Some(nid.clone()),
            FilterOperand::SimpleAttribute(attr) => {
                match event_data.resolve_field(attr) {
                    Variant::NodeId(nid) => Some(nid),
                    _ => None,
                }
            }
            _ => None,
        }
    }

    /// Check if `event_type` is the same as or a subtype of `target_type`.
    fn is_subtype_of(&self, event_type: &NodeId, target_type: &NodeId) -> bool {
        if event_type == target_type {
            return true;
        }

        // Walk the HasSubtype hierarchy in the address space (inverse direction = parent type)
        let mut current = event_type.clone();
        let mut depth = 0;
        const MAX_DEPTH: usize = 50;

        while depth < MAX_DEPTH {
            // Find the parent type (inverse HasSubtype reference)
            let refs = self.address_space.get_references(&current, BrowseDirection::Inverse);
            let parent = refs.iter().find_map(|r| {
                if r.reference_type_id == ReferenceTypeId::HasSubtype {
                    // Inverse HasSubtype: target_node_id is the parent type
                    Some(r.target_node_id.clone())
                } else {
                    None
                }
            });

            match parent {
                Some(parent_id) => {
                    if parent_id == *target_type {
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

/// Compare two Variants for ordering (for comparison operators).
fn compare_variants(a: &Variant, b: &Variant) -> Option<std::cmp::Ordering> {
    match (a, b) {
        (Variant::Double(a), Variant::Double(b)) => a.partial_cmp(b),
        (Variant::Float(a), Variant::Float(b)) => a.partial_cmp(b),
        (Variant::Int32(a), Variant::Int32(b)) => a.partial_cmp(b),
        (Variant::UInt32(a), Variant::UInt32(b)) => a.partial_cmp(b),
        (Variant::Int64(a), Variant::Int64(b)) => a.partial_cmp(b),
        (Variant::UInt64(a), Variant::UInt64(b)) => a.partial_cmp(b),
        (Variant::Int16(a), Variant::Int16(b)) => a.partial_cmp(b),
        (Variant::UInt16(a), Variant::UInt16(b)) => a.partial_cmp(b),
        (Variant::String(a), Variant::String(b)) => a.partial_cmp(b),
        (Variant::DateTime(a), Variant::DateTime(b)) => a.partial_cmp(b),
        // Cross-type numeric comparisons (promote to f64)
        (a, b) => {
            let af = a.as_f64()?;
            let bf = b.as_f64()?;
            af.partial_cmp(&bf)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_event_data_resolve_field() {
        let event = EventData::new(
            NodeId::numeric(0, 2041),
            NodeId::numeric(2, 1001),
            "TestSource",
            500,
            "Test event message",
        );

        let operand = SimpleAttributeOperand {
            type_definition_id: NodeId::numeric(0, 2041),
            browse_path: vec![QualifiedName::new(0, "Message")],
            attribute_id: AttributeId::Value,
            index_range: String::new(),
        };

        let result = event.resolve_field(&operand);
        assert_eq!(result, Variant::String("Test event message".to_string()));
    }

    #[test]
    fn test_event_data_resolve_severity() {
        let event = EventData::new(
            NodeId::numeric(0, 2041),
            NodeId::numeric(2, 1001),
            "TestSource",
            750,
            "High severity event",
        );

        let operand = SimpleAttributeOperand {
            type_definition_id: NodeId::numeric(0, 2041),
            browse_path: vec![QualifiedName::new(0, "Severity")],
            attribute_id: AttributeId::Value,
            index_range: String::new(),
        };

        let result = event.resolve_field(&operand);
        assert_eq!(result, Variant::UInt16(750));
    }

    #[test]
    fn test_event_data_unknown_field() {
        let event = EventData::new(
            NodeId::numeric(0, 2041),
            NodeId::numeric(2, 1001),
            "TestSource",
            500,
            "Test",
        );

        let operand = SimpleAttributeOperand {
            type_definition_id: NodeId::numeric(0, 2041),
            browse_path: vec![QualifiedName::new(0, "NonExistentField")],
            attribute_id: AttributeId::Value,
            index_range: String::new(),
        };

        let result = event.resolve_field(&operand);
        assert_eq!(result, Variant::Null);
    }

    #[test]
    fn test_event_data_custom_field() {
        let event = EventData::new(
            NodeId::numeric(0, 2041),
            NodeId::numeric(2, 1001),
            "TestSource",
            500,
            "Test",
        )
        .with_field(
            vec![QualifiedName::new(2, "CustomField")],
            Variant::Double(42.0),
        );

        let operand = SimpleAttributeOperand {
            type_definition_id: NodeId::numeric(0, 2041),
            browse_path: vec![QualifiedName::new(2, "CustomField")],
            attribute_id: AttributeId::Value,
            index_range: String::new(),
        };

        let result = event.resolve_field(&operand);
        assert_eq!(result, Variant::Double(42.0));
    }

    #[test]
    fn test_event_filter_base_type() {
        let filter = EventFilter::base_event_type_filter();
        assert_eq!(filter.select_clauses.len(), 8);
        assert_eq!(filter.select_clauses[0].browse_path[0].name, "EventId");
        assert_eq!(filter.select_clauses[7].browse_path[0].name, "Severity");
    }

    #[test]
    fn test_filter_operator_from_u32() {
        assert_eq!(FilterOperator::from_u32(0), Some(FilterOperator::Equals));
        assert_eq!(FilterOperator::from_u32(14), Some(FilterOperator::OfType));
        assert_eq!(FilterOperator::from_u32(99), None);
    }

    #[test]
    fn test_compare_variants() {
        assert_eq!(
            compare_variants(&Variant::Double(1.0), &Variant::Double(2.0)),
            Some(std::cmp::Ordering::Less)
        );
        assert_eq!(
            compare_variants(&Variant::Int32(5), &Variant::Int32(5)),
            Some(std::cmp::Ordering::Equal)
        );
        assert_eq!(
            compare_variants(&Variant::String("b".into()), &Variant::String("a".into())),
            Some(std::cmp::Ordering::Greater)
        );
    }

    #[test]
    fn test_paths_match() {
        let a = vec![QualifiedName::new(0, "Foo"), QualifiedName::new(0, "Bar")];
        let b = vec![QualifiedName::new(0, "Foo"), QualifiedName::new(0, "Bar")];
        let c = vec![QualifiedName::new(0, "Foo")];
        let d = vec![QualifiedName::new(1, "Foo"), QualifiedName::new(0, "Bar")];

        assert!(paths_match(&a, &b));
        assert!(!paths_match(&a, &c));
        assert!(!paths_match(&a, &d));
    }
}

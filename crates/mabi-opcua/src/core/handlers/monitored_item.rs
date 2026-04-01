//! MonitoredItem service handlers — CreateMonitoredItems, DeleteMonitoredItems, ModifyMonitoredItems.
//!
//! OPC UA Part 4, Section 5.12.

use std::sync::Arc;

use async_trait::async_trait;
use bytes::{BufMut, Bytes, BytesMut};
use tracing::debug;

use super::discovery::{RequestHeader, ResponseHeader};
use super::registry::{ServiceContext, ServiceHandler, ServiceResponse};
use crate::codec::data_value::ExtensionObject;
use crate::codec::decoder::BinaryDecodable;
use crate::codec::encoder::BinaryEncodable;
use crate::error::OpcUaResult;
use crate::nodes::QualifiedName;
use crate::sdk::event::{
    ContentFilterElement, EventFilter, FilterOperand, FilterOperator, SimpleAttributeOperand,
};
use crate::sdk::subscription::{MonitoredItemConfig, MonitoredItemKind};
use crate::types::{AttributeId, NodeId, StatusCode};

const CREATE_MONITORED_ITEMS_REQUEST_ID: u32 = 751;
const CREATE_MONITORED_ITEMS_RESPONSE_ID: u32 = 754;
const DELETE_MONITORED_ITEMS_REQUEST_ID: u32 = 781;
const DELETE_MONITORED_ITEMS_RESPONSE_ID: u32 = 784;

/// EventFilter binary encoding ID (EventFilter_Encoding_DefaultBinary).
const EVENT_FILTER_ENCODING_ID: u32 = 725;

// =========================================================================
// CreateMonitoredItems
// =========================================================================

pub struct CreateMonitoredItemsHandler;

#[async_trait]
impl ServiceHandler for CreateMonitoredItemsHandler {
    fn request_type_id(&self) -> NodeId {
        NodeId::numeric(0, CREATE_MONITORED_ITEMS_REQUEST_ID)
    }

    async fn handle(
        &self,
        request_body: &[u8],
        context: &ServiceContext,
    ) -> OpcUaResult<ServiceResponse> {
        let mut buf = Bytes::copy_from_slice(request_body);
        let header = RequestHeader::decode(&mut buf)?;
        let _additional_header = ExtensionObject::decode(&mut buf)?;

        let subscription_id = u32::decode(&mut buf)?;
        let _timestamps_to_return = u32::decode(&mut buf)?;

        let count = i32::decode(&mut buf)?;
        let mut results = Vec::new();

        for _ in 0..count.max(0) {
            // ItemToMonitor (ReadValueId)
            let node_id = NodeId::decode(&mut buf)?;
            let attribute_id_val = u32::decode(&mut buf)?;
            let _index_range = String::decode(&mut buf)?;
            // DataEncoding (QualifiedName)
            let _ns = u16::decode(&mut buf)?;
            let _name = String::decode(&mut buf)?;

            // MonitoringMode
            let _monitoring_mode = u32::decode(&mut buf)?;

            // MonitoringParameters
            let client_handle = u32::decode(&mut buf)?;
            let sampling_interval = f64::decode(&mut buf)?;
            // Filter (ExtensionObject)
            let filter_ext = ExtensionObject::decode(&mut buf)?;
            let queue_size = u32::decode(&mut buf)?;
            let discard_oldest = bool::decode(&mut buf)?;

            let attribute_id =
                AttributeId::from_u32(attribute_id_val).unwrap_or(AttributeId::Value);

            // Determine if this is an event-monitoring item based on:
            // 1. The filter is an EventFilter (encoding_id = i=725)
            // 2. OR the attribute is EventNotifier
            let event_filter = parse_event_filter(&filter_ext);
            let is_event = event_filter.is_some() || attribute_id == AttributeId::EventNotifier;

            let config = MonitoredItemConfig {
                node_id: node_id.clone(),
                attribute_id,
                sampling_interval_ms: sampling_interval,
                queue_size,
                discard_oldest,
                filter: None,
                kind: if is_event {
                    MonitoredItemKind::Event
                } else {
                    MonitoredItemKind::DataChange
                },
                event_filter,
            };

            if is_event {
                debug!(
                    node_id = %node_id,
                    client_handle,
                    "Creating event-monitoring item"
                );
            }

            match context
                .subscription_manager
                .create_monitored_item(subscription_id, config)
            {
                Ok(item_id) => {
                    results.push((StatusCode::GOOD, item_id, sampling_interval, queue_size));
                }
                Err(_) => {
                    results.push((StatusCode::BAD_SUBSCRIPTION_ID_INVALID, 0, 0.0, 0));
                }
            }
        }

        let mut out = BytesMut::new();
        ResponseHeader::good(header.request_handle).encode(&mut out)?;

        out.put_i32_le(results.len() as i32);
        for (status, item_id, revised_interval, revised_queue) in &results {
            status.encode(&mut out)?;
            out.put_u32_le(*item_id);
            out.put_f64_le(*revised_interval);
            out.put_u32_le(*revised_queue);
            // Filter result (null ExtensionObject)
            (ExtensionObject {
                type_id: NodeId::numeric(0, 0),
                body: None,
            })
            .encode(&mut out)?;
        }
        // DiagnosticInfos
        out.put_i32_le(0);

        Ok(ServiceResponse {
            type_id: NodeId::numeric(0, CREATE_MONITORED_ITEMS_RESPONSE_ID),
            body: out.to_vec(),
        })
    }
}

// =========================================================================
// EventFilter parsing
// =========================================================================

/// Parse an EventFilter from an ExtensionObject.
///
/// Returns `Some(EventFilter)` if the extension object has type_id matching
/// EventFilter binary encoding (i=725) and the body can be decoded.
/// Returns `None` for null/empty extension objects or non-EventFilter types.
fn parse_event_filter(ext: &ExtensionObject) -> Option<EventFilter> {
    let type_id_num = ext.type_id.as_numeric()?;
    if type_id_num != EVENT_FILTER_ENCODING_ID {
        return None;
    }

    let body = ext.body.as_ref()?;
    let mut buf = Bytes::copy_from_slice(body);

    // SelectClauses: SimpleAttributeOperand[]
    let select_count = i32::decode(&mut buf).ok()?;
    let mut select_clauses = Vec::with_capacity(select_count.max(0) as usize);

    for _ in 0..select_count.max(0) {
        if let Some(operand) = decode_simple_attribute_operand(&mut buf) {
            select_clauses.push(operand);
        }
    }

    // WhereClause: ContentFilter
    let where_count = i32::decode(&mut buf).ok().unwrap_or(0);
    let mut where_clause = Vec::with_capacity(where_count.max(0) as usize);

    for _ in 0..where_count.max(0) {
        if let Some(element) = decode_content_filter_element(&mut buf) {
            where_clause.push(element);
        }
    }

    debug!(
        select_clauses = select_clauses.len(),
        where_elements = where_clause.len(),
        "Parsed EventFilter"
    );

    Some(EventFilter {
        select_clauses,
        where_clause,
    })
}

/// Decode a SimpleAttributeOperand from the buffer.
fn decode_simple_attribute_operand(buf: &mut Bytes) -> Option<SimpleAttributeOperand> {
    // TypeDefinitionId (NodeId)
    let type_definition_id = NodeId::decode(buf).ok()?;

    // BrowsePath (QualifiedName[])
    let path_count = i32::decode(buf).ok()?;
    let mut browse_path = Vec::with_capacity(path_count.max(0) as usize);
    for _ in 0..path_count.max(0) {
        let ns = u16::decode(buf).ok()?;
        let name = String::decode(buf).ok()?;
        browse_path.push(QualifiedName::new(ns, name));
    }

    // AttributeId (u32)
    let attribute_id_val = u32::decode(buf).ok()?;
    let attribute_id = AttributeId::from_u32(attribute_id_val).unwrap_or(AttributeId::Value);

    // IndexRange (String)
    let index_range = String::decode(buf).ok().unwrap_or_default();

    Some(SimpleAttributeOperand {
        type_definition_id,
        browse_path,
        attribute_id,
        index_range,
    })
}

/// Decode a ContentFilterElement from the buffer.
fn decode_content_filter_element(buf: &mut Bytes) -> Option<ContentFilterElement> {
    // FilterOperator (u32 enum)
    let operator_val = u32::decode(buf).ok()?;
    let filter_operator = FilterOperator::from_u32(operator_val)?;

    // FilterOperands: ExtensionObject[]
    let operand_count = i32::decode(buf).ok()?;
    let mut filter_operands = Vec::with_capacity(operand_count.max(0) as usize);

    for _ in 0..operand_count.max(0) {
        // Each operand is an ExtensionObject
        let operand_ext = ExtensionObject::decode(buf).ok()?;
        if let Some(operand) = decode_filter_operand(&operand_ext) {
            filter_operands.push(operand);
        }
    }

    Some(ContentFilterElement {
        filter_operator,
        filter_operands,
    })
}

/// Decode a FilterOperand from an ExtensionObject.
///
/// Supports:
/// - LiteralOperand (encoding_id i=597)
/// - SimpleAttributeOperand (encoding_id i=603)
/// - ElementOperand (encoding_id i=594)
fn decode_filter_operand(ext: &ExtensionObject) -> Option<FilterOperand> {
    let type_id_num = ext.type_id.as_numeric()?;
    let body = ext.body.as_ref()?;
    let mut buf = Bytes::copy_from_slice(body);

    match type_id_num {
        597 => {
            // LiteralOperand: contains a single Variant
            let variant = crate::types::Variant::decode(&mut buf).ok()?;
            Some(FilterOperand::Literal(variant))
        }
        603 => {
            // SimpleAttributeOperand
            let operand = decode_simple_attribute_operand(&mut buf)?;
            Some(FilterOperand::SimpleAttribute(operand))
        }
        594 => {
            // ElementOperand: contains a u32 index
            let index = u32::decode(&mut buf).ok()?;
            Some(FilterOperand::Element(index))
        }
        _ => None, // Unsupported operand type
    }
}

// =========================================================================
// DeleteMonitoredItems
// =========================================================================

pub struct DeleteMonitoredItemsHandler;

#[async_trait]
impl ServiceHandler for DeleteMonitoredItemsHandler {
    fn request_type_id(&self) -> NodeId {
        NodeId::numeric(0, DELETE_MONITORED_ITEMS_REQUEST_ID)
    }

    async fn handle(
        &self,
        request_body: &[u8],
        context: &ServiceContext,
    ) -> OpcUaResult<ServiceResponse> {
        let mut buf = Bytes::copy_from_slice(request_body);
        let header = RequestHeader::decode(&mut buf)?;
        let _additional_header = ExtensionObject::decode(&mut buf)?;

        let subscription_id = u32::decode(&mut buf)?;
        let count = i32::decode(&mut buf)?;

        let mut results = Vec::new();
        for _ in 0..count.max(0) {
            let item_id = u32::decode(&mut buf)?;
            let ok = context
                .subscription_manager
                .delete_monitored_item(subscription_id, item_id);
            results.push(if ok.is_ok() {
                StatusCode::GOOD
            } else {
                StatusCode::BAD_MONITORED_ITEM_ID_INVALID
            });
        }

        let mut out = BytesMut::new();
        ResponseHeader::good(header.request_handle).encode(&mut out)?;
        out.put_i32_le(results.len() as i32);
        for status in &results {
            status.encode(&mut out)?;
        }
        out.put_i32_le(0); // DiagnosticInfos

        Ok(ServiceResponse {
            type_id: NodeId::numeric(0, DELETE_MONITORED_ITEMS_RESPONSE_ID),
            body: out.to_vec(),
        })
    }
}

// =========================================================================
// ModifyMonitoredItems
// =========================================================================

const MODIFY_MONITORED_ITEMS_REQUEST_ID: u32 = 763;
const MODIFY_MONITORED_ITEMS_RESPONSE_ID: u32 = 766;

pub struct ModifyMonitoredItemsHandler;

#[async_trait]
impl ServiceHandler for ModifyMonitoredItemsHandler {
    fn request_type_id(&self) -> NodeId {
        NodeId::numeric(0, MODIFY_MONITORED_ITEMS_REQUEST_ID)
    }

    async fn handle(
        &self,
        request_body: &[u8],
        context: &ServiceContext,
    ) -> OpcUaResult<ServiceResponse> {
        let mut buf = Bytes::copy_from_slice(request_body);
        let header = RequestHeader::decode(&mut buf)?;
        let _additional_header = ExtensionObject::decode(&mut buf)?;

        let subscription_id = u32::decode(&mut buf)?;
        let _timestamps_to_return = u32::decode(&mut buf)?;

        let count = i32::decode(&mut buf)?;
        let mut results = Vec::new();

        for _ in 0..count.max(0) {
            // MonitoredItemModifyRequest
            let monitored_item_id = u32::decode(&mut buf)?;

            // RequestedParameters (MonitoringParameters)
            let _client_handle = u32::decode(&mut buf)?;
            let sampling_interval = f64::decode(&mut buf)?;
            // Filter (ExtensionObject)
            let _filter = ExtensionObject::decode(&mut buf)?;
            let queue_size = u32::decode(&mut buf)?;
            let discard_oldest = bool::decode(&mut buf)?;

            match context.subscription_manager.modify_monitored_item(
                subscription_id,
                monitored_item_id,
                sampling_interval,
                queue_size,
                discard_oldest,
            ) {
                Ok(()) => {
                    results.push((StatusCode::GOOD, sampling_interval, queue_size));
                }
                Err(_) => {
                    results.push((StatusCode::BAD_MONITORED_ITEM_ID_INVALID, 0.0, 0));
                }
            }
        }

        let mut out = BytesMut::new();
        ResponseHeader::good(header.request_handle).encode(&mut out)?;

        // Results array
        out.put_i32_le(results.len() as i32);
        for (status, revised_interval, revised_queue) in &results {
            status.encode(&mut out)?;
            out.put_f64_le(*revised_interval); // RevisedSamplingInterval
            out.put_u32_le(*revised_queue); // RevisedQueueSize
                                            // FilterResult (null ExtensionObject)
            (ExtensionObject {
                type_id: NodeId::numeric(0, 0),
                body: None,
            })
            .encode(&mut out)?;
        }
        // DiagnosticInfos
        out.put_i32_le(0);

        Ok(ServiceResponse {
            type_id: NodeId::numeric(0, MODIFY_MONITORED_ITEMS_RESPONSE_ID),
            body: out.to_vec(),
        })
    }
}

/// Register all monitored item service handlers.
pub fn register_handlers(registry: &mut super::registry::ServiceRegistry) {
    registry.register(Arc::new(CreateMonitoredItemsHandler));
    registry.register(Arc::new(DeleteMonitoredItemsHandler));
    registry.register(Arc::new(ModifyMonitoredItemsHandler));
}

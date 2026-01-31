//! MonitoredItem service handlers — CreateMonitoredItems, DeleteMonitoredItems.
//!
//! OPC UA Part 4, Section 5.12.

use std::sync::Arc;

use async_trait::async_trait;
use bytes::{BufMut, Bytes, BytesMut};

use crate::codec::encoder::BinaryEncodable;
use crate::codec::decoder::BinaryDecodable;
use crate::codec::data_value::ExtensionObject;
use crate::error::OpcUaResult;
use crate::types::{NodeId, StatusCode, AttributeId};
use crate::services::MonitoredItemConfig;
use super::discovery::{RequestHeader, ResponseHeader};
use super::registry::{ServiceContext, ServiceHandler, ServiceResponse};

const CREATE_MONITORED_ITEMS_REQUEST_ID: u32 = 751;
const CREATE_MONITORED_ITEMS_RESPONSE_ID: u32 = 754;
const DELETE_MONITORED_ITEMS_REQUEST_ID: u32 = 781;
const DELETE_MONITORED_ITEMS_RESPONSE_ID: u32 = 784;

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
            let _client_handle = u32::decode(&mut buf)?;
            let sampling_interval = f64::decode(&mut buf)?;
            // Filter (ExtensionObject)
            let _filter = ExtensionObject::decode(&mut buf)?;
            let queue_size = u32::decode(&mut buf)?;
            let discard_oldest = bool::decode(&mut buf)?;

            let attribute_id = AttributeId::from_u32(attribute_id_val)
                .unwrap_or(AttributeId::Value);

            let config = MonitoredItemConfig {
                node_id: node_id.clone(),
                attribute_id,
                sampling_interval_ms: sampling_interval,
                queue_size,
                discard_oldest,
                filter: None,
            };

            match context.subscription_manager.create_monitored_item(subscription_id, config) {
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
            (ExtensionObject { type_id: NodeId::numeric(0, 0), body: None }).encode(&mut out)?;
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
            let ok = context.subscription_manager.delete_monitored_item(subscription_id, item_id);
            results.push(if ok.is_ok() { StatusCode::GOOD } else { StatusCode::BAD_MONITORED_ITEM_ID_INVALID });
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

/// Register all monitored item service handlers.
pub fn register_handlers(registry: &mut super::registry::ServiceRegistry) {
    registry.register(Arc::new(CreateMonitoredItemsHandler));
    registry.register(Arc::new(DeleteMonitoredItemsHandler));
}

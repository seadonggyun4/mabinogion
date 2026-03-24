//! Attribute service handlers — Read, Write.
//!
//! OPC UA Part 4, Section 5.10.

use std::sync::Arc;

use async_trait::async_trait;
use bytes::{BufMut, Bytes, BytesMut};

use super::discovery::{RequestHeader, ResponseHeader};
use super::registry::{ServiceContext, ServiceHandler, ServiceResponse};
use crate::codec::data_value::ExtensionObject;
use crate::codec::decoder::BinaryDecodable;
use crate::codec::encoder::BinaryEncodable;
use crate::error::OpcUaResult;
use crate::types::{AttributeId, DataValue, NodeId};

const READ_REQUEST_ID: u32 = 631;
const READ_RESPONSE_ID: u32 = 634;
const WRITE_REQUEST_ID: u32 = 673;
const WRITE_RESPONSE_ID: u32 = 676;

// =========================================================================
// Read Service
// =========================================================================

pub struct ReadHandler;

#[async_trait]
impl ServiceHandler for ReadHandler {
    fn request_type_id(&self) -> NodeId {
        NodeId::numeric(0, READ_REQUEST_ID)
    }

    async fn handle(
        &self,
        request_body: &[u8],
        context: &ServiceContext,
    ) -> OpcUaResult<ServiceResponse> {
        let mut buf = Bytes::copy_from_slice(request_body);
        let header = RequestHeader::decode(&mut buf)?;
        let _additional_header = ExtensionObject::decode(&mut buf)?;

        // MaxAge
        let _max_age = f64::decode(&mut buf)?;
        // TimestampsToReturn
        let _timestamps = u32::decode(&mut buf)?;
        // NodesToRead array
        let count = i32::decode(&mut buf)?;

        let mut results = Vec::new();
        for _ in 0..count.max(0) {
            // ReadValueId
            let node_id = NodeId::decode(&mut buf)?;
            let attribute_id_val = u32::decode(&mut buf)?;
            let _index_range = String::decode(&mut buf)?;
            let _data_encoding_name = {
                // QualifiedName
                let _ns = u16::decode(&mut buf)?;
                let _name = String::decode(&mut buf)?;
            };

            let attribute_id =
                AttributeId::from_u32(attribute_id_val).unwrap_or(AttributeId::Value);

            let dv = context.address_space.read(&node_id, attribute_id);
            results.push(dv);
        }

        // Build response
        let mut out = BytesMut::new();
        ResponseHeader::good(header.request_handle).encode(&mut out)?;

        // Results array
        out.put_i32_le(results.len() as i32);
        for dv in &results {
            dv.encode(&mut out)?;
        }

        // DiagnosticInfos (empty)
        out.put_i32_le(0);

        Ok(ServiceResponse {
            type_id: NodeId::numeric(0, READ_RESPONSE_ID),
            body: out.to_vec(),
        })
    }
}

// =========================================================================
// Write Service
// =========================================================================

pub struct WriteHandler;

#[async_trait]
impl ServiceHandler for WriteHandler {
    fn request_type_id(&self) -> NodeId {
        NodeId::numeric(0, WRITE_REQUEST_ID)
    }

    async fn handle(
        &self,
        request_body: &[u8],
        context: &ServiceContext,
    ) -> OpcUaResult<ServiceResponse> {
        let mut buf = Bytes::copy_from_slice(request_body);
        let header = RequestHeader::decode(&mut buf)?;
        let _additional_header = ExtensionObject::decode(&mut buf)?;

        // NodesToWrite array
        let count = i32::decode(&mut buf)?;

        let mut results = Vec::new();
        for _ in 0..count.max(0) {
            // WriteValue
            let node_id = NodeId::decode(&mut buf)?;
            let attribute_id_val = u32::decode(&mut buf)?;
            let _index_range = String::decode(&mut buf)?;
            let value = DataValue::decode(&mut buf)?;

            let attribute_id =
                AttributeId::from_u32(attribute_id_val).unwrap_or(AttributeId::Value);

            let status = context.address_space.write(&node_id, attribute_id, value);
            results.push(status);
        }

        // Build response
        let mut out = BytesMut::new();
        ResponseHeader::good(header.request_handle).encode(&mut out)?;

        // Results array
        out.put_i32_le(results.len() as i32);
        for status in &results {
            status.encode(&mut out)?;
        }

        // DiagnosticInfos (empty)
        out.put_i32_le(0);

        Ok(ServiceResponse {
            type_id: NodeId::numeric(0, WRITE_RESPONSE_ID),
            body: out.to_vec(),
        })
    }
}

/// Register all attribute service handlers.
pub fn register_handlers(registry: &mut super::registry::ServiceRegistry) {
    registry.register(Arc::new(ReadHandler));
    registry.register(Arc::new(WriteHandler));
}

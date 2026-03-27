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

#[derive(Debug, Clone)]
pub(crate) struct ReadTarget {
    pub node_id: NodeId,
    pub attribute_id: AttributeId,
}

#[derive(Debug, Clone)]
pub(crate) struct ReadRequest {
    pub request_handle: u32,
    pub nodes_to_read: Vec<ReadTarget>,
}

#[derive(Debug, Clone)]
pub(crate) struct ReadResponse {
    pub request_handle: u32,
    pub results: Vec<DataValue>,
}

#[derive(Debug, Clone)]
pub(crate) struct WriteTarget {
    pub node_id: NodeId,
    pub attribute_id: AttributeId,
    pub value: DataValue,
}

#[derive(Debug, Clone)]
pub(crate) struct WriteRequest {
    pub request_handle: u32,
    pub nodes_to_write: Vec<WriteTarget>,
}

#[derive(Debug, Clone)]
pub(crate) struct WriteResponse {
    pub request_handle: u32,
    pub results: Vec<crate::types::StatusCode>,
}

pub(crate) fn decode_read_request(request_body: &[u8]) -> OpcUaResult<ReadRequest> {
    let mut buf = Bytes::copy_from_slice(request_body);
    let header = RequestHeader::decode(&mut buf)?;
    let _additional_header = ExtensionObject::decode(&mut buf)?;

    let _max_age = f64::decode(&mut buf)?;
    let _timestamps = u32::decode(&mut buf)?;
    let count = i32::decode(&mut buf)?;

    let mut nodes_to_read = Vec::with_capacity(count.max(0) as usize);
    for _ in 0..count.max(0) {
        let node_id = NodeId::decode(&mut buf)?;
        let attribute_id_val = u32::decode(&mut buf)?;
        let _index_range = String::decode(&mut buf)?;
        let _data_encoding_name = {
            let _ns = u16::decode(&mut buf)?;
            let _name = String::decode(&mut buf)?;
        };

        nodes_to_read.push(ReadTarget {
            node_id,
            attribute_id: AttributeId::from_u32(attribute_id_val).unwrap_or(AttributeId::Value),
        });
    }

    Ok(ReadRequest {
        request_handle: header.request_handle,
        nodes_to_read,
    })
}

pub(crate) async fn handle_read_request(
    request: ReadRequest,
    context: &ServiceContext,
) -> OpcUaResult<ReadResponse> {
    let results = request
        .nodes_to_read
        .into_iter()
        .map(|target| {
            context
                .address_space
                .read(&target.node_id, target.attribute_id)
        })
        .collect();

    Ok(ReadResponse {
        request_handle: request.request_handle,
        results,
    })
}

pub(crate) fn encode_read_response(response: &ReadResponse) -> OpcUaResult<Vec<u8>> {
    let mut out = BytesMut::new();
    ResponseHeader::good(response.request_handle).encode(&mut out)?;
    out.put_i32_le(response.results.len() as i32);
    for dv in &response.results {
        dv.encode(&mut out)?;
    }
    out.put_i32_le(0);
    Ok(out.to_vec())
}

pub(crate) fn decode_write_request(request_body: &[u8]) -> OpcUaResult<WriteRequest> {
    let mut buf = Bytes::copy_from_slice(request_body);
    let header = RequestHeader::decode(&mut buf)?;
    let _additional_header = ExtensionObject::decode(&mut buf)?;

    let count = i32::decode(&mut buf)?;
    let mut nodes_to_write = Vec::with_capacity(count.max(0) as usize);
    for _ in 0..count.max(0) {
        let node_id = NodeId::decode(&mut buf)?;
        let attribute_id_val = u32::decode(&mut buf)?;
        let _index_range = String::decode(&mut buf)?;
        let value = DataValue::decode(&mut buf)?;

        nodes_to_write.push(WriteTarget {
            node_id,
            attribute_id: AttributeId::from_u32(attribute_id_val).unwrap_or(AttributeId::Value),
            value,
        });
    }

    Ok(WriteRequest {
        request_handle: header.request_handle,
        nodes_to_write,
    })
}

pub(crate) async fn handle_write_request(
    request: WriteRequest,
    context: &ServiceContext,
) -> OpcUaResult<WriteResponse> {
    let results = request
        .nodes_to_write
        .into_iter()
        .map(|target| {
            context
                .address_space
                .write(&target.node_id, target.attribute_id, target.value)
        })
        .collect();

    Ok(WriteResponse {
        request_handle: request.request_handle,
        results,
    })
}

pub(crate) fn encode_write_response(response: &WriteResponse) -> OpcUaResult<Vec<u8>> {
    let mut out = BytesMut::new();
    ResponseHeader::good(response.request_handle).encode(&mut out)?;
    out.put_i32_le(response.results.len() as i32);
    for status in &response.results {
        status.encode(&mut out)?;
    }
    out.put_i32_le(0);
    Ok(out.to_vec())
}

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
        let request = decode_read_request(request_body)?;
        let response = handle_read_request(request, context).await?;
        let body = encode_read_response(&response)?;

        Ok(ServiceResponse {
            type_id: NodeId::numeric(0, READ_RESPONSE_ID),
            body,
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
        let request = decode_write_request(request_body)?;
        let response = handle_write_request(request, context).await?;
        let body = encode_write_response(&response)?;

        Ok(ServiceResponse {
            type_id: NodeId::numeric(0, WRITE_RESPONSE_ID),
            body,
        })
    }
}

/// Register all attribute service handlers.
pub fn register_handlers(registry: &mut super::registry::ServiceRegistry) {
    registry.register(Arc::new(ReadHandler));
    registry.register(Arc::new(WriteHandler));
}

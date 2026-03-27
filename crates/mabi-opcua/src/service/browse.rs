//! Browse service handlers — Browse, BrowseNext, TranslateBrowsePathsToNodeIds.
//!
//! OPC UA Part 4, Section 5.8.

use std::sync::Arc;

use async_trait::async_trait;
use bytes::{BufMut, Bytes, BytesMut};

use super::discovery::{RequestHeader, ResponseHeader};
use super::registry::{ServiceContext, ServiceHandler, ServiceResponse};
use crate::codec::data_value::ExtensionObject;
use crate::codec::decoder::BinaryDecodable;
use crate::codec::encoder::BinaryEncodable;
use crate::error::OpcUaResult;
use crate::nodes::{BrowseDirection, BrowseResult, ReferenceTypeId};
use crate::types::{NodeId, StatusCode};

const BROWSE_REQUEST_ID: u32 = 527;
const BROWSE_RESPONSE_ID: u32 = 530;

pub struct BrowseHandler;

#[derive(Debug, Clone)]
pub(crate) struct BrowseDescription {
    pub node_id: NodeId,
    pub direction: BrowseDirection,
    pub reference_type_id: Option<ReferenceTypeId>,
    pub include_subtypes: bool,
    pub node_class_mask: Option<u32>,
}

#[derive(Debug, Clone)]
pub(crate) struct BrowseRequest {
    pub request_handle: u32,
    pub max_refs: usize,
    pub nodes_to_browse: Vec<BrowseDescription>,
}

#[derive(Debug, Clone)]
pub(crate) struct BrowseResponse {
    pub request_handle: u32,
    pub results: Vec<BrowseResult>,
}

#[derive(Debug, Clone)]
pub(crate) struct BrowseNextRequest {
    pub request_handle: u32,
    pub release: bool,
    pub continuation_points: Vec<Vec<u8>>,
}

#[derive(Debug, Clone)]
pub(crate) struct BrowseNextResponse {
    pub request_handle: u32,
    pub results: Vec<BrowseResult>,
}

pub(crate) fn decode_browse_request(request_body: &[u8]) -> OpcUaResult<BrowseRequest> {
    let mut buf = Bytes::copy_from_slice(request_body);
    let header = RequestHeader::decode(&mut buf)?;
    let _additional_header = ExtensionObject::decode(&mut buf)?;

    let _view_id = NodeId::decode(&mut buf)?;
    let _view_timestamp = chrono::DateTime::<chrono::Utc>::decode(&mut buf)?;
    let _view_version = u32::decode(&mut buf)?;

    let max_refs = u32::decode(&mut buf)? as usize;
    let max_refs = if max_refs == 0 { 1000 } else { max_refs };

    let count = i32::decode(&mut buf)?;
    let mut nodes_to_browse = Vec::with_capacity(count.max(0) as usize);
    for _ in 0..count.max(0) {
        let node_id = NodeId::decode(&mut buf)?;
        let direction_val = u32::decode(&mut buf)?;
        let reference_type_id = NodeId::decode(&mut buf)?;
        let include_subtypes = bool::decode(&mut buf)?;
        let node_class_mask = u32::decode(&mut buf)?;
        let _result_mask = u32::decode(&mut buf)?;

        let direction = match direction_val {
            0 => BrowseDirection::Forward,
            1 => BrowseDirection::Inverse,
            2 => BrowseDirection::Both,
            _ => BrowseDirection::Forward,
        };

        nodes_to_browse.push(BrowseDescription {
            node_id,
            direction,
            reference_type_id: ReferenceTypeId::from_node_id(&reference_type_id),
            include_subtypes,
            node_class_mask: if node_class_mask == 0 {
                None
            } else {
                Some(node_class_mask)
            },
        });
    }

    Ok(BrowseRequest {
        request_handle: header.request_handle,
        max_refs,
        nodes_to_browse,
    })
}

pub(crate) async fn handle_browse_request(
    request: BrowseRequest,
    context: &ServiceContext,
) -> OpcUaResult<BrowseResponse> {
    let request_handle = request.request_handle;
    let max_refs = request.max_refs;
    let results = request
        .nodes_to_browse
        .into_iter()
        .map(|browse| {
            context.address_space.browse(
                &browse.node_id,
                browse.direction,
                browse.reference_type_id,
                browse.include_subtypes,
                browse.node_class_mask,
                max_refs,
            )
        })
        .collect();

    Ok(BrowseResponse {
        request_handle,
        results,
    })
}

pub(crate) fn encode_browse_response(response: &BrowseResponse) -> OpcUaResult<Vec<u8>> {
    let mut out = BytesMut::new();
    ResponseHeader::good(response.request_handle).encode(&mut out)?;
    out.put_i32_le(response.results.len() as i32);
    for result in &response.results {
        StatusCode::GOOD.encode(&mut out)?;
        match &result.continuation_point {
            Some(cp) => cp.encode(&mut out)?,
            None => out.put_i32_le(-1),
        }
        out.put_i32_le(result.references.len() as i32);
        for reference in &result.references {
            reference.reference_type_id.encode(&mut out)?;
            reference.is_forward.encode(&mut out)?;
            reference.node_id.encode(&mut out)?;
            out.put_u32_le(0);
            out.put_i32_le(-1);
            reference.browse_name.encode(&mut out)?;
            reference.display_name.encode(&mut out)?;
            out.put_u32_le(reference.node_class as u32);
            match &reference.type_definition {
                Some(td) => {
                    td.encode(&mut out)?;
                    out.put_u32_le(0);
                    out.put_i32_le(-1);
                }
                None => {
                    NodeId::numeric(0, 0).encode(&mut out)?;
                    out.put_u32_le(0);
                    out.put_i32_le(-1);
                }
            }
        }
    }
    out.put_i32_le(0);
    Ok(out.to_vec())
}

pub(crate) fn decode_browse_next_request(request_body: &[u8]) -> OpcUaResult<BrowseNextRequest> {
    let mut buf = Bytes::copy_from_slice(request_body);
    let header = RequestHeader::decode(&mut buf)?;
    let _additional_header = ExtensionObject::decode(&mut buf)?;

    let release = bool::decode(&mut buf)?;
    let count = i32::decode(&mut buf)?;
    let mut continuation_points = Vec::with_capacity(count.max(0) as usize);
    for _ in 0..count.max(0) {
        let cp_len = i32::decode(&mut buf)?;
        let continuation_point = if cp_len > 0 {
            let mut cp = vec![0u8; cp_len as usize];
            for b in cp.iter_mut() {
                *b = u8::decode(&mut buf)?;
            }
            cp
        } else {
            Vec::new()
        };
        continuation_points.push(continuation_point);
    }

    Ok(BrowseNextRequest {
        request_handle: header.request_handle,
        release,
        continuation_points,
    })
}

pub(crate) async fn handle_browse_next_request(
    request: BrowseNextRequest,
    context: &ServiceContext,
) -> OpcUaResult<BrowseNextResponse> {
    let mut results = Vec::with_capacity(request.continuation_points.len());
    for continuation_point in request.continuation_points {
        if continuation_point.is_empty() {
            results.push(BrowseResult {
                references: Vec::new(),
                continuation_point: None,
            });
            continue;
        }

        results.push(
            context
                .address_space
                .browse_next(&continuation_point, request.release, 1000),
        );
    }

    Ok(BrowseNextResponse {
        request_handle: request.request_handle,
        results,
    })
}

pub(crate) fn encode_browse_next_response(response: &BrowseNextResponse) -> OpcUaResult<Vec<u8>> {
    let mut out = BytesMut::new();
    ResponseHeader::good(response.request_handle).encode(&mut out)?;
    out.put_i32_le(response.results.len() as i32);
    for result in &response.results {
        StatusCode::GOOD.encode(&mut out)?;
        match &result.continuation_point {
            Some(cp) => cp.encode(&mut out)?,
            None => out.put_i32_le(-1),
        }
        out.put_i32_le(result.references.len() as i32);
        for reference in &result.references {
            reference.reference_type_id.encode(&mut out)?;
            reference.is_forward.encode(&mut out)?;
            reference.node_id.encode(&mut out)?;
            out.put_u32_le(0);
            out.put_i32_le(-1);
            reference.browse_name.encode(&mut out)?;
            reference.display_name.encode(&mut out)?;
            out.put_u32_le(reference.node_class as u32);
            match &reference.type_definition {
                Some(td) => {
                    td.encode(&mut out)?;
                    out.put_u32_le(0);
                    out.put_i32_le(-1);
                }
                None => {
                    NodeId::numeric(0, 0).encode(&mut out)?;
                    out.put_u32_le(0);
                    out.put_i32_le(-1);
                }
            }
        }
    }
    out.put_i32_le(0);
    Ok(out.to_vec())
}

#[async_trait]
impl ServiceHandler for BrowseHandler {
    fn request_type_id(&self) -> NodeId {
        NodeId::numeric(0, BROWSE_REQUEST_ID)
    }

    async fn handle(
        &self,
        request_body: &[u8],
        context: &ServiceContext,
    ) -> OpcUaResult<ServiceResponse> {
        let request = decode_browse_request(request_body)?;
        let response = handle_browse_request(request, context).await?;
        let body = encode_browse_response(&response)?;

        Ok(ServiceResponse {
            type_id: NodeId::numeric(0, BROWSE_RESPONSE_ID),
            body,
        })
    }
}

// =========================================================================
// BrowseNext
// =========================================================================

const BROWSE_NEXT_REQUEST_ID: u32 = 533;
const BROWSE_NEXT_RESPONSE_ID: u32 = 536;

pub struct BrowseNextHandler;

#[async_trait]
impl ServiceHandler for BrowseNextHandler {
    fn request_type_id(&self) -> NodeId {
        NodeId::numeric(0, BROWSE_NEXT_REQUEST_ID)
    }

    async fn handle(
        &self,
        request_body: &[u8],
        context: &ServiceContext,
    ) -> OpcUaResult<ServiceResponse> {
        let request = decode_browse_next_request(request_body)?;
        let response = handle_browse_next_request(request, context).await?;
        let body = encode_browse_next_response(&response)?;

        Ok(ServiceResponse {
            type_id: NodeId::numeric(0, BROWSE_NEXT_RESPONSE_ID),
            body,
        })
    }
}

/// Register all browse service handlers.
pub fn register_handlers(registry: &mut super::registry::ServiceRegistry) {
    registry.register(Arc::new(BrowseHandler));
    registry.register(Arc::new(BrowseNextHandler));
}

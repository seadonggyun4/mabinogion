//! RegisterNodes / UnregisterNodes service handlers.
//!
//! OPC UA Part 4, Section 5.8.5 & 5.8.6.
//!
//! RegisterNodes is a pass-through: the server returns the same NodeIds
//! the client sent. This is correct for servers that do not provide
//! optimised access; TRAP's `NodeRegistrationManager` detects this via
//! `is_effective()` and adapts automatically.

use std::sync::Arc;

use async_trait::async_trait;
use bytes::{BufMut, Bytes, BytesMut};

use crate::codec::encoder::BinaryEncodable;
use crate::codec::decoder::BinaryDecodable;
use crate::codec::data_value::ExtensionObject;
use crate::error::OpcUaResult;
use crate::types::NodeId;
use super::discovery::{RequestHeader, ResponseHeader};
use super::registry::{ServiceContext, ServiceHandler, ServiceResponse};

const REGISTER_NODES_REQUEST_ID: u32 = 560;
const REGISTER_NODES_RESPONSE_ID: u32 = 563;
const UNREGISTER_NODES_REQUEST_ID: u32 = 566;
const UNREGISTER_NODES_RESPONSE_ID: u32 = 569;

// =========================================================================
// RegisterNodes
// =========================================================================

pub struct RegisterNodesHandler;

#[async_trait]
impl ServiceHandler for RegisterNodesHandler {
    fn request_type_id(&self) -> NodeId {
        NodeId::numeric(0, REGISTER_NODES_REQUEST_ID)
    }

    async fn handle(
        &self,
        request_body: &[u8],
        _context: &ServiceContext,
    ) -> OpcUaResult<ServiceResponse> {
        let mut buf = Bytes::copy_from_slice(request_body);
        let header = RequestHeader::decode(&mut buf)?;
        let _additional_header = ExtensionObject::decode(&mut buf)?;

        // NodesToRegister array
        let count = i32::decode(&mut buf)?;
        let mut node_ids = Vec::with_capacity(count.max(0) as usize);
        for _ in 0..count.max(0) {
            let node_id = NodeId::decode(&mut buf)?;
            node_ids.push(node_id);
        }

        // Pass-through: return same NodeIds as RegisteredNodeIds
        let mut out = BytesMut::new();
        ResponseHeader::good(header.request_handle).encode(&mut out)?;

        // RegisteredNodeIds array (same as input)
        out.put_i32_le(node_ids.len() as i32);
        for node_id in &node_ids {
            node_id.encode(&mut out)?;
        }

        Ok(ServiceResponse {
            type_id: NodeId::numeric(0, REGISTER_NODES_RESPONSE_ID),
            body: out.to_vec(),
        })
    }
}

// =========================================================================
// UnregisterNodes
// =========================================================================

pub struct UnregisterNodesHandler;

#[async_trait]
impl ServiceHandler for UnregisterNodesHandler {
    fn request_type_id(&self) -> NodeId {
        NodeId::numeric(0, UNREGISTER_NODES_REQUEST_ID)
    }

    async fn handle(
        &self,
        request_body: &[u8],
        _context: &ServiceContext,
    ) -> OpcUaResult<ServiceResponse> {
        let mut buf = Bytes::copy_from_slice(request_body);
        let header = RequestHeader::decode(&mut buf)?;
        let _additional_header = ExtensionObject::decode(&mut buf)?;

        // NodesToUnregister array — just consume
        let count = i32::decode(&mut buf)?;
        for _ in 0..count.max(0) {
            let _node_id = NodeId::decode(&mut buf)?;
        }

        // UnregisterNodes has no return array — just ResponseHeader
        let mut out = BytesMut::new();
        ResponseHeader::good(header.request_handle).encode(&mut out)?;

        Ok(ServiceResponse {
            type_id: NodeId::numeric(0, UNREGISTER_NODES_RESPONSE_ID),
            body: out.to_vec(),
        })
    }
}

/// Register all register-nodes service handlers.
pub fn register_handlers(registry: &mut super::registry::ServiceRegistry) {
    registry.register(Arc::new(RegisterNodesHandler));
    registry.register(Arc::new(UnregisterNodesHandler));
}

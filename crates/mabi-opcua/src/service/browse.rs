//! Browse service handlers — Browse, BrowseNext, TranslateBrowsePathsToNodeIds.
//!
//! OPC UA Part 4, Section 5.8.

use std::sync::Arc;

use async_trait::async_trait;
use bytes::{BufMut, Bytes, BytesMut};

use crate::codec::encoder::BinaryEncodable;
use crate::codec::decoder::BinaryDecodable;
use crate::codec::data_value::ExtensionObject;
use crate::error::OpcUaResult;
use crate::types::{NodeId, StatusCode};
use crate::nodes::{BrowseDirection, ReferenceTypeId, LocalizedText, QualifiedName, NodeClass};
use super::discovery::{RequestHeader, ResponseHeader};
use super::registry::{ServiceContext, ServiceHandler, ServiceResponse};

const BROWSE_REQUEST_ID: u32 = 527;
const BROWSE_RESPONSE_ID: u32 = 530;

pub struct BrowseHandler;

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
        let mut buf = Bytes::copy_from_slice(request_body);
        let header = RequestHeader::decode(&mut buf)?;
        let _additional_header = ExtensionObject::decode(&mut buf)?;

        // View (ViewDescription) — skip
        let _view_id = NodeId::decode(&mut buf)?;
        let _view_timestamp = chrono::DateTime::<chrono::Utc>::decode(&mut buf)?;
        let _view_version = u32::decode(&mut buf)?;

        // RequestedMaxReferencesPerNode
        let max_refs = u32::decode(&mut buf)? as usize;
        let max_refs = if max_refs == 0 { 1000 } else { max_refs };

        // NodesToBrowse array
        let count = i32::decode(&mut buf)?;

        let mut out = BytesMut::new();
        ResponseHeader::good(header.request_handle).encode(&mut out)?;

        // Results array
        out.put_i32_le(count);
        for _ in 0..count.max(0) {
            // BrowseDescription
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

            let ref_type_filter = ReferenceTypeId::from_node_id(&reference_type_id);

            let node_class_mask_opt = if node_class_mask == 0 { None } else { Some(node_class_mask) };

            let result = context.address_space.browse(
                &node_id,
                direction,
                ref_type_filter,
                include_subtypes,
                node_class_mask_opt,
                max_refs,
            );

            // BrowseResult
            StatusCode::GOOD.encode(&mut out)?;
            // ContinuationPoint
            match &result.continuation_point {
                Some(cp) => cp.encode(&mut out)?,
                None => out.put_i32_le(-1),
            }
            // References array
            out.put_i32_le(result.references.len() as i32);
            for reference in &result.references {
                // ReferenceDescription
                // ReferenceTypeId
                reference.reference_type_id.encode(&mut out)?;
                // IsForward
                reference.is_forward.encode(&mut out)?;
                // NodeId (ExpandedNodeId — for simplicity, encode as regular NodeId)
                reference.node_id.encode(&mut out)?;
                // Add server_index and namespace_uri for ExpandedNodeId
                out.put_u32_le(0); // server_index
                out.put_i32_le(-1); // namespace_uri (null)
                // BrowseName (QualifiedName)
                reference.browse_name.encode(&mut out)?;
                // DisplayName (LocalizedText)
                reference.display_name.encode(&mut out)?;
                // NodeClass
                out.put_u32_le(reference.node_class as u32);
                // TypeDefinition (ExpandedNodeId)
                match &reference.type_definition {
                    Some(td) => {
                        td.encode(&mut out)?;
                        out.put_u32_le(0); // server_index
                        out.put_i32_le(-1); // namespace_uri (null)
                    }
                    None => {
                        NodeId::numeric(0, 0).encode(&mut out)?;
                        out.put_u32_le(0);
                        out.put_i32_le(-1);
                    }
                }
            }
        }

        // DiagnosticInfos (empty)
        out.put_i32_le(0);

        Ok(ServiceResponse {
            type_id: NodeId::numeric(0, BROWSE_RESPONSE_ID),
            body: out.to_vec(),
        })
    }
}

/// Register all browse service handlers.
pub fn register_handlers(registry: &mut super::registry::ServiceRegistry) {
    registry.register(Arc::new(BrowseHandler));
}

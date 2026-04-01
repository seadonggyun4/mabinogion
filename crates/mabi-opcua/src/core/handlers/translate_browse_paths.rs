//! TranslateBrowsePathsToNodeIds service handler.
//!
//! OPC UA Part 4, Section 5.8.4.
//!
//! Resolves browse paths (starting node + relative path elements) to
//! concrete NodeIds. Used by TRAP's `BrowsePathTranslator` and
//! `TranslateBrowsePathsInitTask`.

use std::sync::Arc;

use async_trait::async_trait;
use bytes::{BufMut, Bytes, BytesMut};

use super::discovery::{RequestHeader, ResponseHeader};
use super::registry::{ServiceContext, ServiceHandler, ServiceResponse};
use crate::codec::data_value::ExtensionObject;
use crate::codec::decoder::BinaryDecodable;
use crate::codec::encoder::BinaryEncodable;
use crate::error::OpcUaResult;
use crate::nodes::{QualifiedName, RelativePathElement};
use crate::types::NodeId;

const TRANSLATE_BROWSE_PATHS_REQUEST_ID: u32 = 554;
const TRANSLATE_BROWSE_PATHS_RESPONSE_ID: u32 = 557;

pub struct TranslateBrowsePathsHandler;

#[async_trait]
impl ServiceHandler for TranslateBrowsePathsHandler {
    fn request_type_id(&self) -> NodeId {
        NodeId::numeric(0, TRANSLATE_BROWSE_PATHS_REQUEST_ID)
    }

    async fn handle(
        &self,
        request_body: &[u8],
        context: &ServiceContext,
    ) -> OpcUaResult<ServiceResponse> {
        let mut buf = Bytes::copy_from_slice(request_body);
        let header = RequestHeader::decode(&mut buf)?;
        let _additional_header = ExtensionObject::decode(&mut buf)?;

        // BrowsePaths array
        let path_count = i32::decode(&mut buf)?;

        let mut out = BytesMut::new();
        ResponseHeader::good(header.request_handle).encode(&mut out)?;

        // Results array
        out.put_i32_le(path_count);

        for _ in 0..path_count.max(0) {
            // BrowsePath
            let starting_node = NodeId::decode(&mut buf)?;

            // RelativePath — elements array
            let element_count = i32::decode(&mut buf)?;
            let mut elements = Vec::with_capacity(element_count.max(0) as usize);

            for _ in 0..element_count.max(0) {
                // RelativePathElement
                let reference_type_id = NodeId::decode(&mut buf)?;
                let is_inverse = bool::decode(&mut buf)?;
                let include_subtypes = bool::decode(&mut buf)?;
                let target_name = QualifiedName::decode(&mut buf)?;

                elements.push(RelativePathElement {
                    reference_type_id,
                    is_inverse,
                    include_subtypes,
                    target_name,
                });
            }

            // Resolve the path
            let result = context
                .address_space
                .resolve_browse_path(&starting_node, &elements);

            // BrowsePathResult
            result.status.encode(&mut out)?;

            // Targets array
            out.put_i32_le(result.targets.len() as i32);
            for target in &result.targets {
                // BrowsePathTarget = ExpandedNodeId + RemainingPathIndex
                // ExpandedNodeId: encode as NodeId + server_index(0) + namespace_uri(null)
                target.target_id.encode(&mut out)?;
                out.put_u32_le(0); // server_index
                out.put_i32_le(-1); // namespace_uri (null string)
                out.put_u32_le(target.remaining_path_index);
            }
        }

        // DiagnosticInfos (empty)
        out.put_i32_le(0);

        Ok(ServiceResponse {
            type_id: NodeId::numeric(0, TRANSLATE_BROWSE_PATHS_RESPONSE_ID),
            body: out.to_vec(),
        })
    }
}

/// Register all translate-browse-paths service handlers.
pub fn register_handlers(registry: &mut super::registry::ServiceRegistry) {
    registry.register(Arc::new(TranslateBrowsePathsHandler));
}

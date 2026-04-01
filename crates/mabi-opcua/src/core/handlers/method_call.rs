//! Call (Method invocation) service handler.
//!
//! OPC UA Part 4, Section 5.11.2.
//!
//! Provides a method call registry and dispatch. Used by TRAP Phase 4's
//! `MethodCallManager`.

use std::sync::Arc;

use async_trait::async_trait;
use bytes::{BufMut, Bytes, BytesMut};

use super::discovery::{RequestHeader, ResponseHeader};
use super::registry::{ServiceContext, ServiceHandler, ServiceResponse};
use crate::codec::data_value::ExtensionObject;
use crate::codec::decoder::BinaryDecodable;
use crate::codec::encoder::BinaryEncodable;
use crate::error::OpcUaResult;
use crate::types::{NodeId, StatusCode, Variant};

const CALL_REQUEST_ID: u32 = 712;
const CALL_RESPONSE_ID: u32 = 715;

// =========================================================================
// Call Handler
// =========================================================================

pub struct CallHandler;

#[async_trait]
impl ServiceHandler for CallHandler {
    fn request_type_id(&self) -> NodeId {
        NodeId::numeric(0, CALL_REQUEST_ID)
    }

    async fn handle(
        &self,
        request_body: &[u8],
        context: &ServiceContext,
    ) -> OpcUaResult<ServiceResponse> {
        let mut buf = Bytes::copy_from_slice(request_body);
        let header = RequestHeader::decode(&mut buf)?;
        let _additional_header = ExtensionObject::decode(&mut buf)?;

        // MethodsToCall array
        let count = i32::decode(&mut buf)?;

        let mut out = BytesMut::new();
        ResponseHeader::good(header.request_handle).encode(&mut out)?;

        // Results array
        out.put_i32_le(count);
        for _ in 0..count.max(0) {
            // CallMethodRequest
            let object_id = NodeId::decode(&mut buf)?;
            let method_id = NodeId::decode(&mut buf)?;

            // InputArguments (Variant array)
            let arg_count = i32::decode(&mut buf)?;
            let mut input_args = Vec::with_capacity(arg_count.max(0) as usize);
            for _ in 0..arg_count.max(0) {
                input_args.push(Variant::decode(&mut buf)?);
            }

            // Check if object exists
            if !context.address_space.contains_node(&object_id) {
                // CallMethodResult with BAD_NODE_ID_UNKNOWN
                StatusCode::BAD_NODE_ID_UNKNOWN.encode(&mut out)?;
                out.put_i32_le(0); // InputArgumentResults (empty)
                out.put_i32_le(0); // InputArgumentDiagnosticInfos (empty)
                out.put_i32_le(0); // OutputArguments (empty)
                continue;
            }

            // Check if method exists in address space
            if !context.address_space.contains_node(&method_id) {
                StatusCode::BAD_NODE_ID_UNKNOWN.encode(&mut out)?;
                out.put_i32_le(0);
                out.put_i32_le(0);
                out.put_i32_le(0);
                continue;
            }

            // Look up the method callback
            let callback = context.method_registry.get(&method_id);
            match callback {
                Some(cb) => {
                    match cb(&input_args) {
                        Ok(output_args) => {
                            // Success
                            StatusCode::GOOD.encode(&mut out)?;
                            // InputArgumentResults — all Good
                            out.put_i32_le(input_args.len() as i32);
                            for _ in 0..input_args.len() {
                                StatusCode::GOOD.encode(&mut out)?;
                            }
                            // InputArgumentDiagnosticInfos (empty)
                            out.put_i32_le(0);
                            // OutputArguments
                            out.put_i32_le(output_args.len() as i32);
                            for arg in &output_args {
                                arg.encode(&mut out)?;
                            }
                        }
                        Err(status) => {
                            // Method callback returned error
                            status.encode(&mut out)?;
                            out.put_i32_le(0);
                            out.put_i32_le(0);
                            out.put_i32_le(0);
                        }
                    }
                }
                None => {
                    // Method not registered
                    StatusCode::BAD_NOT_IMPLEMENTED.encode(&mut out)?;
                    out.put_i32_le(0);
                    out.put_i32_le(0);
                    out.put_i32_le(0);
                }
            }
        }

        // DiagnosticInfos (empty)
        out.put_i32_le(0);

        Ok(ServiceResponse {
            type_id: NodeId::numeric(0, CALL_RESPONSE_ID),
            body: out.to_vec(),
        })
    }
}

/// Register all method call service handlers.
pub fn register_handlers(registry: &mut super::registry::ServiceRegistry) {
    registry.register(Arc::new(CallHandler));
}

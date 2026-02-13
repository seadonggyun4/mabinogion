//! TransferSubscriptions service handler.
//!
//! OPC UA Part 4, Section 5.13.7.
//!
//! Allows a client to transfer subscriptions from a previous session to
//! the current one. Used by TRAP Phase 1 session recovery.

use std::sync::Arc;

use async_trait::async_trait;
use bytes::{BufMut, Bytes, BytesMut};

use crate::codec::encoder::BinaryEncodable;
use crate::codec::decoder::BinaryDecodable;
use crate::codec::data_value::ExtensionObject;
use crate::error::OpcUaResult;
use crate::types::{NodeId, StatusCode};
use super::discovery::{RequestHeader, ResponseHeader};
use super::registry::{ServiceContext, ServiceHandler, ServiceResponse};

const TRANSFER_SUBSCRIPTIONS_REQUEST_ID: u32 = 839;
const TRANSFER_SUBSCRIPTIONS_RESPONSE_ID: u32 = 842;

pub struct TransferSubscriptionsHandler;

#[async_trait]
impl ServiceHandler for TransferSubscriptionsHandler {
    fn request_type_id(&self) -> NodeId {
        NodeId::numeric(0, TRANSFER_SUBSCRIPTIONS_REQUEST_ID)
    }

    async fn handle(
        &self,
        request_body: &[u8],
        context: &ServiceContext,
    ) -> OpcUaResult<ServiceResponse> {
        let mut buf = Bytes::copy_from_slice(request_body);
        let header = RequestHeader::decode(&mut buf)?;
        let _additional_header = ExtensionObject::decode(&mut buf)?;

        // SubscriptionIds array
        let count = i32::decode(&mut buf)?;
        let mut subscription_ids = Vec::with_capacity(count.max(0) as usize);
        for _ in 0..count.max(0) {
            subscription_ids.push(u32::decode(&mut buf)?);
        }

        // SendInitialValues
        let _send_initial_values = bool::decode(&mut buf)?;

        let mut out = BytesMut::new();
        ResponseHeader::good(header.request_handle).encode(&mut out)?;

        // Results array
        out.put_i32_le(subscription_ids.len() as i32);
        for sub_id in &subscription_ids {
            // Check if the subscription exists
            match context.subscription_manager.get(*sub_id) {
                Some(_config) => {
                    // Subscription exists — transfer succeeds
                    StatusCode::GOOD_SUBSCRIPTION_TRANSFERRED.encode(&mut out)?;

                    // AvailableSequenceNumbers — return current sequence number
                    // The subscription manager tracks sequence numbers internally
                    if let Some(pr) = context.subscription_manager.process_publish(*sub_id) {
                        out.put_i32_le(pr.available_sequence_numbers.len() as i32);
                        for sn in &pr.available_sequence_numbers {
                            out.put_u32_le(*sn);
                        }
                    } else {
                        out.put_i32_le(1);
                        out.put_u32_le(1); // Default sequence number
                    }
                }
                None => {
                    // Subscription does not exist
                    StatusCode::BAD_SUBSCRIPTION_ID_INVALID.encode(&mut out)?;
                    out.put_i32_le(0); // No available sequence numbers
                }
            }
        }

        // DiagnosticInfos (empty)
        out.put_i32_le(0);

        Ok(ServiceResponse {
            type_id: NodeId::numeric(0, TRANSFER_SUBSCRIPTIONS_RESPONSE_ID),
            body: out.to_vec(),
        })
    }
}

/// Register all transfer subscription service handlers.
pub fn register_handlers(registry: &mut super::registry::ServiceRegistry) {
    registry.register(Arc::new(TransferSubscriptionsHandler));
}

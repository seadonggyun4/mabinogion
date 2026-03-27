//! Subscription service handlers — CreateSubscription, DeleteSubscriptions, Publish.
//!
//! OPC UA Part 4, Section 5.13.

use std::sync::Arc;

use async_trait::async_trait;
use bytes::{BufMut, Bytes, BytesMut};

use super::discovery::{RequestHeader, ResponseHeader};
use super::registry::{ServiceContext, ServiceHandler, ServiceResponse};
use crate::codec::data_value::ExtensionObject;
use crate::codec::decoder::BinaryDecodable;
use crate::codec::encoder::BinaryEncodable;
use crate::error::OpcUaResult;
use crate::sdk::subscription::SubscriptionConfig;
use crate::types::{NodeId, StatusCode};

const CREATE_SUBSCRIPTION_REQUEST_ID: u32 = 787;
const CREATE_SUBSCRIPTION_RESPONSE_ID: u32 = 790;
const DELETE_SUBSCRIPTIONS_REQUEST_ID: u32 = 847;
const DELETE_SUBSCRIPTIONS_RESPONSE_ID: u32 = 850;
const PUBLISH_REQUEST_ID: u32 = 826;
const PUBLISH_RESPONSE_ID: u32 = 829;

// =========================================================================
// CreateSubscription
// =========================================================================

pub struct CreateSubscriptionHandler;

#[async_trait]
impl ServiceHandler for CreateSubscriptionHandler {
    fn request_type_id(&self) -> NodeId {
        NodeId::numeric(0, CREATE_SUBSCRIPTION_REQUEST_ID)
    }

    async fn handle(
        &self,
        request_body: &[u8],
        context: &ServiceContext,
    ) -> OpcUaResult<ServiceResponse> {
        let mut buf = Bytes::copy_from_slice(request_body);
        let header = RequestHeader::decode(&mut buf)?;
        let _additional_header = ExtensionObject::decode(&mut buf)?;

        let publishing_interval = f64::decode(&mut buf)?;
        let lifetime_count = u32::decode(&mut buf)?;
        let max_keep_alive_count = u32::decode(&mut buf)?;
        let max_notifications_per_publish = u32::decode(&mut buf)?;
        let _publishing_enabled = bool::decode(&mut buf)?;
        let priority = u8::decode(&mut buf)?;

        let config = SubscriptionConfig {
            subscription_id: 0, // Will be assigned by manager
            publishing_interval_ms: publishing_interval,
            lifetime_count,
            max_keep_alive_count,
            max_notifications_per_publish,
            priority,
            publishing_enabled: _publishing_enabled,
        };

        let sub_id = context
            .subscription_manager
            .create_subscription(config)
            .map_err(|e| crate::error::OpcUaError::Subscription(format!("{:?}", e)))?;

        let mut out = BytesMut::new();
        ResponseHeader::good(header.request_handle).encode(&mut out)?;
        out.put_u32_le(sub_id);
        out.put_f64_le(publishing_interval); // RevisedPublishingInterval
        out.put_u32_le(lifetime_count); // RevisedLifetimeCount
        out.put_u32_le(max_keep_alive_count); // RevisedMaxKeepAliveCount

        Ok(ServiceResponse {
            type_id: NodeId::numeric(0, CREATE_SUBSCRIPTION_RESPONSE_ID),
            body: out.to_vec(),
        })
    }
}

// =========================================================================
// DeleteSubscriptions
// =========================================================================

pub struct DeleteSubscriptionsHandler;

#[async_trait]
impl ServiceHandler for DeleteSubscriptionsHandler {
    fn request_type_id(&self) -> NodeId {
        NodeId::numeric(0, DELETE_SUBSCRIPTIONS_REQUEST_ID)
    }

    async fn handle(
        &self,
        request_body: &[u8],
        context: &ServiceContext,
    ) -> OpcUaResult<ServiceResponse> {
        let mut buf = Bytes::copy_from_slice(request_body);
        let header = RequestHeader::decode(&mut buf)?;
        let _additional_header = ExtensionObject::decode(&mut buf)?;

        let count = i32::decode(&mut buf)?;
        let mut results = Vec::new();
        for _ in 0..count.max(0) {
            let sub_id = u32::decode(&mut buf)?;
            let ok = context.subscription_manager.delete_subscription(sub_id);
            results.push(if ok {
                StatusCode::GOOD
            } else {
                StatusCode::BAD_SUBSCRIPTION_ID_INVALID
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
            type_id: NodeId::numeric(0, DELETE_SUBSCRIPTIONS_RESPONSE_ID),
            body: out.to_vec(),
        })
    }
}

// =========================================================================
// Publish
// =========================================================================

pub struct PublishHandler;

#[async_trait]
impl ServiceHandler for PublishHandler {
    fn request_type_id(&self) -> NodeId {
        NodeId::numeric(0, PUBLISH_REQUEST_ID)
    }

    async fn handle(
        &self,
        request_body: &[u8],
        context: &ServiceContext,
    ) -> OpcUaResult<ServiceResponse> {
        let mut buf = Bytes::copy_from_slice(request_body);
        let header = RequestHeader::decode(&mut buf)?;
        let _additional_header = ExtensionObject::decode(&mut buf)?;

        // SubscriptionAcknowledgements
        let ack_count = i32::decode(&mut buf)?;
        for _ in 0..ack_count.max(0) {
            let _sub_id = u32::decode(&mut buf)?;
            let _seq_num = u32::decode(&mut buf)?;
        }

        // Try to get a publish response from any subscription
        let sub_ids = context.subscription_manager.subscription_ids();
        let mut response = None;
        for sub_id in &sub_ids {
            if let Some(pr) = context.subscription_manager.process_publish(*sub_id) {
                response = Some(pr);
                break;
            }
        }

        let mut out = BytesMut::new();
        ResponseHeader::good(header.request_handle).encode(&mut out)?;

        match response {
            Some(pr) => {
                out.put_u32_le(pr.subscription_id);
                // AvailableSequenceNumbers
                out.put_i32_le(pr.available_sequence_numbers.len() as i32);
                for sn in &pr.available_sequence_numbers {
                    out.put_u32_le(*sn);
                }
                out.put_u8(if pr.more_notifications { 1 } else { 0 });
                // NotificationMessage
                if let Some(msg) = &pr.notification_message {
                    out.put_u32_le(msg.sequence_number);
                    msg.publish_time.encode(&mut out)?;

                    let has_data_change = !msg.notifications.is_empty();
                    let has_events = !msg.event_notifications.is_empty();

                    if !has_data_change && !has_events {
                        // Keep-alive: empty NotificationData array
                        out.put_i32_le(0);
                    } else {
                        // Count NotificationData elements
                        let mut notification_count = 0i32;
                        if has_data_change {
                            notification_count += 1;
                        }
                        if has_events {
                            notification_count += 1;
                        }
                        out.put_i32_le(notification_count);

                        // === DataChangeNotification (encoding_id i=811) ===
                        if has_data_change {
                            NodeId::numeric(0, 811).encode(&mut out)?;
                            out.put_u8(0x01); // Has binary body

                            let mut body_buf = BytesMut::new();
                            // MonitoredItems array
                            body_buf.put_i32_le(msg.notifications.len() as i32);
                            for notification in &msg.notifications {
                                body_buf.put_u32_le(notification.client_handle);
                                notification.value.encode(&mut body_buf)?;
                            }
                            // DiagnosticInfos (empty)
                            body_buf.put_i32_le(0);

                            out.put_i32_le(body_buf.len() as i32);
                            out.extend_from_slice(&body_buf);
                        }

                        // === EventNotificationList (encoding_id i=916) ===
                        if has_events {
                            NodeId::numeric(0, 916).encode(&mut out)?;
                            out.put_u8(0x01); // Has binary body

                            let mut body_buf = BytesMut::new();
                            // Events array
                            body_buf.put_i32_le(msg.event_notifications.len() as i32);
                            for event_field_list in &msg.event_notifications {
                                // EventFieldList
                                body_buf.put_u32_le(event_field_list.client_handle);
                                // EventFields: Variant[]
                                body_buf.put_i32_le(event_field_list.event_fields.len() as i32);
                                for field in &event_field_list.event_fields {
                                    field.encode(&mut body_buf)?;
                                }
                            }

                            out.put_i32_le(body_buf.len() as i32);
                            out.extend_from_slice(&body_buf);
                        }
                    }
                } else {
                    // Empty notification
                    out.put_u32_le(0);
                    chrono::Utc::now().encode(&mut out)?;
                    out.put_i32_le(0);
                }
                // Results (empty — SubscriptionAcknowledgement results)
                out.put_i32_le(0);
                // DiagnosticInfos (empty)
                out.put_i32_le(0);
            }
            None => {
                // No subscription data — return keep-alive
                out.put_u32_le(sub_ids.first().copied().unwrap_or(0));
                out.put_i32_le(0); // AvailableSequenceNumbers
                out.put_u8(0); // MoreNotifications
                               // Empty NotificationMessage
                out.put_u32_le(0);
                chrono::Utc::now().encode(&mut out)?;
                out.put_i32_le(0);
                out.put_i32_le(0); // Results
                out.put_i32_le(0); // DiagnosticInfos
            }
        }

        Ok(ServiceResponse {
            type_id: NodeId::numeric(0, PUBLISH_RESPONSE_ID),
            body: out.to_vec(),
        })
    }
}

/// Register all subscription service handlers.
pub fn register_handlers(registry: &mut super::registry::ServiceRegistry) {
    registry.register(Arc::new(CreateSubscriptionHandler));
    registry.register(Arc::new(DeleteSubscriptionsHandler));
    registry.register(Arc::new(PublishHandler));
}

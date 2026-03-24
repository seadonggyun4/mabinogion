//! SubscribeCOV service handler.
//!
//! Implements BACnet SubscribeCOV (service choice 5) per ASHRAE 135, Clause 13.
//! Allows clients to subscribe to Change-of-Value notifications for objects.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use tracing::debug;

use crate::apdu::encoding::ApduDecoder;
use crate::apdu::types::{ConfirmedService, ErrorClass, ErrorCode};
use crate::service::cov::{CovManager, CovSubscription};

use super::handler::{ConfirmedServiceHandler, ServiceContext, ServiceResult};

/// Decoded SubscribeCOV request per ASHRAE 135, Clause 13.14.1.
#[derive(Debug, Clone)]
pub struct SubscribeCovRequest {
    /// Subscriber process identifier (context tag 0).
    pub subscriber_process_id: u32,
    /// Monitored object identifier (context tag 1).
    pub monitored_object: crate::object::types::ObjectId,
    /// Issue confirmed notifications (context tag 2, optional).
    /// If absent, this is a cancellation request.
    pub issue_confirmed_notifications: Option<bool>,
    /// Lifetime in seconds (context tag 3, optional).
    /// 0 or absent = infinite lifetime.
    pub lifetime: Option<u32>,
}

/// Decode a SubscribeCOV request from APDU service data.
fn decode_subscribe_cov(data: &[u8]) -> Result<SubscribeCovRequest, SubscribeCovError> {
    let mut decoder = ApduDecoder::new(data);

    // Context tag 0: Subscriber Process Identifier (unsigned)
    let (tag, is_context, len) = decoder
        .decode_tag_info()
        .map_err(|_| SubscribeCovError::InvalidRequest)?;
    if !is_context || tag != 0 {
        return Err(SubscribeCovError::InvalidRequest);
    }
    let subscriber_process_id = decoder
        .decode_unsigned(len)
        .map_err(|_| SubscribeCovError::InvalidRequest)?;

    // Context tag 1: Monitored Object Identifier
    let (tag, is_context, len) = decoder
        .decode_tag_info()
        .map_err(|_| SubscribeCovError::InvalidRequest)?;
    if !is_context || tag != 1 || len != 4 {
        return Err(SubscribeCovError::InvalidRequest);
    }
    let monitored_object = decoder
        .decode_object_identifier()
        .map_err(|_| SubscribeCovError::InvalidRequest)?;

    // Optional context tag 2: Issue Confirmed Notifications (boolean)
    // BACnet context-encoded boolean: value is in the content byte(s), not the length field.
    let issue_confirmed_notifications = if !decoder.is_empty() {
        if let Some(byte) = decoder.peek() {
            let peek_tag = (byte >> 4) & 0x0F;
            let peek_context = (byte & 0x08) != 0;
            if peek_context && peek_tag == 2 {
                let (_, _, len) = decoder
                    .decode_tag_info()
                    .map_err(|_| SubscribeCovError::InvalidRequest)?;
                if len == 0 {
                    Some(false)
                } else {
                    let val = decoder
                        .decode_unsigned(len)
                        .map_err(|_| SubscribeCovError::InvalidRequest)?;
                    Some(val != 0)
                }
            } else {
                None
            }
        } else {
            None
        }
    } else {
        None
    };

    // Optional context tag 3: Lifetime (unsigned, in seconds)
    let lifetime = if !decoder.is_empty() {
        if let Some(byte) = decoder.peek() {
            let peek_tag = (byte >> 4) & 0x0F;
            let peek_context = (byte & 0x08) != 0;
            if peek_context && peek_tag == 3 {
                let (_, _, len) = decoder
                    .decode_tag_info()
                    .map_err(|_| SubscribeCovError::InvalidRequest)?;
                Some(
                    decoder
                        .decode_unsigned(len)
                        .map_err(|_| SubscribeCovError::InvalidRequest)?,
                )
            } else {
                None
            }
        } else {
            None
        }
    } else {
        None
    };

    Ok(SubscribeCovRequest {
        subscriber_process_id,
        monitored_object,
        issue_confirmed_notifications,
        lifetime,
    })
}

/// SubscribeCOV service handler.
///
/// Requires a `CovManager` to register/unregister subscriptions.
/// The `source_addr` is captured from the request context by the server
/// and passed through the ServiceContext.
pub struct SubscribeCovHandler {
    cov_manager: Arc<CovManager>,
    /// Fallback address when source_address is not set in the context.
    default_addr: SocketAddr,
}

impl SubscribeCovHandler {
    /// Create a new handler with the given COV manager.
    pub fn new(cov_manager: Arc<CovManager>) -> Self {
        Self {
            cov_manager,
            default_addr: "0.0.0.0:47808".parse().unwrap(),
        }
    }

    /// Create with a specific default address.
    pub fn with_default_addr(mut self, addr: SocketAddr) -> Self {
        self.default_addr = addr;
        self
    }

    fn subscriber_addr(&self, ctx: &ServiceContext) -> SocketAddr {
        ctx.source_address.unwrap_or(self.default_addr)
    }
}

impl ConfirmedServiceHandler for SubscribeCovHandler {
    fn service_choice(&self) -> ConfirmedService {
        ConfirmedService::SubscribeCov
    }

    fn handle(&self, data: &[u8], ctx: &ServiceContext) -> ServiceResult {
        let request = match decode_subscribe_cov(data) {
            Ok(r) => r,
            Err(_) => {
                return ServiceResult::Error {
                    error_class: ErrorClass::Services,
                    error_code: ErrorCode::InvalidParameterDataType,
                };
            }
        };

        debug!(
            process_id = request.subscriber_process_id,
            object = ?request.monitored_object,
            confirmed = ?request.issue_confirmed_notifications,
            lifetime = ?request.lifetime,
            "SubscribeCOV request"
        );

        // If issue_confirmed_notifications is absent, this is a cancellation
        if request.issue_confirmed_notifications.is_none() {
            let removed = self.cov_manager.unsubscribe(
                self.subscriber_addr(ctx),
                request.subscriber_process_id,
                request.monitored_object,
            );
            if removed {
                debug!("COV subscription cancelled");
            }
            return ServiceResult::SimpleAck;
        }

        // Verify the object exists
        if ctx.objects.get(&request.monitored_object).is_none() {
            return ServiceResult::Error {
                error_class: ErrorClass::Object,
                error_code: ErrorCode::UnknownObject,
            };
        }

        // Create the subscription
        let lifetime = match request.lifetime {
            Some(0) | None => None, // Infinite
            Some(secs) => Some(Duration::from_secs(secs as u64)),
        };

        let subscription = CovSubscription::new(
            self.subscriber_addr(ctx),
            request.subscriber_process_id,
            request.monitored_object,
            request.issue_confirmed_notifications.unwrap_or(false),
            lifetime,
        );

        match self.cov_manager.subscribe(subscription) {
            Ok(()) => {
                debug!("COV subscription created");
                ServiceResult::SimpleAck
            }
            Err(_) => ServiceResult::Error {
                error_class: ErrorClass::Resources,
                error_code: ErrorCode::CovSubscriptionFailed,
            },
        }
    }

    fn name(&self) -> &'static str {
        "SubscribeCOV"
    }

    fn min_data_length(&self) -> usize {
        6 // Minimum: process_id (context tag + 1 byte) + object_id (context tag + 4 bytes)
    }
}

// ── SubscribeCOVProperty Handler (Service 28) ───────────────────────────────

/// Decoded SubscribeCOVProperty request per ASHRAE 135, Clause 13.3.
#[derive(Debug, Clone)]
pub struct SubscribeCovPropertyRequest {
    /// Subscriber process identifier (context tag 0).
    pub subscriber_process_id: u32,
    /// Monitored object identifier (context tag 1).
    pub monitored_object: crate::object::types::ObjectId,
    /// Issue confirmed notifications (context tag 2, optional — absent = cancellation).
    pub issue_confirmed_notifications: Option<bool>,
    /// Lifetime in seconds (context tag 3, optional — 0/absent = infinite).
    pub lifetime: Option<u32>,
    /// Monitored property reference (context tag 4).
    pub monitored_property: Option<crate::object::property::PropertyId>,
    /// COV increment (context tag 5, optional).
    pub cov_increment: Option<f32>,
}

/// Decode a SubscribeCOVProperty request.
fn decode_subscribe_cov_property(
    data: &[u8],
) -> Result<SubscribeCovPropertyRequest, SubscribeCovError> {
    let mut decoder = ApduDecoder::new(data);

    // Context tag 0: Subscriber Process Identifier
    let (tag, is_context, len) = decoder
        .decode_tag_info()
        .map_err(|_| SubscribeCovError::InvalidRequest)?;
    if !is_context || tag != 0 {
        return Err(SubscribeCovError::InvalidRequest);
    }
    let subscriber_process_id = decoder
        .decode_unsigned(len)
        .map_err(|_| SubscribeCovError::InvalidRequest)?;

    // Context tag 1: Monitored Object Identifier
    let (tag, is_context, len) = decoder
        .decode_tag_info()
        .map_err(|_| SubscribeCovError::InvalidRequest)?;
    if !is_context || tag != 1 || len != 4 {
        return Err(SubscribeCovError::InvalidRequest);
    }
    let monitored_object = decoder
        .decode_object_identifier()
        .map_err(|_| SubscribeCovError::InvalidRequest)?;

    // Decode remaining optional context tags
    let mut issue_confirmed = None;
    let mut lifetime = None;
    let mut monitored_property = None;
    let mut cov_increment = None;

    while !decoder.is_empty() {
        if let Some(byte) = decoder.peek() {
            let peek_tag = (byte >> 4) & 0x0F;
            let peek_context = (byte & 0x08) != 0;

            if !peek_context {
                break;
            }

            match peek_tag {
                2 => {
                    let (_, _, len) = decoder
                        .decode_tag_info()
                        .map_err(|_| SubscribeCovError::InvalidRequest)?;
                    if len == 0 {
                        issue_confirmed = Some(false);
                    } else {
                        let val = decoder
                            .decode_unsigned(len)
                            .map_err(|_| SubscribeCovError::InvalidRequest)?;
                        issue_confirmed = Some(val != 0);
                    }
                }
                3 => {
                    let (_, _, len) = decoder
                        .decode_tag_info()
                        .map_err(|_| SubscribeCovError::InvalidRequest)?;
                    lifetime = Some(
                        decoder
                            .decode_unsigned(len)
                            .map_err(|_| SubscribeCovError::InvalidRequest)?,
                    );
                }
                4 => {
                    // Monitored property reference — opening tag [4]
                    if decoder.is_opening_tag(4) {
                        decoder
                            .read_u8()
                            .map_err(|_| SubscribeCovError::InvalidRequest)?;
                        // Property identifier [0]
                        let (_, _, len) = decoder
                            .decode_tag_info()
                            .map_err(|_| SubscribeCovError::InvalidRequest)?;
                        let prop_val = decoder
                            .decode_unsigned(len)
                            .map_err(|_| SubscribeCovError::InvalidRequest)?;
                        monitored_property =
                            crate::object::property::PropertyId::from_u32(prop_val);

                        // Skip optional array index [1] if present
                        while !decoder.is_empty() && !decoder.is_closing_tag(4) {
                            let (_, _, len) = decoder
                                .decode_tag_info()
                                .map_err(|_| SubscribeCovError::InvalidRequest)?;
                            if len > 0 {
                                let _ = decoder.read_bytes(len);
                            }
                        }
                        if decoder.is_closing_tag(4) {
                            decoder
                                .read_u8()
                                .map_err(|_| SubscribeCovError::InvalidRequest)?;
                        }
                    } else {
                        let (_, _, len) = decoder
                            .decode_tag_info()
                            .map_err(|_| SubscribeCovError::InvalidRequest)?;
                        let prop_val = decoder
                            .decode_unsigned(len)
                            .map_err(|_| SubscribeCovError::InvalidRequest)?;
                        monitored_property =
                            crate::object::property::PropertyId::from_u32(prop_val);
                    }
                }
                5 => {
                    // COV increment (real)
                    let (_, _, len) = decoder
                        .decode_tag_info()
                        .map_err(|_| SubscribeCovError::InvalidRequest)?;
                    if len == 4 {
                        let bytes = decoder
                            .read_bytes(4)
                            .map_err(|_| SubscribeCovError::InvalidRequest)?;
                        cov_increment =
                            Some(f32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]));
                    } else {
                        let _ = decoder.read_bytes(len);
                    }
                }
                _ => {
                    let (_, _, len) = decoder
                        .decode_tag_info()
                        .map_err(|_| SubscribeCovError::InvalidRequest)?;
                    if len > 0 {
                        let _ = decoder.read_bytes(len);
                    }
                }
            }
        } else {
            break;
        }
    }

    Ok(SubscribeCovPropertyRequest {
        subscriber_process_id,
        monitored_object,
        issue_confirmed_notifications: issue_confirmed,
        lifetime,
        monitored_property,
        cov_increment,
    })
}

/// Handler for BACnet SubscribeCOVProperty (Confirmed Service 28).
///
/// Allows per-property COV subscriptions. Stores subscriptions in the same
/// CovManager as SubscribeCOV, with the COV increment applied to the subscription.
pub struct SubscribeCovPropertyHandler {
    cov_manager: Arc<CovManager>,
    default_addr: SocketAddr,
}

impl SubscribeCovPropertyHandler {
    pub fn new(cov_manager: Arc<CovManager>) -> Self {
        Self {
            cov_manager,
            default_addr: "0.0.0.0:47808".parse().unwrap(),
        }
    }

    fn subscriber_addr(&self, ctx: &ServiceContext) -> SocketAddr {
        ctx.source_address.unwrap_or(self.default_addr)
    }
}

impl ConfirmedServiceHandler for SubscribeCovPropertyHandler {
    fn service_choice(&self) -> ConfirmedService {
        ConfirmedService::SubscribeCovProperty
    }

    fn handle(&self, data: &[u8], ctx: &ServiceContext) -> ServiceResult {
        let request = match decode_subscribe_cov_property(data) {
            Ok(r) => r,
            Err(_) => {
                return ServiceResult::Error {
                    error_class: ErrorClass::Services,
                    error_code: ErrorCode::InvalidParameterDataType,
                };
            }
        };

        // Cancellation
        if request.issue_confirmed_notifications.is_none() {
            self.cov_manager.unsubscribe(
                self.subscriber_addr(ctx),
                request.subscriber_process_id,
                request.monitored_object,
            );
            return ServiceResult::SimpleAck;
        }

        // Verify object exists
        if ctx.objects.get(&request.monitored_object).is_none() {
            return ServiceResult::Error {
                error_class: ErrorClass::Object,
                error_code: ErrorCode::UnknownObject,
            };
        }

        let lifetime = match request.lifetime {
            Some(0) | None => None,
            Some(secs) => Some(Duration::from_secs(secs as u64)),
        };

        let mut subscription = CovSubscription::new(
            self.subscriber_addr(ctx),
            request.subscriber_process_id,
            request.monitored_object,
            request.issue_confirmed_notifications.unwrap_or(false),
            lifetime,
        );

        // Apply COV increment from the request
        subscription.cov_increment = request.cov_increment;

        match self.cov_manager.subscribe(subscription) {
            Ok(()) => ServiceResult::SimpleAck,
            Err(_) => ServiceResult::Error {
                error_class: ErrorClass::Resources,
                error_code: ErrorCode::CovSubscriptionFailed,
            },
        }
    }

    fn name(&self) -> &'static str {
        "SubscribeCOVProperty"
    }

    fn min_data_length(&self) -> usize {
        6
    }
}

/// SubscribeCOV errors.
#[derive(Debug, thiserror::Error)]
pub enum SubscribeCovError {
    #[error("Invalid request format")]
    InvalidRequest,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::object::registry::ObjectRegistry;
    use crate::object::standard::AnalogInput;
    use crate::object::types::{ObjectId, ObjectType};
    use crate::service::handler::ServiceContext;
    use std::sync::Arc;

    fn make_ctx_with_source(registry: &Arc<ObjectRegistry>, source: SocketAddr) -> ServiceContext {
        ServiceContext {
            objects: registry.clone(),
            device_instance: 1234,
            invoke_id: Some(1),
            max_apdu_length: 1476,
            source_address: Some(source),
        }
    }

    #[test]
    fn test_decode_subscribe_cov_with_confirmed_and_lifetime() {
        // SubscribeCOV request:
        // Context 0: process_id = 1
        // Context 1: object_id = AI:0
        // Context 2: confirmed = true (len=1)
        // Context 3: lifetime = 1
        let data = [
            0x09, 0x01, // Context tag 0, length 1, value 1
            0x1C, 0x00, 0x00, 0x00, 0x00, // Context tag 1, length 4, AI:0
            0x29, 0x01, // Context tag 2, length 1, true
            0x39, 0x01, // Context tag 3, length 1, value = 1 sec
        ];

        let request = decode_subscribe_cov(&data).unwrap();
        assert_eq!(request.subscriber_process_id, 1);
        assert_eq!(
            request.monitored_object,
            ObjectId::new(ObjectType::AnalogInput, 0)
        );
        assert_eq!(request.issue_confirmed_notifications, Some(true));
        assert_eq!(request.lifetime, Some(1));
    }

    #[test]
    fn test_decode_subscribe_cov_cancellation() {
        // Cancellation: only process_id and object_id (no tag 2/3)
        let data = [
            0x09, 0x01, // Context tag 0, length 1, value 1
            0x1C, 0x00, 0x00, 0x00, 0x00, // Context tag 1, length 4, AI:0
        ];

        let request = decode_subscribe_cov(&data).unwrap();
        assert_eq!(request.subscriber_process_id, 1);
        assert!(request.issue_confirmed_notifications.is_none());
        assert!(request.lifetime.is_none());
    }

    #[test]
    fn test_subscribe_cov_handler_creates_subscription() {
        let (cov_manager, _rx) = CovManager::new(1234, 100);
        let cov_manager = Arc::new(cov_manager);
        let handler = SubscribeCovHandler::new(cov_manager.clone());

        let registry = Arc::new(ObjectRegistry::new());
        let ai = Arc::new(AnalogInput::new(0, "AI_0"));
        registry.register(ai);

        let source: SocketAddr = "10.0.0.1:47808".parse().unwrap();
        let ctx = make_ctx_with_source(&registry, source);

        // Subscribe: process_id=1, AI:0, confirmed=false, lifetime=300
        let data = [
            0x09, 0x01, // Context tag 0, process_id = 1
            0x1C, 0x00, 0x00, 0x00, 0x00, // Context tag 1, AI:0
            0x29, 0x00, // Context tag 2, confirmed = false (len=0 → false)
        ];

        let result = handler.handle(&data, &ctx);
        assert!(matches!(result, ServiceResult::SimpleAck));
        assert_eq!(cov_manager.subscription_count(), 1);

        // Verify the subscriber address was captured from source_address
        let subs = cov_manager.subscriptions_for_object(ObjectId::new(ObjectType::AnalogInput, 0));
        assert_eq!(subs.len(), 1);
        assert_eq!(subs[0].subscriber_address, source);
    }

    #[test]
    fn test_subscribe_cov_handler_cancellation() {
        let (cov_manager, _rx) = CovManager::new(1234, 100);
        let cov_manager = Arc::new(cov_manager);
        let handler = SubscribeCovHandler::new(cov_manager.clone());

        let registry = Arc::new(ObjectRegistry::new());
        let ai = Arc::new(AnalogInput::new(0, "AI_0"));
        registry.register(ai);

        let source: SocketAddr = "10.0.0.1:47808".parse().unwrap();
        let ctx = make_ctx_with_source(&registry, source);

        // First subscribe
        let subscribe_data = [0x09, 0x01, 0x1C, 0x00, 0x00, 0x00, 0x00, 0x29, 0x00];
        handler.handle(&subscribe_data, &ctx);
        assert_eq!(cov_manager.subscription_count(), 1);

        // Then cancel (no tag 2 = cancellation)
        let cancel_data = [0x09, 0x01, 0x1C, 0x00, 0x00, 0x00, 0x00];
        let result = handler.handle(&cancel_data, &ctx);
        assert!(matches!(result, ServiceResult::SimpleAck));
        assert_eq!(cov_manager.subscription_count(), 0);
    }

    #[test]
    fn test_subscribe_cov_handler_unknown_object() {
        let (cov_manager, _rx) = CovManager::new(1234, 100);
        let cov_manager = Arc::new(cov_manager);
        let handler = SubscribeCovHandler::new(cov_manager.clone());

        let registry = Arc::new(ObjectRegistry::new());
        // No objects registered

        let source: SocketAddr = "10.0.0.1:47808".parse().unwrap();
        let ctx = make_ctx_with_source(&registry, source);

        let data = [
            0x09, 0x01, 0x1C, 0x00, 0x00, 0x00, 0x00, 0x29, 0x00, // confirmed = false
        ];

        let result = handler.handle(&data, &ctx);
        assert!(matches!(
            result,
            ServiceResult::Error {
                error_class: ErrorClass::Object,
                error_code: ErrorCode::UnknownObject,
            }
        ));
    }

    #[test]
    fn test_subscribe_cov_property_handler_creates_subscription() {
        let (cov_manager, _rx) = CovManager::new(1234, 100);
        let cov_manager = Arc::new(cov_manager);
        let handler = SubscribeCovPropertyHandler::new(cov_manager.clone());

        let registry = Arc::new(ObjectRegistry::new());
        let ai = Arc::new(AnalogInput::new(0, "AI_0"));
        registry.register(ai);

        let source: SocketAddr = "10.0.0.5:47808".parse().unwrap();
        let ctx = make_ctx_with_source(&registry, source);

        // SubscribeCOVProperty: process_id=1, AI:0, confirmed=true
        let data = [
            0x09, 0x01, // Context tag 0, process_id = 1
            0x1C, 0x00, 0x00, 0x00, 0x00, // Context tag 1, AI:0
            0x29, 0x01, // Context tag 2, confirmed = true (len=1)
        ];

        assert_eq!(
            handler.service_choice(),
            ConfirmedService::SubscribeCovProperty
        );
        assert_eq!(handler.name(), "SubscribeCOVProperty");

        let result = handler.handle(&data, &ctx);
        assert!(matches!(result, ServiceResult::SimpleAck));
        assert_eq!(cov_manager.subscription_count(), 1);

        // Verify subscriber address
        let subs = cov_manager.subscriptions_for_object(ObjectId::new(ObjectType::AnalogInput, 0));
        assert_eq!(subs[0].subscriber_address, source);
        assert!(subs[0].confirmed_notifications);
    }

    #[test]
    fn test_subscribe_cov_property_handler_cancellation() {
        let (cov_manager, _rx) = CovManager::new(1234, 100);
        let cov_manager = Arc::new(cov_manager);
        let handler = SubscribeCovPropertyHandler::new(cov_manager.clone());

        let registry = Arc::new(ObjectRegistry::new());
        let ai = Arc::new(AnalogInput::new(0, "AI_0"));
        registry.register(ai);

        let source: SocketAddr = "10.0.0.5:47808".parse().unwrap();
        let ctx = make_ctx_with_source(&registry, source);

        // First subscribe
        let subscribe_data = [0x09, 0x01, 0x1C, 0x00, 0x00, 0x00, 0x00, 0x29, 0x01];
        handler.handle(&subscribe_data, &ctx);
        assert_eq!(cov_manager.subscription_count(), 1);

        // Cancel (no tag 2)
        let cancel_data = [0x09, 0x01, 0x1C, 0x00, 0x00, 0x00, 0x00];
        let result = handler.handle(&cancel_data, &ctx);
        assert!(matches!(result, ServiceResult::SimpleAck));
        assert_eq!(cov_manager.subscription_count(), 0);
    }

    #[test]
    fn test_decode_subscribe_cov_property_with_increment() {
        // SubscribeCOVProperty: process_id=5, AI:3, confirmed=false, lifetime=600,
        // property=PresentValue, cov_increment=2.0
        let cov_val = f32::to_be_bytes(2.0);
        let data = vec![
            0x09, 0x05, // Context tag 0, process_id = 5
            0x1C, 0x00, 0x00, 0x00, 0x03, // Context tag 1, AI:3
            0x29, 0x00, // Context tag 2, confirmed = false
            0x3A, 0x02, 0x58, // Context tag 3, lifetime = 600 (2 bytes)
            // Context tag 5: cov_increment = 2.0 (4 bytes real)
            0x5C, cov_val[0], cov_val[1], cov_val[2], cov_val[3],
        ];

        let request = decode_subscribe_cov_property(&data).unwrap();
        assert_eq!(request.subscriber_process_id, 5);
        assert_eq!(
            request.monitored_object,
            ObjectId::new(ObjectType::AnalogInput, 3)
        );
        assert_eq!(request.issue_confirmed_notifications, Some(false));
        assert_eq!(request.lifetime, Some(600));
        assert_eq!(request.cov_increment, Some(2.0));
    }

    #[test]
    fn test_subscribe_cov_handler_service_identity() {
        let (cov_manager, _rx) = CovManager::new(1234, 100);
        let handler = SubscribeCovHandler::new(Arc::new(cov_manager));
        assert_eq!(handler.service_choice(), ConfirmedService::SubscribeCov);
        assert_eq!(handler.name(), "SubscribeCOV");
        assert_eq!(handler.min_data_length(), 6);
    }
}

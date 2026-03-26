//! Service handler traits and registry.
//!
//! Provides a pluggable architecture for handling BACnet services,
//! similar to the Modbus handler pattern.

use std::collections::HashMap;
use std::sync::Arc;

use crate::apdu::types::{
    AbortReason, ConfirmedService, ErrorClass, ErrorCode, RejectReason, UnconfirmedService,
};
use crate::object::ObjectRegistry;

/// Context provided to service handlers.
pub struct ServiceContext {
    /// Object registry for this device.
    pub objects: Arc<ObjectRegistry>,
    /// Device instance number.
    pub device_instance: u32,
    /// Invoke ID from the request (for confirmed services).
    pub invoke_id: Option<u8>,
    /// Max APDU length for responses.
    pub max_apdu_length: u16,
    /// Source address of the request (for COV subscriptions).
    pub source_address: Option<std::net::SocketAddr>,
}

impl ServiceContext {
    /// Create a new service context.
    pub fn new(objects: Arc<ObjectRegistry>, device_instance: u32) -> Self {
        Self {
            objects,
            device_instance,
            invoke_id: None,
            max_apdu_length: 1476,
            source_address: None,
        }
    }

    /// Set the invoke ID.
    pub fn with_invoke_id(mut self, invoke_id: u8) -> Self {
        self.invoke_id = Some(invoke_id);
        self
    }

    /// Set the source address.
    pub fn with_source_address(mut self, addr: std::net::SocketAddr) -> Self {
        self.source_address = Some(addr);
        self
    }
}

/// Result of handling a service request.
///
/// BACnet defines a 3-tier error protocol:
/// - **Error**: Service-level error (known service, known object, but operation failed).
/// - **Reject**: Protocol violation in the request PDU (invalid tags, missing parameters).
/// - **Abort**: Server-side inability to process (resource exhaustion, segmentation failure).
#[derive(Debug)]
pub enum ServiceResult {
    /// Simple ACK (no data).
    SimpleAck,
    /// Complex ACK with response data.
    ComplexAck(Vec<u8>),
    /// Error response (service-level failure).
    Error {
        error_class: ErrorClass,
        error_code: ErrorCode,
    },
    /// Reject the request (protocol violation in request PDU).
    Reject(u8),
    /// Abort the transaction (server-side inability to process).
    Abort(AbortReason),
    /// No response needed (for unconfirmed services).
    NoResponse,
    /// Broadcast response (for I-Am, etc.).
    Broadcast(Vec<u8>),
}

impl ServiceResult {
    /// Create an error result for an unknown property.
    pub fn unknown_property() -> Self {
        Self::Error {
            error_class: ErrorClass::Property,
            error_code: ErrorCode::UnknownProperty,
        }
    }

    /// Create an error result for an unknown object.
    pub fn unknown_object() -> Self {
        Self::Error {
            error_class: ErrorClass::Object,
            error_code: ErrorCode::UnknownObject,
        }
    }

    /// Create a reject result for an unrecognized service.
    pub fn unrecognized_service() -> Self {
        Self::Reject(RejectReason::UnrecognizedService as u8)
    }

    /// Create a reject result for missing required parameter.
    pub fn missing_required_parameter() -> Self {
        Self::Reject(RejectReason::MissingRequiredParameter as u8)
    }

    /// Create a reject result for invalid tag.
    pub fn invalid_tag() -> Self {
        Self::Reject(RejectReason::InvalidTag as u8)
    }

    /// Create a reject result for too many arguments.
    pub fn too_many_arguments() -> Self {
        Self::Reject(RejectReason::TooManyArguments as u8)
    }

    /// Create an abort result for segmentation not supported.
    pub fn segmentation_not_supported() -> Self {
        Self::Abort(AbortReason::SegmentationNotSupported)
    }

    /// Create an abort result for buffer overflow.
    pub fn buffer_overflow() -> Self {
        Self::Abort(AbortReason::BufferOverflow)
    }

    /// Create an abort result for out of resources.
    pub fn out_of_resources() -> Self {
        Self::Abort(AbortReason::OutOfResources)
    }

    /// Create an abort result for APDU too long.
    pub fn apdu_too_long() -> Self {
        Self::Abort(AbortReason::ApduTooLong)
    }

    /// Create an error result for write access denied.
    pub fn write_access_denied() -> Self {
        Self::Error {
            error_class: ErrorClass::Property,
            error_code: ErrorCode::WriteAccessDenied,
        }
    }

    /// Create an error for service request denied.
    pub fn service_request_denied() -> Self {
        Self::Error {
            error_class: ErrorClass::Services,
            error_code: ErrorCode::ServiceRequestDenied,
        }
    }

    /// Create an error for communication disabled.
    pub fn communication_disabled() -> Self {
        Self::Error {
            error_class: ErrorClass::Device,
            error_code: ErrorCode::CommunicationDisabled,
        }
    }
}

/// Trait for confirmed service handlers.
pub trait ConfirmedServiceHandler: Send + Sync {
    /// Get the service choice this handler processes.
    fn service_choice(&self) -> ConfirmedService;

    /// Handle a confirmed service request.
    fn handle(&self, data: &[u8], ctx: &ServiceContext) -> ServiceResult;

    /// Get the service name.
    fn name(&self) -> &'static str;

    /// Minimum data length for this service.
    fn min_data_length(&self) -> usize {
        0
    }
}

/// Trait for unconfirmed service handlers.
pub trait UnconfirmedServiceHandler: Send + Sync {
    /// Get the service choice this handler processes.
    fn service_choice(&self) -> UnconfirmedService;

    /// Handle an unconfirmed service request.
    fn handle(&self, data: &[u8], ctx: &ServiceContext) -> ServiceResult;

    /// Get the service name.
    fn name(&self) -> &'static str;
}

/// Registry for service handlers.
pub struct ServiceRegistry {
    confirmed_handlers: HashMap<u8, Arc<dyn ConfirmedServiceHandler>>,
    unconfirmed_handlers: HashMap<u8, Arc<dyn UnconfirmedServiceHandler>>,
}

impl ServiceRegistry {
    /// Create a new empty registry.
    pub fn new() -> Self {
        Self {
            confirmed_handlers: HashMap::new(),
            unconfirmed_handlers: HashMap::new(),
        }
    }

    /// Register a confirmed service handler.
    pub fn register_confirmed(
        &mut self,
        handler: Arc<dyn ConfirmedServiceHandler>,
    ) -> Option<Arc<dyn ConfirmedServiceHandler>> {
        let service = handler.service_choice() as u8;
        self.confirmed_handlers.insert(service, handler)
    }

    /// Register an unconfirmed service handler.
    pub fn register_unconfirmed(
        &mut self,
        handler: Arc<dyn UnconfirmedServiceHandler>,
    ) -> Option<Arc<dyn UnconfirmedServiceHandler>> {
        let service = handler.service_choice() as u8;
        self.unconfirmed_handlers.insert(service, handler)
    }

    /// Get a confirmed service handler.
    pub fn get_confirmed(&self, service: u8) -> Option<&Arc<dyn ConfirmedServiceHandler>> {
        self.confirmed_handlers.get(&service)
    }

    /// Get an unconfirmed service handler.
    pub fn get_unconfirmed(&self, service: u8) -> Option<&Arc<dyn UnconfirmedServiceHandler>> {
        self.unconfirmed_handlers.get(&service)
    }

    /// Check if a confirmed service is supported.
    pub fn has_confirmed(&self, service: u8) -> bool {
        self.confirmed_handlers.contains_key(&service)
    }

    /// Check if an unconfirmed service is supported.
    pub fn has_unconfirmed(&self, service: u8) -> bool {
        self.unconfirmed_handlers.contains_key(&service)
    }

    /// Dispatch a confirmed service request.
    ///
    /// Per ASHRAE 135 Clause 5.4.4, an unrecognized service gets a
    /// Reject(UnrecognizedService), not an Error. A data-length violation
    /// gets Reject(MissingRequiredParameter).
    pub fn dispatch_confirmed(
        &self,
        service: u8,
        data: &[u8],
        ctx: &ServiceContext,
    ) -> ServiceResult {
        match self.confirmed_handlers.get(&service) {
            Some(handler) => {
                if data.len() < handler.min_data_length() {
                    return ServiceResult::missing_required_parameter();
                }
                handler.handle(data, ctx)
            }
            None => ServiceResult::unrecognized_service(),
        }
    }

    /// Dispatch an unconfirmed service request.
    pub fn dispatch_unconfirmed(
        &self,
        service: u8,
        data: &[u8],
        ctx: &ServiceContext,
    ) -> ServiceResult {
        match self.unconfirmed_handlers.get(&service) {
            Some(handler) => handler.handle(data, ctx),
            None => ServiceResult::NoResponse,
        }
    }

    /// Get list of supported confirmed services.
    pub fn supported_confirmed_services(&self) -> Vec<u8> {
        self.confirmed_handlers.keys().copied().collect()
    }

    /// Get list of supported unconfirmed services.
    pub fn supported_unconfirmed_services(&self) -> Vec<u8> {
        self.unconfirmed_handlers.keys().copied().collect()
    }
}

impl Default for ServiceRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct MockReadPropertyHandler;

    impl ConfirmedServiceHandler for MockReadPropertyHandler {
        fn service_choice(&self) -> ConfirmedService {
            ConfirmedService::ReadProperty
        }

        fn handle(&self, _data: &[u8], _ctx: &ServiceContext) -> ServiceResult {
            ServiceResult::ComplexAck(vec![0x0C, 0x00])
        }

        fn name(&self) -> &'static str {
            "ReadProperty"
        }
    }

    #[test]
    fn test_service_registry() {
        let mut registry = ServiceRegistry::new();

        let handler = Arc::new(MockReadPropertyHandler);
        registry.register_confirmed(handler);

        assert!(registry.has_confirmed(ConfirmedService::ReadProperty as u8));
        assert!(!registry.has_confirmed(ConfirmedService::WriteProperty as u8));
    }
}

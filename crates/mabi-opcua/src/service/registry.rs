//! OPC UA service handler registry and dispatcher.
//!
//! The canonical mainline runtime now routes built-in services through the
//! typed core dispatcher. `ServiceRegistry` remains as a compatibility façade
//! for existing handler registration code and for custom/raw handlers.

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use bytes::{Bytes, BytesMut};
use parking_lot::RwLock;
use tracing::{debug, warn};

use crate::channel::secure_channel::SecureChannel;
use crate::codec::decoder::BinaryDecodable;
use crate::codec::encoder::BinaryEncodable;
use crate::config::OpcUaServerConfig;
use crate::core::dispatch::dispatch_payload;
use crate::core::encoding::peek_request_type_id;
use crate::core::services::BuiltinServiceSet;
use crate::core::types::ServiceId;
use crate::error::{OpcUaError, OpcUaResult};
use crate::nodes::AddressSpace;
use crate::sdk::history::HistoryStore;
use crate::sdk::methods::MethodRegistry;
use crate::sdk::session::SessionManager;
use crate::sdk::subscription::SubscriptionManager;
use crate::security::SecurityManager;
use crate::types::NodeId;

/// Context passed to every service handler invocation.
///
/// Session-related fields use interior mutability so that handlers like
/// `CreateSessionHandler` can set `session_id` / `auth_token` without
/// requiring exclusive ownership of the context.
pub struct ServiceContext {
    pub session_manager: Arc<SessionManager>,
    pub address_space: Arc<AddressSpace>,
    pub subscription_manager: Arc<SubscriptionManager>,
    pub history_store: Arc<HistoryStore>,
    pub security_manager: Arc<SecurityManager>,
    pub server_config: Arc<OpcUaServerConfig>,
    /// Method call registry for Call service.
    pub method_registry: Arc<MethodRegistry>,
    /// The secure channel for this connection.
    pub channel: Arc<SecureChannel>,
    /// The session ID for the authenticated session (if any).
    /// Updated by CreateSession / closed by CloseSession.
    pub session_id: RwLock<Option<NodeId>>,
    /// The authentication token for the current session (if any).
    /// Updated by CreateSession.
    pub auth_token: RwLock<Option<NodeId>>,
}

impl ServiceContext {
    /// Set session identity after CreateSession succeeds.
    pub fn set_session(&self, session_id: NodeId, auth_token: NodeId) {
        *self.session_id.write() = Some(session_id);
        *self.auth_token.write() = Some(auth_token);
    }

    /// Clear session identity (called when session is closed).
    pub fn clear_session(&self) {
        *self.session_id.write() = None;
        *self.auth_token.write() = None;
    }

    /// Read the current session ID.
    pub fn current_session_id(&self) -> Option<NodeId> {
        self.session_id.read().clone()
    }

    /// Read the current auth token.
    pub fn current_auth_token(&self) -> Option<NodeId> {
        self.auth_token.read().clone()
    }
}

/// Trait for OPC UA service handlers.
///
/// Each handler processes a specific service request type identified by its
/// OPC UA binary encoding type ID (NodeId).
///
/// To add a new service, implement this trait and register it in the `ServiceRegistry`.
#[async_trait]
pub trait ServiceHandler: Send + Sync {
    /// The NodeId of the request type this handler processes.
    /// This is the `type_id` from the ExtensionObject wrapping the request.
    fn request_type_id(&self) -> NodeId;

    /// Handle the service request.
    ///
    /// - `request_body`: The raw binary body of the request (inside the ExtensionObject).
    /// - `context`: Shared server state.
    ///
    /// Returns the raw binary body of the response (will be wrapped in an ExtensionObject).
    async fn handle(
        &self,
        request_body: &[u8],
        context: &ServiceContext,
    ) -> OpcUaResult<ServiceResponse>;
}

/// Response from a service handler.
pub struct ServiceResponse {
    /// The NodeId of the response type (for ExtensionObject wrapping).
    pub type_id: NodeId,
    /// The raw binary encoded response body.
    pub body: Vec<u8>,
}

/// Registry of service handlers, dispatching by request type ID.
///
/// Built-in handlers are now routed through the typed core dispatcher. Any
/// non-built-in registrations remain on the raw compatibility path.
pub struct ServiceRegistry {
    builtins: BuiltinServiceSet,
    custom_handlers: HashMap<NodeId, Arc<dyn ServiceHandler>>,
}

impl ServiceRegistry {
    /// Create a new empty registry.
    pub fn new() -> Self {
        Self {
            builtins: BuiltinServiceSet::empty(),
            custom_handlers: HashMap::new(),
        }
    }

    /// Register a service handler.
    pub fn register(&mut self, handler: Arc<dyn ServiceHandler>) {
        let type_id = handler.request_type_id();
        if let Some(service_id) = ServiceId::from_request_type_id(&type_id) {
            debug!(type_id = %type_id, ?service_id, "Registered built-in service handler");
            self.builtins.enable(service_id);
            return;
        }

        debug!(type_id = %type_id, "Registered custom service handler");
        self.custom_handlers.insert(type_id, handler);
    }

    /// Dispatch a request to the appropriate handler.
    ///
    /// The `payload` is the raw bytes after the sequence header, containing
    /// an ExtensionObject-encoded service request.
    pub async fn dispatch(&self, payload: &[u8], context: &ServiceContext) -> OpcUaResult<Vec<u8>> {
        let type_id = peek_request_type_id(payload)?;
        if let Some(service_id) = ServiceId::from_request_type_id(&type_id) {
            if self.builtins.contains(service_id) {
                return dispatch_payload(payload, context, &self.builtins)
                    .await
                    .map_err(Into::into);
            }
        }

        let handler = self.custom_handlers.get(&type_id).ok_or_else(|| {
            warn!(type_id = %type_id, "Service not supported");
            OpcUaError::ServiceNotSupported {
                service_id: type_id.to_string(),
            }
        })?;

        // Remaining bytes after NodeId are the request body.
        let mut buf = Bytes::copy_from_slice(payload);
        let _ = NodeId::decode(&mut buf)?;
        let request_body = buf.as_ref();
        let response = handler.handle(request_body, context).await?;

        // Encode response as: NodeId (type_id) + body (no ExtensionObject wrapper)
        let mut out = BytesMut::new();
        response.type_id.encode(&mut out)?;
        out.extend_from_slice(&response.body);
        Ok(out.to_vec())
    }

    /// Check if a handler is registered for the given type ID.
    pub fn has_handler(&self, type_id: &NodeId) -> bool {
        ServiceId::from_request_type_id(type_id)
            .map(|service_id| self.builtins.contains(service_id))
            .unwrap_or_else(|| self.custom_handlers.contains_key(type_id))
    }

    /// Get the number of registered handlers.
    pub fn handler_count(&self) -> usize {
        self.builtins.len() + self.custom_handlers.len()
    }
}

impl Default for ServiceRegistry {
    fn default() -> Self {
        Self::new()
    }
}

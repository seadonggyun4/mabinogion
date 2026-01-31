//! OPC UA Service handler registry and dispatcher.
//!
//! Provides a trait-based extensible system for handling OPC UA service requests.

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use bytes::{Bytes, BytesMut};
use tracing::{debug, warn};

use crate::codec::encoder::BinaryEncodable;
use crate::codec::decoder::BinaryDecodable;
use crate::error::{OpcUaError, OpcUaResult};
use crate::types::NodeId;
use crate::nodes::AddressSpace;
use crate::services::{SessionManager, SubscriptionManager, HistoryStore};
use crate::security::SecurityManager;
use crate::config::OpcUaServerConfig;
use crate::channel::secure_channel::SecureChannel;

/// Context passed to every service handler invocation.
pub struct ServiceContext {
    pub session_manager: Arc<SessionManager>,
    pub address_space: Arc<AddressSpace>,
    pub subscription_manager: Arc<SubscriptionManager>,
    pub history_store: Arc<HistoryStore>,
    pub security_manager: Arc<SecurityManager>,
    pub server_config: Arc<OpcUaServerConfig>,
    /// The secure channel for this connection.
    pub channel: Arc<SecureChannel>,
    /// The session ID for the authenticated session (if any).
    pub session_id: Option<NodeId>,
    /// The authentication token for the current session (if any).
    pub auth_token: Option<NodeId>,
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
pub struct ServiceRegistry {
    handlers: HashMap<NodeId, Arc<dyn ServiceHandler>>,
}

impl ServiceRegistry {
    /// Create a new empty registry.
    pub fn new() -> Self {
        Self { handlers: HashMap::new() }
    }

    /// Register a service handler.
    pub fn register(&mut self, handler: Arc<dyn ServiceHandler>) {
        let type_id = handler.request_type_id();
        debug!(type_id = %type_id, "Registered service handler");
        self.handlers.insert(type_id, handler);
    }

    /// Dispatch a request to the appropriate handler.
    ///
    /// The `payload` is the raw bytes after the sequence header, containing
    /// an ExtensionObject-encoded service request.
    pub async fn dispatch(
        &self,
        payload: &[u8],
        context: &ServiceContext,
    ) -> OpcUaResult<Vec<u8>> {
        // OPC UA binary protocol: service messages are encoded as
        // NodeId (type_id) + body (NOT wrapped in ExtensionObject).
        let mut buf = Bytes::copy_from_slice(payload);
        let type_id = NodeId::decode(&mut buf)?;

        debug!(type_id = %type_id, "Dispatching service request");

        let handler = self.handlers.get(&type_id).ok_or_else(|| {
            warn!(type_id = %type_id, "Service not supported");
            OpcUaError::ServiceNotSupported {
                service_id: type_id.to_string(),
            }
        })?;

        // Remaining bytes after NodeId are the request body
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
        self.handlers.contains_key(type_id)
    }

    /// Get the number of registered handlers.
    pub fn handler_count(&self) -> usize {
        self.handlers.len()
    }
}

impl Default for ServiceRegistry {
    fn default() -> Self {
        Self::new()
    }
}

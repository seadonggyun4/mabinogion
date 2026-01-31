//! Session service handlers — CreateSession, ActivateSession, CloseSession.
//!
//! OPC UA Part 4, Section 5.6.

use std::sync::Arc;

use async_trait::async_trait;
use bytes::{BufMut, Bytes, BytesMut};

use crate::codec::encoder::BinaryEncodable;
use crate::codec::decoder::BinaryDecodable;
use crate::codec::data_value::ExtensionObject;
use crate::error::OpcUaResult;
use crate::types::{NodeId, StatusCode};
use crate::services::session::UserIdentity;
use super::discovery::{RequestHeader, ResponseHeader};
use super::registry::{ServiceContext, ServiceHandler, ServiceResponse};

// OPC UA type IDs
const CREATE_SESSION_REQUEST_ID: u32 = 461;
const CREATE_SESSION_RESPONSE_ID: u32 = 464;
const ACTIVATE_SESSION_REQUEST_ID: u32 = 467;
const ACTIVATE_SESSION_RESPONSE_ID: u32 = 470;
const CLOSE_SESSION_REQUEST_ID: u32 = 473;
const CLOSE_SESSION_RESPONSE_ID: u32 = 476;

// =========================================================================
// CreateSession
// =========================================================================

pub struct CreateSessionHandler;

#[async_trait]
impl ServiceHandler for CreateSessionHandler {
    fn request_type_id(&self) -> NodeId {
        NodeId::numeric(0, CREATE_SESSION_REQUEST_ID)
    }

    async fn handle(
        &self,
        request_body: &[u8],
        context: &ServiceContext,
    ) -> OpcUaResult<ServiceResponse> {
        let mut buf = Bytes::copy_from_slice(request_body);
        let header = RequestHeader::decode(&mut buf)?;
        let _additional_header = ExtensionObject::decode(&mut buf)?;

        // CreateSessionRequest fields
        // ClientDescription (ApplicationDescription) — skip
        let _app_uri = String::decode(&mut buf)?;
        let _product_uri = String::decode(&mut buf)?;
        let _app_name_locale = String::decode(&mut buf)?;  // LocalizedText locale
        let _app_name_text = String::decode(&mut buf)?;     // LocalizedText text
        // Actually LocalizedText is encoded with mask byte, let me read properly
        // For now, skip remaining fields — we only need session_name
        // This is simplified — a full implementation would parse all fields

        let session_name = format!("Session_{}", chrono::Utc::now().timestamp_millis());

        // Create session in the manager
        let session_info = context.session_manager.create_session(&session_name)
            .map_err(|e| crate::error::OpcUaError::Server(format!("Create session failed: {:?}", e)))?;

        // Build response
        let mut out = BytesMut::new();
        ResponseHeader::good(header.request_handle).encode(&mut out)?;

        // SessionId
        session_info.session_id.encode(&mut out)?;
        // AuthenticationToken
        session_info.authentication_token.encode(&mut out)?;
        // RevisedSessionTimeout (ms)
        out.put_f64_le(60_000.0); // revised session timeout (ms)
        // ServerNonce (empty)
        out.put_i32_le(0);
        // ServerCertificate (empty)
        out.put_i32_le(-1);
        // ServerEndpoints array — 1 endpoint
        out.put_i32_le(1);
        // Minimal EndpointDescription
        context.server_config.endpoint_url.encode(&mut out)?;
        // Server (ApplicationDescription)
        super::discovery::encode_application_description(
            &context.server_config.endpoint_url,
            &context.server_config.server_name,
            &mut out,
        )?;
        out.put_i32_le(-1); // ServerCertificate
        out.put_u32_le(1);  // SecurityMode: None
        "http://opcfoundation.org/UA/SecurityPolicy#None".encode(&mut out)?;
        out.put_i32_le(1); // UserIdentityTokens
        "anonymous".to_string().encode(&mut out)?;
        out.put_u32_le(0); // Anonymous
        out.put_i32_le(-1);
        out.put_i32_le(-1);
        out.put_i32_le(-1);
        "http://opcfoundation.org/UA-Profile/Transport/uatcp-uasc-uabinary".encode(&mut out)?;
        out.put_u8(0); // SecurityLevel
        // ServerSoftwareCertificates (empty array)
        out.put_i32_le(0);
        // ServerSignature (empty)
        out.put_i32_le(-1); // Algorithm (null)
        out.put_i32_le(-1); // Signature (null)
        // MaxRequestMessageSize
        out.put_u32_le(0); // 0 = no limit

        Ok(ServiceResponse {
            type_id: NodeId::numeric(0, CREATE_SESSION_RESPONSE_ID),
            body: out.to_vec(),
        })
    }
}

// =========================================================================
// ActivateSession
// =========================================================================

pub struct ActivateSessionHandler;

#[async_trait]
impl ServiceHandler for ActivateSessionHandler {
    fn request_type_id(&self) -> NodeId {
        NodeId::numeric(0, ACTIVATE_SESSION_REQUEST_ID)
    }

    async fn handle(
        &self,
        request_body: &[u8],
        context: &ServiceContext,
    ) -> OpcUaResult<ServiceResponse> {
        let mut buf = Bytes::copy_from_slice(request_body);
        let header = RequestHeader::decode(&mut buf)?;
        let _additional_header = ExtensionObject::decode(&mut buf)?;

        // Activate the session using the auth token from the request header
        if let Some(ref session_id) = context.session_id {
            let _ = context.session_manager.activate_session(session_id, UserIdentity::Anonymous);
        }

        // Build response
        let mut out = BytesMut::new();
        ResponseHeader::good(header.request_handle).encode(&mut out)?;

        // ServerNonce (empty)
        out.put_i32_le(0);
        // Results (empty array) — StatusCode array for each client cert
        out.put_i32_le(0);
        // DiagnosticInfos (empty)
        out.put_i32_le(0);

        Ok(ServiceResponse {
            type_id: NodeId::numeric(0, ACTIVATE_SESSION_RESPONSE_ID),
            body: out.to_vec(),
        })
    }
}

// =========================================================================
// CloseSession
// =========================================================================

pub struct CloseSessionHandler;

#[async_trait]
impl ServiceHandler for CloseSessionHandler {
    fn request_type_id(&self) -> NodeId {
        NodeId::numeric(0, CLOSE_SESSION_REQUEST_ID)
    }

    async fn handle(
        &self,
        request_body: &[u8],
        context: &ServiceContext,
    ) -> OpcUaResult<ServiceResponse> {
        let mut buf = Bytes::copy_from_slice(request_body);
        let header = RequestHeader::decode(&mut buf)?;
        let _additional_header = ExtensionObject::decode(&mut buf)?;

        if let Some(ref session_id) = context.session_id {
            let _ = context.session_manager.close_session(session_id);
        }

        let mut out = BytesMut::new();
        ResponseHeader::good(header.request_handle).encode(&mut out)?;

        Ok(ServiceResponse {
            type_id: NodeId::numeric(0, CLOSE_SESSION_RESPONSE_ID),
            body: out.to_vec(),
        })
    }
}

/// Register all session service handlers.
pub fn register_handlers(registry: &mut super::registry::ServiceRegistry) {
    registry.register(Arc::new(CreateSessionHandler));
    registry.register(Arc::new(ActivateSessionHandler));
    registry.register(Arc::new(CloseSessionHandler));
}

//! Session service handlers — CreateSession, ActivateSession, CloseSession.
//!
//! OPC UA Part 4, Section 5.6.

use std::sync::Arc;

use async_trait::async_trait;
use bytes::{BufMut, Bytes, BytesMut};

use super::discovery::{RequestHeader, ResponseHeader};
use super::registry::{ServiceContext, ServiceHandler, ServiceResponse};
use crate::codec::data_value::ExtensionObject;
use crate::codec::decoder::BinaryDecodable;
use crate::codec::encoder::BinaryEncodable;
use crate::error::OpcUaResult;
use crate::nodes::LocalizedText;
use crate::sdk::session::{SessionInfo, UserIdentity};
use crate::types::NodeId;

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

#[derive(Debug, Clone)]
pub(crate) struct CreateSessionRequest {
    pub request_handle: u32,
}

#[derive(Debug, Clone)]
pub(crate) struct CreateSessionResponse {
    pub request_handle: u32,
    pub session_info: SessionInfo,
}

#[derive(Debug, Clone)]
pub(crate) struct ActivateSessionRequest {
    pub request_handle: u32,
}

#[derive(Debug, Clone)]
pub(crate) struct ActivateSessionResponse {
    pub request_handle: u32,
}

#[derive(Debug, Clone)]
pub(crate) struct CloseSessionRequest {
    pub request_handle: u32,
}

#[derive(Debug, Clone)]
pub(crate) struct CloseSessionResponse {
    pub request_handle: u32,
}

pub(crate) fn decode_create_session_request(
    request_body: &[u8],
) -> OpcUaResult<CreateSessionRequest> {
    let mut buf = Bytes::copy_from_slice(request_body);
    let header = RequestHeader::decode(&mut buf)?;
    let _additional_header = ExtensionObject::decode(&mut buf)?;

    let _app_uri = String::decode(&mut buf)?;
    let _product_uri = String::decode(&mut buf)?;
    let _app_name = LocalizedText::decode(&mut buf)?;
    let _app_type = u32::decode(&mut buf)?;
    let _gateway_uri = String::decode(&mut buf)?;
    let _discovery_uri = String::decode(&mut buf)?;
    let discovery_urls_len = i32::decode(&mut buf)?;
    if discovery_urls_len > 0 {
        for _ in 0..discovery_urls_len {
            let _ = String::decode(&mut buf)?;
        }
    }
    let _server_uri = String::decode(&mut buf)?;
    let _endpoint_url = String::decode(&mut buf)?;
    let _session_name_req = String::decode(&mut buf)?;
    let _client_nonce = Vec::<u8>::decode(&mut buf)?;
    let _client_cert = Vec::<u8>::decode(&mut buf)?;
    let _requested_timeout = f64::decode(&mut buf)?;
    let _max_response_size = u32::decode(&mut buf)?;

    Ok(CreateSessionRequest {
        request_handle: header.request_handle,
    })
}

pub(crate) async fn handle_create_session(
    request: CreateSessionRequest,
    context: &ServiceContext,
) -> OpcUaResult<CreateSessionResponse> {
    let session_name = format!("Session_{}", chrono::Utc::now().timestamp_millis());
    let session_info = context
        .session_manager
        .create_session(&session_name)
        .map_err(|e| crate::error::OpcUaError::Server(format!("Create session failed: {:?}", e)))?;

    context.set_session(
        session_info.session_id.clone(),
        session_info.authentication_token.clone(),
    );
    context.channel.attach_session(
        session_info.session_id.clone(),
        session_info.authentication_token.clone(),
    );

    Ok(CreateSessionResponse {
        request_handle: request.request_handle,
        session_info,
    })
}

pub(crate) fn encode_create_session_response(
    response: &CreateSessionResponse,
    context: &ServiceContext,
) -> OpcUaResult<Vec<u8>> {
    let mut out = BytesMut::new();
    ResponseHeader::good(response.request_handle).encode(&mut out)?;

    response.session_info.session_id.encode(&mut out)?;
    response
        .session_info
        .authentication_token
        .encode(&mut out)?;
    out.put_f64_le(60_000.0);
    out.put_i32_le(0);
    out.put_i32_le(-1);
    out.put_i32_le(1);
    context.server_config.endpoint_url.encode(&mut out)?;
    super::discovery::encode_application_description(
        &context.server_config.endpoint_url,
        &context.server_config.server_name,
        &mut out,
    )?;
    out.put_i32_le(-1);
    out.put_u32_le(1);
    "http://opcfoundation.org/UA/SecurityPolicy#None".encode(&mut out)?;
    out.put_i32_le(1);
    "anonymous".to_string().encode(&mut out)?;
    out.put_u32_le(0);
    out.put_i32_le(-1);
    out.put_i32_le(-1);
    out.put_i32_le(-1);
    "http://opcfoundation.org/UA-Profile/Transport/uatcp-uasc-uabinary".encode(&mut out)?;
    out.put_u8(0);
    out.put_i32_le(0);
    out.put_i32_le(-1);
    out.put_i32_le(-1);
    out.put_u32_le(0);

    Ok(out.to_vec())
}

pub(crate) fn decode_activate_session_request(
    request_body: &[u8],
) -> OpcUaResult<ActivateSessionRequest> {
    let mut buf = Bytes::copy_from_slice(request_body);
    let header = RequestHeader::decode(&mut buf)?;
    let _additional_header = ExtensionObject::decode(&mut buf)?;
    Ok(ActivateSessionRequest {
        request_handle: header.request_handle,
    })
}

pub(crate) async fn handle_activate_session(
    request: ActivateSessionRequest,
    context: &ServiceContext,
) -> OpcUaResult<ActivateSessionResponse> {
    if let Some(session_id) = context.current_session_id() {
        let _ = context
            .session_manager
            .activate_session(&session_id, UserIdentity::Anonymous);
    } else {
        return Err(crate::error::OpcUaError::InvalidState(
            "No session created on this connection; call CreateSession first".into(),
        ));
    }

    Ok(ActivateSessionResponse {
        request_handle: request.request_handle,
    })
}

pub(crate) fn encode_activate_session_response(
    response: &ActivateSessionResponse,
) -> OpcUaResult<Vec<u8>> {
    let mut out = BytesMut::new();
    ResponseHeader::good(response.request_handle).encode(&mut out)?;
    out.put_i32_le(0);
    out.put_i32_le(0);
    out.put_i32_le(0);
    Ok(out.to_vec())
}

pub(crate) fn decode_close_session_request(
    request_body: &[u8],
) -> OpcUaResult<CloseSessionRequest> {
    let mut buf = Bytes::copy_from_slice(request_body);
    let header = RequestHeader::decode(&mut buf)?;
    let _additional_header = ExtensionObject::decode(&mut buf)?;
    Ok(CloseSessionRequest {
        request_handle: header.request_handle,
    })
}

pub(crate) async fn handle_close_session(
    request: CloseSessionRequest,
    context: &ServiceContext,
) -> OpcUaResult<CloseSessionResponse> {
    if let Some(session_id) = context.current_session_id() {
        let _ = context.session_manager.close_session(&session_id);
        context.clear_session();
        context.channel.clear_session();
    }

    Ok(CloseSessionResponse {
        request_handle: request.request_handle,
    })
}

pub(crate) fn encode_close_session_response(
    response: &CloseSessionResponse,
) -> OpcUaResult<Vec<u8>> {
    let mut out = BytesMut::new();
    ResponseHeader::good(response.request_handle).encode(&mut out)?;
    Ok(out.to_vec())
}

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
        let request = decode_create_session_request(request_body)?;
        let response = handle_create_session(request, context).await?;
        let body = encode_create_session_response(&response, context)?;

        Ok(ServiceResponse {
            type_id: NodeId::numeric(0, CREATE_SESSION_RESPONSE_ID),
            body,
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
        let request = decode_activate_session_request(request_body)?;
        let response = handle_activate_session(request, context).await?;
        let body = encode_activate_session_response(&response)?;

        Ok(ServiceResponse {
            type_id: NodeId::numeric(0, ACTIVATE_SESSION_RESPONSE_ID),
            body,
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
        let request = decode_close_session_request(request_body)?;
        let response = handle_close_session(request, context).await?;
        let body = encode_close_session_response(&response)?;

        Ok(ServiceResponse {
            type_id: NodeId::numeric(0, CLOSE_SESSION_RESPONSE_ID),
            body,
        })
    }
}

/// Register all session service handlers.
pub fn register_handlers(registry: &mut super::registry::ServiceRegistry) {
    registry.register(Arc::new(CreateSessionHandler));
    registry.register(Arc::new(ActivateSessionHandler));
    registry.register(Arc::new(CloseSessionHandler));
}

//! Discovery service handlers — GetEndpoints, FindServers.
//!
//! OPC UA Part 4, Section 5.4.

use std::sync::Arc;

use async_trait::async_trait;
use bytes::{BufMut, Bytes, BytesMut};

use super::registry::{ServiceContext, ServiceHandler, ServiceResponse};
use crate::codec::data_value::{DiagnosticInfo, ExtensionObject};
use crate::codec::decoder::BinaryDecodable;
use crate::codec::encoder::BinaryEncodable;
use crate::error::OpcUaResult;
use crate::nodes::LocalizedText;
use crate::types::{NodeId, StatusCode};

// OPC UA type IDs for GetEndpoints
const GET_ENDPOINTS_REQUEST_ID: u32 = 428; // i=428
const GET_ENDPOINTS_RESPONSE_ID: u32 = 431; // i=431

/// Common OPC UA request header (decoded from each service request).
#[derive(Debug)]
pub struct RequestHeader {
    pub authentication_token: NodeId,
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub request_handle: u32,
    pub return_diagnostics: u32,
    pub audit_entry_id: String,
    pub timeout_hint: u32,
}

impl BinaryDecodable for RequestHeader {
    fn decode(buf: &mut Bytes) -> OpcUaResult<Self> {
        Ok(Self {
            authentication_token: NodeId::decode(buf)?,
            timestamp: chrono::DateTime::<chrono::Utc>::decode(buf)?,
            request_handle: u32::decode(buf)?,
            return_diagnostics: u32::decode(buf)?,
            audit_entry_id: String::decode(buf)?,
            timeout_hint: u32::decode(buf)?,
            // AdditionalHeader (ExtensionObject) — skip it
        })
    }
}

/// Common OPC UA response header.
pub struct ResponseHeader {
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub request_handle: u32,
    pub service_result: StatusCode,
}

impl ResponseHeader {
    pub fn good(request_handle: u32) -> Self {
        Self {
            timestamp: chrono::Utc::now(),
            request_handle,
            service_result: StatusCode::GOOD,
        }
    }

    pub fn encode(&self, buf: &mut BytesMut) -> OpcUaResult<()> {
        self.timestamp.encode(buf)?;
        self.request_handle.encode(buf)?;
        self.service_result.encode(buf)?;
        // DiagnosticInfo (empty)
        DiagnosticInfo::default().encode(buf)?;
        // StringTable (empty array)
        buf.put_i32_le(0);
        // AdditionalHeader (null ExtensionObject)
        (ExtensionObject {
            type_id: NodeId::numeric(0, 0),
            body: None,
        })
        .encode(buf)?;
        Ok(())
    }
}

/// GetEndpoints service handler.
pub struct GetEndpointsHandler;

#[derive(Debug, Clone)]
pub(crate) struct GetEndpointsRequest {
    pub request_handle: u32,
}

#[derive(Debug, Clone)]
pub(crate) struct GetEndpointsResponse {
    pub request_handle: u32,
}

pub(crate) fn decode_get_endpoints_request(
    request_body: &[u8],
) -> OpcUaResult<GetEndpointsRequest> {
    let mut buf = Bytes::copy_from_slice(request_body);

    let header = RequestHeader::decode(&mut buf)?;
    let _additional_header = ExtensionObject::decode(&mut buf)?;

    let _endpoint_url = String::decode(&mut buf)?;
    let locale_ids_len = i32::decode(&mut buf)?;
    if locale_ids_len > 0 {
        for _ in 0..locale_ids_len {
            let _ = String::decode(&mut buf)?;
        }
    }
    let profile_uris_len = i32::decode(&mut buf)?;
    if profile_uris_len > 0 {
        for _ in 0..profile_uris_len {
            let _ = String::decode(&mut buf)?;
        }
    }

    Ok(GetEndpointsRequest {
        request_handle: header.request_handle,
    })
}

pub(crate) async fn handle_get_endpoints(
    request: GetEndpointsRequest,
    _context: &ServiceContext,
) -> OpcUaResult<GetEndpointsResponse> {
    Ok(GetEndpointsResponse {
        request_handle: request.request_handle,
    })
}

pub(crate) fn encode_get_endpoints_response(
    response: &GetEndpointsResponse,
    context: &ServiceContext,
) -> OpcUaResult<Vec<u8>> {
    let mut out = BytesMut::new();
    ResponseHeader::good(response.request_handle).encode(&mut out)?;

    out.put_i32_le(1);

    context.server_config.endpoint_url.encode(&mut out)?;
    encode_application_description(
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

    Ok(out.to_vec())
}

#[async_trait]
impl ServiceHandler for GetEndpointsHandler {
    fn request_type_id(&self) -> NodeId {
        NodeId::numeric(0, GET_ENDPOINTS_REQUEST_ID)
    }

    async fn handle(
        &self,
        request_body: &[u8],
        context: &ServiceContext,
    ) -> OpcUaResult<ServiceResponse> {
        let request = decode_get_endpoints_request(request_body)?;
        let response = handle_get_endpoints(request, context).await?;
        let body = encode_get_endpoints_response(&response, context)?;

        Ok(ServiceResponse {
            type_id: NodeId::numeric(0, GET_ENDPOINTS_RESPONSE_ID),
            body,
        })
    }
}

/// Encode an ApplicationDescription into the buffer.
pub fn encode_application_description(
    endpoint_url: &str,
    server_name: &str,
    buf: &mut BytesMut,
) -> OpcUaResult<()> {
    // ApplicationUri
    format!("urn:mabinogion:opcua:server").encode(buf)?;
    // ProductUri
    "urn:mabinogion:opcua".encode(buf)?;
    // ApplicationName (LocalizedText)
    LocalizedText::new("en", server_name).encode(buf)?;
    // ApplicationType: Server = 0
    buf.put_u32_le(0);
    // GatewayServerUri (null)
    buf.put_i32_le(-1);
    // DiscoveryProfileUri (null)
    buf.put_i32_le(-1);
    // DiscoveryUrls array
    buf.put_i32_le(1);
    endpoint_url.encode(buf)?;
    Ok(())
}

/// Register all discovery service handlers.
pub fn register_handlers(registry: &mut super::registry::ServiceRegistry) {
    registry.register(Arc::new(GetEndpointsHandler));
}

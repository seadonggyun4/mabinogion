//! Per-connection handler for OPC UA TCP connections.
//!
//! Implements the full OPC UA connection lifecycle:
//! 1. Hello/Acknowledge handshake
//! 2. OpenSecureChannel (Issue)
//! 3. Service message loop (MSG → dispatch → response)
//!    - Handles in-band OPN renewals (requestType=Renew)
//! 4. CloseSecureChannel / disconnect

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use bytes::{Bytes, BytesMut};
use futures::{SinkExt, StreamExt};
use tokio::net::TcpStream;
use tokio_util::codec::Framed;
use tracing::{debug, error, info, warn};

use crate::codec::encoder::BinaryEncodable;
use crate::codec::decoder::BinaryDecodable;
use crate::codec::data_value::ExtensionObject;
use crate::channel::message::{
    OpenSecureChannelBody, SecureMessageBody,
    SequenceHeader, build_msg_response_body, build_opn_response_body,
};
use crate::channel::secure_channel::SecureChannel;
use crate::error::{OpcUaError, OpcUaResult};
use crate::service::registry::{ServiceContext, ServiceRegistry};
use crate::transport::codec::{OpcUaTransportCodec, build_response};
use crate::transport::messages::*;
use crate::transport::metrics::TransportMetrics;
use crate::types::{NodeId, StatusCode};

/// Configuration for a TCP connection handler.
pub struct ConnectionConfig {
    pub server_buffer_size: u32,
    pub connection_timeout: std::time::Duration,
}

impl Default for ConnectionConfig {
    fn default() -> Self {
        Self {
            server_buffer_size: DEFAULT_BUFFER_SIZE,
            connection_timeout: std::time::Duration::from_secs(60),
        }
    }
}

/// Handle a single OPC UA TCP connection.
pub async fn handle_connection(
    stream: TcpStream,
    service_registry: Arc<ServiceRegistry>,
    service_context_template: Arc<ServiceContextTemplate>,
    config: ConnectionConfig,
    metrics: Arc<TransportMetrics>,
    shutdown: Arc<AtomicBool>,
) -> OpcUaResult<()> {
    let peer = stream.peer_addr().map_err(|e| OpcUaError::Io(e))?;
    info!(peer = %peer, "New OPC UA connection");

    let mut framed = Framed::new(stream, OpcUaTransportCodec::new());

    // =====================================================================
    // Phase 1: Hello/Acknowledge handshake
    // =====================================================================
    let hello = match tokio::time::timeout(config.connection_timeout, framed.next()).await {
        Ok(Some(Ok(msg))) => {
            if msg.header.message_type != MessageType::Hello {
                let err_body = encode_error(StatusCode::BAD_UNEXPECTED_ERROR.raw(), "Expected HEL message");
                let _ = framed.send(build_response(MessageType::Error, err_body)).await;
                return Err(OpcUaError::ProtocolError("Expected HEL".into()));
            }
            metrics.record_message_received(msg.body.len() + MessageHeader::SIZE);
            let mut body = Bytes::from(msg.body);
            HelloMessage::decode(&mut body)?
        }
        Ok(Some(Err(e))) => return Err(e),
        Ok(None) => return Ok(()), // Connection closed
        Err(_) => return Err(OpcUaError::ProtocolError("Hello timeout".into())),
    };

    debug!(peer = %peer, endpoint = %hello.endpoint_url, "Received Hello");

    let ack = AcknowledgeMessage::from_hello(&hello, config.server_buffer_size);

    // Update codec with negotiated sizes
    framed.codec_mut().set_limits(ack.receive_buffer_size, ack.max_message_size);

    let mut ack_body = BytesMut::new();
    ack.encode(&mut ack_body)?;
    let ack_msg = build_response(MessageType::Acknowledge, ack_body.to_vec());
    framed.send(ack_msg).await.map_err(|e| OpcUaError::Io(std::io::Error::new(std::io::ErrorKind::Other, e.to_string())))?;
    metrics.record_message_sent(20 + MessageHeader::SIZE);

    debug!(peer = %peer, "Sent Acknowledge");

    // =====================================================================
    // Phase 2: OpenSecureChannel (initial Issue)
    // =====================================================================
    let channel = match tokio::time::timeout(config.connection_timeout, framed.next()).await {
        Ok(Some(Ok(msg))) => {
            if msg.header.message_type != MessageType::OpenSecureChannel {
                let err_body = encode_error(StatusCode::BAD_UNEXPECTED_ERROR.raw(), "Expected OPN message");
                let _ = framed.send(build_response(MessageType::Error, err_body)).await;
                return Err(OpcUaError::ProtocolError("Expected OPN".into()));
            }
            metrics.record_message_received(msg.body.len() + MessageHeader::SIZE);

            let opn = OpenSecureChannelBody::decode_from(&msg.body)?;
            let channel = SecureChannel::new_unsecured();
            let (request_handle, requested_lifetime) = decode_opn_request_fields(&opn.payload)?;

            // Build OpenSecureChannelResponse
            let response_body = build_opn_response(
                &channel,
                request_handle,
                requested_lifetime,
            )?;

            let opn_response = build_opn_response_body(
                channel.channel_id(),
                &SequenceHeader {
                    sequence_number: channel.next_server_sequence_number(),
                    request_id: opn.sequence_header.request_id,
                },
                &response_body,
            );

            let response_msg = build_response(MessageType::OpenSecureChannel, opn_response);
            framed.send(response_msg).await
                .map_err(|e| OpcUaError::Io(std::io::Error::new(std::io::ErrorKind::Other, e.to_string())))?;
            metrics.record_message_sent(0);

            debug!(peer = %peer, channel_id = channel.channel_id(), "Secure channel opened");
            channel
        }
        Ok(Some(Err(e))) => return Err(e),
        Ok(None) => return Ok(()),
        Err(_) => return Err(OpcUaError::ProtocolError("OPN timeout".into())),
    };

    let channel = Arc::new(channel);

    // Build the service context for this connection
    let context = Arc::new(ServiceContext {
        session_manager: service_context_template.session_manager.clone(),
        address_space: service_context_template.address_space.clone(),
        subscription_manager: service_context_template.subscription_manager.clone(),
        history_store: service_context_template.history_store.clone(),
        security_manager: service_context_template.security_manager.clone(),
        server_config: service_context_template.server_config.clone(),
        method_registry: service_context_template.method_registry.clone(),
        channel: channel.clone(),
        session_id: parking_lot::RwLock::new(None),
        auth_token: parking_lot::RwLock::new(None),
    });

    // =====================================================================
    // Phase 3: Service message loop
    // =====================================================================
    loop {
        if shutdown.load(Ordering::Relaxed) {
            break;
        }

        let msg = match tokio::time::timeout(config.connection_timeout, framed.next()).await {
            Ok(Some(Ok(msg))) => msg,
            Ok(Some(Err(e))) => {
                metrics.record_error();
                warn!(peer = %peer, error = %e, "Message decode error");
                break;
            }
            Ok(None) => {
                debug!(peer = %peer, "Connection closed by client");
                break;
            }
            Err(_) => {
                debug!(peer = %peer, "Connection timeout");
                break;
            }
        };

        metrics.record_message_received(msg.body.len() + MessageHeader::SIZE);

        match msg.header.message_type {
            // =============================================================
            // OPN inside the message loop = token renewal
            // =============================================================
            MessageType::OpenSecureChannel => {
                let opn = match OpenSecureChannelBody::decode_from(&msg.body) {
                    Ok(m) => m,
                    Err(e) => {
                        metrics.record_error();
                        warn!(peer = %peer, error = %e, "Failed to decode OPN renewal");
                        continue;
                    }
                };

                let (request_handle, requested_lifetime) =
                    match decode_opn_request_fields(&opn.payload) {
                        Ok(v) => v,
                        Err(e) => {
                            metrics.record_error();
                            warn!(peer = %peer, error = %e, "Failed to decode OPN request fields");
                            continue;
                        }
                    };

                // Renew the token on the existing channel (same channel_id,
                // new token_id) per OPC UA Part 6 §6.7.4.
                let lifetime = if requested_lifetime == 0 { 3_600_000 } else { requested_lifetime };
                channel.renew_token(lifetime);

                debug!(
                    peer = %peer,
                    channel_id = channel.channel_id(),
                    new_token_id = channel.token_id(),
                    lifetime_ms = lifetime,
                    "Secure channel token renewed"
                );

                let response_body = build_opn_response(
                    &channel,
                    request_handle,
                    lifetime,
                )?;

                let opn_response = build_opn_response_body(
                    channel.channel_id(),
                    &SequenceHeader {
                        sequence_number: channel.next_server_sequence_number(),
                        request_id: opn.sequence_header.request_id,
                    },
                    &response_body,
                );

                let response_msg = build_response(MessageType::OpenSecureChannel, opn_response);
                if let Err(e) = framed.send(response_msg).await {
                    error!(peer = %peer, error = %e, "Failed to send OPN renewal response");
                    break;
                }
                metrics.record_message_sent(0);
            }
            // =============================================================
            // MSG — normal service request dispatch
            // =============================================================
            MessageType::Message => {
                let secure_msg = match SecureMessageBody::decode_from(&msg.body) {
                    Ok(m) => m,
                    Err(e) => {
                        metrics.record_error();
                        warn!(peer = %peer, error = %e, "Failed to decode MSG");
                        continue;
                    }
                };

                // Dispatch to service handler
                match service_registry.dispatch(&secure_msg.payload, &context).await {
                    Ok(response_payload) => {
                        let response_body = build_msg_response_body(
                            channel.channel_id(),
                            channel.token_id(),
                            &SequenceHeader {
                                sequence_number: channel.next_server_sequence_number(),
                                request_id: secure_msg.sequence_header.request_id,
                            },
                            &response_payload,
                        );
                        let response_msg = build_response(MessageType::Message, response_body);
                        if let Err(e) = framed.send(response_msg).await {
                            error!(peer = %peer, error = %e, "Failed to send response");
                            break;
                        }
                        metrics.record_message_sent(0);
                    }
                    Err(e) => {
                        warn!(peer = %peer, error = %e, "Service handler error");
                        // Send a ServiceFault response
                        let fault_payload = build_service_fault(
                            secure_msg.sequence_header.request_id,
                            StatusCode::BAD_SERVICE_UNSUPPORTED,
                        )?;
                        let response_body = build_msg_response_body(
                            channel.channel_id(),
                            channel.token_id(),
                            &SequenceHeader {
                                sequence_number: channel.next_server_sequence_number(),
                                request_id: secure_msg.sequence_header.request_id,
                            },
                            &fault_payload,
                        );
                        let _ = framed.send(build_response(MessageType::Message, response_body)).await;
                        metrics.record_error();
                    }
                }
            }
            // =============================================================
            // CLO — close secure channel
            // =============================================================
            MessageType::CloseSecureChannel => {
                debug!(peer = %peer, "Received CLO — closing channel");

                // Clean up session if one exists
                if let Some(session_id) = context.current_session_id() {
                    let _ = context.session_manager.close_session(&session_id);
                    context.clear_session();
                    debug!(peer = %peer, "Session closed during CLO");
                }

                // Send CLO response per OPC UA spec (mirror the CLO message type)
                let clo_body = build_msg_response_body(
                    channel.channel_id(),
                    channel.token_id(),
                    &SequenceHeader {
                        sequence_number: channel.next_server_sequence_number(),
                        request_id: 0,
                    },
                    &[],
                );
                let _ = framed
                    .send(build_response(MessageType::CloseSecureChannel, clo_body))
                    .await;

                break;
            }
            other => {
                warn!(peer = %peer, msg_type = ?other, "Unexpected message type in service loop");
                metrics.record_error();
            }
        }
    }

    info!(peer = %peer, "Connection closed");
    Ok(())
}

/// Template for creating per-connection ServiceContext instances.
/// Holds shared references to all server components.
pub struct ServiceContextTemplate {
    pub session_manager: Arc<crate::services::SessionManager>,
    pub address_space: Arc<crate::nodes::AddressSpace>,
    pub subscription_manager: Arc<crate::services::SubscriptionManager>,
    pub history_store: Arc<crate::services::HistoryStore>,
    pub security_manager: Arc<crate::security::SecurityManager>,
    pub server_config: Arc<crate::config::OpcUaServerConfig>,
    pub method_registry: Arc<crate::service::method_call::MethodRegistry>,
}

// =============================================================================
// OpenSecureChannel helpers
// =============================================================================

/// Decode the OpenSecureChannelRequest payload, returning
/// `(request_handle, requested_lifetime)`.
///
/// The `request_type` is parsed but currently not branched on at this
/// level — the caller decides whether to Issue or Renew based on
/// context (Phase 2 = Issue, Phase 3 = Renew).
fn decode_opn_request_fields(payload: &[u8]) -> OpcUaResult<(u32, u32)> {
    let mut buf = Bytes::from(payload.to_vec());
    // type_id (NodeId)
    let _type_id = NodeId::decode(&mut buf)?;
    // RequestHeader fields
    let _auth_token = NodeId::decode(&mut buf)?;
    let _timestamp = chrono::DateTime::<chrono::Utc>::decode(&mut buf)?;
    let request_handle = u32::decode(&mut buf)?;
    let _return_diagnostics = u32::decode(&mut buf)?;
    let _audit_entry_id = String::decode(&mut buf)?;
    let _timeout_hint = u32::decode(&mut buf)?;
    let _additional_header = ExtensionObject::decode(&mut buf)?;
    // OpenSecureChannelRequest fields
    let _client_protocol_version = u32::decode(&mut buf)?;
    let _request_type = u32::decode(&mut buf)?; // 0=Issue, 1=Renew
    let _security_mode = u32::decode(&mut buf)?;
    let _client_nonce = Vec::<u8>::decode(&mut buf)?;
    let requested_lifetime = u32::decode(&mut buf)?;

    Ok((request_handle, requested_lifetime))
}

/// Build an OpenSecureChannelResponse payload.
fn build_opn_response(
    channel: &SecureChannel,
    request_handle: u32,
    requested_lifetime: u32,
) -> OpcUaResult<Vec<u8>> {
    let mut buf = BytesMut::new();

    // Response is wrapped as: NodeId (type) + body
    // OpenSecureChannelResponse type id = 449
    let type_id = NodeId::numeric(0, 449);
    type_id.encode(&mut buf)?;

    // ResponseHeader
    crate::service::discovery::ResponseHeader::good(request_handle).encode(&mut buf)?;

    // ServerProtocolVersion
    buf.extend_from_slice(&0u32.to_le_bytes());
    // SecurityToken
    buf.extend_from_slice(&channel.channel_id().to_le_bytes());
    buf.extend_from_slice(&channel.token_id().to_le_bytes());
    // CreatedAt
    chrono::Utc::now().encode(&mut buf)?;
    // RevisedLifetime
    let lifetime = if requested_lifetime == 0 { 3_600_000u32 } else { requested_lifetime };
    buf.extend_from_slice(&lifetime.to_le_bytes());
    // ServerNonce (empty)
    buf.extend_from_slice(&0i32.to_le_bytes());

    Ok(buf.to_vec())
}

/// Build a ServiceFault response.
fn build_service_fault(request_id: u32, status: StatusCode) -> OpcUaResult<Vec<u8>> {
    let mut buf = BytesMut::new();

    // ServiceFault type id = 397 — encode as raw NodeId + body (no ExtensionObject wrapper)
    NodeId::numeric(0, 397).encode(&mut buf)?;
    crate::service::discovery::ResponseHeader {
        timestamp: chrono::Utc::now(),
        request_handle: request_id,
        service_result: status,
    }.encode(&mut buf)?;
    Ok(buf.to_vec())
}

/// Encode an error message body.
fn encode_error(error_code: u32, reason: &str) -> Vec<u8> {
    let msg = ErrorMessage {
        error: error_code,
        reason: reason.to_string(),
    };
    let mut buf = BytesMut::new();
    let _ = msg.encode(&mut buf);
    buf.to_vec()
}

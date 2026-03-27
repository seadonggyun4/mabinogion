use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use futures::{SinkExt, StreamExt};
use tokio::net::TcpStream;
use tokio_util::codec::Framed;
use tracing::{debug, error, info, warn};

use crate::channel::message::{
    build_msg_response_body, build_opn_response_body, OpenSecureChannelBody, SecureMessageBody,
    SequenceHeader,
};
use crate::codec::decoder::BinaryDecodable;
use crate::codec::encoder::BinaryEncodable;
use crate::core::dispatch::dispatch_payload;
use crate::core::registry::ServiceContext;
use crate::core::services::BuiltinServiceSet;
use crate::core::status::ServiceError;
use crate::error::{OpcUaError, OpcUaResult};
use crate::transport::codec::{build_response, OpcUaTransportCodec};
use crate::transport::connection::ServiceContextTemplate;
use crate::transport::hooks::TransportHooks;
use crate::transport::messages::{AcknowledgeMessage, HelloMessage, MessageHeader, MessageType};
use crate::transport::secure_channel_runtime::{
    build_service_fault, decode_open_secure_channel_request_fields, encode_transport_error,
    SecureChannelRuntime,
};
use crate::types::StatusCode;

/// Internal runtime policy for a single UA-TCP transport stack.
#[derive(Debug, Clone)]
pub(crate) struct TransportRuntimePolicy {
    pub(crate) connection_timeout: Duration,
    pub(crate) server_buffer_size: u32,
}

impl Default for TransportRuntimePolicy {
    fn default() -> Self {
        Self {
            connection_timeout: Duration::from_secs(60),
            server_buffer_size: 65_535,
        }
    }
}

/// Canonical transport runtime for UA-TCP connections.
#[derive(Clone)]
pub(crate) struct TransportRuntime {
    services: Arc<BuiltinServiceSet>,
    context_template: Arc<ServiceContextTemplate>,
    policy: TransportRuntimePolicy,
    hooks: TransportHooks,
}

impl TransportRuntime {
    pub(crate) fn new(
        services: Arc<BuiltinServiceSet>,
        context_template: Arc<ServiceContextTemplate>,
        policy: TransportRuntimePolicy,
        hooks: TransportHooks,
    ) -> Self {
        Self {
            services,
            context_template,
            policy,
            hooks,
        }
    }

    pub(crate) fn metrics(&self) -> &Arc<crate::transport::metrics::TransportMetrics> {
        self.hooks.metrics()
    }

    pub(crate) fn record_rejection(&self) {
        self.hooks.record_rejection();
    }

    pub(crate) async fn handle_tcp_stream(
        &self,
        stream: TcpStream,
        shutdown: Arc<AtomicBool>,
    ) -> OpcUaResult<()> {
        let peer = stream.peer_addr().map_err(OpcUaError::Io)?;
        self.hooks.record_connection();
        info!(peer = %peer, "New OPC UA connection");

        let result = self.handle_tcp_stream_inner(stream, shutdown).await;
        self.hooks.record_disconnection();
        if let Err(error) = &result {
            self.hooks.record_error();
            warn!(peer = %peer, error = %error, "Transport runtime error");
        }
        info!(peer = %peer, "Connection closed");
        result
    }

    async fn handle_tcp_stream_inner(
        &self,
        stream: TcpStream,
        shutdown: Arc<AtomicBool>,
    ) -> OpcUaResult<()> {
        let peer = stream.peer_addr().map_err(OpcUaError::Io)?;
        let mut framed = Framed::new(stream, OpcUaTransportCodec::new());

        let hello = match tokio::time::timeout(self.policy.connection_timeout, framed.next()).await
        {
            Ok(Some(Ok(message))) => {
                if message.header.message_type != MessageType::Hello {
                    let err_body = encode_transport_error(
                        StatusCode::BAD_UNEXPECTED_ERROR.raw(),
                        "Expected HEL message",
                    );
                    let _ = framed
                        .send(build_response(MessageType::Error, err_body))
                        .await;
                    return Err(OpcUaError::ProtocolError("Expected HEL".into()));
                }
                self.hooks
                    .record_message_received(message.body.len() + MessageHeader::SIZE);
                let mut body = bytes::Bytes::from(message.body);
                HelloMessage::decode(&mut body)?
            }
            Ok(Some(Err(error))) => return Err(error),
            Ok(None) => return Ok(()),
            Err(_) => return Err(OpcUaError::ProtocolError("Hello timeout".into())),
        };

        let acknowledge = AcknowledgeMessage::from_hello(&hello, self.policy.server_buffer_size);
        framed.codec_mut().set_limits(
            acknowledge.receive_buffer_size,
            acknowledge.max_message_size,
        );

        let mut ack_body = bytes::BytesMut::new();
        acknowledge.encode(&mut ack_body)?;
        let ack_message = build_response(MessageType::Acknowledge, ack_body.to_vec());
        framed.send(ack_message).await.map_err(io_error_from_sink)?;
        self.hooks
            .record_message_sent(ack_body.len() + MessageHeader::SIZE);

        debug!(peer = %peer, "Sent Acknowledge");

        let secure_channel =
            match tokio::time::timeout(self.policy.connection_timeout, framed.next()).await {
                Ok(Some(Ok(message))) => {
                    if message.header.message_type != MessageType::OpenSecureChannel {
                        let err_body = encode_transport_error(
                            StatusCode::BAD_UNEXPECTED_ERROR.raw(),
                            "Expected OPN message",
                        );
                        let _ = framed
                            .send(build_response(MessageType::Error, err_body))
                            .await;
                        return Err(OpcUaError::ProtocolError("Expected OPN".into()));
                    }
                    self.hooks
                        .record_message_received(message.body.len() + MessageHeader::SIZE);

                    let request = OpenSecureChannelBody::decode_from(&message.body)?;
                    let (request_handle, requested_lifetime_ms) =
                        decode_open_secure_channel_request_fields(&request.payload)?;

                    let secure_channel = SecureChannelRuntime::new_unsecured();
                    let response_payload = secure_channel.build_open_secure_channel_response(
                        request_handle,
                        requested_lifetime_ms,
                    )?;
                    let response_body = build_opn_response_body(
                        secure_channel.channel_id(),
                        &SequenceHeader {
                            sequence_number: secure_channel.next_server_sequence_number(),
                            request_id: request.sequence_header.request_id,
                        },
                        &response_payload,
                    );

                    let response =
                        build_response(MessageType::OpenSecureChannel, response_body.clone());
                    framed.send(response).await.map_err(io_error_from_sink)?;
                    self.hooks
                        .record_message_sent(response_body.len() + MessageHeader::SIZE);
                    secure_channel
                }
                Ok(Some(Err(error))) => return Err(error),
                Ok(None) => return Ok(()),
                Err(_) => return Err(OpcUaError::ProtocolError("OPN timeout".into())),
            };

        let context = Arc::new(ServiceContext {
            session_manager: self.context_template.session_manager.clone(),
            address_space: self.context_template.address_space.clone(),
            subscription_manager: self.context_template.subscription_manager.clone(),
            history_store: self.context_template.history_store.clone(),
            security_manager: self.context_template.security_manager.clone(),
            server_config: self.context_template.server_config.clone(),
            method_registry: self.context_template.method_registry.clone(),
            channel: secure_channel.channel(),
            session_id: parking_lot::RwLock::new(None),
            auth_token: parking_lot::RwLock::new(None),
        });

        loop {
            if shutdown.load(Ordering::Relaxed) {
                break;
            }

            let message =
                match tokio::time::timeout(self.policy.connection_timeout, framed.next()).await {
                    Ok(Some(Ok(message))) => message,
                    Ok(Some(Err(error))) => return Err(error),
                    Ok(None) => break,
                    Err(_) => {
                        debug!(peer = %peer, "Connection timeout");
                        break;
                    }
                };

            self.hooks
                .record_message_received(message.body.len() + MessageHeader::SIZE);

            match message.header.message_type {
                MessageType::OpenSecureChannel => {
                    let renewal = OpenSecureChannelBody::decode_from(&message.body)?;
                    let (request_handle, requested_lifetime_ms) =
                        decode_open_secure_channel_request_fields(&renewal.payload)?;
                    let revised_lifetime_ms = secure_channel.renew_token(requested_lifetime_ms);
                    let response_payload = secure_channel
                        .build_open_secure_channel_response(request_handle, revised_lifetime_ms)?;
                    let response_body = build_opn_response_body(
                        secure_channel.channel_id(),
                        &SequenceHeader {
                            sequence_number: secure_channel.next_server_sequence_number(),
                            request_id: renewal.sequence_header.request_id,
                        },
                        &response_payload,
                    );

                    let response =
                        build_response(MessageType::OpenSecureChannel, response_body.clone());
                    framed.send(response).await.map_err(io_error_from_sink)?;
                    self.hooks
                        .record_message_sent(response_body.len() + MessageHeader::SIZE);
                }
                MessageType::Message => {
                    let secure_message = match SecureMessageBody::decode_from(&message.body) {
                        Ok(message) => message,
                        Err(error) => {
                            self.hooks.record_error();
                            warn!(peer = %peer, error = %error, "Failed to decode MSG");
                            continue;
                        }
                    };

                    if let Err(error) = secure_channel.validate_message(&secure_message) {
                        self.hooks.record_error();
                        let fault_payload = build_service_fault(
                            secure_message.sequence_header.request_id,
                            map_service_status(&ServiceError::from(error)),
                        )?;
                        let response_body = build_msg_response_body(
                            secure_channel.channel_id(),
                            secure_channel.token_id(),
                            &SequenceHeader {
                                sequence_number: secure_channel.next_server_sequence_number(),
                                request_id: secure_message.sequence_header.request_id,
                            },
                            &fault_payload,
                        );
                        let response = build_response(MessageType::Message, response_body.clone());
                        let _ = framed.send(response).await;
                        self.hooks
                            .record_message_sent(response_body.len() + MessageHeader::SIZE);
                        continue;
                    }

                    match dispatch_payload(&secure_message.payload, &context, &self.services).await
                    {
                        Ok(response_payload) => {
                            let response_body = build_msg_response_body(
                                secure_channel.channel_id(),
                                secure_channel.token_id(),
                                &SequenceHeader {
                                    sequence_number: secure_channel.next_server_sequence_number(),
                                    request_id: secure_message.sequence_header.request_id,
                                },
                                &response_payload,
                            );
                            let response =
                                build_response(MessageType::Message, response_body.clone());
                            framed.send(response).await.map_err(io_error_from_sink)?;
                            self.hooks
                                .record_message_sent(response_body.len() + MessageHeader::SIZE);
                        }
                        Err(error) => {
                            self.hooks.record_error();
                            let fault_payload = build_service_fault(
                                secure_message.sequence_header.request_id,
                                map_service_status(&error),
                            )?;
                            let response_body = build_msg_response_body(
                                secure_channel.channel_id(),
                                secure_channel.token_id(),
                                &SequenceHeader {
                                    sequence_number: secure_channel.next_server_sequence_number(),
                                    request_id: secure_message.sequence_header.request_id,
                                },
                                &fault_payload,
                            );
                            let response =
                                build_response(MessageType::Message, response_body.clone());
                            if let Err(send_error) = framed.send(response).await {
                                error!(peer = %peer, error = %send_error, "Failed to send ServiceFault");
                                break;
                            }
                            self.hooks
                                .record_message_sent(response_body.len() + MessageHeader::SIZE);
                        }
                    }
                }
                MessageType::CloseSecureChannel => {
                    if let Some(session_id) = context.current_session_id() {
                        let _ = context.session_manager.close_session(&session_id);
                        context.clear_session();
                        context.channel.clear_session();
                    }

                    let response_body = build_msg_response_body(
                        secure_channel.channel_id(),
                        secure_channel.token_id(),
                        &SequenceHeader {
                            sequence_number: secure_channel.next_server_sequence_number(),
                            request_id: 0,
                        },
                        &[],
                    );
                    let response =
                        build_response(MessageType::CloseSecureChannel, response_body.clone());
                    let _ = framed.send(response).await;
                    self.hooks
                        .record_message_sent(response_body.len() + MessageHeader::SIZE);
                    break;
                }
                other => {
                    self.hooks.record_error();
                    warn!(peer = %peer, ?other, "Unexpected message type in service loop");
                }
            }
        }

        Ok(())
    }
}

fn io_error_from_sink(error: impl std::fmt::Display) -> OpcUaError {
    OpcUaError::Io(std::io::Error::other(error.to_string()))
}

fn map_service_status(error: &ServiceError) -> StatusCode {
    error.status_code()
}

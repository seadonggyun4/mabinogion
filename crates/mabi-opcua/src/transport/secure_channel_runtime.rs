use std::sync::Arc;

use bytes::{Bytes, BytesMut};

use crate::channel::message::SecureMessageBody;
use crate::channel::secure_channel::SecureChannel;
use crate::codec::data_value::ExtensionObject;
use crate::codec::decoder::BinaryDecodable;
use crate::codec::encoder::BinaryEncodable;
use crate::core::headers::ResponseHeader;
use crate::error::{OpcUaError, OpcUaResult};
use crate::types::{NodeId, StatusCode};

/// Runtime-owned secure channel state handle.
#[derive(Debug, Clone)]
pub(crate) struct SecureChannelRuntime {
    channel: Arc<SecureChannel>,
}

impl SecureChannelRuntime {
    pub(crate) fn new_unsecured() -> Self {
        Self {
            channel: Arc::new(SecureChannel::new_unsecured()),
        }
    }

    pub(crate) fn channel(&self) -> Arc<SecureChannel> {
        self.channel.clone()
    }

    pub(crate) fn channel_id(&self) -> u32 {
        self.channel.channel_id()
    }

    pub(crate) fn token_id(&self) -> u32 {
        self.channel.token_id()
    }

    pub(crate) fn next_server_sequence_number(&self) -> u32 {
        self.channel.next_server_sequence_number()
    }

    pub(crate) fn renew_token(&self, requested_lifetime_ms: u32) -> u32 {
        let lifetime_ms = if requested_lifetime_ms == 0 {
            3_600_000
        } else {
            requested_lifetime_ms
        };
        self.channel.renew_token(lifetime_ms);
        lifetime_ms
    }

    pub(crate) fn validate_message(&self, message: &SecureMessageBody) -> OpcUaResult<()> {
        if message.secure_channel_id != self.channel_id() {
            return Err(OpcUaError::BadSecureChannelId(message.secure_channel_id));
        }
        if !self.channel.matches_token(message.security_header.token_id) {
            return Err(OpcUaError::BadSecureChannelId(
                message.security_header.token_id,
            ));
        }
        if self.channel.is_token_expired() {
            return Err(OpcUaError::Security(
                "Secure channel token expired".to_string(),
            ));
        }
        if !self
            .channel
            .validate_sequence_number(message.sequence_header.sequence_number)
        {
            return Err(OpcUaError::BadSequenceNumber {
                expected: self.channel.expected_client_sequence_number(),
                actual: message.sequence_header.sequence_number,
            });
        }
        Ok(())
    }

    pub(crate) fn build_open_secure_channel_response(
        &self,
        request_handle: u32,
        requested_lifetime_ms: u32,
    ) -> OpcUaResult<Vec<u8>> {
        let mut buf = BytesMut::new();

        NodeId::numeric(0, 449).encode(&mut buf)?;
        ResponseHeader::good(request_handle).encode(&mut buf)?;
        0u32.encode(&mut buf)?;
        self.channel_id().encode(&mut buf)?;
        self.token_id().encode(&mut buf)?;
        chrono::Utc::now().encode(&mut buf)?;
        let revised_lifetime_ms = if requested_lifetime_ms == 0 {
            3_600_000
        } else {
            requested_lifetime_ms
        };
        revised_lifetime_ms.encode(&mut buf)?;
        Vec::<u8>::new().encode(&mut buf)?;

        Ok(buf.to_vec())
    }
}

pub(crate) fn decode_open_secure_channel_request_fields(payload: &[u8]) -> OpcUaResult<(u32, u32)> {
    let mut buf = Bytes::copy_from_slice(payload);
    let _type_id = NodeId::decode(&mut buf)?;
    let _auth_token = NodeId::decode(&mut buf)?;
    let _timestamp = chrono::DateTime::<chrono::Utc>::decode(&mut buf)?;
    let request_handle = u32::decode(&mut buf)?;
    let _return_diagnostics = u32::decode(&mut buf)?;
    let _audit_entry_id = String::decode(&mut buf)?;
    let _timeout_hint = u32::decode(&mut buf)?;
    let _additional_header = ExtensionObject::decode(&mut buf)?;
    let _client_protocol_version = u32::decode(&mut buf)?;
    let _request_type = u32::decode(&mut buf)?;
    let _security_mode = u32::decode(&mut buf)?;
    let _client_nonce = Vec::<u8>::decode(&mut buf)?;
    let requested_lifetime = u32::decode(&mut buf)?;

    Ok((request_handle, requested_lifetime))
}

pub(crate) fn build_service_fault(request_handle: u32, status: StatusCode) -> OpcUaResult<Vec<u8>> {
    let mut buf = BytesMut::new();
    NodeId::numeric(0, 397).encode(&mut buf)?;
    ResponseHeader {
        timestamp: chrono::Utc::now(),
        request_handle,
        service_result: status,
    }
    .encode(&mut buf)?;
    Ok(buf.to_vec())
}

pub(crate) fn encode_transport_error(error_code: u32, reason: &str) -> Vec<u8> {
    use crate::transport::messages::ErrorMessage;

    let message = ErrorMessage {
        error: error_code,
        reason: reason.to_string(),
    };
    let mut buf = BytesMut::new();
    let _ = message.encode(&mut buf);
    buf.to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::channel::message::{SecureMessageBody, SequenceHeader, SymmetricSecurityHeader};

    #[test]
    fn secure_channel_renew_keeps_channel_id() {
        let runtime = SecureChannelRuntime::new_unsecured();
        let channel_id = runtime.channel_id();
        let first_token = runtime.token_id();

        runtime.renew_token(30_000);

        assert_eq!(runtime.channel_id(), channel_id);
        assert_ne!(runtime.token_id(), first_token);
    }

    #[test]
    fn sequence_validation_rejects_replay() {
        let runtime = SecureChannelRuntime::new_unsecured();
        let body = SecureMessageBody {
            secure_channel_id: runtime.channel_id(),
            security_header: SymmetricSecurityHeader {
                token_id: runtime.token_id(),
            },
            sequence_header: SequenceHeader {
                sequence_number: 1,
                request_id: 1,
            },
            payload: Vec::new(),
        };

        runtime.validate_message(&body).unwrap();
        let replay = SecureMessageBody {
            sequence_header: SequenceHeader {
                sequence_number: 1,
                request_id: 2,
            },
            ..body
        };

        assert!(matches!(
            runtime.validate_message(&replay),
            Err(OpcUaError::BadSequenceNumber { .. })
        ));
    }
}

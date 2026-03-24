//! OPC UA Secure Channel message parsing.
//!
//! Handles the security headers and sequence headers in OPN/MSG/CLO messages.

use bytes::{BufMut, Bytes, BytesMut};

use crate::codec::decoder::BinaryDecodable;
use crate::codec::encoder::BinaryEncodable;
use crate::error::OpcUaResult;

// =========================================================================
// Asymmetric Security Header (used in OPN messages)
// =========================================================================

/// Asymmetric security header — present in OpenSecureChannel messages.
#[derive(Debug, Clone)]
pub struct AsymmetricSecurityHeader {
    /// Security policy URI (e.g., "http://opcfoundation.org/UA/SecurityPolicy#None").
    pub security_policy_uri: String,
    /// Sender certificate (DER encoded). Empty for SecurityPolicy::None.
    pub sender_certificate: Vec<u8>,
    /// Receiver certificate thumbprint. Empty for SecurityPolicy::None.
    pub receiver_certificate_thumbprint: Vec<u8>,
}

impl AsymmetricSecurityHeader {
    /// Create a header for SecurityPolicy::None (no security).
    pub fn none() -> Self {
        Self {
            security_policy_uri: "http://opcfoundation.org/UA/SecurityPolicy#None".into(),
            sender_certificate: Vec::new(),
            receiver_certificate_thumbprint: Vec::new(),
        }
    }
}

impl BinaryEncodable for AsymmetricSecurityHeader {
    fn encode(&self, buf: &mut BytesMut) -> OpcUaResult<()> {
        self.security_policy_uri.encode(buf)?;
        self.sender_certificate.encode(buf)?;
        self.receiver_certificate_thumbprint.encode(buf)?;
        Ok(())
    }
    fn encoded_size(&self) -> usize {
        (4 + self.security_policy_uri.len())
            + (4 + self.sender_certificate.len())
            + (4 + self.receiver_certificate_thumbprint.len())
    }
}

impl BinaryDecodable for AsymmetricSecurityHeader {
    fn decode(buf: &mut Bytes) -> OpcUaResult<Self> {
        Ok(Self {
            security_policy_uri: String::decode(buf)?,
            sender_certificate: Vec::<u8>::decode(buf)?,
            receiver_certificate_thumbprint: Vec::<u8>::decode(buf)?,
        })
    }
}

// =========================================================================
// Symmetric Security Header (used in MSG/CLO messages)
// =========================================================================

/// Symmetric security header — present in MSG and CLO messages.
#[derive(Debug, Clone, Copy)]
pub struct SymmetricSecurityHeader {
    /// Token ID identifying the secure channel token.
    pub token_id: u32,
}

impl BinaryEncodable for SymmetricSecurityHeader {
    fn encode(&self, buf: &mut BytesMut) -> OpcUaResult<()> {
        self.token_id.encode(buf)
    }
    fn encoded_size(&self) -> usize {
        4
    }
}

impl BinaryDecodable for SymmetricSecurityHeader {
    fn decode(buf: &mut Bytes) -> OpcUaResult<Self> {
        Ok(Self {
            token_id: u32::decode(buf)?,
        })
    }
}

// =========================================================================
// Sequence Header (common to OPN/MSG/CLO)
// =========================================================================

/// Sequence header — present in all secure messages.
#[derive(Debug, Clone, Copy)]
pub struct SequenceHeader {
    pub sequence_number: u32,
    pub request_id: u32,
}

impl BinaryEncodable for SequenceHeader {
    fn encode(&self, buf: &mut BytesMut) -> OpcUaResult<()> {
        self.sequence_number.encode(buf)?;
        self.request_id.encode(buf)?;
        Ok(())
    }
    fn encoded_size(&self) -> usize {
        8
    }
}

impl BinaryDecodable for SequenceHeader {
    fn decode(buf: &mut Bytes) -> OpcUaResult<Self> {
        Ok(Self {
            sequence_number: u32::decode(buf)?,
            request_id: u32::decode(buf)?,
        })
    }
}

// =========================================================================
// Secure channel message body parsing
// =========================================================================

/// Parsed secure channel message from an OPN raw body.
#[derive(Debug)]
pub struct OpenSecureChannelBody {
    pub secure_channel_id: u32,
    pub security_header: AsymmetricSecurityHeader,
    pub sequence_header: SequenceHeader,
    /// The remaining body after headers (contains the service request).
    pub payload: Vec<u8>,
}

impl OpenSecureChannelBody {
    /// Parse from raw body bytes (after the 8-byte message header).
    pub fn decode_from(body: &[u8]) -> OpcUaResult<Self> {
        let mut buf = Bytes::copy_from_slice(body);
        let secure_channel_id = u32::decode(&mut buf)?;
        let security_header = AsymmetricSecurityHeader::decode(&mut buf)?;
        let sequence_header = SequenceHeader::decode(&mut buf)?;
        let payload = buf.to_vec();
        Ok(Self {
            secure_channel_id,
            security_header,
            sequence_header,
            payload,
        })
    }
}

/// Parsed secure channel message from a MSG/CLO raw body.
#[derive(Debug)]
pub struct SecureMessageBody {
    pub secure_channel_id: u32,
    pub security_header: SymmetricSecurityHeader,
    pub sequence_header: SequenceHeader,
    /// The remaining body after headers (contains the service request).
    pub payload: Vec<u8>,
}

impl SecureMessageBody {
    /// Parse from raw body bytes (after the 8-byte message header).
    pub fn decode_from(body: &[u8]) -> OpcUaResult<Self> {
        let mut buf = Bytes::copy_from_slice(body);
        let secure_channel_id = u32::decode(&mut buf)?;
        let security_header = SymmetricSecurityHeader::decode(&mut buf)?;
        let sequence_header = SequenceHeader::decode(&mut buf)?;
        let payload = buf.to_vec();
        Ok(Self {
            secure_channel_id,
            security_header,
            sequence_header,
            payload,
        })
    }
}

/// Build a response body for an OPN response (asymmetric header).
pub fn build_opn_response_body(
    secure_channel_id: u32,
    sequence_header: &SequenceHeader,
    payload: &[u8],
) -> Vec<u8> {
    let mut buf = BytesMut::new();
    buf.put_u32_le(secure_channel_id);
    let _ = AsymmetricSecurityHeader::none().encode(&mut buf);
    let _ = sequence_header.encode(&mut buf);
    buf.extend_from_slice(payload);
    buf.to_vec()
}

/// Build a response body for a MSG response (symmetric header).
pub fn build_msg_response_body(
    secure_channel_id: u32,
    token_id: u32,
    sequence_header: &SequenceHeader,
    payload: &[u8],
) -> Vec<u8> {
    let mut buf = BytesMut::new();
    buf.put_u32_le(secure_channel_id);
    let _ = (SymmetricSecurityHeader { token_id }).encode(&mut buf);
    let _ = sequence_header.encode(&mut buf);
    buf.extend_from_slice(payload);
    buf.to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_asymmetric_header_roundtrip() {
        let header = AsymmetricSecurityHeader::none();
        let mut buf = BytesMut::new();
        header.encode(&mut buf).unwrap();
        let mut data = buf.freeze();
        let decoded = AsymmetricSecurityHeader::decode(&mut data).unwrap();
        assert_eq!(decoded.security_policy_uri, header.security_policy_uri);
    }

    #[test]
    fn test_sequence_header_roundtrip() {
        let header = SequenceHeader {
            sequence_number: 1,
            request_id: 42,
        };
        let mut buf = BytesMut::new();
        header.encode(&mut buf).unwrap();
        let mut data = buf.freeze();
        let decoded = SequenceHeader::decode(&mut data).unwrap();
        assert_eq!(decoded.sequence_number, 1);
        assert_eq!(decoded.request_id, 42);
    }
}

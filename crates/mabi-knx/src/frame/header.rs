//! KNXnet/IP header.

use bytes::{Buf, BufMut, BytesMut};

use super::ServiceType;
use crate::error::{KnxError, KnxResult};

/// KNXnet/IP header size (always 6 bytes).
pub const HEADER_SIZE: u8 = 0x06;

/// KNXnet/IP protocol version (1.0).
pub const PROTOCOL_VERSION: u8 = 0x10;

/// KNXnet/IP header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KnxNetIpHeader {
    /// Service type.
    pub service_type: ServiceType,
    /// Total frame length (including header).
    pub total_length: u16,
}

impl KnxNetIpHeader {
    /// Create a new header.
    pub fn new(service_type: ServiceType, body_length: usize) -> Self {
        Self {
            service_type,
            total_length: (HEADER_SIZE as usize + body_length) as u16,
        }
    }

    /// Get body length.
    pub fn body_length(&self) -> usize {
        self.total_length as usize - HEADER_SIZE as usize
    }

    /// Encode to bytes.
    pub fn encode(&self) -> [u8; 6] {
        let mut buf = [0u8; 6];
        buf[0] = HEADER_SIZE;
        buf[1] = PROTOCOL_VERSION;
        let service_bytes = u16::from(self.service_type).to_be_bytes();
        buf[2] = service_bytes[0];
        buf[3] = service_bytes[1];
        let length_bytes = self.total_length.to_be_bytes();
        buf[4] = length_bytes[0];
        buf[5] = length_bytes[1];
        buf
    }

    /// Encode to BytesMut.
    pub fn encode_to(&self, buf: &mut BytesMut) {
        buf.put_u8(HEADER_SIZE);
        buf.put_u8(PROTOCOL_VERSION);
        buf.put_u16(self.service_type.into());
        buf.put_u16(self.total_length);
    }

    /// Decode from bytes.
    pub fn decode(data: &[u8]) -> KnxResult<Self> {
        if data.len() < HEADER_SIZE as usize {
            return Err(KnxError::frame_too_short(HEADER_SIZE as usize, data.len()));
        }

        let mut buf = data;

        let header_size = buf.get_u8();
        if header_size != HEADER_SIZE {
            return Err(KnxError::InvalidHeader(format!(
                "Invalid header size: expected {}, got {}",
                HEADER_SIZE, header_size
            )));
        }

        let version = buf.get_u8();
        if version != PROTOCOL_VERSION {
            return Err(KnxError::InvalidProtocolVersion {
                expected: PROTOCOL_VERSION,
                actual: version,
            });
        }

        let service_type = ServiceType::try_from(buf.get_u16())?;
        let total_length = buf.get_u16();

        Ok(Self {
            service_type,
            total_length,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_header_encode_decode() {
        let header = KnxNetIpHeader::new(ServiceType::SearchRequest, 8);

        let encoded = header.encode();
        assert_eq!(encoded[0], HEADER_SIZE);
        assert_eq!(encoded[1], PROTOCOL_VERSION);

        let decoded = KnxNetIpHeader::decode(&encoded).unwrap();
        assert_eq!(decoded.service_type, ServiceType::SearchRequest);
        assert_eq!(decoded.total_length, 14); // 6 + 8
    }

    #[test]
    fn test_header_body_length() {
        let header = KnxNetIpHeader::new(ServiceType::SearchRequest, 100);
        assert_eq!(header.body_length(), 100);
    }
}

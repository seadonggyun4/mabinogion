//! KNXnet/IP frame handling.
//!
//! This module provides structures and utilities for encoding/decoding
//! KNXnet/IP protocol frames.

mod dib;
mod header;
mod hpai;
mod service;

pub use dib::{DeviceInfo as DibDeviceInfo, Dib, DibType, SupportedServiceFamilies};
pub use header::{KnxNetIpHeader, HEADER_SIZE, PROTOCOL_VERSION};
pub use hpai::{HostProtocol, Hpai};
pub use service::{ServiceFamily, ServiceType};

use crate::error::{KnxError, KnxResult};
use bytes::{Buf, BufMut, BytesMut};

/// Complete KNXnet/IP frame.
#[derive(Debug, Clone)]
pub struct KnxFrame {
    /// Service type.
    pub service_type: ServiceType,
    /// Frame body (after header).
    pub body: Vec<u8>,
}

impl KnxFrame {
    /// Create a new frame.
    pub fn new(service_type: ServiceType, body: Vec<u8>) -> Self {
        Self { service_type, body }
    }

    /// Create an empty frame.
    pub fn empty(service_type: ServiceType) -> Self {
        Self {
            service_type,
            body: Vec::new(),
        }
    }

    /// Total frame length.
    pub fn total_length(&self) -> usize {
        HEADER_SIZE as usize + self.body.len()
    }

    /// Encode to bytes.
    pub fn encode(&self) -> Vec<u8> {
        let total_length = self.total_length();
        let mut buf = BytesMut::with_capacity(total_length);

        // Header
        buf.put_u8(HEADER_SIZE);
        buf.put_u8(PROTOCOL_VERSION);
        buf.put_u16(self.service_type.into());
        buf.put_u16(total_length as u16);

        // Body
        buf.put_slice(&self.body);

        buf.to_vec()
    }

    /// Decode from bytes.
    pub fn decode(data: &[u8]) -> KnxResult<Self> {
        if data.len() < HEADER_SIZE as usize {
            return Err(KnxError::frame_too_short(HEADER_SIZE as usize, data.len()));
        }

        let mut buf = data;

        // Validate header
        let header_size = buf.get_u8();
        if header_size != HEADER_SIZE {
            return Err(KnxError::InvalidHeader(format!(
                "Expected header size {}, got {}",
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
        let total_length = buf.get_u16() as usize;

        if data.len() < total_length {
            return Err(KnxError::FrameLengthMismatch {
                header_length: total_length,
                actual_length: data.len(),
            });
        }

        let body = buf[..(total_length - HEADER_SIZE as usize)].to_vec();

        Ok(Self { service_type, body })
    }

    /// Parse just the header to check frame validity.
    pub fn parse_header(data: &[u8]) -> KnxResult<KnxNetIpHeader> {
        KnxNetIpHeader::decode(data)
    }

    /// Get the minimum bytes needed for a complete frame.
    pub fn frame_length(data: &[u8]) -> Option<usize> {
        if data.len() < HEADER_SIZE as usize {
            return None;
        }
        Some(u16::from_be_bytes([data[4], data[5]]) as usize)
    }
}

/// Frame builder for constructing KNXnet/IP frames.
pub struct FrameBuilder {
    service_type: ServiceType,
    body: BytesMut,
}

impl FrameBuilder {
    /// Create a new builder.
    pub fn new(service_type: ServiceType) -> Self {
        Self {
            service_type,
            body: BytesMut::new(),
        }
    }

    /// Add raw bytes.
    pub fn put_bytes(mut self, data: &[u8]) -> Self {
        self.body.put_slice(data);
        self
    }

    /// Add a single byte.
    pub fn put_u8(mut self, value: u8) -> Self {
        self.body.put_u8(value);
        self
    }

    /// Add a u16 (big-endian).
    pub fn put_u16(mut self, value: u16) -> Self {
        self.body.put_u16(value);
        self
    }

    /// Add a u32 (big-endian).
    pub fn put_u32(mut self, value: u32) -> Self {
        self.body.put_u32(value);
        self
    }

    /// Add HPAI.
    pub fn put_hpai(self, hpai: &Hpai) -> Self {
        self.put_bytes(&hpai.encode())
    }

    /// Build the frame.
    pub fn build(self) -> KnxFrame {
        KnxFrame {
            service_type: self.service_type,
            body: self.body.to_vec(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_frame_encode_decode() {
        let frame = KnxFrame::new(ServiceType::SearchRequest, vec![0x01, 0x02, 0x03]);

        let encoded = frame.encode();
        assert_eq!(encoded.len(), HEADER_SIZE as usize + 3);

        let decoded = KnxFrame::decode(&encoded).unwrap();
        assert_eq!(decoded.service_type, ServiceType::SearchRequest);
        assert_eq!(decoded.body, vec![0x01, 0x02, 0x03]);
    }

    #[test]
    fn test_frame_builder() {
        let frame = FrameBuilder::new(ServiceType::ConnectRequest)
            .put_u8(0x01)
            .put_u16(0x0203)
            .build();

        assert_eq!(frame.service_type, ServiceType::ConnectRequest);
        assert_eq!(frame.body, vec![0x01, 0x02, 0x03]);
    }

    #[test]
    fn test_frame_length() {
        let frame = KnxFrame::new(ServiceType::SearchRequest, vec![0x00; 10]);
        let encoded = frame.encode();

        assert_eq!(KnxFrame::frame_length(&encoded), Some(16)); // 6 + 10
    }
}

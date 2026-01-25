//! Modbus TCP frame codec (MBAP).
//!
//! This module implements the Modbus Application Protocol (MBAP) header
//! parsing and frame encoding/decoding for Modbus TCP communication.
//!
//! # MBAP Header Format
//!
//! ```text
//! ┌──────────────────┬──────────────────┬──────────────┬──────────┬─────────┐
//! │ Transaction ID   │ Protocol ID      │ Length       │ Unit ID  │ PDU     │
//! │ (2 bytes)        │ (2 bytes = 0)    │ (2 bytes)    │ (1 byte) │ (N bytes│
//! └──────────────────┴──────────────────┴──────────────┴──────────┴─────────┘
//! ```
//!
//! - Transaction ID: Unique identifier for request/response matching
//! - Protocol ID: Always 0 for Modbus
//! - Length: Number of following bytes (Unit ID + PDU)
//! - Unit ID: Slave address (1-247) or broadcast (0)
//! - PDU: Protocol Data Unit (function code + data)

use bytes::{Buf, BufMut, BytesMut};
use tokio_util::codec::{Decoder, Encoder};

use crate::error::{ModbusError, ModbusResult};

/// Size of the MBAP header in bytes.
pub const MBAP_HEADER_SIZE: usize = 7;

/// Maximum allowed length in MBAP header.
pub const MAX_MBAP_LENGTH: u16 = 253;

/// Minimum PDU size (function code only).
/// Note: Unused but kept for documentation purposes.
#[allow(dead_code)]
pub const MIN_PDU_SIZE: usize = 1;

/// Maximum PDU size.
pub const MAX_PDU_SIZE: usize = 253;

/// MBAP (Modbus Application Protocol) header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MbapHeader {
    /// Transaction identifier for request/response correlation.
    pub transaction_id: u16,

    /// Protocol identifier (always 0 for Modbus).
    pub protocol_id: u16,

    /// Length of following bytes (unit_id + pdu).
    pub length: u16,

    /// Unit identifier (slave address).
    pub unit_id: u8,
}

impl MbapHeader {
    /// Create a new MBAP header.
    pub fn new(transaction_id: u16, unit_id: u8, pdu_length: usize) -> Self {
        Self {
            transaction_id,
            protocol_id: 0,
            length: (pdu_length + 1) as u16, // +1 for unit_id
            unit_id,
        }
    }

    /// Parse MBAP header from bytes.
    pub fn parse(bytes: &[u8]) -> ModbusResult<Self> {
        if bytes.len() < MBAP_HEADER_SIZE {
            return Err(ModbusError::InvalidData(format!(
                "MBAP header too short: {} bytes (expected {})",
                bytes.len(),
                MBAP_HEADER_SIZE
            )));
        }

        let transaction_id = u16::from_be_bytes([bytes[0], bytes[1]]);
        let protocol_id = u16::from_be_bytes([bytes[2], bytes[3]]);
        let length = u16::from_be_bytes([bytes[4], bytes[5]]);
        let unit_id = bytes[6];

        // Validate protocol ID
        if protocol_id != 0 {
            return Err(ModbusError::InvalidData(format!(
                "Invalid protocol ID: {} (expected 0)",
                protocol_id
            )));
        }

        // Validate length
        if length == 0 || length > MAX_MBAP_LENGTH {
            return Err(ModbusError::InvalidData(format!(
                "Invalid MBAP length: {} (expected 1-{})",
                length, MAX_MBAP_LENGTH
            )));
        }

        Ok(Self {
            transaction_id,
            protocol_id,
            length,
            unit_id,
        })
    }

    /// Encode MBAP header to bytes.
    pub fn encode(&self, buf: &mut BytesMut) {
        buf.put_u16(self.transaction_id);
        buf.put_u16(self.protocol_id);
        buf.put_u16(self.length);
        buf.put_u8(self.unit_id);
    }

    /// Get the expected PDU length.
    pub fn pdu_length(&self) -> usize {
        (self.length as usize).saturating_sub(1)
    }

    /// Get the total frame length (header + PDU).
    pub fn total_frame_length(&self) -> usize {
        MBAP_HEADER_SIZE + self.pdu_length()
    }
}

/// Complete Modbus TCP frame (MBAP header + PDU).
#[derive(Debug, Clone)]
pub struct MbapFrame {
    /// MBAP header.
    pub header: MbapHeader,

    /// Protocol Data Unit (function code + data).
    pub pdu: Vec<u8>,
}

impl MbapFrame {
    /// Create a new MBAP frame.
    pub fn new(transaction_id: u16, unit_id: u8, pdu: Vec<u8>) -> Self {
        let header = MbapHeader::new(transaction_id, unit_id, pdu.len());
        Self { header, pdu }
    }

    /// Create a response frame for a request.
    pub fn response(request: &MbapFrame, response_pdu: Vec<u8>) -> Self {
        Self::new(request.header.transaction_id, request.header.unit_id, response_pdu)
    }

    /// Get the function code.
    pub fn function_code(&self) -> Option<u8> {
        self.pdu.first().copied()
    }

    /// Check if this is an exception response.
    pub fn is_exception(&self) -> bool {
        self.pdu.first().map(|fc| fc & 0x80 != 0).unwrap_or(false)
    }

    /// Get the total frame size.
    pub fn frame_size(&self) -> usize {
        MBAP_HEADER_SIZE + self.pdu.len()
    }

    /// Encode the frame to bytes.
    pub fn encode(&self, buf: &mut BytesMut) {
        self.header.encode(buf);
        buf.put_slice(&self.pdu);
    }

    /// Encode the frame to a new buffer.
    pub fn to_bytes(&self) -> BytesMut {
        let mut buf = BytesMut::with_capacity(self.frame_size());
        self.encode(&mut buf);
        buf
    }
}

/// Codec for encoding/decoding MBAP frames.
///
/// This codec handles the framing of Modbus TCP messages according to
/// the MBAP specification. It properly handles partial reads and
/// ensures complete frames are delivered.
#[derive(Debug, Clone, Default)]
pub struct MbapCodec {
    /// Current state of frame parsing.
    state: DecodeState,
}

#[derive(Debug, Clone, Default)]
enum DecodeState {
    /// Waiting for MBAP header.
    #[default]
    WaitingForHeader,

    /// Have header, waiting for PDU.
    WaitingForPdu(MbapHeader),
}

impl MbapCodec {
    /// Create a new MBAP codec.
    pub fn new() -> Self {
        Self {
            state: DecodeState::WaitingForHeader,
        }
    }
}

impl Decoder for MbapCodec {
    type Item = MbapFrame;
    type Error = ModbusError;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        loop {
            match &self.state {
                DecodeState::WaitingForHeader => {
                    // Need at least MBAP header
                    if src.len() < MBAP_HEADER_SIZE {
                        return Ok(None);
                    }

                    // Parse header (peek, don't consume yet)
                    let header = MbapHeader::parse(&src[..MBAP_HEADER_SIZE])?;

                    // Validate PDU length
                    let pdu_len = header.pdu_length();
                    if pdu_len == 0 {
                        return Err(ModbusError::InvalidData(
                            "PDU length cannot be zero".into(),
                        ));
                    }
                    if pdu_len > MAX_PDU_SIZE {
                        return Err(ModbusError::InvalidData(format!(
                            "PDU length too large: {} (max {})",
                            pdu_len, MAX_PDU_SIZE
                        )));
                    }

                    self.state = DecodeState::WaitingForPdu(header);
                }

                DecodeState::WaitingForPdu(header) => {
                    let total_len = header.total_frame_length();

                    // Need full frame
                    if src.len() < total_len {
                        return Ok(None);
                    }

                    // Consume header
                    src.advance(MBAP_HEADER_SIZE);

                    // Extract PDU
                    let pdu_len = header.pdu_length();
                    let pdu = src.split_to(pdu_len).to_vec();

                    let frame = MbapFrame {
                        header: *header,
                        pdu,
                    };

                    // Reset state
                    self.state = DecodeState::WaitingForHeader;

                    return Ok(Some(frame));
                }
            }
        }
    }
}

impl Encoder<MbapFrame> for MbapCodec {
    type Error = ModbusError;

    fn encode(&mut self, item: MbapFrame, dst: &mut BytesMut) -> Result<(), Self::Error> {
        // Validate PDU length
        if item.pdu.is_empty() {
            return Err(ModbusError::InvalidData("PDU cannot be empty".into()));
        }
        if item.pdu.len() > MAX_PDU_SIZE {
            return Err(ModbusError::InvalidData(format!(
                "PDU too large: {} (max {})",
                item.pdu.len(),
                MAX_PDU_SIZE
            )));
        }

        // Reserve space
        dst.reserve(item.frame_size());

        // Encode frame
        item.encode(dst);

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mbap_header_parse() {
        // Valid header: TID=1, PID=0, Len=6, UID=1
        let bytes = [0x00, 0x01, 0x00, 0x00, 0x00, 0x06, 0x01];
        let header = MbapHeader::parse(&bytes).unwrap();

        assert_eq!(header.transaction_id, 1);
        assert_eq!(header.protocol_id, 0);
        assert_eq!(header.length, 6);
        assert_eq!(header.unit_id, 1);
        assert_eq!(header.pdu_length(), 5);
    }

    #[test]
    fn test_mbap_header_parse_invalid_protocol() {
        // Invalid protocol ID = 1
        let bytes = [0x00, 0x01, 0x00, 0x01, 0x00, 0x06, 0x01];
        let result = MbapHeader::parse(&bytes);

        assert!(result.is_err());
    }

    #[test]
    fn test_mbap_header_encode() {
        let header = MbapHeader::new(42, 1, 5);
        let mut buf = BytesMut::new();
        header.encode(&mut buf);

        assert_eq!(buf.len(), MBAP_HEADER_SIZE);
        assert_eq!(&buf[..], &[0x00, 0x2A, 0x00, 0x00, 0x00, 0x06, 0x01]);
    }

    #[test]
    fn test_mbap_frame_creation() {
        let pdu = vec![0x03, 0x00, 0x00, 0x00, 0x0A]; // Read 10 holding registers
        let frame = MbapFrame::new(1, 1, pdu.clone());

        assert_eq!(frame.header.transaction_id, 1);
        assert_eq!(frame.header.unit_id, 1);
        assert_eq!(frame.header.length, 6); // 1 (unit_id) + 5 (pdu)
        assert_eq!(frame.pdu, pdu);
        assert_eq!(frame.function_code(), Some(0x03));
        assert!(!frame.is_exception());
    }

    #[test]
    fn test_mbap_frame_exception() {
        let pdu = vec![0x83, 0x02]; // Exception: function 0x03, code 0x02
        let frame = MbapFrame::new(1, 1, pdu);

        assert!(frame.is_exception());
        assert_eq!(frame.function_code(), Some(0x83));
    }

    #[test]
    fn test_codec_decode() {
        let mut codec = MbapCodec::new();

        // Complete frame: Read holding registers request
        let mut buf = BytesMut::from(&[
            0x00, 0x01, // Transaction ID
            0x00, 0x00, // Protocol ID
            0x00, 0x06, // Length (1 + 5)
            0x01,       // Unit ID
            0x03,       // Function code
            0x00, 0x00, // Start address
            0x00, 0x0A, // Quantity
        ][..]);

        let frame = codec.decode(&mut buf).unwrap().unwrap();

        assert_eq!(frame.header.transaction_id, 1);
        assert_eq!(frame.header.unit_id, 1);
        assert_eq!(frame.pdu, vec![0x03, 0x00, 0x00, 0x00, 0x0A]);
        assert!(buf.is_empty());
    }

    #[test]
    fn test_codec_decode_partial() {
        let mut codec = MbapCodec::new();

        // Partial frame (header only)
        let mut buf = BytesMut::from(&[
            0x00, 0x01, 0x00, 0x00, 0x00, 0x06, 0x01,
        ][..]);

        // Should return None (need more data)
        let result = codec.decode(&mut buf).unwrap();
        assert!(result.is_none());

        // Add the rest
        buf.extend_from_slice(&[0x03, 0x00, 0x00, 0x00, 0x0A]);

        let frame = codec.decode(&mut buf).unwrap().unwrap();
        assert_eq!(frame.pdu, vec![0x03, 0x00, 0x00, 0x00, 0x0A]);
    }

    #[test]
    fn test_codec_decode_multiple() {
        let mut codec = MbapCodec::new();

        // Two complete frames
        let mut buf = BytesMut::from(&[
            // Frame 1
            0x00, 0x01, 0x00, 0x00, 0x00, 0x03, 0x01, 0x03, 0x00,
            // Frame 2
            0x00, 0x02, 0x00, 0x00, 0x00, 0x03, 0x02, 0x04, 0x00,
        ][..]);

        let frame1 = codec.decode(&mut buf).unwrap().unwrap();
        assert_eq!(frame1.header.transaction_id, 1);
        assert_eq!(frame1.header.unit_id, 1);

        let frame2 = codec.decode(&mut buf).unwrap().unwrap();
        assert_eq!(frame2.header.transaction_id, 2);
        assert_eq!(frame2.header.unit_id, 2);

        assert!(buf.is_empty());
    }

    #[test]
    fn test_codec_encode() {
        let mut codec = MbapCodec::new();
        let frame = MbapFrame::new(1, 1, vec![0x03, 0x00, 0x00, 0x00, 0x0A]);

        let mut buf = BytesMut::new();
        codec.encode(frame, &mut buf).unwrap();

        assert_eq!(buf, &[
            0x00, 0x01, // Transaction ID
            0x00, 0x00, // Protocol ID
            0x00, 0x06, // Length
            0x01,       // Unit ID
            0x03, 0x00, 0x00, 0x00, 0x0A, // PDU
        ][..]);
    }

    #[test]
    fn test_codec_round_trip() {
        let mut codec = MbapCodec::new();

        let original = MbapFrame::new(42, 5, vec![0x10, 0x00, 0x10, 0x00, 0x02, 0x04, 0x01, 0x02, 0x03, 0x04]);

        // Encode
        let mut buf = BytesMut::new();
        codec.encode(original.clone(), &mut buf).unwrap();

        // Decode
        let decoded = codec.decode(&mut buf).unwrap().unwrap();

        assert_eq!(decoded.header.transaction_id, original.header.transaction_id);
        assert_eq!(decoded.header.unit_id, original.header.unit_id);
        assert_eq!(decoded.pdu, original.pdu);
    }
}

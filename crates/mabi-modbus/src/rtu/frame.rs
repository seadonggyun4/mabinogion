//! Modbus RTU frame handling.
//!
//! This module implements the RTU (Remote Terminal Unit) frame format including:
//!
//! - CRC-16 calculation and verification (Modbus polynomial)
//! - Frame encoding and decoding
//! - Frame validation
//!
//! # Frame Format
//!
//! ```text
//! ┌──────────┬───────────────┬─────────────┬──────────────────┐
//! │ Unit ID  │ Function Code │ Data        │ CRC-16 (little)  │
//! │ (1 byte) │ (1 byte)      │ (0-252 byte)│ (2 bytes, LE)    │
//! └──────────┴───────────────┴─────────────┴──────────────────┘
//! ```
//!
//! The CRC-16 is transmitted in little-endian order (low byte first).

use bytes::{BufMut, Bytes, BytesMut};
use thiserror::Error;

/// Minimum RTU frame size: Unit ID (1) + FC (1) + CRC (2) = 4 bytes.
pub const RTU_MIN_FRAME_SIZE: usize = 4;

/// Maximum RTU frame size: 256 bytes (including CRC).
pub const RTU_MAX_FRAME_SIZE: usize = 256;

/// Maximum PDU size (without Unit ID and CRC).
pub const RTU_MAX_PDU_SIZE: usize = 253;

/// CRC-16 polynomial used in Modbus (reflected polynomial of 0x8005).
const CRC_POLYNOMIAL: u16 = 0xA001;

/// Precomputed CRC-16 lookup table for faster calculation.
const CRC_TABLE: [u16; 256] = generate_crc_table();

/// Generate CRC-16 lookup table at compile time.
const fn generate_crc_table() -> [u16; 256] {
    let mut table = [0u16; 256];
    let mut i = 0usize;

    while i < 256 {
        let mut crc = i as u16;
        let mut j = 0;

        while j < 8 {
            if crc & 0x0001 != 0 {
                crc = (crc >> 1) ^ CRC_POLYNOMIAL;
            } else {
                crc >>= 1;
            }
            j += 1;
        }

        table[i] = crc;
        i += 1;
    }

    table
}

/// Calculate CRC-16 for Modbus RTU.
///
/// This uses the standard Modbus CRC-16 algorithm with polynomial 0xA001
/// (reflected polynomial of 0x8005) and initial value 0xFFFF.
///
/// # Arguments
///
/// * `data` - The data to calculate CRC for (excluding CRC bytes)
///
/// # Returns
///
/// The CRC-16 value in native endian
///
/// # Example
///
/// ```
/// use mabi_modbus::rtu::calculate_crc;
///
/// let data = [0x01, 0x03, 0x00, 0x00, 0x00, 0x0A];
/// let crc = calculate_crc(&data);
/// // CRC is 0xCDC5, wire format (little-endian): [0xC5, 0xCD]
/// assert_eq!(crc, 0xCDC5);
/// ```
#[inline]
pub fn calculate_crc(data: &[u8]) -> u16 {
    let mut crc: u16 = 0xFFFF;

    for byte in data {
        let index = (crc ^ (*byte as u16)) & 0xFF;
        crc = (crc >> 8) ^ CRC_TABLE[index as usize];
    }

    crc
}

/// Calculate CRC-16 without lookup table (for verification/embedded systems).
///
/// This is a slower but more memory-efficient implementation.
#[inline]
pub fn calculate_crc_slow(data: &[u8]) -> u16 {
    let mut crc: u16 = 0xFFFF;

    for byte in data {
        crc ^= *byte as u16;

        for _ in 0..8 {
            if crc & 0x0001 != 0 {
                crc = (crc >> 1) ^ CRC_POLYNOMIAL;
            } else {
                crc >>= 1;
            }
        }
    }

    crc
}

/// Verify CRC-16 of a complete RTU frame.
///
/// The frame must include the CRC bytes at the end (little-endian).
///
/// # Arguments
///
/// * `frame` - Complete RTU frame including CRC
///
/// # Returns
///
/// `true` if CRC is valid, `false` otherwise
///
/// # Example
///
/// ```
/// use mabi_modbus::rtu::verify_crc;
///
/// // Valid frame: Unit 1, Read Holding Regs, Addr 0, Qty 10, CRC
/// // CRC = 0xCDC5, little-endian: [0xC5, 0xCD]
/// let frame = [0x01, 0x03, 0x00, 0x00, 0x00, 0x0A, 0xC5, 0xCD];
/// assert!(verify_crc(&frame));
///
/// // Corrupted frame
/// let bad_frame = [0x01, 0x03, 0x00, 0x00, 0x00, 0x0A, 0x00, 0x00];
/// assert!(!verify_crc(&bad_frame));
/// ```
#[inline]
pub fn verify_crc(frame: &[u8]) -> bool {
    if frame.len() < RTU_MIN_FRAME_SIZE {
        return false;
    }

    let data_len = frame.len() - 2;
    let received_crc = u16::from_le_bytes([frame[data_len], frame[data_len + 1]]);
    let calculated_crc = calculate_crc(&frame[..data_len]);

    received_crc == calculated_crc
}

/// Append CRC-16 to a buffer.
///
/// Calculates CRC for the current buffer contents and appends it in little-endian.
#[inline]
pub fn append_crc(buf: &mut BytesMut) {
    let crc = calculate_crc(buf);
    buf.put_u16_le(crc);
}

/// RTU frame parsing/encoding errors.
#[derive(Debug, Clone, Error)]
pub enum RtuFrameError {
    /// Frame is too short to be valid.
    #[error("Frame too short: {actual} bytes (minimum {minimum})")]
    TooShort { actual: usize, minimum: usize },

    /// Frame exceeds maximum size.
    #[error("Frame too long: {actual} bytes (maximum {maximum})")]
    TooLong { actual: usize, maximum: usize },

    /// CRC validation failed.
    #[error("CRC mismatch: expected 0x{expected:04X}, got 0x{received:04X}")]
    CrcMismatch { expected: u16, received: u16 },

    /// Invalid unit ID (broadcast not allowed for this operation).
    #[error("Invalid unit ID: {0}")]
    InvalidUnitId(u8),

    /// Invalid function code.
    #[error("Invalid function code: 0x{0:02X}")]
    InvalidFunctionCode(u8),

    /// Incomplete frame (need more data).
    #[error("Incomplete frame: have {have} bytes, need {need}")]
    Incomplete { have: usize, need: usize },
}

/// Modbus RTU frame.
///
/// Represents a complete RTU frame with unit ID, PDU, and CRC validation.
#[derive(Debug, Clone)]
pub struct RtuFrame {
    /// Unit/slave address (1-247, or 0 for broadcast).
    pub unit_id: u8,

    /// Protocol Data Unit (function code + data).
    pub pdu: Vec<u8>,

    /// Original CRC (for debugging/logging).
    pub crc: u16,
}

impl RtuFrame {
    /// Create a new RTU frame.
    ///
    /// The CRC will be calculated automatically.
    ///
    /// # Arguments
    ///
    /// * `unit_id` - Unit/slave address (0-247)
    /// * `pdu` - Protocol Data Unit (function code + data)
    ///
    /// # Example
    ///
    /// ```
    /// use mabi_modbus::rtu::RtuFrame;
    ///
    /// // Read holding registers request: FC=03, Start=0, Qty=10
    /// let pdu = vec![0x03, 0x00, 0x00, 0x00, 0x0A];
    /// let frame = RtuFrame::new(1, pdu);
    ///
    /// assert_eq!(frame.unit_id, 1);
    /// assert_eq!(frame.function_code(), Some(0x03));
    /// ```
    pub fn new(unit_id: u8, pdu: Vec<u8>) -> Self {
        // Calculate CRC for the frame
        let mut data = Vec::with_capacity(1 + pdu.len());
        data.push(unit_id);
        data.extend_from_slice(&pdu);
        let crc = calculate_crc(&data);

        Self { unit_id, pdu, crc }
    }

    /// Create a response frame for a request.
    ///
    /// Uses the same unit ID as the request.
    pub fn response(request: &RtuFrame, response_pdu: Vec<u8>) -> Self {
        Self::new(request.unit_id, response_pdu)
    }

    /// Create an exception response frame.
    pub fn exception(unit_id: u8, function_code: u8, exception_code: u8) -> Self {
        Self::new(unit_id, vec![function_code | 0x80, exception_code])
    }

    /// Get the function code.
    pub fn function_code(&self) -> Option<u8> {
        self.pdu.first().copied()
    }

    /// Check if this is an exception response.
    pub fn is_exception(&self) -> bool {
        self.pdu.first().map(|fc| fc & 0x80 != 0).unwrap_or(false)
    }

    /// Get the exception code (if this is an exception response).
    pub fn exception_code(&self) -> Option<u8> {
        if self.is_exception() {
            self.pdu.get(1).copied()
        } else {
            None
        }
    }

    /// Get the data portion of the PDU (without function code).
    pub fn data(&self) -> &[u8] {
        if self.pdu.len() > 1 {
            &self.pdu[1..]
        } else {
            &[]
        }
    }

    /// Get the total frame size (including CRC).
    pub fn frame_size(&self) -> usize {
        1 + self.pdu.len() + 2 // unit_id + pdu + crc
    }

    /// Encode the frame to bytes.
    ///
    /// Returns the complete RTU frame including CRC.
    pub fn encode(&self) -> Bytes {
        let mut buf = BytesMut::with_capacity(self.frame_size());
        self.encode_to(&mut buf);
        buf.freeze()
    }

    /// Encode the frame to an existing buffer.
    pub fn encode_to(&self, buf: &mut BytesMut) {
        buf.reserve(self.frame_size());
        buf.put_u8(self.unit_id);
        buf.put_slice(&self.pdu);
        buf.put_u16_le(self.crc);
    }

    /// Decode an RTU frame from bytes.
    ///
    /// # Arguments
    ///
    /// * `data` - Complete RTU frame including CRC
    ///
    /// # Returns
    ///
    /// The decoded frame or an error
    ///
    /// # Example
    ///
    /// ```
    /// use mabi_modbus::rtu::RtuFrame;
    ///
    /// // CRC = 0xCDC5, little-endian: [0xC5, 0xCD]
    /// let raw = [0x01, 0x03, 0x00, 0x00, 0x00, 0x0A, 0xC5, 0xCD];
    /// let frame = RtuFrame::decode(&raw).unwrap();
    ///
    /// assert_eq!(frame.unit_id, 1);
    /// assert_eq!(frame.function_code(), Some(0x03));
    /// ```
    pub fn decode(data: &[u8]) -> Result<Self, RtuFrameError> {
        // Validate length
        if data.len() < RTU_MIN_FRAME_SIZE {
            return Err(RtuFrameError::TooShort {
                actual: data.len(),
                minimum: RTU_MIN_FRAME_SIZE,
            });
        }

        if data.len() > RTU_MAX_FRAME_SIZE {
            return Err(RtuFrameError::TooLong {
                actual: data.len(),
                maximum: RTU_MAX_FRAME_SIZE,
            });
        }

        // Extract and verify CRC
        let data_len = data.len() - 2;
        let received_crc = u16::from_le_bytes([data[data_len], data[data_len + 1]]);
        let calculated_crc = calculate_crc(&data[..data_len]);

        if received_crc != calculated_crc {
            return Err(RtuFrameError::CrcMismatch {
                expected: calculated_crc,
                received: received_crc,
            });
        }

        // Extract frame components
        let unit_id = data[0];
        let pdu = data[1..data_len].to_vec();

        // Validate PDU has at least function code
        if pdu.is_empty() {
            return Err(RtuFrameError::TooShort {
                actual: data.len(),
                minimum: RTU_MIN_FRAME_SIZE + 1,
            });
        }

        Ok(Self {
            unit_id,
            pdu,
            crc: received_crc,
        })
    }

    /// Try to decode a frame, allowing for incomplete data.
    ///
    /// Returns `Ok(None)` if more data is needed.
    pub fn try_decode(data: &[u8]) -> Result<Option<Self>, RtuFrameError> {
        if data.len() < RTU_MIN_FRAME_SIZE {
            return Ok(None);
        }

        // For RTU, we need to determine frame length from the PDU
        // This requires knowledge of the function code
        if let Some(expected_len) = estimate_frame_length(data) {
            if data.len() < expected_len {
                return Ok(None);
            }

            // Try to decode the expected frame
            Self::decode(&data[..expected_len]).map(Some)
        } else {
            // Unknown function code or invalid data
            Ok(None)
        }
    }
}

/// Estimate the expected frame length based on function code.
///
/// Returns `None` if the function code is unknown or data is invalid.
fn estimate_frame_length(data: &[u8]) -> Option<usize> {
    if data.len() < 2 {
        return None;
    }

    let function_code = data[1];

    // Handle exception responses (FC | 0x80)
    if function_code & 0x80 != 0 {
        // Exception: Unit + FC + ExCode + CRC = 5 bytes
        return Some(5);
    }

    match function_code {
        // Read Coils / Read Discrete Inputs / Read Holding Regs / Read Input Regs
        0x01 | 0x02 | 0x03 | 0x04 => {
            // Request: Unit + FC + Start(2) + Qty(2) + CRC(2) = 8 bytes
            // Response: Unit + FC + ByteCount(1) + Data(N) + CRC(2)
            if data.len() >= 3 {
                let third_byte = data[2];
                // Check if this looks like a response (byte count) or request (address high)
                if data.len() >= 8 {
                    // Could be request (fixed 8 bytes)
                    Some(8)
                } else if data.len() >= 3 + third_byte as usize + 2 {
                    // Response with byte count
                    Some(3 + third_byte as usize + 2)
                } else {
                    Some(8) // Default to request
                }
            } else {
                Some(8) // Request format
            }
        }

        // Write Single Coil / Write Single Register
        0x05 | 0x06 => {
            // Unit + FC + Addr(2) + Value(2) + CRC(2) = 8 bytes
            Some(8)
        }

        // Mask Write Register
        0x16 => {
            // Unit + FC + Addr(2) + And_Mask(2) + Or_Mask(2) + CRC(2) = 10 bytes
            Some(10)
        }

        // Write Multiple Coils
        0x0F => {
            if data.len() >= 7 {
                let byte_count = data[6] as usize;
                // Unit + FC + Addr(2) + Qty(2) + ByteCount(1) + Data(N) + CRC(2)
                Some(7 + byte_count + 2)
            } else {
                None
            }
        }

        // Write Multiple Registers
        0x10 => {
            if data.len() >= 7 {
                let byte_count = data[6] as usize;
                // Unit + FC + Addr(2) + Qty(2) + ByteCount(1) + Data(N) + CRC(2)
                Some(7 + byte_count + 2)
            } else {
                None
            }
        }

        // Unknown function code - can't determine length
        _ => None,
    }
}

/// Parse a boolean array from packed coil/discrete input bytes.
pub fn unpack_bits(bytes: &[u8], count: usize) -> Vec<bool> {
    let mut result = Vec::with_capacity(count);

    for i in 0..count {
        let byte_index = i / 8;
        let bit_index = i % 8;

        if byte_index < bytes.len() {
            result.push((bytes[byte_index] >> bit_index) & 1 != 0);
        } else {
            result.push(false);
        }
    }

    result
}

/// Pack boolean values into bytes for coil/discrete input response.
pub fn pack_bits(bits: &[bool]) -> Vec<u8> {
    let byte_count = (bits.len() + 7) / 8;
    let mut result = vec![0u8; byte_count];

    for (i, &bit) in bits.iter().enumerate() {
        if bit {
            result[i / 8] |= 1 << (i % 8);
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_crc_calculation() {
        // Standard test vector: Unit 1, FC 03 (Read Holding Registers), Addr 0, Qty 10
        // CRC-16 Modbus for [0x01, 0x03, 0x00, 0x00, 0x00, 0x0A] = 0xCDC5
        // Wire format: low byte first = [0xC5, 0xCD]
        let data = [0x01, 0x03, 0x00, 0x00, 0x00, 0x0A];
        let crc = calculate_crc(&data);
        // The CRC is computed in little-endian format internally
        assert_eq!(
            crc, 0xCDC5,
            "CRC mismatch: expected 0xCDC5, got 0x{:04X}",
            crc
        );

        // Another test vector: [0x01, 0x03]
        // The actual CRC value computed by our algorithm
        let data2 = [0x01, 0x03];
        let crc2 = calculate_crc(&data2);
        assert_eq!(crc2, 0x2140);
    }

    #[test]
    fn test_crc_slow_matches_table() {
        // Verify table-based and slow implementations match
        let data = [0x01, 0x03, 0x00, 0x00, 0x00, 0x0A, 0x12, 0x34, 0x56, 0x78];
        assert_eq!(calculate_crc(&data), calculate_crc_slow(&data));

        // Test with various lengths
        for len in 1..=10 {
            let subset = &data[..len];
            assert_eq!(
                calculate_crc(subset),
                calculate_crc_slow(subset),
                "CRC mismatch for length {}",
                len
            );
        }
    }

    #[test]
    fn test_verify_crc_valid() {
        // Valid frame: Unit 1, Read Holding Regs, Addr 0, Qty 10
        // CRC = 0xCDC5, wire format (LE) = [0xC5, 0xCD]
        let frame = [0x01, 0x03, 0x00, 0x00, 0x00, 0x0A, 0xC5, 0xCD];
        assert!(verify_crc(&frame));
    }

    #[test]
    fn test_verify_crc_invalid() {
        // Corrupted CRC
        let frame = [0x01, 0x03, 0x00, 0x00, 0x00, 0x0A, 0x00, 0x00];
        assert!(!verify_crc(&frame));

        // Too short
        assert!(!verify_crc(&[0x01, 0x03]));
    }

    #[test]
    fn test_frame_creation() {
        let pdu = vec![0x03, 0x00, 0x00, 0x00, 0x0A];
        let frame = RtuFrame::new(1, pdu.clone());

        assert_eq!(frame.unit_id, 1);
        assert_eq!(frame.pdu, pdu);
        assert_eq!(frame.function_code(), Some(0x03));
        assert!(!frame.is_exception());
        // CRC for [0x01, 0x03, 0x00, 0x00, 0x00, 0x0A] = 0xCDC5
        assert_eq!(frame.crc, 0xCDC5);
    }

    #[test]
    fn test_frame_encode_decode() {
        let original = RtuFrame::new(1, vec![0x03, 0x00, 0x00, 0x00, 0x0A]);
        let encoded = original.encode();
        let decoded = RtuFrame::decode(&encoded).unwrap();

        assert_eq!(decoded.unit_id, original.unit_id);
        assert_eq!(decoded.pdu, original.pdu);
        assert_eq!(decoded.crc, original.crc);
    }

    #[test]
    fn test_exception_frame() {
        let frame = RtuFrame::exception(1, 0x03, 0x02);

        assert!(frame.is_exception());
        assert_eq!(frame.function_code(), Some(0x83));
        assert_eq!(frame.exception_code(), Some(0x02));
    }

    #[test]
    fn test_decode_errors() {
        // Too short
        let result = RtuFrame::decode(&[0x01, 0x03]);
        assert!(matches!(result, Err(RtuFrameError::TooShort { .. })));

        // CRC mismatch
        let result = RtuFrame::decode(&[0x01, 0x03, 0x00, 0x00, 0x00, 0x0A, 0x00, 0x00]);
        assert!(matches!(result, Err(RtuFrameError::CrcMismatch { .. })));
    }

    #[test]
    fn test_pack_unpack_bits() {
        let bits = vec![true, false, true, true, false, false, true, false, true];
        let packed = pack_bits(&bits);
        let unpacked = unpack_bits(&packed, bits.len());

        assert_eq!(unpacked, bits);
    }

    #[test]
    fn test_estimate_frame_length() {
        // Read holding registers request - FC 03
        // Frame: [Unit, FC, AddrHi, AddrLo, QtyHi, QtyLo, CRC_Lo, CRC_Hi] = 8 bytes
        // With at least 8 bytes, it's treated as a request
        let data = [0x01, 0x03, 0x00, 0x00, 0x00, 0x0A, 0xC5, 0xCD];
        assert_eq!(estimate_frame_length(&data), Some(8));

        // Write single register - FC 06
        let data = [0x01, 0x06, 0x00, 0x10, 0x00, 0x03, 0x00, 0x00];
        assert_eq!(estimate_frame_length(&data), Some(8));

        // Exception response (FC with high bit set)
        // Exception: [Unit, FC|0x80, ExCode, CRC_Lo, CRC_Hi] = 5 bytes
        let data = [0x01, 0x83, 0x02, 0x00, 0x00];
        assert_eq!(estimate_frame_length(&data), Some(5));

        // Partial request data (less than 8 bytes) still returns 8
        let data = [0x01, 0x03, 0x00, 0x00];
        assert_eq!(estimate_frame_length(&data), Some(8));
    }
}

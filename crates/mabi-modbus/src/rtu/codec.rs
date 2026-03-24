//! Modbus RTU codec for async I/O.
//!
//! This module provides a tokio-compatible codec for encoding and decoding
//! Modbus RTU frames with proper timing-based frame detection.
//!
//! # Frame Detection
//!
//! Unlike TCP which has length headers, RTU relies on timing gaps to detect
//! frame boundaries:
//!
//! - **Inter-frame gap**: 3.5 character times of silence marks end of frame
//! - **Inter-character gap**: Max 1.5 character times between bytes in a frame
//!
//! This codec implements both timing-based detection (when timing info is available)
//! and function-code based detection as a fallback.

use std::time::{Duration, Instant};

use bytes::BytesMut;
use tokio_util::codec::{Decoder, Encoder};

use crate::error::ModbusError;

use super::frame::{verify_crc, RtuFrame, RtuFrameError, RTU_MAX_FRAME_SIZE, RTU_MIN_FRAME_SIZE};

/// RTU timing configuration.
///
/// Defines the timing parameters for frame detection based on baud rate.
#[derive(Debug, Clone, Copy)]
pub struct RtuTiming {
    /// Time for one character (start + data + parity + stop bits).
    pub char_time: Duration,

    /// Inter-character timeout (1.5 character times).
    pub inter_char_timeout: Duration,

    /// Inter-frame timeout (3.5 character times).
    pub inter_frame_timeout: Duration,
}

impl RtuTiming {
    /// Create timing configuration for a specific baud rate.
    ///
    /// Assumes 11 bits per character (1 start + 8 data + 1 parity + 1 stop).
    ///
    /// # Arguments
    ///
    /// * `baud_rate` - Serial baud rate (e.g., 9600, 19200, 115200)
    ///
    /// # Example
    ///
    /// ```
    /// use mabi_modbus::rtu::RtuTiming;
    ///
    /// let timing = RtuTiming::from_baud_rate(9600);
    /// // At 9600 baud: char_time ≈ 1.145ms, inter_frame ≈ 4ms
    /// ```
    pub fn from_baud_rate(baud_rate: u32) -> Self {
        Self::from_baud_rate_with_bits(baud_rate, 11)
    }

    /// Create timing configuration with custom bits per character.
    ///
    /// # Arguments
    ///
    /// * `baud_rate` - Serial baud rate
    /// * `bits_per_char` - Total bits per character (typically 10-11)
    pub fn from_baud_rate_with_bits(baud_rate: u32, bits_per_char: u32) -> Self {
        let char_time_us = (bits_per_char as u64 * 1_000_000) / baud_rate as u64;
        let char_time = Duration::from_micros(char_time_us);

        // Modbus spec: 1.5 char times inter-character, 3.5 char times inter-frame
        // For high baud rates (> 19200), use fixed minimums
        let (inter_char, inter_frame) = if baud_rate > 19200 {
            (Duration::from_micros(750), Duration::from_micros(1750))
        } else {
            (char_time.mul_f32(1.5), char_time.mul_f32(3.5))
        };

        Self {
            char_time,
            inter_char_timeout: inter_char,
            inter_frame_timeout: inter_frame,
        }
    }

    /// Get transmission time for a given number of bytes.
    pub fn transmission_time(&self, bytes: usize) -> Duration {
        self.char_time * bytes as u32
    }
}

impl Default for RtuTiming {
    fn default() -> Self {
        Self::from_baud_rate(9600)
    }
}

/// Frame detection state.
#[derive(Debug, Clone)]
enum DecodeState {
    /// Waiting for first byte of a new frame.
    Idle,

    /// Receiving frame data.
    Receiving {
        /// When the last byte was received.
        last_byte_time: Instant,
        #[allow(dead_code)]
        /// Expected frame length (if known from function code).
        expected_length: Option<usize>,
    },

    #[allow(dead_code)]
    /// Frame complete, ready to emit.
    Complete,
}

impl Default for DecodeState {
    fn default() -> Self {
        Self::Idle
    }
}

/// Modbus RTU codec.
///
/// This codec handles the encoding and decoding of RTU frames with
/// proper timing-based frame detection.
///
/// # Example
///
/// ```rust,no_run
/// use mabi_modbus::rtu::{RtuCodec, RtuTiming};
/// use tokio_util::codec::Framed;
///
/// // Create codec with default timing (9600 baud)
/// let codec = RtuCodec::new();
///
/// // Create codec with custom timing
/// let codec = RtuCodec::with_timing(RtuTiming::from_baud_rate(115200));
/// ```
#[derive(Debug)]
pub struct RtuCodec {
    /// Timing configuration.
    timing: RtuTiming,

    /// Current decode state.
    state: DecodeState,

    /// Buffer for accumulating frame data.
    buffer: BytesMut,

    /// Enable strict timing-based frame detection.
    /// When false, uses function-code based detection only.
    strict_timing: bool,

    /// Unit ID filter (None = accept all).
    unit_id_filter: Option<Vec<u8>>,
}

impl RtuCodec {
    /// Create a new RTU codec with default timing (9600 baud).
    pub fn new() -> Self {
        Self::with_timing(RtuTiming::default())
    }

    /// Create a codec with specific timing configuration.
    pub fn with_timing(timing: RtuTiming) -> Self {
        Self {
            timing,
            state: DecodeState::Idle,
            buffer: BytesMut::with_capacity(RTU_MAX_FRAME_SIZE),
            strict_timing: false,
            unit_id_filter: None,
        }
    }

    /// Create a codec for a specific baud rate.
    pub fn with_baud_rate(baud_rate: u32) -> Self {
        Self::with_timing(RtuTiming::from_baud_rate(baud_rate))
    }

    /// Enable strict timing-based frame detection.
    pub fn strict_timing(mut self, enabled: bool) -> Self {
        self.strict_timing = enabled;
        self
    }

    /// Set unit ID filter.
    ///
    /// Only frames with matching unit IDs will be accepted.
    pub fn unit_id_filter(mut self, unit_ids: Vec<u8>) -> Self {
        self.unit_id_filter = Some(unit_ids);
        self
    }

    /// Get the timing configuration.
    pub fn timing(&self) -> &RtuTiming {
        &self.timing
    }

    /// Reset the codec state.
    pub fn reset(&mut self) {
        self.state = DecodeState::Idle;
        self.buffer.clear();
    }

    /// Try to parse a complete frame from the buffer.
    fn try_parse_frame(&mut self) -> Result<Option<RtuFrame>, ModbusError> {
        if self.buffer.len() < RTU_MIN_FRAME_SIZE {
            return Ok(None);
        }

        // Try to determine expected length from function code
        let expected_len = self.estimate_frame_length();

        match expected_len {
            Some(len) if self.buffer.len() >= len => {
                // We have enough data, try to decode
                let frame_data = self.buffer.split_to(len);

                match RtuFrame::decode(&frame_data) {
                    Ok(frame) => {
                        // Check unit ID filter
                        if let Some(ref filter) = self.unit_id_filter {
                            if !filter.contains(&frame.unit_id) && frame.unit_id != 0 {
                                // Ignore frame, reset and continue
                                self.state = DecodeState::Idle;
                                return Ok(None);
                            }
                        }

                        self.state = DecodeState::Idle;
                        Ok(Some(frame))
                    }
                    Err(RtuFrameError::CrcMismatch { expected, received }) => {
                        // CRC error - frame is corrupt
                        self.state = DecodeState::Idle;
                        Err(ModbusError::InvalidData(format!(
                            "CRC mismatch: expected 0x{:04X}, got 0x{:04X}",
                            expected, received
                        )))
                    }
                    Err(e) => {
                        self.state = DecodeState::Idle;
                        Err(ModbusError::InvalidData(e.to_string()))
                    }
                }
            }
            Some(_) => {
                // Need more data
                Ok(None)
            }
            None if self.buffer.len() >= RTU_MAX_FRAME_SIZE => {
                // Buffer full but can't determine frame - discard
                self.buffer.clear();
                self.state = DecodeState::Idle;
                Err(ModbusError::InvalidData(
                    "Unable to determine frame length, buffer overflow".into(),
                ))
            }
            None => {
                // Unknown function code, wait for more data
                Ok(None)
            }
        }
    }

    /// Estimate frame length based on function code.
    fn estimate_frame_length(&self) -> Option<usize> {
        if self.buffer.len() < 2 {
            return None;
        }

        let function_code = self.buffer[1];

        // Handle exception responses
        if function_code & 0x80 != 0 {
            // Exception: Unit + FC + ExCode + CRC = 5 bytes
            return Some(5);
        }

        match function_code {
            // Read requests and single writes (fixed 8 bytes)
            0x01 | 0x02 | 0x03 | 0x04 | 0x05 | 0x06 => Some(8),

            // Mask Write Register (fixed 10 bytes)
            // Unit + FC + Addr(2) + And_Mask(2) + Or_Mask(2) + CRC(2)
            0x16 => Some(10),

            // Write multiple coils / registers
            0x0F | 0x10 => {
                if self.buffer.len() >= 7 {
                    let byte_count = self.buffer[6] as usize;
                    Some(7 + byte_count + 2)
                } else {
                    None
                }
            }

            // Read exception status
            0x07 => Some(4),

            // Diagnostics
            0x08 => Some(8),

            // Get comm event counter / log
            0x0B | 0x0C => Some(4),

            // Report server ID (variable)
            0x11 => {
                if self.buffer.len() >= 3 {
                    let byte_count = self.buffer[2] as usize;
                    Some(3 + byte_count + 2)
                } else {
                    None
                }
            }

            // Read/Write multiple registers
            0x17 => {
                if self.buffer.len() >= 11 {
                    let write_byte_count = self.buffer[10] as usize;
                    Some(11 + write_byte_count + 2)
                } else {
                    None
                }
            }

            // Unknown - use timing or max size
            _ => None,
        }
    }

    /// Check if inter-frame timeout has elapsed.
    fn check_frame_timeout(&mut self) -> bool {
        if !self.strict_timing {
            return false;
        }

        if let DecodeState::Receiving { last_byte_time, .. } = &self.state {
            last_byte_time.elapsed() >= self.timing.inter_frame_timeout
        } else {
            false
        }
    }
}

impl Default for RtuCodec {
    fn default() -> Self {
        Self::new()
    }
}

impl Decoder for RtuCodec {
    type Item = RtuFrame;
    type Error = ModbusError;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        // Check for inter-frame timeout (frame complete by timing)
        if self.check_frame_timeout() && !self.buffer.is_empty() {
            // Try to parse what we have
            if self.buffer.len() >= RTU_MIN_FRAME_SIZE && verify_crc(&self.buffer) {
                return self.try_parse_frame();
            } else {
                // Invalid frame, discard
                self.buffer.clear();
                self.state = DecodeState::Idle;
            }
        }

        // Process incoming data
        if src.is_empty() {
            return Ok(None);
        }

        // Move data to internal buffer
        self.buffer.extend_from_slice(src);
        src.clear();

        // Update state
        self.state = DecodeState::Receiving {
            last_byte_time: Instant::now(),
            expected_length: self.estimate_frame_length(),
        };

        // Try to parse a complete frame
        self.try_parse_frame()
    }
}

impl Encoder<RtuFrame> for RtuCodec {
    type Error = ModbusError;

    fn encode(&mut self, item: RtuFrame, dst: &mut BytesMut) -> Result<(), Self::Error> {
        // Validate PDU size
        if item.pdu.is_empty() {
            return Err(ModbusError::InvalidData("PDU cannot be empty".into()));
        }

        if item.pdu.len() > super::frame::RTU_MAX_PDU_SIZE {
            return Err(ModbusError::InvalidData(format!(
                "PDU too large: {} bytes (max {})",
                item.pdu.len(),
                super::frame::RTU_MAX_PDU_SIZE
            )));
        }

        // Reserve space and encode
        dst.reserve(item.frame_size());
        item.encode_to(dst);

        Ok(())
    }
}

/// Streaming RTU codec for continuous byte processing.
///
/// This codec is designed for scenarios where bytes arrive continuously
/// (e.g., from a physical serial port) and timing information is critical.
#[derive(Debug)]
pub struct StreamingRtuCodec {
    /// Base codec for frame handling.
    inner: RtuCodec,

    /// Current partial frame buffer.
    partial_frame: BytesMut,

    /// Time of last byte received.
    last_byte_time: Option<Instant>,
}

impl StreamingRtuCodec {
    /// Create a new streaming codec.
    pub fn new(timing: RtuTiming) -> Self {
        Self {
            inner: RtuCodec::with_timing(timing).strict_timing(true),
            partial_frame: BytesMut::with_capacity(RTU_MAX_FRAME_SIZE),
            last_byte_time: None,
        }
    }

    /// Process a single byte.
    ///
    /// Returns a frame if one is complete.
    pub fn process_byte(&mut self, byte: u8) -> Result<Option<RtuFrame>, ModbusError> {
        let now = Instant::now();

        // Check for inter-frame gap
        if let Some(last_time) = self.last_byte_time {
            if now.duration_since(last_time) >= self.inner.timing.inter_frame_timeout {
                // Gap detected - previous frame is complete
                if !self.partial_frame.is_empty() {
                    if self.partial_frame.len() >= RTU_MIN_FRAME_SIZE
                        && verify_crc(&self.partial_frame)
                    {
                        let frame_data = std::mem::replace(
                            &mut self.partial_frame,
                            BytesMut::with_capacity(RTU_MAX_FRAME_SIZE),
                        );
                        self.last_byte_time = Some(now);
                        self.partial_frame.extend_from_slice(&[byte]);

                        return RtuFrame::decode(&frame_data)
                            .map(Some)
                            .map_err(|e| ModbusError::InvalidData(e.to_string()));
                    } else {
                        // Invalid frame, discard
                        self.partial_frame.clear();
                    }
                }
            }
        }

        self.last_byte_time = Some(now);
        self.partial_frame.extend_from_slice(&[byte]);

        // Check if we have a complete frame
        if self.partial_frame.len() >= RTU_MIN_FRAME_SIZE {
            if let Some(expected_len) = self.inner.estimate_frame_length() {
                if self.partial_frame.len() >= expected_len
                    && verify_crc(&self.partial_frame[..expected_len])
                {
                    let frame_data = self.partial_frame.split_to(expected_len);
                    return RtuFrame::decode(&frame_data)
                        .map(Some)
                        .map_err(|e| ModbusError::InvalidData(e.to_string()));
                }
            }
        }

        // Check for buffer overflow
        if self.partial_frame.len() >= RTU_MAX_FRAME_SIZE {
            self.partial_frame.clear();
            return Err(ModbusError::InvalidData("Frame buffer overflow".into()));
        }

        Ok(None)
    }

    /// Check if there's a pending frame due to timeout.
    ///
    /// Call this periodically when no bytes are being received.
    pub fn check_timeout(&mut self) -> Result<Option<RtuFrame>, ModbusError> {
        if let Some(last_time) = self.last_byte_time {
            if Instant::now().duration_since(last_time) >= self.inner.timing.inter_frame_timeout {
                if self.partial_frame.len() >= RTU_MIN_FRAME_SIZE && verify_crc(&self.partial_frame)
                {
                    let frame_data = std::mem::replace(
                        &mut self.partial_frame,
                        BytesMut::with_capacity(RTU_MAX_FRAME_SIZE),
                    );
                    self.last_byte_time = None;

                    return RtuFrame::decode(&frame_data)
                        .map(Some)
                        .map_err(|e| ModbusError::InvalidData(e.to_string()));
                } else if !self.partial_frame.is_empty() {
                    // Invalid frame, discard
                    self.partial_frame.clear();
                    self.last_byte_time = None;
                }
            }
        }

        Ok(None)
    }

    /// Reset the codec state.
    pub fn reset(&mut self) {
        self.inner.reset();
        self.partial_frame.clear();
        self.last_byte_time = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_timing_calculation() {
        let timing = RtuTiming::from_baud_rate(9600);

        // At 9600 baud with 11 bits per char: char_time ≈ 1.145ms
        let char_time_us = timing.char_time.as_micros();
        assert!(char_time_us > 1100 && char_time_us < 1200);

        // Inter-frame should be ~4ms
        let inter_frame_us = timing.inter_frame_timeout.as_micros();
        assert!(inter_frame_us > 3500 && inter_frame_us < 4500);
    }

    #[test]
    fn test_high_baud_rate_minimums() {
        // High baud rates should use fixed minimums
        let timing = RtuTiming::from_baud_rate(115200);

        assert_eq!(timing.inter_char_timeout, Duration::from_micros(750));
        assert_eq!(timing.inter_frame_timeout, Duration::from_micros(1750));
    }

    #[test]
    fn test_codec_encode_decode() {
        let mut codec = RtuCodec::new();

        // Create a frame
        let frame = RtuFrame::new(1, vec![0x03, 0x00, 0x00, 0x00, 0x0A]);

        // Encode
        let mut buf = BytesMut::new();
        codec.encode(frame.clone(), &mut buf).unwrap();

        // Verify encoded size
        assert_eq!(buf.len(), 8); // 1 + 5 + 2

        // Decode
        let mut codec = RtuCodec::new();
        let decoded = codec.decode(&mut buf).unwrap().unwrap();

        assert_eq!(decoded.unit_id, frame.unit_id);
        assert_eq!(decoded.pdu, frame.pdu);
    }

    #[test]
    fn test_codec_partial_frame() {
        let mut codec = RtuCodec::new();

        // Create a complete frame first
        let frame = RtuFrame::new(1, vec![0x03, 0x00, 0x00, 0x00, 0x0A]);
        let full = frame.encode();

        // Send first 3 bytes - should return None (need more data)
        let mut buf = BytesMut::from(&full[..3]);
        let result = codec.decode(&mut buf).unwrap();
        assert!(result.is_none());

        // Send remaining bytes - should now complete the frame
        let mut remaining = BytesMut::from(&full[3..]);
        let result = codec.decode(&mut remaining).unwrap();
        assert!(result.is_some());
    }

    #[test]
    fn test_codec_exception_frame() {
        let mut codec = RtuCodec::new();

        // Exception response
        let frame = RtuFrame::exception(1, 0x03, 0x02);

        let mut buf = BytesMut::new();
        codec.encode(frame.clone(), &mut buf).unwrap();

        // Should be 5 bytes
        assert_eq!(buf.len(), 5);

        let decoded = codec.decode(&mut buf).unwrap().unwrap();
        assert!(decoded.is_exception());
    }

    #[test]
    fn test_codec_unit_id_filter() {
        let mut codec = RtuCodec::new().unit_id_filter(vec![1, 2]);

        // Frame for unit 1 should pass
        let frame1 = RtuFrame::new(1, vec![0x03, 0x00, 0x00, 0x00, 0x0A]);
        let mut buf = frame1.encode().into();

        let result = codec.decode(&mut buf).unwrap();
        assert!(result.is_some());

        // Frame for unit 5 should be filtered
        codec.reset();
        let frame5 = RtuFrame::new(5, vec![0x03, 0x00, 0x00, 0x00, 0x0A]);
        let mut buf = frame5.encode().into();

        let result = codec.decode(&mut buf).unwrap();
        assert!(result.is_none()); // Filtered out
    }

    #[test]
    fn test_transmission_time() {
        let timing = RtuTiming::from_baud_rate(9600);

        // 8 bytes at 9600 baud ≈ 9.16ms
        let time = timing.transmission_time(8);
        let time_ms = time.as_millis();
        assert!(time_ms >= 8 && time_ms <= 10);
    }
}

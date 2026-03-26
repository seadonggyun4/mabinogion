//! APDU tag encoding and decoding.
//!
//! BACnet uses a tag-length-value (TLV) encoding scheme for data.

use bytes::{BufMut, BytesMut};

use super::types::ApduError;
use crate::object::property::BACnetValue;
use crate::object::types::ObjectId;

/// BACnet application tag numbers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum TagNumber {
    Null = 0,
    Boolean = 1,
    UnsignedInt = 2,
    SignedInt = 3,
    Real = 4,
    Double = 5,
    OctetString = 6,
    CharacterString = 7,
    BitString = 8,
    Enumerated = 9,
    Date = 10,
    Time = 11,
    ObjectIdentifier = 12,
    // Reserved: 13-15
}

impl TagNumber {
    pub fn from_u8(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::Null),
            1 => Some(Self::Boolean),
            2 => Some(Self::UnsignedInt),
            3 => Some(Self::SignedInt),
            4 => Some(Self::Real),
            5 => Some(Self::Double),
            6 => Some(Self::OctetString),
            7 => Some(Self::CharacterString),
            8 => Some(Self::BitString),
            9 => Some(Self::Enumerated),
            10 => Some(Self::Date),
            11 => Some(Self::Time),
            12 => Some(Self::ObjectIdentifier),
            _ => None,
        }
    }
}

/// APDU encoder for building BACnet messages.
pub struct ApduEncoder {
    buf: BytesMut,
}

impl ApduEncoder {
    /// Create a new encoder.
    pub fn new() -> Self {
        Self {
            buf: BytesMut::with_capacity(256),
        }
    }

    /// Create with specific capacity.
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            buf: BytesMut::with_capacity(capacity),
        }
    }

    /// Get the encoded bytes.
    pub fn into_bytes(self) -> Vec<u8> {
        self.buf.to_vec()
    }

    /// Get a reference to the buffer.
    pub fn as_bytes(&self) -> &[u8] {
        &self.buf
    }

    /// Encode a raw byte.
    pub fn put_u8(&mut self, value: u8) {
        self.buf.put_u8(value);
    }

    /// Encode raw bytes.
    pub fn put_bytes(&mut self, bytes: &[u8]) {
        self.buf.put_slice(bytes);
    }

    /// Encode an application tag header.
    fn encode_application_tag(&mut self, tag: TagNumber, len: usize) {
        let tag_byte = (tag as u8) << 4;

        if len < 5 {
            self.buf.put_u8(tag_byte | len as u8);
        } else if len < 254 {
            self.buf.put_u8(tag_byte | 5);
            self.buf.put_u8(len as u8);
        } else if len < 65536 {
            self.buf.put_u8(tag_byte | 5);
            self.buf.put_u8(254);
            self.buf.put_u16(len as u16);
        } else {
            self.buf.put_u8(tag_byte | 5);
            self.buf.put_u8(255);
            self.buf.put_u32(len as u32);
        }
    }

    /// Encode a context tag header (opening or closing).
    pub fn encode_context_tag(&mut self, tag_number: u8, len: usize) {
        if tag_number < 15 {
            if len < 5 {
                self.buf.put_u8((tag_number << 4) | 0x08 | len as u8);
            } else if len < 254 {
                self.buf.put_u8((tag_number << 4) | 0x0D);
                self.buf.put_u8(len as u8);
            } else if len < 65536 {
                self.buf.put_u8((tag_number << 4) | 0x0D);
                self.buf.put_u8(254);
                self.buf.put_u16(len as u16);
            } else {
                self.buf.put_u8((tag_number << 4) | 0x0D);
                self.buf.put_u8(255);
                self.buf.put_u32(len as u32);
            }
        } else {
            // Extended tag number
            if len < 5 {
                self.buf.put_u8(0xF8 | len as u8);
            } else {
                self.buf.put_u8(0xFD);
            }
            self.buf.put_u8(tag_number);
            if len >= 5 {
                if len < 254 {
                    self.buf.put_u8(len as u8);
                } else if len < 65536 {
                    self.buf.put_u8(254);
                    self.buf.put_u16(len as u16);
                } else {
                    self.buf.put_u8(255);
                    self.buf.put_u32(len as u32);
                }
            }
        }
    }

    /// Encode an opening tag.
    pub fn encode_opening_tag(&mut self, tag_number: u8) {
        if tag_number < 15 {
            self.buf.put_u8((tag_number << 4) | 0x0E);
        } else {
            self.buf.put_u8(0xFE);
            self.buf.put_u8(tag_number);
        }
    }

    /// Encode a closing tag.
    pub fn encode_closing_tag(&mut self, tag_number: u8) {
        if tag_number < 15 {
            self.buf.put_u8((tag_number << 4) | 0x0F);
        } else {
            self.buf.put_u8(0xFF);
            self.buf.put_u8(tag_number);
        }
    }

    /// Encode null.
    pub fn encode_null(&mut self) {
        self.encode_application_tag(TagNumber::Null, 0);
    }

    /// Encode boolean.
    pub fn encode_boolean(&mut self, value: bool) {
        self.buf
            .put_u8((TagNumber::Boolean as u8) << 4 | if value { 1 } else { 0 });
    }

    /// Encode unsigned integer (application tagged).
    pub fn encode_unsigned(&mut self, value: u32) {
        let bytes = if value < 0x100 {
            vec![value as u8]
        } else if value < 0x10000 {
            vec![(value >> 8) as u8, value as u8]
        } else if value < 0x1000000 {
            vec![(value >> 16) as u8, (value >> 8) as u8, value as u8]
        } else {
            vec![
                (value >> 24) as u8,
                (value >> 16) as u8,
                (value >> 8) as u8,
                value as u8,
            ]
        };

        self.encode_application_tag(TagNumber::UnsignedInt, bytes.len());
        self.buf.put_slice(&bytes);
    }

    /// Encode unsigned integer (context tagged).
    pub fn encode_context_unsigned(&mut self, tag_number: u8, value: u32) {
        let bytes = if value < 0x100 {
            vec![value as u8]
        } else if value < 0x10000 {
            vec![(value >> 8) as u8, value as u8]
        } else if value < 0x1000000 {
            vec![(value >> 16) as u8, (value >> 8) as u8, value as u8]
        } else {
            vec![
                (value >> 24) as u8,
                (value >> 16) as u8,
                (value >> 8) as u8,
                value as u8,
            ]
        };

        self.encode_context_tag(tag_number, bytes.len());
        self.buf.put_slice(&bytes);
    }

    /// Encode signed integer.
    pub fn encode_signed(&mut self, value: i32) {
        let bytes = value.to_be_bytes();
        let len = if value >= -128 && value < 128 {
            1
        } else if value >= -32768 && value < 32768 {
            2
        } else if value >= -8388608 && value < 8388608 {
            3
        } else {
            4
        };

        self.encode_application_tag(TagNumber::SignedInt, len);
        self.buf.put_slice(&bytes[4 - len..]);
    }

    /// Encode real (float32).
    pub fn encode_real(&mut self, value: f32) {
        self.encode_application_tag(TagNumber::Real, 4);
        self.buf.put_f32(value);
    }

    /// Encode real (context tagged).
    pub fn encode_context_real(&mut self, tag_number: u8, value: f32) {
        self.encode_context_tag(tag_number, 4);
        self.buf.put_f32(value);
    }

    /// Encode double (float64).
    pub fn encode_double(&mut self, value: f64) {
        self.encode_application_tag(TagNumber::Double, 8);
        self.buf.put_f64(value);
    }

    /// Encode octet string.
    pub fn encode_octet_string(&mut self, value: &[u8]) {
        self.encode_application_tag(TagNumber::OctetString, value.len());
        self.buf.put_slice(value);
    }

    /// Encode character string.
    pub fn encode_character_string(&mut self, value: &str) {
        // Encoding: 1 byte for character set (0 = UTF-8) + string bytes
        let bytes = value.as_bytes();
        self.encode_application_tag(TagNumber::CharacterString, bytes.len() + 1);
        self.buf.put_u8(0); // UTF-8 encoding
        self.buf.put_slice(bytes);
    }

    /// Encode context character string.
    pub fn encode_context_character_string(&mut self, tag_number: u8, value: &str) {
        let bytes = value.as_bytes();
        self.encode_context_tag(tag_number, bytes.len() + 1);
        self.buf.put_u8(0); // UTF-8 encoding
        self.buf.put_slice(bytes);
    }

    /// Encode bit string.
    pub fn encode_bit_string(&mut self, bits: &[bool]) {
        let num_bytes = (bits.len() + 7) / 8;
        let unused_bits = (8 - (bits.len() % 8)) % 8;

        self.encode_application_tag(TagNumber::BitString, num_bytes + 1);
        self.buf.put_u8(unused_bits as u8);

        let mut byte = 0u8;
        for (i, &bit) in bits.iter().enumerate() {
            if bit {
                byte |= 1 << (7 - (i % 8));
            }
            if (i + 1) % 8 == 0 || i == bits.len() - 1 {
                self.buf.put_u8(byte);
                byte = 0;
            }
        }
    }

    /// Encode context bit string.
    pub fn encode_context_bit_string(&mut self, tag_number: u8, bits: &[bool]) {
        let num_bytes = (bits.len() + 7) / 8;
        let unused_bits = (8 - (bits.len() % 8)) % 8;

        self.encode_context_tag(tag_number, num_bytes + 1);
        self.buf.put_u8(unused_bits as u8);

        let mut byte = 0u8;
        for (i, &bit) in bits.iter().enumerate() {
            if bit {
                byte |= 1 << (7 - (i % 8));
            }
            if (i + 1) % 8 == 0 || i == bits.len() - 1 {
                self.buf.put_u8(byte);
                byte = 0;
            }
        }
    }

    /// Encode enumerated (application tagged).
    pub fn encode_enumerated(&mut self, value: u32) {
        let bytes = if value < 0x100 {
            vec![value as u8]
        } else if value < 0x10000 {
            vec![(value >> 8) as u8, value as u8]
        } else if value < 0x1000000 {
            vec![(value >> 16) as u8, (value >> 8) as u8, value as u8]
        } else {
            vec![
                (value >> 24) as u8,
                (value >> 16) as u8,
                (value >> 8) as u8,
                value as u8,
            ]
        };

        self.encode_application_tag(TagNumber::Enumerated, bytes.len());
        self.buf.put_slice(&bytes);
    }

    /// Encode enumerated (context tagged).
    pub fn encode_context_enumerated(&mut self, tag_number: u8, value: u32) {
        let bytes = if value < 0x100 {
            vec![value as u8]
        } else if value < 0x10000 {
            vec![(value >> 8) as u8, value as u8]
        } else {
            vec![
                (value >> 24) as u8,
                (value >> 16) as u8,
                (value >> 8) as u8,
                value as u8,
            ]
        };

        self.encode_context_tag(tag_number, bytes.len());
        self.buf.put_slice(&bytes);
    }

    /// Encode object identifier (application tagged).
    pub fn encode_object_identifier(&mut self, object_id: ObjectId) {
        self.encode_application_tag(TagNumber::ObjectIdentifier, 4);
        self.buf.put_u32(object_id.encode());
    }

    /// Encode object identifier (context tagged).
    pub fn encode_context_object_identifier(&mut self, tag_number: u8, object_id: ObjectId) {
        self.encode_context_tag(tag_number, 4);
        self.buf.put_u32(object_id.encode());
    }

    /// Encode a BACnet value.
    pub fn encode_value(&mut self, value: &BACnetValue) {
        match value {
            BACnetValue::Null => self.encode_null(),
            BACnetValue::Boolean(v) => self.encode_boolean(*v),
            BACnetValue::Unsigned(v) => self.encode_unsigned(*v),
            BACnetValue::Signed(v) => self.encode_signed(*v),
            BACnetValue::Real(v) => self.encode_real(*v),
            BACnetValue::Double(v) => self.encode_double(*v),
            BACnetValue::OctetString(v) => self.encode_octet_string(v),
            BACnetValue::CharacterString(v) => self.encode_character_string(v),
            BACnetValue::BitString(v) => self.encode_bit_string(v),
            BACnetValue::Enumerated(v) => self.encode_enumerated(*v),
            BACnetValue::ObjectIdentifier(v) => self.encode_object_identifier(*v),
            BACnetValue::Array(arr) => {
                for item in arr {
                    self.encode_value(item);
                }
            }
            BACnetValue::List(list) => {
                for item in list {
                    self.encode_value(item);
                }
            }
            _ => {} // Date, Time, etc. can be added as needed
        }
    }

    /// Encode a value as context tagged.
    pub fn encode_context_value(&mut self, tag_number: u8, value: &BACnetValue) {
        self.encode_opening_tag(tag_number);
        self.encode_value(value);
        self.encode_closing_tag(tag_number);
    }

    /// Encode a complete BACnet-Error-PDU.
    ///
    /// Per ASHRAE 135, Clause 21.8, the Error PDU consists of:
    ///
    /// ```text
    /// | 0x50 | Invoke ID | Service Choice | Error Class | Error Code |
    /// |  1B  |    1B     |      1B        | Enumerated  | Enumerated |
    /// ```
    ///
    /// Both `error_class` and `error_code` are encoded using Application
    /// Tag 9 (Enumerated). This replaces manual byte construction that
    /// previously used incorrect tag numbers (Tag 0 / Tag 1).
    pub fn encode_error_pdu(
        &mut self,
        invoke_id: u8,
        service_choice: u8,
        error_class: u32,
        error_code: u32,
    ) {
        self.put_u8(0x50); // Error PDU type
        self.put_u8(invoke_id);
        self.put_u8(service_choice);
        self.encode_enumerated(error_class); // Application Tag 9
        self.encode_enumerated(error_code); // Application Tag 9
    }

    /// Encode a complete BACnet-Reject-PDU.
    ///
    /// Per ASHRAE 135, Clause 21.6, the Reject PDU is sent when the
    /// server detects a protocol violation in the request PDU itself
    /// (e.g., invalid tags, missing parameters, unrecognized service).
    ///
    /// ```text
    /// | PDU Type (0x60) | Invoke ID | Reject Reason |
    /// |       1B        |    1B     |      1B       |
    /// ```
    pub fn encode_reject_pdu(&mut self, invoke_id: u8, reject_reason: u8) {
        self.put_u8(0x60); // Reject PDU type
        self.put_u8(invoke_id);
        self.put_u8(reject_reason);
    }

    /// Encode a complete BACnet-Abort-PDU.
    ///
    /// Per ASHRAE 135, Clause 21.7, the Abort PDU terminates a transaction
    /// due to server-side issues (resource exhaustion, segmentation failure,
    /// internal error). Unlike Reject (protocol violation in request),
    /// Abort indicates the server cannot process the request.
    ///
    /// ```text
    /// | PDU Type (0x70) + Server bit | Invoke ID | Abort Reason |
    /// |             1B               |    1B     |      1B      |
    /// ```
    ///
    /// The `sent_by_server` flag indicates whether this Abort is sent
    /// by the server (true) or client (false). Bit 0 of the PDU type byte.
    pub fn encode_abort_pdu(&mut self, invoke_id: u8, abort_reason: u8, sent_by_server: bool) {
        let pdu_type = 0x70 | if sent_by_server { 0x01 } else { 0x00 };
        self.put_u8(pdu_type); // Abort PDU type + server bit
        self.put_u8(invoke_id);
        self.put_u8(abort_reason);
    }

    /// Encode a complete BACnet-SegmentACK-PDU.
    ///
    /// Per ASHRAE 135, Clause 21.5, the SegmentACK PDU acknowledges
    /// receipt of one or more segments during a segmented message transfer.
    ///
    /// ```text
    /// | PDU Type (0x40) + Flags | Invoke ID | Sequence Number | Actual Window Size |
    /// |          1B             |    1B     |       1B        |         1B         |
    /// ```
    ///
    /// Flags (bits 1-0 of byte 0):
    /// - Bit 1: negative-ack (NAK) — request retransmission from sequence_number
    /// - Bit 0: sent-by-server — 1 if server originated this ACK
    pub fn encode_segment_ack_pdu(
        &mut self,
        invoke_id: u8,
        sequence_number: u8,
        actual_window_size: u8,
        sent_by_server: bool,
        negative_ack: bool,
    ) {
        let mut pdu_type: u8 = 0x40; // SegmentACK PDU type (4 << 4)
        if negative_ack {
            pdu_type |= 0x02;
        }
        if sent_by_server {
            pdu_type |= 0x01;
        }
        self.put_u8(pdu_type);
        self.put_u8(invoke_id);
        self.put_u8(sequence_number);
        self.put_u8(actual_window_size);
    }

    /// Encode a segmented ComplexACK PDU header.
    ///
    /// Per ASHRAE 135, Clause 20.1.7, a segmented ComplexACK has:
    ///
    /// ```text
    /// | PDU Type (0x30) + Flags | Invoke ID | Sequence Number | Window Size | Service Choice | Data |
    /// |          1B             |    1B     |       1B        |     1B      |       1B       |  nB  |
    /// ```
    ///
    /// Flags in byte 0:
    /// - Bit 3 (0x08): segmented = 1
    /// - Bit 2 (0x04): more-follows = 1 if more segments follow
    pub fn encode_segmented_complex_ack_header(
        &mut self,
        invoke_id: u8,
        sequence_number: u8,
        proposed_window_size: u8,
        more_follows: bool,
        service_choice: u8,
    ) {
        let mut pdu_type: u8 = 0x30; // ComplexACK PDU type (3 << 4)
        pdu_type |= 0x08; // segmented = 1
        if more_follows {
            pdu_type |= 0x04; // more-follows = 1
        }
        self.put_u8(pdu_type);
        self.put_u8(invoke_id);
        self.put_u8(sequence_number);
        self.put_u8(proposed_window_size);
        self.put_u8(service_choice);
    }

    /// Get the current buffer length.
    pub fn len(&self) -> usize {
        self.buf.len()
    }

    /// Check if the buffer is empty.
    pub fn is_empty(&self) -> bool {
        self.buf.is_empty()
    }
}

impl Default for ApduEncoder {
    fn default() -> Self {
        Self::new()
    }
}

/// APDU decoder for parsing BACnet messages.
pub struct ApduDecoder<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> ApduDecoder<'a> {
    /// Create a new decoder.
    pub fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    /// Get remaining bytes.
    pub fn remaining(&self) -> usize {
        self.data.len() - self.pos
    }

    /// Check if at end.
    pub fn is_empty(&self) -> bool {
        self.pos >= self.data.len()
    }

    /// Get current position.
    pub fn position(&self) -> usize {
        self.pos
    }

    /// Peek at next byte.
    pub fn peek(&self) -> Option<u8> {
        self.data.get(self.pos).copied()
    }

    /// Read a single byte.
    pub fn read_u8(&mut self) -> Result<u8, ApduError> {
        if self.pos >= self.data.len() {
            return Err(ApduError::TooShort);
        }
        let value = self.data[self.pos];
        self.pos += 1;
        Ok(value)
    }

    /// Read multiple bytes.
    pub fn read_bytes(&mut self, len: usize) -> Result<&'a [u8], ApduError> {
        if self.pos + len > self.data.len() {
            return Err(ApduError::TooShort);
        }
        let bytes = &self.data[self.pos..self.pos + len];
        self.pos += len;
        Ok(bytes)
    }

    /// Skip bytes.
    pub fn skip(&mut self, len: usize) -> Result<(), ApduError> {
        if self.pos + len > self.data.len() {
            return Err(ApduError::TooShort);
        }
        self.pos += len;
        Ok(())
    }

    /// Decode tag info.
    pub fn decode_tag_info(&mut self) -> Result<(u8, bool, usize), ApduError> {
        let tag_byte = self.read_u8()?;

        let tag_number = (tag_byte >> 4) & 0x0F;
        let is_context = (tag_byte & 0x08) != 0;
        let lvt = tag_byte & 0x07;

        // Handle extended tag number
        let tag_number = if tag_number == 0x0F {
            self.read_u8()?
        } else {
            tag_number
        };

        // Check for opening/closing tags
        if is_context && lvt == 6 {
            // Opening tag
            return Ok((tag_number, true, 0));
        }
        if is_context && lvt == 7 {
            // Closing tag
            return Ok((tag_number, true, 0));
        }

        // Decode length
        let len = if lvt == 5 {
            let len_byte = self.read_u8()?;
            if len_byte == 254 {
                let mut bytes = [0u8; 2];
                bytes.copy_from_slice(self.read_bytes(2)?);
                u16::from_be_bytes(bytes) as usize
            } else if len_byte == 255 {
                let mut bytes = [0u8; 4];
                bytes.copy_from_slice(self.read_bytes(4)?);
                u32::from_be_bytes(bytes) as usize
            } else {
                len_byte as usize
            }
        } else {
            lvt as usize
        };

        Ok((tag_number, is_context, len))
    }

    /// Check if next tag is opening tag with given number.
    pub fn is_opening_tag(&self, tag_number: u8) -> bool {
        if let Some(byte) = self.peek() {
            if tag_number < 15 {
                byte == ((tag_number << 4) | 0x0E)
            } else {
                byte == 0xFE
            }
        } else {
            false
        }
    }

    /// Check if next tag is closing tag with given number.
    pub fn is_closing_tag(&self, tag_number: u8) -> bool {
        if let Some(byte) = self.peek() {
            if tag_number < 15 {
                byte == ((tag_number << 4) | 0x0F)
            } else {
                byte == 0xFF
            }
        } else {
            false
        }
    }

    /// Decode unsigned integer from raw bytes.
    pub fn decode_unsigned_raw(&self, bytes: &[u8]) -> u32 {
        let mut value = 0u32;
        for &b in bytes {
            value = (value << 8) | (b as u32);
        }
        value
    }

    /// Decode an unsigned integer.
    pub fn decode_unsigned(&mut self, len: usize) -> Result<u32, ApduError> {
        let bytes = self.read_bytes(len)?;
        Ok(self.decode_unsigned_raw(bytes))
    }

    /// Decode a signed integer.
    pub fn decode_signed(&mut self, len: usize) -> Result<i32, ApduError> {
        let bytes = self.read_bytes(len)?;
        let mut value = if bytes[0] & 0x80 != 0 { -1i32 } else { 0i32 };
        for &b in bytes {
            value = (value << 8) | (b as i32);
        }
        Ok(value)
    }

    /// Decode a real (float32).
    pub fn decode_real(&mut self) -> Result<f32, ApduError> {
        let bytes = self.read_bytes(4)?;
        let mut arr = [0u8; 4];
        arr.copy_from_slice(bytes);
        Ok(f32::from_be_bytes(arr))
    }

    /// Decode a double (float64).
    pub fn decode_double(&mut self) -> Result<f64, ApduError> {
        let bytes = self.read_bytes(8)?;
        let mut arr = [0u8; 8];
        arr.copy_from_slice(bytes);
        Ok(f64::from_be_bytes(arr))
    }

    /// Decode an object identifier.
    pub fn decode_object_identifier(&mut self) -> Result<ObjectId, ApduError> {
        let bytes = self.read_bytes(4)?;
        let mut arr = [0u8; 4];
        arr.copy_from_slice(bytes);
        let value = u32::from_be_bytes(arr);
        ObjectId::decode(value).ok_or(ApduError::DecodeError(
            "Invalid object identifier".to_string(),
        ))
    }

    /// Decode a character string.
    pub fn decode_character_string(&mut self, len: usize) -> Result<String, ApduError> {
        if len == 0 {
            return Ok(String::new());
        }
        let _encoding = self.read_u8()?;
        let bytes = self.read_bytes(len - 1)?;
        String::from_utf8(bytes.to_vec())
            .map_err(|_| ApduError::DecodeError("Invalid UTF-8 string".to_string()))
    }

    /// Decode a bit string.
    pub fn decode_bit_string(&mut self, len: usize) -> Result<Vec<bool>, ApduError> {
        if len == 0 {
            return Ok(Vec::new());
        }
        let unused_bits = self.read_u8()? as usize;
        let bytes = self.read_bytes(len - 1)?;

        let total_bits = bytes.len() * 8 - unused_bits;
        let mut bits = Vec::with_capacity(total_bits);

        for (i, &byte) in bytes.iter().enumerate() {
            let bits_in_byte = if i == bytes.len() - 1 {
                8 - unused_bits
            } else {
                8
            };
            for j in 0..bits_in_byte {
                bits.push((byte & (1 << (7 - j))) != 0);
            }
        }

        Ok(bits)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::object::types::ObjectType;

    #[test]
    fn test_encode_unsigned() {
        let mut encoder = ApduEncoder::new();
        encoder.encode_unsigned(42);

        let bytes = encoder.into_bytes();
        assert_eq!(bytes[0], 0x21); // Unsigned tag (2) << 4 | length (1)
        assert_eq!(bytes[1], 42);
    }

    #[test]
    fn test_encode_real() {
        let mut encoder = ApduEncoder::new();
        encoder.encode_real(3.14);

        let bytes = encoder.into_bytes();
        assert_eq!(bytes[0], 0x44); // Real tag (4) << 4 | length (4)
    }

    #[test]
    fn test_encode_object_identifier() {
        let mut encoder = ApduEncoder::new();
        let oid = ObjectId::new(ObjectType::AnalogInput, 100);
        encoder.encode_object_identifier(oid);

        let bytes = encoder.into_bytes();
        assert_eq!(bytes[0], 0xC4); // ObjectId tag (12) << 4 | length (4)
    }

    #[test]
    fn test_decode_unsigned() {
        let data = [0x21, 42];
        let mut decoder = ApduDecoder::new(&data);

        let (tag, is_context, len) = decoder.decode_tag_info().unwrap();
        assert_eq!(tag, 2); // Unsigned tag
        assert!(!is_context);
        assert_eq!(len, 1);

        let value = decoder.decode_unsigned(len).unwrap();
        assert_eq!(value, 42);
    }

    #[test]
    fn test_encode_boolean() {
        let mut encoder = ApduEncoder::new();
        encoder.encode_boolean(true);
        encoder.encode_boolean(false);

        let bytes = encoder.into_bytes();
        assert_eq!(bytes[0], 0x11); // Boolean tag (1) << 4 | 1
        assert_eq!(bytes[1], 0x10); // Boolean tag (1) << 4 | 0
    }

    #[test]
    fn test_context_tags() {
        let mut encoder = ApduEncoder::new();
        encoder.encode_context_unsigned(0, 100);

        let bytes = encoder.into_bytes();
        assert_eq!(bytes[0], 0x09); // Context tag 0, length 1
        assert_eq!(bytes[1], 100);
    }

    #[test]
    fn test_encode_error_pdu_uses_enumerated_tags() {
        let mut encoder = ApduEncoder::new();
        // ErrorClass::Property = 2, ErrorCode::UnknownProperty = 32
        encoder.encode_error_pdu(1, 12, 2, 32);
        let bytes = encoder.into_bytes();

        // Header
        assert_eq!(bytes[0], 0x50); // Error PDU type
        assert_eq!(bytes[1], 1); // invoke_id
        assert_eq!(bytes[2], 12); // service_choice

        // Error class: Enumerated Tag 9, length 1 → (9 << 4) | 1 = 0x91
        assert_eq!(bytes[3], 0x91);
        assert_eq!(bytes[4], 2); // ErrorClass::Property

        // Error code: Enumerated Tag 9, length 1 → 0x91
        assert_eq!(bytes[5], 0x91);
        assert_eq!(bytes[6], 32); // ErrorCode::UnknownProperty

        assert_eq!(bytes.len(), 7);
    }

    #[test]
    fn test_encode_error_pdu_object_unknown_object() {
        let mut encoder = ApduEncoder::new();
        // ErrorClass::Object = 1, ErrorCode::UnknownObject = 31
        encoder.encode_error_pdu(5, 12, 1, 31);
        let bytes = encoder.into_bytes();

        assert_eq!(bytes[0], 0x50);
        assert_eq!(bytes[3], 0x91);
        assert_eq!(bytes[4], 1); // ErrorClass::Object
        assert_eq!(bytes[5], 0x91);
        assert_eq!(bytes[6], 31); // ErrorCode::UnknownObject
    }
}

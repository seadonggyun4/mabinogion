//! OPC UA Binary Encoder — primitive types.
//!
//! OPC UA Part 6, Section 5.2.1: All integer types use little-endian byte order.

use bytes::{BufMut, BytesMut};
use chrono::{DateTime, Utc};

use crate::error::OpcUaResult;

/// Trait for types that can be encoded into OPC UA binary format.
pub trait BinaryEncodable {
    /// Encode this value into the buffer.
    fn encode(&self, buf: &mut BytesMut) -> OpcUaResult<()>;

    /// Return the encoded byte size of this value.
    fn encoded_size(&self) -> usize;
}

// =========================================================================
// Primitive types
// =========================================================================

impl BinaryEncodable for bool {
    fn encode(&self, buf: &mut BytesMut) -> OpcUaResult<()> {
        buf.put_u8(if *self { 1 } else { 0 });
        Ok(())
    }
    fn encoded_size(&self) -> usize {
        1
    }
}

impl BinaryEncodable for i8 {
    fn encode(&self, buf: &mut BytesMut) -> OpcUaResult<()> {
        buf.put_i8(*self);
        Ok(())
    }
    fn encoded_size(&self) -> usize {
        1
    }
}

impl BinaryEncodable for u8 {
    fn encode(&self, buf: &mut BytesMut) -> OpcUaResult<()> {
        buf.put_u8(*self);
        Ok(())
    }
    fn encoded_size(&self) -> usize {
        1
    }
}

impl BinaryEncodable for i16 {
    fn encode(&self, buf: &mut BytesMut) -> OpcUaResult<()> {
        buf.put_i16_le(*self);
        Ok(())
    }
    fn encoded_size(&self) -> usize {
        2
    }
}

impl BinaryEncodable for u16 {
    fn encode(&self, buf: &mut BytesMut) -> OpcUaResult<()> {
        buf.put_u16_le(*self);
        Ok(())
    }
    fn encoded_size(&self) -> usize {
        2
    }
}

impl BinaryEncodable for i32 {
    fn encode(&self, buf: &mut BytesMut) -> OpcUaResult<()> {
        buf.put_i32_le(*self);
        Ok(())
    }
    fn encoded_size(&self) -> usize {
        4
    }
}

impl BinaryEncodable for u32 {
    fn encode(&self, buf: &mut BytesMut) -> OpcUaResult<()> {
        buf.put_u32_le(*self);
        Ok(())
    }
    fn encoded_size(&self) -> usize {
        4
    }
}

impl BinaryEncodable for i64 {
    fn encode(&self, buf: &mut BytesMut) -> OpcUaResult<()> {
        buf.put_i64_le(*self);
        Ok(())
    }
    fn encoded_size(&self) -> usize {
        8
    }
}

impl BinaryEncodable for u64 {
    fn encode(&self, buf: &mut BytesMut) -> OpcUaResult<()> {
        buf.put_u64_le(*self);
        Ok(())
    }
    fn encoded_size(&self) -> usize {
        8
    }
}

impl BinaryEncodable for f32 {
    fn encode(&self, buf: &mut BytesMut) -> OpcUaResult<()> {
        buf.put_f32_le(*self);
        Ok(())
    }
    fn encoded_size(&self) -> usize {
        4
    }
}

impl BinaryEncodable for f64 {
    fn encode(&self, buf: &mut BytesMut) -> OpcUaResult<()> {
        buf.put_f64_le(*self);
        Ok(())
    }
    fn encoded_size(&self) -> usize {
        8
    }
}

// =========================================================================
// String — Int32 length-prefixed UTF-8 (-1 = null)
// =========================================================================

impl BinaryEncodable for str {
    fn encode(&self, buf: &mut BytesMut) -> OpcUaResult<()> {
        let bytes = self.as_bytes();
        buf.put_i32_le(bytes.len() as i32);
        buf.put_slice(bytes);
        Ok(())
    }
    fn encoded_size(&self) -> usize {
        4 + self.len()
    }
}

impl BinaryEncodable for String {
    fn encode(&self, buf: &mut BytesMut) -> OpcUaResult<()> {
        self.as_str().encode(buf)
    }
    fn encoded_size(&self) -> usize {
        4 + self.len()
    }
}

/// Encode an optional string (None → -1 length).
pub fn encode_optional_string(s: &Option<String>, buf: &mut BytesMut) -> OpcUaResult<()> {
    match s {
        Some(s) => s.encode(buf),
        None => {
            buf.put_i32_le(-1);
            Ok(())
        }
    }
}

/// Encoded size of an optional string.
pub fn optional_string_size(s: &Option<String>) -> usize {
    match s {
        Some(s) => 4 + s.len(),
        None => 4,
    }
}

// =========================================================================
// ByteString — Int32 length-prefixed bytes (-1 = null)
// =========================================================================

impl BinaryEncodable for Vec<u8> {
    fn encode(&self, buf: &mut BytesMut) -> OpcUaResult<()> {
        buf.put_i32_le(self.len() as i32);
        buf.put_slice(self);
        Ok(())
    }
    fn encoded_size(&self) -> usize {
        4 + self.len()
    }
}

/// Encode an optional byte string (None → -1 length).
pub fn encode_optional_byte_string(bs: &Option<Vec<u8>>, buf: &mut BytesMut) -> OpcUaResult<()> {
    match bs {
        Some(bs) => bs.encode(buf),
        None => {
            buf.put_i32_le(-1);
            Ok(())
        }
    }
}

/// Encoded size of an optional byte string.
pub fn optional_byte_string_size(bs: &Option<Vec<u8>>) -> usize {
    match bs {
        Some(bs) => 4 + bs.len(),
        None => 4,
    }
}

// =========================================================================
// DateTime — Int64 Windows FILETIME (100ns ticks from 1601-01-01)
// =========================================================================

/// Offset between Unix epoch (1970-01-01) and Windows FILETIME epoch (1601-01-01) in 100ns ticks.
const FILETIME_UNIX_DIFF: i64 = 116_444_736_000_000_000;

impl BinaryEncodable for DateTime<Utc> {
    fn encode(&self, buf: &mut BytesMut) -> OpcUaResult<()> {
        let nanos = self.timestamp_nanos_opt().unwrap_or(0);
        let ticks = nanos / 100 + FILETIME_UNIX_DIFF;
        buf.put_i64_le(ticks);
        Ok(())
    }
    fn encoded_size(&self) -> usize {
        8
    }
}

// =========================================================================
// Guid — 16 bytes with mixed endianness
// =========================================================================

impl BinaryEncodable for uuid::Uuid {
    fn encode(&self, buf: &mut BytesMut) -> OpcUaResult<()> {
        let (d1, d2, d3, d4) = self.as_fields();
        buf.put_u32_le(d1);
        buf.put_u16_le(d2);
        buf.put_u16_le(d3);
        buf.put_slice(d4);
        Ok(())
    }
    fn encoded_size(&self) -> usize {
        16
    }
}

// =========================================================================
// Array helper
// =========================================================================

/// Encode an array of encodable items: Int32 length + items.
pub fn encode_array<T: BinaryEncodable>(items: &[T], buf: &mut BytesMut) -> OpcUaResult<()> {
    buf.put_i32_le(items.len() as i32);
    for item in items {
        item.encode(buf)?;
    }
    Ok(())
}

/// Encode an optional array (None → -1 length).
pub fn encode_optional_array<T: BinaryEncodable>(
    items: &Option<Vec<T>>,
    buf: &mut BytesMut,
) -> OpcUaResult<()> {
    match items {
        Some(items) => encode_array(items, buf),
        None => {
            buf.put_i32_le(-1);
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bool_encode() {
        let mut buf = BytesMut::new();
        true.encode(&mut buf).unwrap();
        assert_eq!(&buf[..], &[1]);

        let mut buf = BytesMut::new();
        false.encode(&mut buf).unwrap();
        assert_eq!(&buf[..], &[0]);
    }

    #[test]
    fn test_i32_encode() {
        let mut buf = BytesMut::new();
        42i32.encode(&mut buf).unwrap();
        assert_eq!(&buf[..], &42i32.to_le_bytes());
    }

    #[test]
    fn test_string_encode() {
        let mut buf = BytesMut::new();
        "hello".encode(&mut buf).unwrap();
        assert_eq!(&buf[0..4], &5i32.to_le_bytes());
        assert_eq!(&buf[4..], b"hello");
    }

    #[test]
    fn test_optional_string_null() {
        let mut buf = BytesMut::new();
        encode_optional_string(&None, &mut buf).unwrap();
        assert_eq!(&buf[..], &(-1i32).to_le_bytes());
    }

    #[test]
    fn test_uuid_encode() {
        let id = uuid::Uuid::nil();
        let mut buf = BytesMut::new();
        id.encode(&mut buf).unwrap();
        assert_eq!(buf.len(), 16);
    }
}

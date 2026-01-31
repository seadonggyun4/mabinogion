//! OPC UA Binary Decoder — primitive types.
//!
//! OPC UA Part 6, Section 5.2.1: All integer types use little-endian byte order.

use bytes::{Buf, Bytes};
use chrono::{DateTime, TimeZone, Utc};

use crate::error::{OpcUaError, OpcUaResult};

/// Trait for types that can be decoded from OPC UA binary format.
pub trait BinaryDecodable: Sized {
    /// Decode this value from the buffer, advancing the cursor.
    fn decode(buf: &mut Bytes) -> OpcUaResult<Self>;
}

/// Check that the buffer has at least `n` bytes remaining.
fn ensure_remaining(buf: &Bytes, n: usize) -> OpcUaResult<()> {
    if buf.remaining() < n {
        Err(OpcUaError::Codec(format!(
            "Not enough data: need {} bytes, have {}",
            n,
            buf.remaining()
        )))
    } else {
        Ok(())
    }
}

// =========================================================================
// Primitive types
// =========================================================================

impl BinaryDecodable for bool {
    fn decode(buf: &mut Bytes) -> OpcUaResult<Self> {
        ensure_remaining(buf, 1)?;
        Ok(buf.get_u8() != 0)
    }
}

impl BinaryDecodable for i8 {
    fn decode(buf: &mut Bytes) -> OpcUaResult<Self> {
        ensure_remaining(buf, 1)?;
        Ok(buf.get_i8())
    }
}

impl BinaryDecodable for u8 {
    fn decode(buf: &mut Bytes) -> OpcUaResult<Self> {
        ensure_remaining(buf, 1)?;
        Ok(buf.get_u8())
    }
}

impl BinaryDecodable for i16 {
    fn decode(buf: &mut Bytes) -> OpcUaResult<Self> {
        ensure_remaining(buf, 2)?;
        Ok(buf.get_i16_le())
    }
}

impl BinaryDecodable for u16 {
    fn decode(buf: &mut Bytes) -> OpcUaResult<Self> {
        ensure_remaining(buf, 2)?;
        Ok(buf.get_u16_le())
    }
}

impl BinaryDecodable for i32 {
    fn decode(buf: &mut Bytes) -> OpcUaResult<Self> {
        ensure_remaining(buf, 4)?;
        Ok(buf.get_i32_le())
    }
}

impl BinaryDecodable for u32 {
    fn decode(buf: &mut Bytes) -> OpcUaResult<Self> {
        ensure_remaining(buf, 4)?;
        Ok(buf.get_u32_le())
    }
}

impl BinaryDecodable for i64 {
    fn decode(buf: &mut Bytes) -> OpcUaResult<Self> {
        ensure_remaining(buf, 8)?;
        Ok(buf.get_i64_le())
    }
}

impl BinaryDecodable for u64 {
    fn decode(buf: &mut Bytes) -> OpcUaResult<Self> {
        ensure_remaining(buf, 8)?;
        Ok(buf.get_u64_le())
    }
}

impl BinaryDecodable for f32 {
    fn decode(buf: &mut Bytes) -> OpcUaResult<Self> {
        ensure_remaining(buf, 4)?;
        Ok(buf.get_f32_le())
    }
}

impl BinaryDecodable for f64 {
    fn decode(buf: &mut Bytes) -> OpcUaResult<Self> {
        ensure_remaining(buf, 8)?;
        Ok(buf.get_f64_le())
    }
}

// =========================================================================
// String — Int32 length-prefixed UTF-8 (-1 = null)
// =========================================================================

impl BinaryDecodable for String {
    fn decode(buf: &mut Bytes) -> OpcUaResult<Self> {
        ensure_remaining(buf, 4)?;
        let len = buf.get_i32_le();
        if len < 0 {
            return Ok(String::new());
        }
        let len = len as usize;
        ensure_remaining(buf, len)?;
        let bytes = buf.split_to(len);
        String::from_utf8(bytes.to_vec())
            .map_err(|e| OpcUaError::Codec(format!("Invalid UTF-8: {}", e)))
    }
}

/// Decode an optional string (-1 length → None).
pub fn decode_optional_string(buf: &mut Bytes) -> OpcUaResult<Option<String>> {
    ensure_remaining(buf, 4)?;
    let len = buf.get_i32_le();
    if len < 0 {
        return Ok(None);
    }
    let len = len as usize;
    ensure_remaining(buf, len)?;
    let bytes = buf.split_to(len);
    let s = String::from_utf8(bytes.to_vec())
        .map_err(|e| OpcUaError::Codec(format!("Invalid UTF-8: {}", e)))?;
    Ok(Some(s))
}

// =========================================================================
// ByteString — Int32 length-prefixed bytes (-1 = null)
// =========================================================================

impl BinaryDecodable for Vec<u8> {
    fn decode(buf: &mut Bytes) -> OpcUaResult<Self> {
        ensure_remaining(buf, 4)?;
        let len = buf.get_i32_le();
        if len < 0 {
            return Ok(Vec::new());
        }
        let len = len as usize;
        ensure_remaining(buf, len)?;
        Ok(buf.split_to(len).to_vec())
    }
}

/// Decode an optional byte string (-1 length → None).
pub fn decode_optional_byte_string(buf: &mut Bytes) -> OpcUaResult<Option<Vec<u8>>> {
    ensure_remaining(buf, 4)?;
    let len = buf.get_i32_le();
    if len < 0 {
        return Ok(None);
    }
    let len = len as usize;
    ensure_remaining(buf, len)?;
    Ok(Some(buf.split_to(len).to_vec()))
}

// =========================================================================
// DateTime — Int64 Windows FILETIME (100ns ticks from 1601-01-01)
// =========================================================================

/// Offset between Unix epoch (1970-01-01) and Windows FILETIME epoch (1601-01-01) in 100ns ticks.
const FILETIME_UNIX_DIFF: i64 = 116_444_736_000_000_000;

impl BinaryDecodable for DateTime<Utc> {
    fn decode(buf: &mut Bytes) -> OpcUaResult<Self> {
        ensure_remaining(buf, 8)?;
        let ticks = buf.get_i64_le();
        if ticks <= 0 {
            return Ok(DateTime::<Utc>::UNIX_EPOCH);
        }
        let unix_ticks = ticks - FILETIME_UNIX_DIFF;
        let secs = unix_ticks / 10_000_000;
        let nsecs = ((unix_ticks % 10_000_000) * 100) as u32;
        Utc.timestamp_opt(secs, nsecs)
            .single()
            .ok_or_else(|| OpcUaError::Codec(format!("Invalid DateTime ticks: {}", ticks)))
    }
}

// =========================================================================
// Guid — 16 bytes with mixed endianness
// =========================================================================

impl BinaryDecodable for uuid::Uuid {
    fn decode(buf: &mut Bytes) -> OpcUaResult<Self> {
        ensure_remaining(buf, 16)?;
        let d1 = buf.get_u32_le();
        let d2 = buf.get_u16_le();
        let d3 = buf.get_u16_le();
        let mut d4 = [0u8; 8];
        buf.copy_to_slice(&mut d4);
        Ok(uuid::Uuid::from_fields(d1, d2, d3, &d4))
    }
}

// =========================================================================
// Array helper
// =========================================================================

/// Decode an array of decodable items: Int32 length + items.
pub fn decode_array<T: BinaryDecodable>(buf: &mut Bytes) -> OpcUaResult<Vec<T>> {
    ensure_remaining(buf, 4)?;
    let len = buf.get_i32_le();
    if len < 0 {
        return Ok(Vec::new());
    }
    let len = len as usize;
    let mut items = Vec::with_capacity(len.min(65536));
    for _ in 0..len {
        items.push(T::decode(buf)?);
    }
    Ok(items)
}

/// Decode an optional array (-1 length → None).
pub fn decode_optional_array<T: BinaryDecodable>(buf: &mut Bytes) -> OpcUaResult<Option<Vec<T>>> {
    ensure_remaining(buf, 4)?;
    let len = buf.get_i32_le();
    if len < 0 {
        return Ok(None);
    }
    let len = len as usize;
    let mut items = Vec::with_capacity(len.min(65536));
    for _ in 0..len {
        items.push(T::decode(buf)?);
    }
    Ok(Some(items))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codec::encoder::BinaryEncodable;
    use bytes::BytesMut;

    fn roundtrip_encode(val: &impl BinaryEncodable) -> Bytes {
        let mut buf = BytesMut::new();
        val.encode(&mut buf).unwrap();
        buf.freeze()
    }

    #[test]
    fn test_bool_roundtrip() {
        let mut data = roundtrip_encode(&true);
        assert_eq!(bool::decode(&mut data).unwrap(), true);

        let mut data = roundtrip_encode(&false);
        assert_eq!(bool::decode(&mut data).unwrap(), false);
    }

    #[test]
    fn test_i32_roundtrip() {
        let mut data = roundtrip_encode(&(-42i32));
        assert_eq!(i32::decode(&mut data).unwrap(), -42);
    }

    #[test]
    fn test_f64_roundtrip() {
        let mut data = roundtrip_encode(&3.14159f64);
        assert!((f64::decode(&mut data).unwrap() - 3.14159).abs() < 1e-10);
    }

    #[test]
    fn test_string_roundtrip() {
        let mut data = roundtrip_encode(&"hello world".to_string());
        assert_eq!(String::decode(&mut data).unwrap(), "hello world");
    }

    #[test]
    fn test_null_string() {
        let mut buf = BytesMut::new();
        buf.extend_from_slice(&(-1i32).to_le_bytes());
        let mut data = buf.freeze();
        assert_eq!(String::decode(&mut data).unwrap(), "");
    }

    #[test]
    fn test_byte_string_roundtrip() {
        let original = vec![1u8, 2, 3, 4, 5];
        let mut data = roundtrip_encode(&original);
        assert_eq!(Vec::<u8>::decode(&mut data).unwrap(), original);
    }

    #[test]
    fn test_uuid_roundtrip() {
        let original = uuid::Uuid::new_v4();
        let mut data = roundtrip_encode(&original);
        assert_eq!(uuid::Uuid::decode(&mut data).unwrap(), original);
    }

    #[test]
    fn test_datetime_roundtrip() {
        let original = Utc::now();
        let mut data = roundtrip_encode(&original);
        let decoded = DateTime::<Utc>::decode(&mut data).unwrap();
        // Allow 1μs tolerance due to 100ns tick granularity
        let diff = (original - decoded).num_microseconds().unwrap_or(0).abs();
        assert!(diff < 2, "DateTime roundtrip diff: {}μs", diff);
    }

    #[test]
    fn test_insufficient_data() {
        let mut data = Bytes::from_static(&[0u8, 1]);
        assert!(i32::decode(&mut data).is_err());
    }
}

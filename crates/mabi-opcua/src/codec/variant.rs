//! OPC UA Variant binary encoding/decoding.
//!
//! OPC UA Part 6, Section 5.2.2.16 — Variant encoding:
//! - Encoding byte: bits 0-5 = type ID, bit 6 = dimensions, bit 7 = array flag
//! - Followed by the encoded value

use bytes::{BufMut, Bytes, BytesMut};
use chrono::{DateTime, Utc};

use crate::codec::decoder::BinaryDecodable;
use crate::codec::encoder::BinaryEncodable;
use crate::error::{OpcUaError, OpcUaResult};
use crate::types::{variant::DataTypeId, NodeId, Variant};

/// Array flag bit in the encoding byte.
const ARRAY_BIT: u8 = 0x80;

impl BinaryEncodable for Variant {
    fn encode(&self, buf: &mut BytesMut) -> OpcUaResult<()> {
        match self {
            Variant::Null => {
                buf.put_u8(0); // Null type
            }
            Variant::Boolean(v) => {
                buf.put_u8(DataTypeId::Boolean as u8);
                v.encode(buf)?;
            }
            Variant::SByte(v) => {
                buf.put_u8(DataTypeId::SByte as u8);
                v.encode(buf)?;
            }
            Variant::Byte(v) => {
                buf.put_u8(DataTypeId::Byte as u8);
                v.encode(buf)?;
            }
            Variant::Int16(v) => {
                buf.put_u8(DataTypeId::Int16 as u8);
                v.encode(buf)?;
            }
            Variant::UInt16(v) => {
                buf.put_u8(DataTypeId::UInt16 as u8);
                v.encode(buf)?;
            }
            Variant::Int32(v) => {
                buf.put_u8(DataTypeId::Int32 as u8);
                v.encode(buf)?;
            }
            Variant::UInt32(v) => {
                buf.put_u8(DataTypeId::UInt32 as u8);
                v.encode(buf)?;
            }
            Variant::Int64(v) => {
                buf.put_u8(DataTypeId::Int64 as u8);
                v.encode(buf)?;
            }
            Variant::UInt64(v) => {
                buf.put_u8(DataTypeId::UInt64 as u8);
                v.encode(buf)?;
            }
            Variant::Float(v) => {
                buf.put_u8(DataTypeId::Float as u8);
                v.encode(buf)?;
            }
            Variant::Double(v) => {
                buf.put_u8(DataTypeId::Double as u8);
                v.encode(buf)?;
            }
            Variant::String(v) => {
                buf.put_u8(DataTypeId::String as u8);
                v.encode(buf)?;
            }
            Variant::DateTime(v) => {
                buf.put_u8(DataTypeId::DateTime as u8);
                v.encode(buf)?;
            }
            Variant::Guid(v) => {
                buf.put_u8(DataTypeId::Guid as u8);
                v.encode(buf)?;
            }
            Variant::ByteString(v) => {
                buf.put_u8(DataTypeId::ByteString as u8);
                v.encode(buf)?;
            }
            Variant::NodeId(v) => {
                buf.put_u8(DataTypeId::NodeId as u8);
                v.encode(buf)?;
            }
            Variant::StatusCode(v) => {
                buf.put_u8(DataTypeId::StatusCode as u8);
                buf.put_u32_le(*v);
            }
            Variant::Scalar(sv) => {
                encode_scalar_value(sv, buf)?;
            }
            Variant::Array(av) => {
                encode_array_value(av, buf)?;
            }
        }
        Ok(())
    }

    fn encoded_size(&self) -> usize {
        1 + match self {
            Variant::Null => 0,
            Variant::Boolean(_) => 1,
            Variant::SByte(_) | Variant::Byte(_) => 1,
            Variant::Int16(_) | Variant::UInt16(_) => 2,
            Variant::Int32(_) | Variant::UInt32(_) | Variant::Float(_) => 4,
            Variant::Int64(_) | Variant::UInt64(_) | Variant::Double(_) => 8,
            Variant::String(s) => 4 + s.len(),
            Variant::DateTime(_) => 8,
            Variant::Guid(_) => 16,
            Variant::ByteString(bs) => 4 + bs.len(),
            Variant::NodeId(n) => n.encoded_size(),
            Variant::StatusCode(_) => 4,
            Variant::Scalar(sv) => scalar_value_size(sv),
            Variant::Array(av) => array_value_size(av),
        }
    }
}

impl BinaryDecodable for Variant {
    fn decode(buf: &mut Bytes) -> OpcUaResult<Self> {
        let encoding = u8::decode(buf)?;
        let is_array = (encoding & ARRAY_BIT) != 0;
        let type_id = encoding & 0x3F;

        if is_array {
            decode_array_variant(type_id, buf)
        } else {
            decode_scalar_variant(type_id, buf)
        }
    }
}

fn decode_scalar_variant(type_id: u8, buf: &mut Bytes) -> OpcUaResult<Variant> {
    match DataTypeId::from_u8(type_id) {
        Some(DataTypeId::Null) | None if type_id == 0 => Ok(Variant::Null),
        Some(DataTypeId::Boolean) => Ok(Variant::Boolean(bool::decode(buf)?)),
        Some(DataTypeId::SByte) => Ok(Variant::SByte(i8::decode(buf)?)),
        Some(DataTypeId::Byte) => Ok(Variant::Byte(u8::decode(buf)?)),
        Some(DataTypeId::Int16) => Ok(Variant::Int16(i16::decode(buf)?)),
        Some(DataTypeId::UInt16) => Ok(Variant::UInt16(u16::decode(buf)?)),
        Some(DataTypeId::Int32) => Ok(Variant::Int32(i32::decode(buf)?)),
        Some(DataTypeId::UInt32) => Ok(Variant::UInt32(u32::decode(buf)?)),
        Some(DataTypeId::Int64) => Ok(Variant::Int64(i64::decode(buf)?)),
        Some(DataTypeId::UInt64) => Ok(Variant::UInt64(u64::decode(buf)?)),
        Some(DataTypeId::Float) => Ok(Variant::Float(f32::decode(buf)?)),
        Some(DataTypeId::Double) => Ok(Variant::Double(f64::decode(buf)?)),
        Some(DataTypeId::String) => Ok(Variant::String(String::decode(buf)?)),
        Some(DataTypeId::DateTime) => Ok(Variant::DateTime(DateTime::<Utc>::decode(buf)?)),
        Some(DataTypeId::Guid) => Ok(Variant::Guid(uuid::Uuid::decode(buf)?)),
        Some(DataTypeId::ByteString) => Ok(Variant::ByteString(Vec::<u8>::decode(buf)?)),
        Some(DataTypeId::NodeId) => Ok(Variant::NodeId(NodeId::decode(buf)?)),
        Some(DataTypeId::StatusCode) => Ok(Variant::StatusCode(u32::decode(buf)?)),
        _ => Err(OpcUaError::Codec(format!(
            "Unsupported variant type: {}",
            type_id
        ))),
    }
}

fn decode_array_variant(type_id: u8, buf: &mut Bytes) -> OpcUaResult<Variant> {
    use crate::codec::decoder::decode_array;
    use crate::types::variant::VariantArrayValue;

    let result = match DataTypeId::from_u8(type_id) {
        Some(DataTypeId::Boolean) => Variant::Array(VariantArrayValue::Boolean(decode_array(buf)?)),
        Some(DataTypeId::SByte) => Variant::Array(VariantArrayValue::SByte(decode_array(buf)?)),
        Some(DataTypeId::Byte) => Variant::Array(VariantArrayValue::Byte(decode_array(buf)?)),
        Some(DataTypeId::Int16) => Variant::Array(VariantArrayValue::Int16(decode_array(buf)?)),
        Some(DataTypeId::UInt16) => Variant::Array(VariantArrayValue::UInt16(decode_array(buf)?)),
        Some(DataTypeId::Int32) => Variant::Array(VariantArrayValue::Int32(decode_array(buf)?)),
        Some(DataTypeId::UInt32) => Variant::Array(VariantArrayValue::UInt32(decode_array(buf)?)),
        Some(DataTypeId::Int64) => Variant::Array(VariantArrayValue::Int64(decode_array(buf)?)),
        Some(DataTypeId::UInt64) => Variant::Array(VariantArrayValue::UInt64(decode_array(buf)?)),
        Some(DataTypeId::Float) => Variant::Array(VariantArrayValue::Float(decode_array(buf)?)),
        Some(DataTypeId::Double) => Variant::Array(VariantArrayValue::Double(decode_array(buf)?)),
        Some(DataTypeId::String) => Variant::Array(VariantArrayValue::String(decode_array(buf)?)),
        Some(DataTypeId::DateTime) => {
            Variant::Array(VariantArrayValue::DateTime(decode_array(buf)?))
        }
        Some(DataTypeId::Guid) => Variant::Array(VariantArrayValue::Guid(decode_array(buf)?)),
        Some(DataTypeId::ByteString) => {
            Variant::Array(VariantArrayValue::ByteString(decode_array(buf)?))
        }
        Some(DataTypeId::NodeId) => Variant::Array(VariantArrayValue::NodeId(decode_array(buf)?)),
        Some(DataTypeId::StatusCode) => {
            Variant::Array(VariantArrayValue::StatusCode(decode_array(buf)?))
        }
        _ => {
            return Err(OpcUaError::Codec(format!(
                "Unsupported array type: {}",
                type_id
            )))
        }
    };
    Ok(result)
}

// =========================================================================
// Scalar/Array value encoding helpers (for Variant::Scalar and Variant::Array)
// =========================================================================

use crate::types::variant::{VariantArrayValue, VariantScalarValue};

fn encode_scalar_value(sv: &VariantScalarValue, buf: &mut BytesMut) -> OpcUaResult<()> {
    match sv {
        VariantScalarValue::Null => buf.put_u8(0),
        VariantScalarValue::Boolean(v) => {
            buf.put_u8(DataTypeId::Boolean as u8);
            v.encode(buf)?;
        }
        VariantScalarValue::SByte(v) => {
            buf.put_u8(DataTypeId::SByte as u8);
            v.encode(buf)?;
        }
        VariantScalarValue::Byte(v) => {
            buf.put_u8(DataTypeId::Byte as u8);
            v.encode(buf)?;
        }
        VariantScalarValue::Int16(v) => {
            buf.put_u8(DataTypeId::Int16 as u8);
            v.encode(buf)?;
        }
        VariantScalarValue::UInt16(v) => {
            buf.put_u8(DataTypeId::UInt16 as u8);
            v.encode(buf)?;
        }
        VariantScalarValue::Int32(v) => {
            buf.put_u8(DataTypeId::Int32 as u8);
            v.encode(buf)?;
        }
        VariantScalarValue::UInt32(v) => {
            buf.put_u8(DataTypeId::UInt32 as u8);
            v.encode(buf)?;
        }
        VariantScalarValue::Int64(v) => {
            buf.put_u8(DataTypeId::Int64 as u8);
            v.encode(buf)?;
        }
        VariantScalarValue::UInt64(v) => {
            buf.put_u8(DataTypeId::UInt64 as u8);
            v.encode(buf)?;
        }
        VariantScalarValue::Float(v) => {
            buf.put_u8(DataTypeId::Float as u8);
            v.encode(buf)?;
        }
        VariantScalarValue::Double(v) => {
            buf.put_u8(DataTypeId::Double as u8);
            v.encode(buf)?;
        }
        VariantScalarValue::String(v) => {
            buf.put_u8(DataTypeId::String as u8);
            v.encode(buf)?;
        }
        VariantScalarValue::DateTime(v) => {
            buf.put_u8(DataTypeId::DateTime as u8);
            v.encode(buf)?;
        }
        VariantScalarValue::Guid(v) => {
            buf.put_u8(DataTypeId::Guid as u8);
            v.encode(buf)?;
        }
        VariantScalarValue::ByteString(v) => {
            buf.put_u8(DataTypeId::ByteString as u8);
            v.encode(buf)?;
        }
        VariantScalarValue::XmlElement(v) => {
            buf.put_u8(DataTypeId::XmlElement as u8);
            v.encode(buf)?;
        }
        VariantScalarValue::NodeId(v) => {
            buf.put_u8(DataTypeId::NodeId as u8);
            v.encode(buf)?;
        }
        VariantScalarValue::StatusCode(v) => {
            buf.put_u8(DataTypeId::StatusCode as u8);
            buf.put_u32_le(*v);
        }
    }
    Ok(())
}

fn scalar_value_size(sv: &VariantScalarValue) -> usize {
    match sv {
        VariantScalarValue::Null => 0,
        VariantScalarValue::Boolean(_) => 1,
        VariantScalarValue::SByte(_) | VariantScalarValue::Byte(_) => 1,
        VariantScalarValue::Int16(_) | VariantScalarValue::UInt16(_) => 2,
        VariantScalarValue::Int32(_)
        | VariantScalarValue::UInt32(_)
        | VariantScalarValue::Float(_) => 4,
        VariantScalarValue::Int64(_)
        | VariantScalarValue::UInt64(_)
        | VariantScalarValue::Double(_) => 8,
        VariantScalarValue::String(s) => 4 + s.len(),
        VariantScalarValue::DateTime(_) => 8,
        VariantScalarValue::Guid(_) => 16,
        VariantScalarValue::ByteString(bs) => 4 + bs.len(),
        VariantScalarValue::XmlElement(s) => 4 + s.len(),
        VariantScalarValue::NodeId(n) => n.encoded_size(),
        VariantScalarValue::StatusCode(_) => 4,
    }
}

fn encode_array_value(av: &VariantArrayValue, buf: &mut BytesMut) -> OpcUaResult<()> {
    use crate::codec::encoder::encode_array;
    match av {
        VariantArrayValue::Boolean(v) => {
            buf.put_u8(DataTypeId::Boolean as u8 | ARRAY_BIT);
            encode_array(v, buf)?;
        }
        VariantArrayValue::SByte(v) => {
            buf.put_u8(DataTypeId::SByte as u8 | ARRAY_BIT);
            encode_array(v, buf)?;
        }
        VariantArrayValue::Byte(v) => {
            buf.put_u8(DataTypeId::Byte as u8 | ARRAY_BIT);
            encode_array(v, buf)?;
        }
        VariantArrayValue::Int16(v) => {
            buf.put_u8(DataTypeId::Int16 as u8 | ARRAY_BIT);
            encode_array(v, buf)?;
        }
        VariantArrayValue::UInt16(v) => {
            buf.put_u8(DataTypeId::UInt16 as u8 | ARRAY_BIT);
            encode_array(v, buf)?;
        }
        VariantArrayValue::Int32(v) => {
            buf.put_u8(DataTypeId::Int32 as u8 | ARRAY_BIT);
            encode_array(v, buf)?;
        }
        VariantArrayValue::UInt32(v) => {
            buf.put_u8(DataTypeId::UInt32 as u8 | ARRAY_BIT);
            encode_array(v, buf)?;
        }
        VariantArrayValue::Int64(v) => {
            buf.put_u8(DataTypeId::Int64 as u8 | ARRAY_BIT);
            encode_array(v, buf)?;
        }
        VariantArrayValue::UInt64(v) => {
            buf.put_u8(DataTypeId::UInt64 as u8 | ARRAY_BIT);
            encode_array(v, buf)?;
        }
        VariantArrayValue::Float(v) => {
            buf.put_u8(DataTypeId::Float as u8 | ARRAY_BIT);
            encode_array(v, buf)?;
        }
        VariantArrayValue::Double(v) => {
            buf.put_u8(DataTypeId::Double as u8 | ARRAY_BIT);
            encode_array(v, buf)?;
        }
        VariantArrayValue::String(v) => {
            buf.put_u8(DataTypeId::String as u8 | ARRAY_BIT);
            encode_array(v, buf)?;
        }
        VariantArrayValue::DateTime(v) => {
            buf.put_u8(DataTypeId::DateTime as u8 | ARRAY_BIT);
            encode_array(v, buf)?;
        }
        VariantArrayValue::Guid(v) => {
            buf.put_u8(DataTypeId::Guid as u8 | ARRAY_BIT);
            encode_array(v, buf)?;
        }
        VariantArrayValue::ByteString(v) => {
            buf.put_u8(DataTypeId::ByteString as u8 | ARRAY_BIT);
            encode_array(v, buf)?;
        }
        VariantArrayValue::NodeId(v) => {
            buf.put_u8(DataTypeId::NodeId as u8 | ARRAY_BIT);
            encode_array(v, buf)?;
        }
        VariantArrayValue::StatusCode(v) => {
            buf.put_u8(DataTypeId::StatusCode as u8 | ARRAY_BIT);
            encode_array(v, buf)?;
        }
    }
    Ok(())
}

fn array_value_size(av: &VariantArrayValue) -> usize {
    // 4 bytes for length prefix + element sizes
    4 + match av {
        VariantArrayValue::Boolean(v) => v.len(),
        VariantArrayValue::SByte(v) => v.len(),
        VariantArrayValue::Byte(v) => v.len(),
        VariantArrayValue::Int16(v) => v.len() * 2,
        VariantArrayValue::UInt16(v) => v.len() * 2,
        VariantArrayValue::Int32(v) => v.len() * 4,
        VariantArrayValue::UInt32(v) => v.len() * 4,
        VariantArrayValue::Int64(v) => v.len() * 8,
        VariantArrayValue::UInt64(v) => v.len() * 8,
        VariantArrayValue::Float(v) => v.len() * 4,
        VariantArrayValue::Double(v) => v.len() * 8,
        VariantArrayValue::String(v) => v.iter().map(|s| 4 + s.len()).sum(),
        VariantArrayValue::DateTime(v) => v.len() * 8,
        VariantArrayValue::Guid(v) => v.len() * 16,
        VariantArrayValue::ByteString(v) => v.iter().map(|bs| 4 + bs.len()).sum(),
        VariantArrayValue::NodeId(v) => v.iter().map(|n| n.encoded_size()).sum(),
        VariantArrayValue::StatusCode(v) => v.len() * 4,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roundtrip(v: &Variant) -> Variant {
        let mut buf = BytesMut::new();
        v.encode(&mut buf).unwrap();
        let mut data = buf.freeze();
        Variant::decode(&mut data).unwrap()
    }

    #[test]
    fn test_null() {
        assert_eq!(roundtrip(&Variant::Null), Variant::Null);
    }

    #[test]
    fn test_scalars() {
        assert_eq!(roundtrip(&Variant::Boolean(true)), Variant::Boolean(true));
        assert_eq!(roundtrip(&Variant::Int32(42)), Variant::Int32(42));
        assert_eq!(roundtrip(&Variant::Double(3.14)), Variant::Double(3.14));
        assert_eq!(
            roundtrip(&Variant::String("hello".into())),
            Variant::String("hello".into())
        );
        assert_eq!(
            roundtrip(&Variant::NodeId(NodeId::numeric(2, 1001))),
            Variant::NodeId(NodeId::numeric(2, 1001))
        );
        assert_eq!(roundtrip(&Variant::StatusCode(0)), Variant::StatusCode(0));
    }

    #[test]
    fn test_array() {
        let arr = Variant::Array(VariantArrayValue::Int32(vec![1, 2, 3]));
        let result = roundtrip(&arr);
        assert_eq!(result, arr);
    }
}

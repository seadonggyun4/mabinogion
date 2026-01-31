//! OPC UA NodeId binary encoding/decoding.
//!
//! OPC UA Part 6, Section 5.2.2.9 — NodeId encoding uses compact representations:
//! - TwoByte:    0x00 + u8 id          (ns=0, id 0-255)
//! - FourByte:   0x01 + u8 ns + u16 id (ns 0-255, id 0-65535)
//! - Numeric:    0x02 + u16 ns + u32 id
//! - String:     0x03 + u16 ns + String
//! - Guid:       0x04 + u16 ns + Guid
//! - ByteString: 0x05 + u16 ns + ByteString

use bytes::{BufMut, Bytes, BytesMut};

use crate::codec::encoder::BinaryEncodable;
use crate::codec::decoder::BinaryDecodable;
use crate::error::{OpcUaError, OpcUaResult};
use crate::types::{NodeId, node_id::NodeIdType};

/// NodeId encoding type bytes.
const TWO_BYTE: u8 = 0x00;
const FOUR_BYTE: u8 = 0x01;
const NUMERIC: u8 = 0x02;
const STRING: u8 = 0x03;
const GUID: u8 = 0x04;
const BYTE_STRING: u8 = 0x05;

impl BinaryEncodable for NodeId {
    fn encode(&self, buf: &mut BytesMut) -> OpcUaResult<()> {
        let ns = self.namespace();
        match self.identifier() {
            NodeIdType::Numeric(id) => {
                let id = *id;
                if ns == 0 && id <= 255 {
                    // TwoByte encoding
                    buf.put_u8(TWO_BYTE);
                    buf.put_u8(id as u8);
                } else if ns <= 255 && id <= 65535 {
                    // FourByte encoding
                    buf.put_u8(FOUR_BYTE);
                    buf.put_u8(ns as u8);
                    buf.put_u16_le(id as u16);
                } else {
                    // Full Numeric encoding
                    buf.put_u8(NUMERIC);
                    buf.put_u16_le(ns);
                    buf.put_u32_le(id);
                }
            }
            NodeIdType::String(s) => {
                buf.put_u8(STRING);
                buf.put_u16_le(ns);
                s.encode(buf)?;
            }
            NodeIdType::Guid(g) => {
                buf.put_u8(GUID);
                buf.put_u16_le(ns);
                g.encode(buf)?;
            }
            NodeIdType::ByteString(bs) => {
                buf.put_u8(BYTE_STRING);
                buf.put_u16_le(ns);
                bs.encode(buf)?;
            }
        }
        Ok(())
    }

    fn encoded_size(&self) -> usize {
        let ns = self.namespace();
        match self.identifier() {
            NodeIdType::Numeric(id) => {
                if ns == 0 && *id <= 255 {
                    2 // TwoByte
                } else if ns <= 255 && *id <= 65535 {
                    4 // FourByte
                } else {
                    7 // Numeric: 1 + 2 + 4
                }
            }
            NodeIdType::String(s) => 1 + 2 + 4 + s.len(),
            NodeIdType::Guid(_) => 1 + 2 + 16,
            NodeIdType::ByteString(bs) => 1 + 2 + 4 + bs.len(),
        }
    }
}

impl BinaryDecodable for NodeId {
    fn decode(buf: &mut Bytes) -> OpcUaResult<Self> {
        if buf.is_empty() {
            return Err(OpcUaError::Codec("Empty buffer for NodeId".into()));
        }
        let encoding = u8::decode(buf)?;
        match encoding {
            TWO_BYTE => {
                let id = u8::decode(buf)?;
                Ok(NodeId::numeric(0, id as u32))
            }
            FOUR_BYTE => {
                let ns = u8::decode(buf)?;
                let id = u16::decode(buf)?;
                Ok(NodeId::numeric(ns as u16, id as u32))
            }
            NUMERIC => {
                let ns = u16::decode(buf)?;
                let id = u32::decode(buf)?;
                Ok(NodeId::numeric(ns, id))
            }
            STRING => {
                let ns = u16::decode(buf)?;
                let s = String::decode(buf)?;
                Ok(NodeId::string(ns, s))
            }
            GUID => {
                let ns = u16::decode(buf)?;
                let g = uuid::Uuid::decode(buf)?;
                Ok(NodeId::guid(ns, g))
            }
            BYTE_STRING => {
                let ns = u16::decode(buf)?;
                let bs = Vec::<u8>::decode(buf)?;
                Ok(NodeId::byte_string(ns, bs))
            }
            _ => Err(OpcUaError::Codec(format!("Unknown NodeId encoding: 0x{:02X}", encoding))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roundtrip(node_id: &NodeId) -> NodeId {
        let mut buf = BytesMut::new();
        node_id.encode(&mut buf).unwrap();
        assert_eq!(buf.len(), node_id.encoded_size());
        let mut data = buf.freeze();
        NodeId::decode(&mut data).unwrap()
    }

    #[test]
    fn test_two_byte_encoding() {
        let id = NodeId::numeric(0, 42);
        let result = roundtrip(&id);
        assert_eq!(result, id);
        assert_eq!(id.encoded_size(), 2);
    }

    #[test]
    fn test_four_byte_encoding() {
        let id = NodeId::numeric(2, 1001);
        let result = roundtrip(&id);
        assert_eq!(result, id);
        assert_eq!(id.encoded_size(), 4);
    }

    #[test]
    fn test_numeric_encoding() {
        let id = NodeId::numeric(300, 100000);
        let result = roundtrip(&id);
        assert_eq!(result, id);
        assert_eq!(id.encoded_size(), 7);
    }

    #[test]
    fn test_string_encoding() {
        let id = NodeId::string(2, "Temperature");
        let result = roundtrip(&id);
        assert_eq!(result, id);
    }

    #[test]
    fn test_guid_encoding() {
        let id = NodeId::guid(1, uuid::Uuid::new_v4());
        let result = roundtrip(&id);
        assert_eq!(result, id);
    }

    #[test]
    fn test_byte_string_encoding() {
        let id = NodeId::byte_string(3, vec![1, 2, 3, 4]);
        let result = roundtrip(&id);
        assert_eq!(result, id);
    }
}

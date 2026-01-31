//! OPC UA DataValue binary encoding/decoding.
//!
//! OPC UA Part 6, Section 5.2.2.17 — DataValue encoding:
//! - Encoding mask byte with flags for which fields are present
//! - Followed by present fields in order

use bytes::{BufMut, Bytes, BytesMut};
use chrono::{DateTime, Utc};

use crate::codec::encoder::BinaryEncodable;
use crate::codec::decoder::BinaryDecodable;
use crate::error::OpcUaResult;
use crate::types::{DataValue, StatusCode, Variant};

// DataValue encoding mask bits
const HAS_VALUE: u8 = 0x01;
const HAS_STATUS: u8 = 0x02;
const HAS_SOURCE_TIMESTAMP: u8 = 0x04;
const HAS_SERVER_TIMESTAMP: u8 = 0x08;
const HAS_SOURCE_PICOSECONDS: u8 = 0x10;
const HAS_SERVER_PICOSECONDS: u8 = 0x20;

impl BinaryEncodable for DataValue {
    fn encode(&self, buf: &mut BytesMut) -> OpcUaResult<()> {
        let mut mask: u8 = 0;
        if self.value().is_some() { mask |= HAS_VALUE; }
        if self.status().raw() != 0 { mask |= HAS_STATUS; }
        if self.source_timestamp().is_some() { mask |= HAS_SOURCE_TIMESTAMP; }
        if self.server_timestamp().is_some() { mask |= HAS_SERVER_TIMESTAMP; }
        if self.source_picoseconds() != 0 { mask |= HAS_SOURCE_PICOSECONDS; }
        if self.server_picoseconds() != 0 { mask |= HAS_SERVER_PICOSECONDS; }

        buf.put_u8(mask);

        if let Some(v) = self.value() {
            v.encode(buf)?;
        }
        if (mask & HAS_STATUS) != 0 {
            buf.put_u32_le(self.status().raw());
        }
        if let Some(ts) = self.source_timestamp() {
            ts.encode(buf)?;
        }
        if (mask & HAS_SOURCE_PICOSECONDS) != 0 {
            buf.put_u16_le(self.source_picoseconds());
        }
        if let Some(ts) = self.server_timestamp() {
            ts.encode(buf)?;
        }
        if (mask & HAS_SERVER_PICOSECONDS) != 0 {
            buf.put_u16_le(self.server_picoseconds());
        }

        Ok(())
    }

    fn encoded_size(&self) -> usize {
        let mut size = 1; // mask byte
        if let Some(v) = self.value() { size += v.encoded_size(); }
        if self.status().raw() != 0 { size += 4; }
        if self.source_timestamp().is_some() { size += 8; }
        if self.source_picoseconds() != 0 { size += 2; }
        if self.server_timestamp().is_some() { size += 8; }
        if self.server_picoseconds() != 0 { size += 2; }
        size
    }
}

impl BinaryDecodable for DataValue {
    fn decode(buf: &mut Bytes) -> OpcUaResult<Self> {
        let mask = u8::decode(buf)?;

        let value = if (mask & HAS_VALUE) != 0 {
            Some(Variant::decode(buf)?)
        } else {
            None
        };

        let status = if (mask & HAS_STATUS) != 0 {
            StatusCode::from_raw(u32::decode(buf)?)
        } else {
            StatusCode::GOOD
        };

        let source_timestamp = if (mask & HAS_SOURCE_TIMESTAMP) != 0 {
            Some(DateTime::<Utc>::decode(buf)?)
        } else {
            None
        };

        let source_picoseconds = if (mask & HAS_SOURCE_PICOSECONDS) != 0 {
            u16::decode(buf)?
        } else {
            0
        };

        let server_timestamp = if (mask & HAS_SERVER_TIMESTAMP) != 0 {
            Some(DateTime::<Utc>::decode(buf)?)
        } else {
            None
        };

        let server_picoseconds = if (mask & HAS_SERVER_PICOSECONDS) != 0 {
            u16::decode(buf)?
        } else {
            0
        };

        let mut dv = if let Some(value) = value {
            DataValue::with_status(value, status)
        } else {
            let mut dv = DataValue::null();
            dv.set_status(status);
            dv
        };

        // Override timestamps if present in the wire data
        if let Some(ts) = source_timestamp {
            dv.set_source_timestamp(ts);
        }
        if source_picoseconds != 0 {
            dv.set_source_picoseconds(source_picoseconds);
        }
        if let Some(ts) = server_timestamp {
            dv.set_server_timestamp(ts);
        }
        if server_picoseconds != 0 {
            dv.set_server_picoseconds(server_picoseconds);
        }

        Ok(dv)
    }
}

// =========================================================================
// QualifiedName and LocalizedText encoding
// =========================================================================

use crate::nodes::{QualifiedName, LocalizedText};

impl BinaryEncodable for QualifiedName {
    fn encode(&self, buf: &mut BytesMut) -> OpcUaResult<()> {
        self.namespace_index.encode(buf)?;
        self.name.encode(buf)?;
        Ok(())
    }
    fn encoded_size(&self) -> usize { 2 + 4 + self.name.len() }
}

impl BinaryDecodable for QualifiedName {
    fn decode(buf: &mut Bytes) -> OpcUaResult<Self> {
        let namespace_index = u16::decode(buf)?;
        let name = String::decode(buf)?;
        Ok(QualifiedName::new(namespace_index, name))
    }
}

impl BinaryEncodable for LocalizedText {
    fn encode(&self, buf: &mut BytesMut) -> OpcUaResult<()> {
        let mut mask: u8 = 0;
        if !self.locale.is_empty() { mask |= 0x01; }
        if !self.text.is_empty() { mask |= 0x02; }
        buf.put_u8(mask);
        if (mask & 0x01) != 0 { self.locale.encode(buf)?; }
        if (mask & 0x02) != 0 { self.text.encode(buf)?; }
        Ok(())
    }
    fn encoded_size(&self) -> usize {
        let mut size = 1; // mask
        if !self.locale.is_empty() { size += 4 + self.locale.len(); }
        if !self.text.is_empty() { size += 4 + self.text.len(); }
        size
    }
}

impl BinaryDecodable for LocalizedText {
    fn decode(buf: &mut Bytes) -> OpcUaResult<Self> {
        let mask = u8::decode(buf)?;
        let locale = if (mask & 0x01) != 0 { String::decode(buf)? } else { String::new() };
        let text = if (mask & 0x02) != 0 { String::decode(buf)? } else { String::new() };
        Ok(LocalizedText::new(locale, text))
    }
}

// =========================================================================
// StatusCode encoding (it's a newtype around u32)
// =========================================================================

impl BinaryEncodable for StatusCode {
    fn encode(&self, buf: &mut BytesMut) -> OpcUaResult<()> {
        buf.put_u32_le(self.raw());
        Ok(())
    }
    fn encoded_size(&self) -> usize { 4 }
}

impl BinaryDecodable for StatusCode {
    fn decode(buf: &mut Bytes) -> OpcUaResult<Self> {
        Ok(StatusCode::from_raw(u32::decode(buf)?))
    }
}

// =========================================================================
// ExtensionObject encoding — used for service request/response wrapping
// =========================================================================

/// OPC UA ExtensionObject — wraps a service request or response body.
///
/// Encoding: NodeId (type_id) + encoding byte + body
/// - encoding 0x00: no body
/// - encoding 0x01: ByteString body (binary encoded)
/// - encoding 0x02: XmlElement body
#[derive(Debug, Clone)]
pub struct ExtensionObject {
    /// The NodeId of the type being wrapped.
    pub type_id: crate::types::NodeId,
    /// The raw encoded body (binary).
    pub body: Option<Vec<u8>>,
}

impl BinaryEncodable for ExtensionObject {
    fn encode(&self, buf: &mut BytesMut) -> OpcUaResult<()> {
        self.type_id.encode(buf)?;
        match &self.body {
            Some(body) => {
                buf.put_u8(0x01); // Binary body
                buf.put_i32_le(body.len() as i32);
                buf.put_slice(body);
            }
            None => {
                buf.put_u8(0x00); // No body
            }
        }
        Ok(())
    }
    fn encoded_size(&self) -> usize {
        self.type_id.encoded_size() + 1 + match &self.body {
            Some(body) => 4 + body.len(),
            None => 0,
        }
    }
}

impl BinaryDecodable for ExtensionObject {
    fn decode(buf: &mut Bytes) -> OpcUaResult<Self> {
        let type_id = crate::types::NodeId::decode(buf)?;
        let encoding = u8::decode(buf)?;
        let body = match encoding {
            0x00 => None,
            0x01 => {
                let body_bytes = Vec::<u8>::decode(buf)?;
                Some(body_bytes)
            }
            0x02 => {
                // XML body — decode as string, store as bytes
                let xml = String::decode(buf)?;
                Some(xml.into_bytes())
            }
            _ => {
                return Err(crate::error::OpcUaError::Codec(
                    format!("Unknown ExtensionObject encoding: 0x{:02X}", encoding),
                ));
            }
        };
        Ok(ExtensionObject { type_id, body })
    }
}

// =========================================================================
// DiagnosticInfo encoding
// =========================================================================

/// OPC UA DiagnosticInfo.
#[derive(Debug, Clone, Default)]
pub struct DiagnosticInfo {
    pub symbolic_id: Option<i32>,
    pub namespace_uri: Option<i32>,
    pub locale: Option<i32>,
    pub localized_text: Option<i32>,
    pub additional_info: Option<String>,
    pub inner_status_code: Option<StatusCode>,
    pub inner_diagnostic_info: Option<Box<DiagnosticInfo>>,
}

impl BinaryEncodable for DiagnosticInfo {
    fn encode(&self, buf: &mut BytesMut) -> OpcUaResult<()> {
        let mut mask: u8 = 0;
        if self.symbolic_id.is_some() { mask |= 0x01; }
        if self.namespace_uri.is_some() { mask |= 0x02; }
        if self.localized_text.is_some() { mask |= 0x04; }
        if self.locale.is_some() { mask |= 0x08; }
        if self.additional_info.is_some() { mask |= 0x10; }
        if self.inner_status_code.is_some() { mask |= 0x20; }
        if self.inner_diagnostic_info.is_some() { mask |= 0x40; }

        buf.put_u8(mask);
        if let Some(v) = self.symbolic_id { v.encode(buf)?; }
        if let Some(v) = self.namespace_uri { v.encode(buf)?; }
        if let Some(v) = self.localized_text { v.encode(buf)?; }
        if let Some(v) = self.locale { v.encode(buf)?; }
        if let Some(v) = &self.additional_info { v.encode(buf)?; }
        if let Some(v) = &self.inner_status_code { v.encode(buf)?; }
        if let Some(v) = &self.inner_diagnostic_info { v.encode(buf)?; }
        Ok(())
    }
    fn encoded_size(&self) -> usize {
        let mut size = 1;
        if self.symbolic_id.is_some() { size += 4; }
        if self.namespace_uri.is_some() { size += 4; }
        if self.localized_text.is_some() { size += 4; }
        if self.locale.is_some() { size += 4; }
        if let Some(s) = &self.additional_info { size += 4 + s.len(); }
        if self.inner_status_code.is_some() { size += 4; }
        if let Some(d) = &self.inner_diagnostic_info { size += d.encoded_size(); }
        size
    }
}

impl BinaryDecodable for DiagnosticInfo {
    fn decode(buf: &mut Bytes) -> OpcUaResult<Self> {
        let mask = u8::decode(buf)?;
        Ok(DiagnosticInfo {
            symbolic_id: if (mask & 0x01) != 0 { Some(i32::decode(buf)?) } else { None },
            namespace_uri: if (mask & 0x02) != 0 { Some(i32::decode(buf)?) } else { None },
            localized_text: if (mask & 0x04) != 0 { Some(i32::decode(buf)?) } else { None },
            locale: if (mask & 0x08) != 0 { Some(i32::decode(buf)?) } else { None },
            additional_info: if (mask & 0x10) != 0 { Some(String::decode(buf)?) } else { None },
            inner_status_code: if (mask & 0x20) != 0 { Some(StatusCode::decode(buf)?) } else { None },
            inner_diagnostic_info: if (mask & 0x40) != 0 { Some(Box::new(DiagnosticInfo::decode(buf)?)) } else { None },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roundtrip_dv(dv: &DataValue) -> DataValue {
        let mut buf = BytesMut::new();
        dv.encode(&mut buf).unwrap();
        let mut data = buf.freeze();
        DataValue::decode(&mut data).unwrap()
    }

    #[test]
    fn test_null_datavalue() {
        let dv = DataValue::null();
        let result = roundtrip_dv(&dv);
        assert!(result.value().is_none());
    }

    #[test]
    fn test_datavalue_with_value() {
        let dv = DataValue::new(Variant::Double(42.0));
        let result = roundtrip_dv(&dv);
        assert_eq!(result.value(), Some(&Variant::Double(42.0)));
    }

    #[test]
    fn test_extension_object_roundtrip() {
        let eo = ExtensionObject {
            type_id: crate::types::NodeId::numeric(0, 461),
            body: Some(vec![1, 2, 3, 4]),
        };
        let mut buf = BytesMut::new();
        eo.encode(&mut buf).unwrap();
        let mut data = buf.freeze();
        let result = ExtensionObject::decode(&mut data).unwrap();
        assert_eq!(result.type_id, eo.type_id);
        assert_eq!(result.body, eo.body);
    }

    #[test]
    fn test_qualified_name_roundtrip() {
        let qn = QualifiedName::new(2, "Temperature");
        let mut buf = BytesMut::new();
        qn.encode(&mut buf).unwrap();
        let mut data = buf.freeze();
        let result = QualifiedName::decode(&mut data).unwrap();
        assert_eq!(result.namespace_index, 2);
        assert_eq!(result.name, "Temperature");
    }

    #[test]
    fn test_localized_text_roundtrip() {
        let lt = LocalizedText::new("en", "Hello");
        let mut buf = BytesMut::new();
        lt.encode(&mut buf).unwrap();
        let mut data = buf.freeze();
        let result = LocalizedText::decode(&mut data).unwrap();
        assert_eq!(result.locale, "en");
        assert_eq!(result.text, "Hello");
    }

    #[test]
    fn test_diagnostic_info_roundtrip() {
        let di = DiagnosticInfo {
            symbolic_id: Some(1),
            additional_info: Some("test".into()),
            ..Default::default()
        };
        let mut buf = BytesMut::new();
        di.encode(&mut buf).unwrap();
        let mut data = buf.freeze();
        let result = DiagnosticInfo::decode(&mut data).unwrap();
        assert_eq!(result.symbolic_id, Some(1));
        assert_eq!(result.additional_info.as_deref(), Some("test"));
    }
}

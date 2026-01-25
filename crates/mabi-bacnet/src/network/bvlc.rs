//! BVLC (BACnet Virtual Link Control) protocol implementation.
//!
//! BVLC provides the virtual link layer for BACnet/IP communications,
//! encapsulating NPDUs for transmission over UDP.

use bytes::{Buf, BufMut, Bytes, BytesMut};
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};

/// BVLC type identifier (always 0x81 for BACnet/IP).
pub const BVLC_TYPE: u8 = 0x81;

/// Minimum BVLC header size.
pub const BVLC_HEADER_SIZE: usize = 4;

/// BACnet/IP port.
pub const BACNET_IP_PORT: u16 = 47808;

/// BVLC Function codes per ASHRAE 135.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum BvlcFunction {
    /// BVLC-Result
    Result = 0x00,
    /// Write-Broadcast-Distribution-Table
    WriteBroadcastDistributionTable = 0x01,
    /// Read-Broadcast-Distribution-Table
    ReadBroadcastDistributionTable = 0x02,
    /// Read-Broadcast-Distribution-Table-Ack
    ReadBroadcastDistributionTableAck = 0x03,
    /// Forwarded-NPDU
    ForwardedNpdu = 0x04,
    /// Register-Foreign-Device
    RegisterForeignDevice = 0x05,
    /// Read-Foreign-Device-Table
    ReadForeignDeviceTable = 0x06,
    /// Read-Foreign-Device-Table-Ack
    ReadForeignDeviceTableAck = 0x07,
    /// Delete-Foreign-Device-Table-Entry
    DeleteForeignDeviceTableEntry = 0x08,
    /// Distribute-Broadcast-To-Network
    DistributeBroadcastToNetwork = 0x09,
    /// Original-Unicast-NPDU
    OriginalUnicastNpdu = 0x0A,
    /// Original-Broadcast-NPDU
    OriginalBroadcastNpdu = 0x0B,
    /// Secure-BVLL
    SecureBvll = 0x0C,
}

impl BvlcFunction {
    /// Try to create from a raw byte value.
    pub fn from_u8(value: u8) -> Option<Self> {
        match value {
            0x00 => Some(Self::Result),
            0x01 => Some(Self::WriteBroadcastDistributionTable),
            0x02 => Some(Self::ReadBroadcastDistributionTable),
            0x03 => Some(Self::ReadBroadcastDistributionTableAck),
            0x04 => Some(Self::ForwardedNpdu),
            0x05 => Some(Self::RegisterForeignDevice),
            0x06 => Some(Self::ReadForeignDeviceTable),
            0x07 => Some(Self::ReadForeignDeviceTableAck),
            0x08 => Some(Self::DeleteForeignDeviceTableEntry),
            0x09 => Some(Self::DistributeBroadcastToNetwork),
            0x0A => Some(Self::OriginalUnicastNpdu),
            0x0B => Some(Self::OriginalBroadcastNpdu),
            0x0C => Some(Self::SecureBvll),
            _ => None,
        }
    }

    /// Check if this function carries an NPDU.
    pub fn has_npdu(&self) -> bool {
        matches!(
            self,
            Self::ForwardedNpdu | Self::OriginalUnicastNpdu | Self::OriginalBroadcastNpdu
        )
    }

    /// Check if this function is a broadcast type.
    pub fn is_broadcast(&self) -> bool {
        matches!(
            self,
            Self::OriginalBroadcastNpdu | Self::DistributeBroadcastToNetwork
        )
    }
}

impl TryFrom<u8> for BvlcFunction {
    type Error = BvlcError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        Self::from_u8(value).ok_or(BvlcError::UnknownFunction(value))
    }
}

/// BVLC Result codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum BvlcResultCode {
    /// Successful completion
    Success = 0x0000,
    /// Write-Broadcast-Distribution-Table NAK
    WriteBdtNak = 0x0010,
    /// Read-Broadcast-Distribution-Table NAK
    ReadBdtNak = 0x0020,
    /// Register-Foreign-Device NAK
    RegisterForeignDeviceNak = 0x0030,
    /// Read-Foreign-Device-Table NAK
    ReadFdtNak = 0x0040,
    /// Delete-Foreign-Device-Table-Entry NAK
    DeleteFdtEntryNak = 0x0050,
    /// Distribute-Broadcast-To-Network NAK
    DistributeBroadcastNak = 0x0060,
}

impl BvlcResultCode {
    pub fn from_u16(value: u16) -> Option<Self> {
        match value {
            0x0000 => Some(Self::Success),
            0x0010 => Some(Self::WriteBdtNak),
            0x0020 => Some(Self::ReadBdtNak),
            0x0030 => Some(Self::RegisterForeignDeviceNak),
            0x0040 => Some(Self::ReadFdtNak),
            0x0050 => Some(Self::DeleteFdtEntryNak),
            0x0060 => Some(Self::DistributeBroadcastNak),
            _ => None,
        }
    }
}

/// BVLC message header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BvlcHeader {
    /// BVLC function code.
    pub function: BvlcFunction,
    /// Total message length (including header).
    pub length: u16,
}

impl BvlcHeader {
    /// Create a new BVLC header.
    pub fn new(function: BvlcFunction, length: u16) -> Self {
        Self { function, length }
    }

    /// Encode the header to bytes.
    pub fn encode(&self, buf: &mut BytesMut) {
        buf.put_u8(BVLC_TYPE);
        buf.put_u8(self.function as u8);
        buf.put_u16(self.length);
    }

    /// Decode header from bytes.
    pub fn decode(data: &[u8]) -> Result<Self, BvlcError> {
        if data.len() < BVLC_HEADER_SIZE {
            return Err(BvlcError::TooShort {
                expected: BVLC_HEADER_SIZE,
                actual: data.len(),
            });
        }

        let bvlc_type = data[0];
        if bvlc_type != BVLC_TYPE {
            return Err(BvlcError::InvalidType(bvlc_type));
        }

        let function = BvlcFunction::try_from(data[1])?;
        let length = u16::from_be_bytes([data[2], data[3]]);

        Ok(Self { function, length })
    }
}

/// BVLC message containing header and payload.
#[derive(Debug, Clone)]
pub struct BvlcMessage {
    /// Message header.
    pub header: BvlcHeader,
    /// NPDU payload (for messages that carry NPDUs).
    pub npdu: Vec<u8>,
    /// Original source address (for Forwarded-NPDU).
    pub original_source: Option<SocketAddrV4>,
    /// Result code (for BVLC-Result messages).
    pub result_code: Option<BvlcResultCode>,
}

impl BvlcMessage {
    /// Create a new Original-Unicast-NPDU message.
    pub fn original_unicast(npdu: Vec<u8>) -> Self {
        let length = (BVLC_HEADER_SIZE + npdu.len()) as u16;
        Self {
            header: BvlcHeader::new(BvlcFunction::OriginalUnicastNpdu, length),
            npdu,
            original_source: None,
            result_code: None,
        }
    }

    /// Create a new Original-Broadcast-NPDU message.
    pub fn original_broadcast(npdu: Vec<u8>) -> Self {
        let length = (BVLC_HEADER_SIZE + npdu.len()) as u16;
        Self {
            header: BvlcHeader::new(BvlcFunction::OriginalBroadcastNpdu, length),
            npdu,
            original_source: None,
            result_code: None,
        }
    }

    /// Create a new Forwarded-NPDU message.
    pub fn forwarded_npdu(npdu: Vec<u8>, original_source: SocketAddrV4) -> Self {
        // Forwarded-NPDU includes 6 bytes for original address (4 IP + 2 port)
        let length = (BVLC_HEADER_SIZE + 6 + npdu.len()) as u16;
        Self {
            header: BvlcHeader::new(BvlcFunction::ForwardedNpdu, length),
            npdu,
            original_source: Some(original_source),
            result_code: None,
        }
    }

    /// Create a BVLC-Result message.
    pub fn result(code: BvlcResultCode) -> Self {
        Self {
            header: BvlcHeader::new(BvlcFunction::Result, 6), // 4 header + 2 result code
            npdu: Vec::new(),
            original_source: None,
            result_code: Some(code),
        }
    }

    /// Encode the message to bytes.
    pub fn encode(&self) -> Bytes {
        let mut buf = BytesMut::with_capacity(self.header.length as usize);

        self.header.encode(&mut buf);

        match self.header.function {
            BvlcFunction::Result => {
                if let Some(code) = self.result_code {
                    buf.put_u16(code as u16);
                }
            }
            BvlcFunction::ForwardedNpdu => {
                if let Some(addr) = &self.original_source {
                    buf.put_slice(&addr.ip().octets());
                    buf.put_u16(addr.port());
                }
                buf.put_slice(&self.npdu);
            }
            _ => {
                buf.put_slice(&self.npdu);
            }
        }

        buf.freeze()
    }

    /// Decode a message from bytes.
    pub fn decode(data: &[u8]) -> Result<Self, BvlcError> {
        let header = BvlcHeader::decode(data)?;

        if data.len() < header.length as usize {
            return Err(BvlcError::TooShort {
                expected: header.length as usize,
                actual: data.len(),
            });
        }

        let mut cursor = &data[BVLC_HEADER_SIZE..];

        match header.function {
            BvlcFunction::Result => {
                if cursor.len() < 2 {
                    return Err(BvlcError::TooShort {
                        expected: 2,
                        actual: cursor.len(),
                    });
                }
                let code_value = cursor.get_u16();
                let result_code = BvlcResultCode::from_u16(code_value);

                Ok(Self {
                    header,
                    npdu: Vec::new(),
                    original_source: None,
                    result_code,
                })
            }
            BvlcFunction::ForwardedNpdu => {
                if cursor.len() < 6 {
                    return Err(BvlcError::TooShort {
                        expected: 6,
                        actual: cursor.len(),
                    });
                }

                let ip = Ipv4Addr::new(
                    cursor.get_u8(),
                    cursor.get_u8(),
                    cursor.get_u8(),
                    cursor.get_u8(),
                );
                let port = cursor.get_u16();
                let original_source = SocketAddrV4::new(ip, port);

                Ok(Self {
                    header,
                    npdu: cursor.to_vec(),
                    original_source: Some(original_source),
                    result_code: None,
                })
            }
            BvlcFunction::OriginalUnicastNpdu | BvlcFunction::OriginalBroadcastNpdu => Ok(Self {
                header,
                npdu: cursor.to_vec(),
                original_source: None,
                result_code: None,
            }),
            _ => Ok(Self {
                header,
                npdu: cursor.to_vec(),
                original_source: None,
                result_code: None,
            }),
        }
    }

    /// Get the NPDU payload if present.
    pub fn npdu(&self) -> Option<&[u8]> {
        if self.header.function.has_npdu() && !self.npdu.is_empty() {
            Some(&self.npdu)
        } else {
            None
        }
    }

    /// Check if this is a broadcast message.
    pub fn is_broadcast(&self) -> bool {
        self.header.function.is_broadcast()
    }

    /// Get the effective source address (original source for forwarded, or None).
    pub fn effective_source(&self) -> Option<SocketAddr> {
        self.original_source.map(SocketAddr::V4)
    }
}

/// BVLC protocol errors.
#[derive(Debug, thiserror::Error)]
pub enum BvlcError {
    #[error("Message too short: expected {expected}, got {actual}")]
    TooShort { expected: usize, actual: usize },

    #[error("Invalid BVLC type: 0x{0:02X} (expected 0x81)")]
    InvalidType(u8),

    #[error("Unknown BVLC function: 0x{0:02X}")]
    UnknownFunction(u8),

    #[error("Invalid message format")]
    InvalidFormat,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bvlc_function_from_u8() {
        assert_eq!(
            BvlcFunction::from_u8(0x0A),
            Some(BvlcFunction::OriginalUnicastNpdu)
        );
        assert_eq!(
            BvlcFunction::from_u8(0x0B),
            Some(BvlcFunction::OriginalBroadcastNpdu)
        );
        assert_eq!(BvlcFunction::from_u8(0xFF), None);
    }

    #[test]
    fn test_bvlc_header_encode_decode() {
        let header = BvlcHeader::new(BvlcFunction::OriginalUnicastNpdu, 10);
        let mut buf = BytesMut::new();
        header.encode(&mut buf);

        let decoded = BvlcHeader::decode(&buf).unwrap();
        assert_eq!(decoded.function, BvlcFunction::OriginalUnicastNpdu);
        assert_eq!(decoded.length, 10);
    }

    #[test]
    fn test_original_unicast_encode_decode() {
        let npdu = vec![0x01, 0x04, 0x00, 0x05]; // Simple NPDU
        let msg = BvlcMessage::original_unicast(npdu.clone());

        let encoded = msg.encode();
        let decoded = BvlcMessage::decode(&encoded).unwrap();

        assert_eq!(decoded.header.function, BvlcFunction::OriginalUnicastNpdu);
        assert_eq!(decoded.npdu, npdu);
    }

    #[test]
    fn test_original_broadcast_encode_decode() {
        let npdu = vec![0x01, 0x04, 0x00, 0x05];
        let msg = BvlcMessage::original_broadcast(npdu.clone());

        let encoded = msg.encode();
        let decoded = BvlcMessage::decode(&encoded).unwrap();

        assert_eq!(decoded.header.function, BvlcFunction::OriginalBroadcastNpdu);
        assert!(decoded.is_broadcast());
    }

    #[test]
    fn test_forwarded_npdu_encode_decode() {
        let npdu = vec![0x01, 0x04, 0x00, 0x05];
        let source = SocketAddrV4::new(Ipv4Addr::new(192, 168, 1, 100), 47808);
        let msg = BvlcMessage::forwarded_npdu(npdu.clone(), source);

        let encoded = msg.encode();
        let decoded = BvlcMessage::decode(&encoded).unwrap();

        assert_eq!(decoded.header.function, BvlcFunction::ForwardedNpdu);
        assert_eq!(decoded.original_source, Some(source));
        assert_eq!(decoded.npdu, npdu);
    }

    #[test]
    fn test_bvlc_result_encode_decode() {
        let msg = BvlcMessage::result(BvlcResultCode::Success);

        let encoded = msg.encode();
        let decoded = BvlcMessage::decode(&encoded).unwrap();

        assert_eq!(decoded.header.function, BvlcFunction::Result);
        assert_eq!(decoded.result_code, Some(BvlcResultCode::Success));
    }
}

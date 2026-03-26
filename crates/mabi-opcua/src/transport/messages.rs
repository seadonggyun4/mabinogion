//! OPC UA Transport-level message types.
//!
//! OPC UA Part 6, Section 7.1 — Transport layer messages:
//! - HEL (Hello): Client initiates connection
//! - ACK (Acknowledge): Server responds to Hello
//! - ERR (Error): Error notification

use bytes::{BufMut, Bytes, BytesMut};

use crate::codec::decoder::BinaryDecodable;
use crate::codec::encoder::BinaryEncodable;
use crate::error::{OpcUaError, OpcUaResult};

/// Default protocol version.
pub const PROTOCOL_VERSION: u32 = 0;

/// Default receive buffer size (64 KB).
pub const DEFAULT_BUFFER_SIZE: u32 = 65535;

/// Default max message size (16 MB).
pub const DEFAULT_MAX_MESSAGE_SIZE: u32 = 16 * 1024 * 1024;

/// Default max chunk count.
pub const DEFAULT_MAX_CHUNK_COUNT: u32 = 5000;

/// OPC UA message type identifiers (3-byte ASCII).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageType {
    /// Hello message from client.
    Hello,
    /// Acknowledge message from server.
    Acknowledge,
    /// Error message.
    Error,
    /// OpenSecureChannel request/response.
    OpenSecureChannel,
    /// CloseSecureChannel request.
    CloseSecureChannel,
    /// Service message (request/response).
    Message,
}

impl MessageType {
    /// Get the 3-byte ASCII identifier.
    pub fn bytes(&self) -> &[u8; 3] {
        match self {
            Self::Hello => b"HEL",
            Self::Acknowledge => b"ACK",
            Self::Error => b"ERR",
            Self::OpenSecureChannel => b"OPN",
            Self::CloseSecureChannel => b"CLO",
            Self::Message => b"MSG",
        }
    }

    /// Parse from 3-byte ASCII.
    pub fn from_bytes(bytes: &[u8]) -> OpcUaResult<Self> {
        if bytes.len() < 3 {
            return Err(OpcUaError::Codec("Message type must be 3 bytes".into()));
        }
        match &bytes[..3] {
            b"HEL" => Ok(Self::Hello),
            b"ACK" => Ok(Self::Acknowledge),
            b"ERR" => Ok(Self::Error),
            b"OPN" => Ok(Self::OpenSecureChannel),
            b"CLO" => Ok(Self::CloseSecureChannel),
            b"MSG" => Ok(Self::Message),
            _ => Err(OpcUaError::Codec(format!(
                "Unknown message type: {:?}",
                std::str::from_utf8(&bytes[..3]).unwrap_or("???")
            ))),
        }
    }
}

/// Chunk type — indicates whether a message is final, intermediate, or aborted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChunkType {
    /// Final chunk (or only chunk).
    Final,
    /// Intermediate chunk.
    Intermediate,
    /// Abort — discard all chunks for this message.
    Abort,
}

impl ChunkType {
    pub fn byte(&self) -> u8 {
        match self {
            Self::Final => b'F',
            Self::Intermediate => b'C',
            Self::Abort => b'A',
        }
    }

    pub fn from_byte(b: u8) -> OpcUaResult<Self> {
        match b {
            b'F' => Ok(Self::Final),
            b'C' => Ok(Self::Intermediate),
            b'A' => Ok(Self::Abort),
            _ => Err(OpcUaError::Codec(format!(
                "Unknown chunk type: 0x{:02X}",
                b
            ))),
        }
    }
}

/// Message header — common to all OPC UA binary messages.
///
/// Layout: `[message_type: 3 bytes][chunk_type: 1 byte][message_size: u32]`
#[derive(Debug, Clone)]
pub struct MessageHeader {
    pub message_type: MessageType,
    pub chunk_type: ChunkType,
    pub message_size: u32,
}

impl MessageHeader {
    /// Header size in bytes.
    pub const SIZE: usize = 8;

    pub fn new(message_type: MessageType, chunk_type: ChunkType, message_size: u32) -> Self {
        Self {
            message_type,
            chunk_type,
            message_size,
        }
    }

    pub fn encode(&self, buf: &mut BytesMut) {
        buf.put_slice(self.message_type.bytes());
        buf.put_u8(self.chunk_type.byte());
        buf.put_u32_le(self.message_size);
    }

    pub fn decode(buf: &[u8]) -> OpcUaResult<Self> {
        if buf.len() < Self::SIZE {
            return Err(OpcUaError::Codec(
                "Not enough bytes for message header".into(),
            ));
        }
        let message_type = MessageType::from_bytes(&buf[..3])?;
        let chunk_type = ChunkType::from_byte(buf[3])?;
        let message_size = u32::from_le_bytes([buf[4], buf[5], buf[6], buf[7]]);
        Ok(Self {
            message_type,
            chunk_type,
            message_size,
        })
    }
}

// =========================================================================
// Hello Message (HEL)
// =========================================================================

/// Hello message sent by the client to initiate the connection.
#[derive(Debug, Clone)]
pub struct HelloMessage {
    pub protocol_version: u32,
    pub receive_buffer_size: u32,
    pub send_buffer_size: u32,
    pub max_message_size: u32,
    pub max_chunk_count: u32,
    pub endpoint_url: String,
}

impl Default for HelloMessage {
    fn default() -> Self {
        Self {
            protocol_version: PROTOCOL_VERSION,
            receive_buffer_size: DEFAULT_BUFFER_SIZE,
            send_buffer_size: DEFAULT_BUFFER_SIZE,
            max_message_size: DEFAULT_MAX_MESSAGE_SIZE,
            max_chunk_count: DEFAULT_MAX_CHUNK_COUNT,
            endpoint_url: String::new(),
        }
    }
}

impl BinaryEncodable for HelloMessage {
    fn encode(&self, buf: &mut BytesMut) -> OpcUaResult<()> {
        self.protocol_version.encode(buf)?;
        self.receive_buffer_size.encode(buf)?;
        self.send_buffer_size.encode(buf)?;
        self.max_message_size.encode(buf)?;
        self.max_chunk_count.encode(buf)?;
        self.endpoint_url.encode(buf)?;
        Ok(())
    }
    fn encoded_size(&self) -> usize {
        20 + 4 + self.endpoint_url.len()
    }
}

impl BinaryDecodable for HelloMessage {
    fn decode(buf: &mut Bytes) -> OpcUaResult<Self> {
        Ok(Self {
            protocol_version: u32::decode(buf)?,
            receive_buffer_size: u32::decode(buf)?,
            send_buffer_size: u32::decode(buf)?,
            max_message_size: u32::decode(buf)?,
            max_chunk_count: u32::decode(buf)?,
            endpoint_url: String::decode(buf)?,
        })
    }
}

// =========================================================================
// Acknowledge Message (ACK)
// =========================================================================

/// Acknowledge message sent by the server in response to Hello.
#[derive(Debug, Clone)]
pub struct AcknowledgeMessage {
    pub protocol_version: u32,
    pub receive_buffer_size: u32,
    pub send_buffer_size: u32,
    pub max_message_size: u32,
    pub max_chunk_count: u32,
}

impl AcknowledgeMessage {
    /// Create an ACK by negotiating with the client's Hello.
    pub fn from_hello(hello: &HelloMessage, server_buffer_size: u32) -> Self {
        Self {
            protocol_version: PROTOCOL_VERSION,
            receive_buffer_size: hello.send_buffer_size.min(server_buffer_size),
            send_buffer_size: hello.receive_buffer_size.min(server_buffer_size),
            max_message_size: if hello.max_message_size == 0 {
                DEFAULT_MAX_MESSAGE_SIZE
            } else {
                hello.max_message_size.min(DEFAULT_MAX_MESSAGE_SIZE)
            },
            max_chunk_count: if hello.max_chunk_count == 0 {
                DEFAULT_MAX_CHUNK_COUNT
            } else {
                hello.max_chunk_count.min(DEFAULT_MAX_CHUNK_COUNT)
            },
        }
    }
}

impl BinaryEncodable for AcknowledgeMessage {
    fn encode(&self, buf: &mut BytesMut) -> OpcUaResult<()> {
        self.protocol_version.encode(buf)?;
        self.receive_buffer_size.encode(buf)?;
        self.send_buffer_size.encode(buf)?;
        self.max_message_size.encode(buf)?;
        self.max_chunk_count.encode(buf)?;
        Ok(())
    }
    fn encoded_size(&self) -> usize {
        20
    }
}

impl BinaryDecodable for AcknowledgeMessage {
    fn decode(buf: &mut Bytes) -> OpcUaResult<Self> {
        Ok(Self {
            protocol_version: u32::decode(buf)?,
            receive_buffer_size: u32::decode(buf)?,
            send_buffer_size: u32::decode(buf)?,
            max_message_size: u32::decode(buf)?,
            max_chunk_count: u32::decode(buf)?,
        })
    }
}

// =========================================================================
// Error Message (ERR)
// =========================================================================

/// Error message indicating a transport-level error.
#[derive(Debug, Clone)]
pub struct ErrorMessage {
    pub error: u32,
    pub reason: String,
}

impl BinaryEncodable for ErrorMessage {
    fn encode(&self, buf: &mut BytesMut) -> OpcUaResult<()> {
        self.error.encode(buf)?;
        self.reason.encode(buf)?;
        Ok(())
    }
    fn encoded_size(&self) -> usize {
        4 + 4 + self.reason.len()
    }
}

impl BinaryDecodable for ErrorMessage {
    fn decode(buf: &mut Bytes) -> OpcUaResult<Self> {
        Ok(Self {
            error: u32::decode(buf)?,
            reason: String::decode(buf)?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_message_header_roundtrip() {
        let header = MessageHeader::new(MessageType::Hello, ChunkType::Final, 100);
        let mut buf = BytesMut::new();
        header.encode(&mut buf);
        let decoded = MessageHeader::decode(&buf).unwrap();
        assert_eq!(decoded.message_type, MessageType::Hello);
        assert_eq!(decoded.chunk_type, ChunkType::Final);
        assert_eq!(decoded.message_size, 100);
    }

    #[test]
    fn test_hello_roundtrip() {
        let hello = HelloMessage {
            endpoint_url: "opc.tcp://localhost:4840".into(),
            ..Default::default()
        };
        let mut buf = BytesMut::new();
        hello.encode(&mut buf).unwrap();
        let mut data = buf.freeze();
        let decoded = HelloMessage::decode(&mut data).unwrap();
        assert_eq!(decoded.endpoint_url, "opc.tcp://localhost:4840");
        assert_eq!(decoded.protocol_version, PROTOCOL_VERSION);
    }

    #[test]
    fn test_ack_from_hello() {
        let hello = HelloMessage {
            receive_buffer_size: 32768,
            send_buffer_size: 32768,
            ..Default::default()
        };
        let ack = AcknowledgeMessage::from_hello(&hello, 65535);
        assert_eq!(ack.receive_buffer_size, 32768);
        assert_eq!(ack.send_buffer_size, 32768);
    }
}

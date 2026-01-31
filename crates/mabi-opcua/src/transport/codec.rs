//! OPC UA Transport framing codec for tokio-util.
//!
//! Implements `Decoder` and `Encoder` for reading/writing OPC UA messages
//! from a TCP stream using `Framed<TcpStream, OpcUaTransportCodec>`.

use bytes::{Buf, BytesMut};
use tokio_util::codec::{Decoder, Encoder};

use crate::error::{OpcUaError, OpcUaResult};
use crate::transport::messages::{MessageHeader, MessageType, DEFAULT_BUFFER_SIZE, DEFAULT_MAX_MESSAGE_SIZE};

/// A raw OPC UA message frame read from the wire.
#[derive(Debug, Clone)]
pub struct RawMessage {
    /// Parsed message header.
    pub header: MessageHeader,
    /// Raw body bytes (everything after the 8-byte header).
    pub body: Vec<u8>,
}

/// OPC UA transport-level framing codec.
///
/// Reads messages by parsing the 8-byte header to determine the total size,
/// then reads the remaining body bytes.
pub struct OpcUaTransportCodec {
    /// Maximum allowed receive buffer size (negotiated via Hello/Acknowledge).
    max_receive_buffer: u32,
    /// Maximum allowed message size.
    max_message_size: u32,
}

impl OpcUaTransportCodec {
    /// Create a new codec with default limits.
    pub fn new() -> Self {
        Self {
            max_receive_buffer: DEFAULT_BUFFER_SIZE,
            max_message_size: DEFAULT_MAX_MESSAGE_SIZE,
        }
    }

    /// Create a codec with negotiated buffer sizes.
    pub fn with_limits(max_receive_buffer: u32, max_message_size: u32) -> Self {
        Self { max_receive_buffer, max_message_size }
    }

    /// Update limits after Hello/Acknowledge negotiation.
    pub fn set_limits(&mut self, max_receive_buffer: u32, max_message_size: u32) {
        self.max_receive_buffer = max_receive_buffer;
        self.max_message_size = max_message_size;
    }
}

impl Default for OpcUaTransportCodec {
    fn default() -> Self {
        Self::new()
    }
}

impl Decoder for OpcUaTransportCodec {
    type Item = RawMessage;
    type Error = OpcUaError;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        // Need at least 8 bytes for the header
        if src.len() < MessageHeader::SIZE {
            return Ok(None);
        }

        // Peek at the header without consuming
        let header = MessageHeader::decode(&src[..MessageHeader::SIZE])?;
        let total_size = header.message_size as usize;

        // Validate message size
        if total_size < MessageHeader::SIZE {
            return Err(OpcUaError::Codec(format!(
                "Message size {} is less than header size",
                total_size,
            )));
        }
        if total_size > self.max_message_size as usize {
            return Err(OpcUaError::MessageTooLarge {
                size: total_size,
                max: self.max_message_size as usize,
            });
        }

        // Wait for full message
        if src.len() < total_size {
            // Reserve space for the rest
            src.reserve(total_size - src.len());
            return Ok(None);
        }

        // Consume the header
        src.advance(MessageHeader::SIZE);

        // Consume the body
        let body_len = total_size - MessageHeader::SIZE;
        let body = src.split_to(body_len).to_vec();

        Ok(Some(RawMessage { header, body }))
    }
}

impl Encoder<RawMessage> for OpcUaTransportCodec {
    type Error = OpcUaError;

    fn encode(&mut self, item: RawMessage, dst: &mut BytesMut) -> Result<(), Self::Error> {
        let total_size = MessageHeader::SIZE + item.body.len();
        dst.reserve(total_size);

        let header = MessageHeader::new(
            item.header.message_type,
            item.header.chunk_type,
            total_size as u32,
        );
        header.encode(dst);
        dst.extend_from_slice(&item.body);
        Ok(())
    }
}

/// Helper to build a response `RawMessage`.
pub fn build_response(msg_type: MessageType, body: Vec<u8>) -> RawMessage {
    use crate::transport::messages::ChunkType;
    RawMessage {
        header: MessageHeader::new(msg_type, ChunkType::Final, (MessageHeader::SIZE + body.len()) as u32),
        body,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::messages::{ChunkType, MessageType};

    #[test]
    fn test_codec_roundtrip() {
        let body = vec![1, 2, 3, 4, 5];
        let total_size = MessageHeader::SIZE + body.len();
        let msg = RawMessage {
            header: MessageHeader::new(MessageType::Message, ChunkType::Final, total_size as u32),
            body: body.clone(),
        };

        let mut codec = OpcUaTransportCodec::new();
        let mut buf = BytesMut::new();
        codec.encode(msg, &mut buf).unwrap();

        let decoded = codec.decode(&mut buf).unwrap().unwrap();
        assert_eq!(decoded.header.message_type, MessageType::Message);
        assert_eq!(decoded.body, body);
    }

    #[test]
    fn test_partial_message() {
        let mut codec = OpcUaTransportCodec::new();
        let mut buf = BytesMut::from(&[0x4D, 0x53, 0x47, 0x46][..]); // "MSGF" only 4 bytes
        assert!(codec.decode(&mut buf).unwrap().is_none());
    }

    #[test]
    fn test_message_too_large() {
        let mut codec = OpcUaTransportCodec::with_limits(1024, 1024);
        let mut buf = BytesMut::new();
        // Write header with message_size = 2048
        let header = MessageHeader::new(MessageType::Message, ChunkType::Final, 2048);
        header.encode(&mut buf);
        // Fill enough bytes
        buf.extend_from_slice(&vec![0u8; 2048 - MessageHeader::SIZE]);

        let result = codec.decode(&mut buf);
        assert!(matches!(result, Err(OpcUaError::MessageTooLarge { .. })));
    }
}

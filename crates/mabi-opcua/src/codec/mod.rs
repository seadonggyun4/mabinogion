//! OPC UA Binary Encoding/Decoding.
//!
//! Implements the OPC UA Binary Encoding as defined in OPC UA Part 6, Section 5.2.
//! All multi-byte values use little-endian byte order.

pub mod encoder;
pub mod decoder;
pub mod node_id;
pub mod variant;
pub mod data_value;

pub use encoder::BinaryEncodable;
pub use decoder::BinaryDecodable;

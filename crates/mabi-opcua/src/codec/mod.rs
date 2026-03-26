//! OPC UA Binary Encoding/Decoding.
//!
//! Implements the OPC UA Binary Encoding as defined in OPC UA Part 6, Section 5.2.
//! All multi-byte values use little-endian byte order.

pub mod data_value;
pub mod decoder;
pub mod encoder;
pub mod node_id;
pub mod variant;

pub use decoder::BinaryDecodable;
pub use encoder::BinaryEncodable;

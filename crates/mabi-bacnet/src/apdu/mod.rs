//! APDU (Application Protocol Data Unit) encoding and decoding.
//!
//! This module provides APDU types and encoding for BACnet application layer.
//!
//! ## Features
//!
//! - **Encoding/Decoding**: TLV-based encoding for all BACnet data types
//! - **Service Types**: Confirmed and unconfirmed service definitions
//! - **Segmentation**: Support for large message segmentation and reassembly
//! - **Error Handling**: Comprehensive error types

pub mod encoding;
pub mod segmentation;
pub mod types;

pub use encoding::{ApduDecoder, ApduEncoder, TagNumber};
pub use segmentation::{
    decode_max_segments, encode_max_segments, AssemblyResult, Segment, SegmentAssembler,
    SegmentHeader, SegmentTransmitter, SegmentationError, SegmentationSupport,
};
pub use types::{
    ApduError, ApduType, ConfirmedService, ErrorClass, ErrorCode, RejectReason, UnconfirmedService,
};

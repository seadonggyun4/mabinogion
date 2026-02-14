//! AtomicReadFile and AtomicWriteFile service handlers.
//!
//! Per ASHRAE 135, Clause 15.1 and 15.2:
//!
//! - **AtomicReadFile**: Reads data from a File object atomically.
//!   Supports both stream access (byte range) and record access (record range).
//!
//! - **AtomicWriteFile**: Writes data to a File object atomically.
//!   Supports both stream access and record access.
//!
//! ## APDU Encoding
//!
//! ### AtomicReadFile-Request
//! ```text
//! ObjectIdentifier    [0] BACnetObjectIdentifier
//! Stream-Access       [0] SEQUENCE { fileStartPosition INTEGER, requestedOctetCount Unsigned }
//!   -- OR --
//! Record-Access       [1] SEQUENCE { fileStartRecord INTEGER, requestedRecordCount Unsigned }
//! ```
//!
//! ### AtomicReadFile-ACK
//! ```text
//! endOfFile            BOOLEAN
//! Stream-Access       [0] SEQUENCE { fileStartPosition INTEGER, fileData OCTET STRING }
//!   -- OR --
//! Record-Access       [1] SEQUENCE { fileStartRecord INTEGER, returnedRecordCount Unsigned,
//!                                    fileRecordData SEQUENCE OF OCTET STRING }
//! ```
//!
//! ### AtomicWriteFile-Request
//! ```text
//! ObjectIdentifier    [0] BACnetObjectIdentifier
//! Stream-Access       [0] SEQUENCE { fileStartPosition INTEGER, fileData OCTET STRING }
//!   -- OR --
//! Record-Access       [1] SEQUENCE { fileStartRecord INTEGER, recordCount Unsigned,
//!                                    fileRecordData SEQUENCE OF OCTET STRING }
//! ```
//!
//! ### AtomicWriteFile-ACK
//! ```text
//! Stream-Access       [0] fileStartPosition INTEGER
//!   -- OR --
//! Record-Access       [1] fileStartRecord INTEGER
//! ```

use std::sync::Arc;

use crate::apdu::encoding::{ApduDecoder, ApduEncoder};
use crate::apdu::types::{ConfirmedService, ErrorClass, ErrorCode};
use crate::object::file::{FileAccessMethod, FileObject};
use crate::object::types::ObjectType;

use super::handler::{ConfirmedServiceHandler, ServiceContext, ServiceResult};

// ============================================================================
// AtomicReadFile
// ============================================================================

/// Decoded AtomicReadFile-Request.
#[derive(Debug)]
pub struct AtomicReadFileRequest {
    /// Object identifier of the File object.
    pub object_type: u16,
    pub instance: u32,
    /// Access type: stream or record.
    pub access: ReadFileAccess,
}

/// Read file access type.
#[derive(Debug)]
pub enum ReadFileAccess {
    /// Stream access: start position (signed) and requested octet count.
    Stream {
        file_start_position: i32,
        requested_octet_count: u32,
    },
    /// Record access: start record (signed) and requested record count.
    Record {
        file_start_record: i32,
        requested_record_count: u32,
    },
}

impl AtomicReadFileRequest {
    /// Decode from APDU service data.
    pub fn decode(data: &[u8]) -> Result<Self, &'static str> {
        let mut decoder = ApduDecoder::new(data);

        // 1. Object Identifier (application tag)
        let (tag_num, is_context, len) = decoder
            .decode_tag_info()
            .map_err(|_| "Failed to decode object identifier tag")?;

        if is_context {
            return Err("Expected application tag for object identifier");
        }
        if tag_num != 12 {
            // Application tag 12 = ObjectIdentifier
            return Err("Expected ObjectIdentifier tag");
        }
        if len != 4 {
            return Err("Invalid ObjectIdentifier length");
        }

        let obj_id = decoder
            .decode_object_identifier()
            .map_err(|_| "Failed to decode object identifier")?;

        // 2. Access choice: context tag [0] for stream, [1] for record
        // Peek at the opening tag
        let peek = decoder.peek().ok_or("No access choice data")?;
        let access_tag = (peek >> 4) & 0x0F;
        let is_opening = (peek & 0x0F) == 0x0E;

        if !is_opening {
            return Err("Expected opening tag for access choice");
        }

        // Skip the opening tag
        decoder.skip(1).map_err(|_| "Failed to skip opening tag")?;

        let access = match access_tag {
            0 => {
                // Stream access
                // fileStartPosition: context [0] signed integer
                let (_, _, len) = decoder
                    .decode_tag_info()
                    .map_err(|_| "Failed to decode start position tag")?;
                let start = decoder
                    .decode_signed(len)
                    .map_err(|_| "Failed to decode start position")?;

                // requestedOctetCount: context [1] unsigned
                let (_, _, len) = decoder
                    .decode_tag_info()
                    .map_err(|_| "Failed to decode octet count tag")?;
                let count = decoder
                    .decode_unsigned(len)
                    .map_err(|_| "Failed to decode octet count")?;

                ReadFileAccess::Stream {
                    file_start_position: start,
                    requested_octet_count: count,
                }
            }
            1 => {
                // Record access
                // fileStartRecord: signed integer
                let (_, _, len) = decoder
                    .decode_tag_info()
                    .map_err(|_| "Failed to decode start record tag")?;
                let start = decoder
                    .decode_signed(len)
                    .map_err(|_| "Failed to decode start record")?;

                // requestedRecordCount: unsigned
                let (_, _, len) = decoder
                    .decode_tag_info()
                    .map_err(|_| "Failed to decode record count tag")?;
                let count = decoder
                    .decode_unsigned(len)
                    .map_err(|_| "Failed to decode record count")?;

                ReadFileAccess::Record {
                    file_start_record: start,
                    requested_record_count: count,
                }
            }
            _ => return Err("Unknown access choice"),
        };

        // Skip closing tag
        if !decoder.is_empty() {
            decoder.skip(1).ok();
        }

        Ok(Self {
            object_type: obj_id.object_type as u16,
            instance: obj_id.instance,
            access,
        })
    }
}

/// AtomicReadFile service handler.
pub struct AtomicReadFileHandler;

impl AtomicReadFileHandler {
    pub fn new() -> Self {
        Self
    }
}

impl ConfirmedServiceHandler for AtomicReadFileHandler {
    fn service_choice(&self) -> ConfirmedService {
        ConfirmedService::AtomicReadFile
    }

    fn handle(&self, data: &[u8], ctx: &ServiceContext) -> ServiceResult {
        // Decode request
        let request = match AtomicReadFileRequest::decode(data) {
            Ok(req) => req,
            Err(_) => return ServiceResult::invalid_tag(),
        };

        // Verify it's a File object
        if request.object_type != ObjectType::File as u16 {
            return ServiceResult::Error {
                error_class: ErrorClass::Object,
                error_code: ErrorCode::UnsupportedObjectType,
            };
        }

        // Lookup the object
        let obj_id = crate::object::types::ObjectId::new(ObjectType::File, request.instance);
        let obj = match ctx.objects.get(&obj_id) {
            Some(o) => o,
            None => return ServiceResult::unknown_object(),
        };

        // Downcast to FileObject
        let file_obj = match downcast_file_object(&obj) {
            Some(f) => f,
            None => {
                return ServiceResult::Error {
                    error_class: ErrorClass::Object,
                    error_code: ErrorCode::UnsupportedObjectType,
                };
            }
        };

        // Verify access method matches
        match (&request.access, file_obj.access_method()) {
            (ReadFileAccess::Stream { .. }, FileAccessMethod::StreamAccess) => {}
            (ReadFileAccess::Record { .. }, FileAccessMethod::RecordAccess) => {}
            _ => {
                return ServiceResult::Error {
                    error_class: ErrorClass::Services,
                    error_code: ErrorCode::InvalidFileAccessMethod,
                };
            }
        }

        // Perform the read
        match request.access {
            ReadFileAccess::Stream {
                file_start_position,
                requested_octet_count,
            } => {
                let (data, eof) = file_obj.read_stream(file_start_position, requested_octet_count);
                let response = encode_read_file_stream_ack(eof, file_start_position, &data);
                ServiceResult::ComplexAck(response)
            }
            ReadFileAccess::Record {
                file_start_record,
                requested_record_count,
            } => {
                let (records, eof) =
                    file_obj.read_records(file_start_record, requested_record_count as i32);
                let response =
                    encode_read_file_record_ack(eof, file_start_record, &records);
                ServiceResult::ComplexAck(response)
            }
        }
    }

    fn name(&self) -> &'static str {
        "AtomicReadFile"
    }

    fn min_data_length(&self) -> usize {
        6 // ObjectIdentifier (4+tag) + minimal access
    }
}

// ============================================================================
// AtomicWriteFile
// ============================================================================

/// Decoded AtomicWriteFile-Request.
#[derive(Debug)]
pub struct AtomicWriteFileRequest {
    /// Object identifier of the File object.
    pub object_type: u16,
    pub instance: u32,
    /// Access type: stream or record.
    pub access: WriteFileAccess,
}

/// Write file access type.
#[derive(Debug)]
pub enum WriteFileAccess {
    /// Stream access: start position and data.
    Stream {
        file_start_position: i32,
        file_data: Vec<u8>,
    },
    /// Record access: start record and records.
    Record {
        file_start_record: i32,
        file_record_data: Vec<Vec<u8>>,
    },
}

impl AtomicWriteFileRequest {
    /// Decode from APDU service data.
    pub fn decode(data: &[u8]) -> Result<Self, &'static str> {
        let mut decoder = ApduDecoder::new(data);

        // 1. Object Identifier (application tag)
        let (tag_num, is_context, len) = decoder
            .decode_tag_info()
            .map_err(|_| "Failed to decode object identifier tag")?;

        if is_context {
            return Err("Expected application tag for object identifier");
        }
        if tag_num != 12 {
            return Err("Expected ObjectIdentifier tag");
        }
        if len != 4 {
            return Err("Invalid ObjectIdentifier length");
        }

        let obj_id = decoder
            .decode_object_identifier()
            .map_err(|_| "Failed to decode object identifier")?;

        // 2. Access choice: context tag [0] for stream, [1] for record
        let peek = decoder.peek().ok_or("No access choice data")?;
        let access_tag = (peek >> 4) & 0x0F;
        let is_opening = (peek & 0x0F) == 0x0E;

        if !is_opening {
            return Err("Expected opening tag for access choice");
        }

        decoder.skip(1).map_err(|_| "Failed to skip opening tag")?;

        let access = match access_tag {
            0 => {
                // Stream access
                // fileStartPosition: signed integer
                let (_, _, len) = decoder
                    .decode_tag_info()
                    .map_err(|_| "Failed to decode start position tag")?;
                let start = decoder
                    .decode_signed(len)
                    .map_err(|_| "Failed to decode start position")?;

                // fileData: octet string (application tag 6)
                let (_, _, len) = decoder
                    .decode_tag_info()
                    .map_err(|_| "Failed to decode file data tag")?;
                let file_data = decoder
                    .read_bytes(len)
                    .map_err(|_| "Failed to read file data")?
                    .to_vec();

                WriteFileAccess::Stream {
                    file_start_position: start,
                    file_data,
                }
            }
            1 => {
                // Record access
                // fileStartRecord: signed integer
                let (_, _, len) = decoder
                    .decode_tag_info()
                    .map_err(|_| "Failed to decode start record tag")?;
                let start = decoder
                    .decode_signed(len)
                    .map_err(|_| "Failed to decode start record")?;

                // recordCount: unsigned
                let (_, _, len) = decoder
                    .decode_tag_info()
                    .map_err(|_| "Failed to decode record count tag")?;
                let count = decoder
                    .decode_unsigned(len)
                    .map_err(|_| "Failed to decode record count")?;

                // fileRecordData: SEQUENCE OF OCTET STRING
                let mut records = Vec::with_capacity(count as usize);
                for _ in 0..count {
                    if decoder.is_empty() {
                        break;
                    }
                    let (_, _, len) = decoder
                        .decode_tag_info()
                        .map_err(|_| "Failed to decode record data tag")?;
                    let record = decoder
                        .read_bytes(len)
                        .map_err(|_| "Failed to read record data")?
                        .to_vec();
                    records.push(record);
                }

                WriteFileAccess::Record {
                    file_start_record: start,
                    file_record_data: records,
                }
            }
            _ => return Err("Unknown access choice"),
        };

        // Skip closing tag
        if !decoder.is_empty() {
            decoder.skip(1).ok();
        }

        Ok(Self {
            object_type: obj_id.object_type as u16,
            instance: obj_id.instance,
            access,
        })
    }
}

/// AtomicWriteFile service handler.
pub struct AtomicWriteFileHandler;

impl AtomicWriteFileHandler {
    pub fn new() -> Self {
        Self
    }
}

impl ConfirmedServiceHandler for AtomicWriteFileHandler {
    fn service_choice(&self) -> ConfirmedService {
        ConfirmedService::AtomicWriteFile
    }

    fn handle(&self, data: &[u8], ctx: &ServiceContext) -> ServiceResult {
        // Decode request
        let request = match AtomicWriteFileRequest::decode(data) {
            Ok(req) => req,
            Err(_) => return ServiceResult::invalid_tag(),
        };

        // Verify it's a File object
        if request.object_type != ObjectType::File as u16 {
            return ServiceResult::Error {
                error_class: ErrorClass::Object,
                error_code: ErrorCode::UnsupportedObjectType,
            };
        }

        // Lookup the object
        let obj_id = crate::object::types::ObjectId::new(ObjectType::File, request.instance);
        let obj = match ctx.objects.get(&obj_id) {
            Some(o) => o,
            None => return ServiceResult::unknown_object(),
        };

        // Downcast to FileObject
        let file_obj = match downcast_file_object(&obj) {
            Some(f) => f,
            None => {
                return ServiceResult::Error {
                    error_class: ErrorClass::Object,
                    error_code: ErrorCode::UnsupportedObjectType,
                };
            }
        };

        // Verify access method matches
        match (&request.access, file_obj.access_method()) {
            (WriteFileAccess::Stream { .. }, FileAccessMethod::StreamAccess) => {}
            (WriteFileAccess::Record { .. }, FileAccessMethod::RecordAccess) => {}
            _ => {
                return ServiceResult::Error {
                    error_class: ErrorClass::Services,
                    error_code: ErrorCode::InvalidFileAccessMethod,
                };
            }
        }

        // Perform the write
        match request.access {
            WriteFileAccess::Stream {
                file_start_position,
                file_data,
            } => {
                match file_obj.write_stream(file_start_position, &file_data) {
                    Ok(actual_pos) => {
                        let response = encode_write_file_stream_ack(actual_pos);
                        ServiceResult::ComplexAck(response)
                    }
                    Err(_) => ServiceResult::write_access_denied(),
                }
            }
            WriteFileAccess::Record {
                file_start_record,
                file_record_data,
            } => {
                match file_obj.write_records(file_start_record, file_record_data) {
                    Ok(actual_idx) => {
                        let response = encode_write_file_record_ack(actual_idx);
                        ServiceResult::ComplexAck(response)
                    }
                    Err(_) => ServiceResult::write_access_denied(),
                }
            }
        }
    }

    fn name(&self) -> &'static str {
        "AtomicWriteFile"
    }

    fn min_data_length(&self) -> usize {
        6
    }
}

// ============================================================================
// Downcast helper
// ============================================================================

/// Downcast an `Arc<dyn BACnetObject>` to `&FileObject`.
///
/// Uses the `as_any()` trait method for safe downcasting to the
/// concrete `FileObject` type, enabling access to file-specific
/// operations (`read_stream`, `write_stream`, etc.).
fn downcast_file_object(
    obj: &Arc<dyn crate::object::traits::BACnetObject>,
) -> Option<&FileObject> {
    obj.as_any().downcast_ref::<FileObject>()
}

// ============================================================================
// Response encoding
// ============================================================================

/// Encode AtomicReadFile-ACK for stream access.
///
/// ```text
/// endOfFile            BOOLEAN (application tag 1)
/// [0] SEQUENCE {
///     fileStartPosition  INTEGER (application tag 3)
///     fileData           OCTET STRING (application tag 6)
/// }
/// ```
fn encode_read_file_stream_ack(eof: bool, start_position: i32, data: &[u8]) -> Vec<u8> {
    let mut encoder = ApduEncoder::new();

    // endOfFile: application Boolean
    encoder.encode_boolean(eof);

    // [0] opening tag — stream access
    encoder.encode_opening_tag(0);

    // fileStartPosition: application Signed Integer
    encode_signed_application(&mut encoder, start_position);

    // fileData: application Octet String
    encoder.encode_octet_string(data);

    // [0] closing tag
    encoder.encode_closing_tag(0);

    encoder.into_bytes()
}

/// Encode AtomicReadFile-ACK for record access.
///
/// ```text
/// endOfFile            BOOLEAN
/// [1] SEQUENCE {
///     fileStartRecord    INTEGER
///     returnedRecordCount  Unsigned
///     fileRecordData     SEQUENCE OF OCTET STRING
/// }
/// ```
fn encode_read_file_record_ack(
    eof: bool,
    start_record: i32,
    records: &[Vec<u8>],
) -> Vec<u8> {
    let mut encoder = ApduEncoder::new();

    // endOfFile
    encoder.encode_boolean(eof);

    // [1] opening tag — record access
    encoder.encode_opening_tag(1);

    // fileStartRecord: application Signed Integer
    encode_signed_application(&mut encoder, start_record);

    // returnedRecordCount: application Unsigned
    encoder.encode_unsigned(records.len() as u32);

    // fileRecordData: SEQUENCE OF OCTET STRING
    for record in records {
        encoder.encode_octet_string(record);
    }

    // [1] closing tag
    encoder.encode_closing_tag(1);

    encoder.into_bytes()
}

/// Encode AtomicWriteFile-ACK for stream access.
///
/// ```text
/// [0] fileStartPosition INTEGER
/// ```
fn encode_write_file_stream_ack(start_position: i32) -> Vec<u8> {
    let mut encoder = ApduEncoder::new();
    // [0] context tagged signed integer
    encode_signed_context(&mut encoder, 0, start_position);
    encoder.into_bytes()
}

/// Encode AtomicWriteFile-ACK for record access.
///
/// ```text
/// [1] fileStartRecord INTEGER
/// ```
fn encode_write_file_record_ack(start_record: i32) -> Vec<u8> {
    let mut encoder = ApduEncoder::new();
    // [1] context tagged signed integer
    encode_signed_context(&mut encoder, 1, start_record);
    encoder.into_bytes()
}

// ============================================================================
// Encoding helpers
// ============================================================================

/// Encode a signed integer as an application-tagged value.
/// BACnet application tag 3 = Signed Integer.
fn encode_signed_application(encoder: &mut ApduEncoder, value: i32) {
    let bytes = signed_to_bytes(value);
    // Application tag 3 (Signed Integer), length = bytes.len()
    let tag_byte = (3 << 4) | (bytes.len() as u8 & 0x07);
    encoder.put_u8(tag_byte);
    for &b in &bytes {
        encoder.put_u8(b);
    }
}

/// Encode a signed integer as a context-tagged value.
fn encode_signed_context(encoder: &mut ApduEncoder, tag_number: u8, value: i32) {
    let bytes = signed_to_bytes(value);
    // Context tag with given tag number
    let tag_byte = (tag_number << 4) | 0x08 | (bytes.len() as u8 & 0x07);
    encoder.put_u8(tag_byte);
    for &b in &bytes {
        encoder.put_u8(b);
    }
}

/// Convert a signed i32 to minimal big-endian byte representation.
fn signed_to_bytes(value: i32) -> Vec<u8> {
    let bytes = value.to_be_bytes();
    // Find minimal encoding
    if value >= -128 && value <= 127 {
        vec![bytes[3]]
    } else if value >= -32768 && value <= 32767 {
        vec![bytes[2], bytes[3]]
    } else if value >= -8_388_608 && value <= 8_388_607 {
        vec![bytes[1], bytes[2], bytes[3]]
    } else {
        bytes.to_vec()
    }
}

// ============================================================================
// ErrorCode extension
// ============================================================================

// Note: InvalidFileAccessMethod may not exist in the ErrorCode enum.
// We add it as a trait extension or use an existing code.
// Let's check and handle it inline.

#[cfg(test)]
mod tests {
    use super::*;
    use crate::object::file::FileObject;
    use crate::object::registry::ObjectRegistry;
    use crate::object::types::ObjectId;

    fn make_stream_file(instance: u32, data: &[u8]) -> Arc<FileObject> {
        let file = FileObject::new(instance, format!("File{}", instance))
            .with_data(data.to_vec());
        Arc::new(file)
    }

    fn make_record_file(instance: u32, records: Vec<Vec<u8>>) -> Arc<FileObject> {
        let file = FileObject::with_access_method(
            instance,
            format!("RecFile{}", instance),
            FileAccessMethod::RecordAccess,
        )
        .with_records(records);
        Arc::new(file)
    }

    fn make_ctx(registry: &Arc<ObjectRegistry>) -> ServiceContext {
        ServiceContext {
            objects: registry.clone(),
            device_instance: 1234,
            invoke_id: Some(1),
            max_apdu_length: 1476,
            source_address: None,
        }
    }

    // ── Request encoding helpers for tests ──────────────────────────

    fn encode_read_file_stream_request(instance: u32, start: i32, count: u32) -> Vec<u8> {
        let mut encoder = ApduEncoder::new();

        // ObjectIdentifier: application tag 12, length 4
        encoder.encode_context_object_identifier(12, ObjectId::new(ObjectType::File, instance));
        // Actually we need APPLICATION tag for ObjectIdentifier
        // Let's manually encode it properly
        drop(encoder);

        let mut buf = Vec::new();

        // Application tag 12 (ObjectIdentifier), length 4
        // tag_number=12, class=application(0), length=4
        buf.push((12 << 4) | 4); // 0xC4
        let obj_val = ((ObjectType::File as u32) << 22) | instance;
        buf.extend_from_slice(&obj_val.to_be_bytes());

        // Opening tag [0] for stream access
        buf.push(0x0E); // tag 0, opening

        // fileStartPosition: application Signed Integer (tag 3)
        let start_bytes = signed_to_bytes(start);
        buf.push((3 << 4) | (start_bytes.len() as u8));
        buf.extend_from_slice(&start_bytes);

        // requestedOctetCount: application Unsigned (tag 2)
        let count_bytes = unsigned_to_bytes(count);
        buf.push((2 << 4) | (count_bytes.len() as u8));
        buf.extend_from_slice(&count_bytes);

        // Closing tag [0]
        buf.push(0x0F);

        buf
    }

    fn encode_read_file_record_request(instance: u32, start: i32, count: u32) -> Vec<u8> {
        let mut buf = Vec::new();

        // Application tag 12 (ObjectIdentifier), length 4
        buf.push(0xC4);
        let obj_val = ((ObjectType::File as u32) << 22) | instance;
        buf.extend_from_slice(&obj_val.to_be_bytes());

        // Opening tag [1] for record access
        buf.push(0x1E);

        // fileStartRecord: Signed Integer
        let start_bytes = signed_to_bytes(start);
        buf.push((3 << 4) | (start_bytes.len() as u8));
        buf.extend_from_slice(&start_bytes);

        // requestedRecordCount: Unsigned
        let count_bytes = unsigned_to_bytes(count);
        buf.push((2 << 4) | (count_bytes.len() as u8));
        buf.extend_from_slice(&count_bytes);

        // Closing tag [1]
        buf.push(0x1F);

        buf
    }

    fn encode_write_file_stream_request(instance: u32, start: i32, data: &[u8]) -> Vec<u8> {
        let mut buf = Vec::new();

        // ObjectIdentifier
        buf.push(0xC4);
        let obj_val = ((ObjectType::File as u32) << 22) | instance;
        buf.extend_from_slice(&obj_val.to_be_bytes());

        // Opening tag [0]
        buf.push(0x0E);

        // fileStartPosition: Signed Integer
        let start_bytes = signed_to_bytes(start);
        buf.push((3 << 4) | (start_bytes.len() as u8));
        buf.extend_from_slice(&start_bytes);

        // fileData: Octet String (application tag 6)
        if data.len() < 5 {
            buf.push((6 << 4) | (data.len() as u8));
        } else {
            buf.push((6 << 4) | 5);
            buf.push(data.len() as u8);
        }
        buf.extend_from_slice(data);

        // Closing tag [0]
        buf.push(0x0F);

        buf
    }

    fn encode_write_file_record_request(
        instance: u32,
        start: i32,
        records: &[&[u8]],
    ) -> Vec<u8> {
        let mut buf = Vec::new();

        // ObjectIdentifier
        buf.push(0xC4);
        let obj_val = ((ObjectType::File as u32) << 22) | instance;
        buf.extend_from_slice(&obj_val.to_be_bytes());

        // Opening tag [1]
        buf.push(0x1E);

        // fileStartRecord: Signed Integer
        let start_bytes = signed_to_bytes(start);
        buf.push((3 << 4) | (start_bytes.len() as u8));
        buf.extend_from_slice(&start_bytes);

        // recordCount: Unsigned
        let count = records.len() as u32;
        let count_bytes = unsigned_to_bytes(count);
        buf.push((2 << 4) | (count_bytes.len() as u8));
        buf.extend_from_slice(&count_bytes);

        // records: SEQUENCE OF OCTET STRING
        for record in records {
            if record.len() < 5 {
                buf.push((6 << 4) | (record.len() as u8));
            } else {
                buf.push((6 << 4) | 5);
                buf.push(record.len() as u8);
            }
            buf.extend_from_slice(record);
        }

        // Closing tag [1]
        buf.push(0x1F);

        buf
    }

    fn unsigned_to_bytes(value: u32) -> Vec<u8> {
        if value <= 0xFF {
            vec![value as u8]
        } else if value <= 0xFFFF {
            vec![(value >> 8) as u8, value as u8]
        } else if value <= 0xFFFFFF {
            vec![(value >> 16) as u8, (value >> 8) as u8, value as u8]
        } else {
            value.to_be_bytes().to_vec()
        }
    }

    // ── Tests ───────────────────────────────────────────────────────

    #[test]
    fn test_handler_creation() {
        let handler = AtomicReadFileHandler::new();
        assert_eq!(handler.service_choice(), ConfirmedService::AtomicReadFile);
        assert_eq!(handler.name(), "AtomicReadFile");

        let handler = AtomicWriteFileHandler::new();
        assert_eq!(handler.service_choice(), ConfirmedService::AtomicWriteFile);
        assert_eq!(handler.name(), "AtomicWriteFile");
    }

    #[test]
    fn test_decode_read_file_stream_request() {
        let data = encode_read_file_stream_request(1, 0, 100);
        let req = AtomicReadFileRequest::decode(&data).unwrap();
        assert_eq!(req.instance, 1);
        match req.access {
            ReadFileAccess::Stream {
                file_start_position,
                requested_octet_count,
            } => {
                assert_eq!(file_start_position, 0);
                assert_eq!(requested_octet_count, 100);
            }
            _ => panic!("Expected stream access"),
        }
    }

    #[test]
    fn test_decode_read_file_record_request() {
        let data = encode_read_file_record_request(2, 5, 10);
        let req = AtomicReadFileRequest::decode(&data).unwrap();
        assert_eq!(req.instance, 2);
        match req.access {
            ReadFileAccess::Record {
                file_start_record,
                requested_record_count,
            } => {
                assert_eq!(file_start_record, 5);
                assert_eq!(requested_record_count, 10);
            }
            _ => panic!("Expected record access"),
        }
    }

    #[test]
    fn test_decode_write_file_stream_request() {
        let data = encode_write_file_stream_request(1, 0, b"test");
        let req = AtomicWriteFileRequest::decode(&data).unwrap();
        assert_eq!(req.instance, 1);
        match req.access {
            WriteFileAccess::Stream {
                file_start_position,
                file_data,
            } => {
                assert_eq!(file_start_position, 0);
                assert_eq!(file_data, b"test");
            }
            _ => panic!("Expected stream access"),
        }
    }

    #[test]
    fn test_decode_write_file_record_request() {
        let data = encode_write_file_record_request(3, 0, &[b"rec1", b"rec2"]);
        let req = AtomicWriteFileRequest::decode(&data).unwrap();
        assert_eq!(req.instance, 3);
        match req.access {
            WriteFileAccess::Record {
                file_start_record,
                file_record_data,
            } => {
                assert_eq!(file_start_record, 0);
                assert_eq!(file_record_data.len(), 2);
                assert_eq!(file_record_data[0], b"rec1");
                assert_eq!(file_record_data[1], b"rec2");
            }
            _ => panic!("Expected record access"),
        }
    }

    #[test]
    fn test_read_file_stream_handler() {
        let file = make_stream_file(1, b"Hello, BACnet File!");
        let registry = Arc::new(ObjectRegistry::new());
        registry.register(file);
        let ctx = make_ctx(&registry);

        let handler = AtomicReadFileHandler::new();
        let req_data = encode_read_file_stream_request(1, 0, 100);

        match handler.handle(&req_data, &ctx) {
            ServiceResult::ComplexAck(ack_data) => {
                assert!(!ack_data.is_empty());
                // First byte is the boolean tag for endOfFile
                // Verify we got a response
            }
            other => panic!("Expected ComplexAck, got {:?}", other),
        }
    }

    #[test]
    fn test_read_file_stream_partial() {
        let file = make_stream_file(1, b"Hello, BACnet!");
        let registry = Arc::new(ObjectRegistry::new());
        registry.register(file);
        let ctx = make_ctx(&registry);

        let handler = AtomicReadFileHandler::new();
        let req_data = encode_read_file_stream_request(1, 0, 5);

        match handler.handle(&req_data, &ctx) {
            ServiceResult::ComplexAck(_) => {} // Success
            other => panic!("Expected ComplexAck, got {:?}", other),
        }
    }

    #[test]
    fn test_read_file_record_handler() {
        let file = make_record_file(
            2,
            vec![b"Record0".to_vec(), b"Record1".to_vec(), b"Record2".to_vec()],
        );
        let registry = Arc::new(ObjectRegistry::new());
        registry.register(file);
        let ctx = make_ctx(&registry);

        let handler = AtomicReadFileHandler::new();
        let req_data = encode_read_file_record_request(2, 0, 3);

        match handler.handle(&req_data, &ctx) {
            ServiceResult::ComplexAck(ack_data) => {
                assert!(!ack_data.is_empty());
            }
            other => panic!("Expected ComplexAck, got {:?}", other),
        }
    }

    #[test]
    fn test_write_file_stream_handler() {
        let file = make_stream_file(1, b"");
        let registry = Arc::new(ObjectRegistry::new());
        registry.register(file.clone());
        let ctx = make_ctx(&registry);

        let handler = AtomicWriteFileHandler::new();
        let req_data = encode_write_file_stream_request(1, 0, b"New data");

        match handler.handle(&req_data, &ctx) {
            ServiceResult::ComplexAck(_) => {
                assert_eq!(file.get_data(), b"New data");
            }
            other => panic!("Expected ComplexAck, got {:?}", other),
        }
    }

    #[test]
    fn test_write_file_record_handler() {
        let file = make_record_file(3, Vec::new());
        let registry = Arc::new(ObjectRegistry::new());
        registry.register(file.clone());
        let ctx = make_ctx(&registry);

        let handler = AtomicWriteFileHandler::new();
        let req_data = encode_write_file_record_request(3, -1, &[b"NewRec"]);

        match handler.handle(&req_data, &ctx) {
            ServiceResult::ComplexAck(_) => {
                assert_eq!(file.get_records().len(), 1);
                assert_eq!(file.get_records()[0], b"NewRec");
            }
            other => panic!("Expected ComplexAck, got {:?}", other),
        }
    }

    #[test]
    fn test_write_read_only_file() {
        let file = Arc::new(
            FileObject::new(1, "ReadOnlyFile").with_read_only(true),
        );
        let registry = Arc::new(ObjectRegistry::new());
        registry.register(file);
        let ctx = make_ctx(&registry);

        let handler = AtomicWriteFileHandler::new();
        let req_data = encode_write_file_stream_request(1, 0, b"data");

        match handler.handle(&req_data, &ctx) {
            ServiceResult::Error {
                error_class,
                error_code,
            } => {
                assert_eq!(error_class, ErrorClass::Property);
                assert_eq!(error_code, ErrorCode::WriteAccessDenied);
            }
            other => panic!("Expected Error, got {:?}", other),
        }
    }

    #[test]
    fn test_read_unknown_file() {
        let registry = Arc::new(ObjectRegistry::new());
        let ctx = make_ctx(&registry);

        let handler = AtomicReadFileHandler::new();
        let req_data = encode_read_file_stream_request(999, 0, 100);

        match handler.handle(&req_data, &ctx) {
            ServiceResult::Error {
                error_class,
                error_code,
            } => {
                assert_eq!(error_class, ErrorClass::Object);
                assert_eq!(error_code, ErrorCode::UnknownObject);
            }
            other => panic!("Expected Error, got {:?}", other),
        }
    }

    #[test]
    fn test_encode_read_file_stream_ack() {
        let ack = encode_read_file_stream_ack(true, 0, b"Hello");
        assert!(!ack.is_empty());
        // Verify structure: Boolean + Opening[0] + SignedInt + OctetString + Closing[0]
        // Boolean true: application tag 1, value 1
        assert_eq!(ack[0], 0x11); // tag 1, length 1
    }

    #[test]
    fn test_encode_read_file_record_ack() {
        let records = vec![b"R0".to_vec(), b"R1".to_vec()];
        let ack = encode_read_file_record_ack(false, 0, &records);
        assert!(!ack.is_empty());
        // Boolean false: application tag 1, value 0
        assert_eq!(ack[0], 0x10); // tag 1, length 0 (boolean false)
    }

    #[test]
    fn test_encode_write_file_stream_ack() {
        let ack = encode_write_file_stream_ack(42);
        assert!(!ack.is_empty());
        // Context tag [0], signed integer 42
    }

    #[test]
    fn test_encode_write_file_record_ack() {
        let ack = encode_write_file_record_ack(0);
        assert!(!ack.is_empty());
    }

    #[test]
    fn test_signed_to_bytes() {
        assert_eq!(signed_to_bytes(0), vec![0]);
        assert_eq!(signed_to_bytes(1), vec![1]);
        assert_eq!(signed_to_bytes(-1), vec![0xFF]);
        assert_eq!(signed_to_bytes(127), vec![0x7F]);
        assert_eq!(signed_to_bytes(128), vec![0x00, 0x80]);
        assert_eq!(signed_to_bytes(-128), vec![0x80]);
        assert_eq!(signed_to_bytes(-129), vec![0xFF, 0x7F]);
        assert_eq!(signed_to_bytes(256), vec![0x01, 0x00]);
    }

    #[test]
    fn test_stream_access_mismatch() {
        // Create a record-access file but send a stream read request
        let file = make_record_file(1, vec![b"data".to_vec()]);
        let registry = Arc::new(ObjectRegistry::new());
        registry.register(file);
        let ctx = make_ctx(&registry);

        let handler = AtomicReadFileHandler::new();
        let req_data = encode_read_file_stream_request(1, 0, 100);

        match handler.handle(&req_data, &ctx) {
            ServiceResult::Error { .. } => {} // Expected mismatch error
            other => panic!("Expected Error for access method mismatch, got {:?}", other),
        }
    }

    #[test]
    fn test_write_stream_append() {
        let file = make_stream_file(1, b"Hello");
        let registry = Arc::new(ObjectRegistry::new());
        registry.register(file.clone());
        let ctx = make_ctx(&registry);

        let handler = AtomicWriteFileHandler::new();

        // Write at position -1 (append)
        let req_data = encode_write_file_stream_request(1, -1, b" World");

        match handler.handle(&req_data, &ctx) {
            ServiceResult::ComplexAck(_) => {
                assert_eq!(file.get_data(), b"Hello World");
            }
            other => panic!("Expected ComplexAck, got {:?}", other),
        }
    }
}

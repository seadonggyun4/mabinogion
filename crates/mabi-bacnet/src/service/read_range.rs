//! ReadRange service handler per ASHRAE 135, Clause 15.8.
//!
//! ReadRange provides indexed access to list or log properties
//! (such as TrendLog.Log_Buffer) by position, sequence number,
//! or timestamp.
//!
//! ## Supported Range Types
//!
//! - **By Position**: 1-based index + count
//! - **By Sequence Number**: Sequence number + count
//! - **By Time**: BACnet DateTime + count
//! - **No Range**: Returns entire buffer

use crate::apdu::encoding::{ApduDecoder, ApduEncoder};
use crate::apdu::types::{ConfirmedService, ErrorClass, ErrorCode};
use crate::object::property::{BACnetDate, BACnetTime, BACnetValue, PropertyId};
use crate::object::types::{ObjectId, ObjectType};

use super::handler::{ConfirmedServiceHandler, ServiceContext, ServiceResult};

// ============================================================================
// ReadRange Request
// ============================================================================

/// Range specification for ReadRange.
#[derive(Debug, Clone)]
pub enum RangeType {
    /// No range specified — return all available data.
    None,
    /// By position: (reference_index, count).
    /// reference_index is 1-based. Negative count = read backwards.
    ByPosition { reference_index: i32, count: i32 },
    /// By sequence number: (reference_seq, count).
    BySequenceNumber { reference_seq: u64, count: i32 },
    /// By time: (reference_time, count).
    ByTime {
        reference_date: BACnetDate,
        reference_time: BACnetTime,
        count: i32,
    },
}

/// Decoded ReadRange request.
#[derive(Debug)]
pub struct ReadRangeRequest {
    /// Object identifier.
    pub object_id: ObjectId,
    /// Property identifier (typically LogBuffer).
    pub property_id: PropertyId,
    /// Optional array index.
    pub array_index: Option<u32>,
    /// Range specification.
    pub range: RangeType,
}

impl ReadRangeRequest {
    /// Decode a ReadRange request from APDU service data.
    ///
    /// Per ASHRAE 135, Clause 21.17 (ReadRange-Request):
    /// ```text
    /// ReadRange-Request ::= SEQUENCE {
    ///   objectIdentifier     [0] BACnetObjectIdentifier,
    ///   propertyIdentifier   [1] BACnetPropertyIdentifier,
    ///   propertyArrayIndex   [2] Unsigned OPTIONAL,
    ///   range                CHOICE {
    ///     byPosition         [3] SEQUENCE { referenceIndex Unsigned, count INTEGER },
    ///     bySequenceNumber   [6] SEQUENCE { referenceSequenceNumber Unsigned, count INTEGER },
    ///     byTime             [7] SEQUENCE { referenceTime BACnetDateTime, count INTEGER },
    ///   } OPTIONAL
    /// }
    /// ```
    pub fn decode(data: &[u8]) -> Result<Self, ServiceResult> {
        if data.len() < 7 {
            return Err(ServiceResult::missing_required_parameter());
        }

        let mut decoder = ApduDecoder::new(data);

        // [0] Object Identifier (context tag 0, 4 bytes)
        let (tag0, is_ctx0, _len0) = decoder
            .decode_tag_info()
            .map_err(|_| ServiceResult::missing_required_parameter())?;
        if tag0 != 0 || !is_ctx0 {
            return Err(ServiceResult::missing_required_parameter());
        }
        let object_id = decoder
            .decode_object_identifier()
            .map_err(|_| ServiceResult::missing_required_parameter())?;

        // [1] Property Identifier (context tag 1, enumerated)
        let (tag1, is_ctx1, len1) = decoder
            .decode_tag_info()
            .map_err(|_| ServiceResult::missing_required_parameter())?;
        if tag1 != 1 || !is_ctx1 {
            return Err(ServiceResult::missing_required_parameter());
        }
        let prop_raw = decoder
            .decode_unsigned(len1)
            .map_err(|_| ServiceResult::missing_required_parameter())?;
        let property_id = PropertyId::from_u32(prop_raw)
            .ok_or_else(ServiceResult::missing_required_parameter)?;

        // [2] Optional array index
        let mut array_index = None;
        let mut range = RangeType::None;

        // Peek at remaining data
        while decoder.remaining() > 0 {
            let (tag, is_ctx, len) = match decoder.decode_tag_info() {
                Ok(info) => info,
                Err(_) => break,
            };

            if !is_ctx {
                break;
            }

            match tag {
                2 => {
                    // [2] propertyArrayIndex (Unsigned)
                    let idx = decoder
                        .decode_unsigned(len)
                        .map_err(|_| ServiceResult::missing_required_parameter())?;
                    array_index = Some(idx);
                }
                3 => {
                    // [3] byPosition: opening tag, then referenceIndex + count, closing tag
                    // The len returned is for opening tag (should be 0).
                    // Contents: Unsigned referenceIndex, Signed count
                    let ref_idx = decode_unsigned_from_decoder(&mut decoder)?;
                    let count = decode_signed_from_decoder(&mut decoder)?;
                    // Consume closing tag [3]
                    let _ = decoder.decode_tag_info();
                    range = RangeType::ByPosition {
                        reference_index: ref_idx as i32,
                        count,
                    };
                }
                6 => {
                    // [6] bySequenceNumber: opening tag, then referenceSequenceNumber + count
                    let ref_seq = decode_unsigned_from_decoder(&mut decoder)?;
                    let count = decode_signed_from_decoder(&mut decoder)?;
                    let _ = decoder.decode_tag_info();
                    range = RangeType::BySequenceNumber {
                        reference_seq: ref_seq as u64,
                        count,
                    };
                }
                7 => {
                    // [7] byTime: opening tag, then BACnetDateTime + count
                    let date = decode_date_from_decoder(&mut decoder)?;
                    let time = decode_time_from_decoder(&mut decoder)?;
                    let count = decode_signed_from_decoder(&mut decoder)?;
                    let _ = decoder.decode_tag_info();
                    range = RangeType::ByTime {
                        reference_date: date,
                        reference_time: time,
                        count,
                    };
                }
                _ => {
                    // Skip unknown tags
                    decoder.skip(len).map_err(|_| ServiceResult::missing_required_parameter())?;
                }
            }
        }

        Ok(Self {
            object_id,
            property_id,
            array_index,
            range,
        })
    }
}

// ============================================================================
// Helper decoders
// ============================================================================

fn decode_unsigned_from_decoder(decoder: &mut ApduDecoder) -> Result<u32, ServiceResult> {
    let (_tag, _ctx, len) = decoder
        .decode_tag_info()
        .map_err(|_| ServiceResult::missing_required_parameter())?;
    decoder
        .decode_unsigned(len)
        .map_err(|_| ServiceResult::missing_required_parameter())
}

fn decode_signed_from_decoder(decoder: &mut ApduDecoder) -> Result<i32, ServiceResult> {
    let (_tag, _ctx, len) = decoder
        .decode_tag_info()
        .map_err(|_| ServiceResult::missing_required_parameter())?;
    // Read unsigned and interpret as signed
    let raw = decoder
        .decode_unsigned(len)
        .map_err(|_| ServiceResult::missing_required_parameter())?;
    Ok(raw as i32)
}

fn decode_date_from_decoder(decoder: &mut ApduDecoder) -> Result<BACnetDate, ServiceResult> {
    let (_tag, _ctx, _len) = decoder
        .decode_tag_info()
        .map_err(|_| ServiceResult::missing_required_parameter())?;
    // Date is always 4 bytes: year, month, day, day_of_week
    if decoder.remaining() < 4 {
        return Err(ServiceResult::missing_required_parameter());
    }
    let bytes = decoder
        .read_bytes(4)
        .map_err(|_| ServiceResult::missing_required_parameter())?;
    Ok(BACnetDate {
        year: bytes[0],
        month: bytes[1],
        day: bytes[2],
        day_of_week: bytes[3],
    })
}

fn decode_time_from_decoder(decoder: &mut ApduDecoder) -> Result<BACnetTime, ServiceResult> {
    let (_tag, _ctx, _len) = decoder
        .decode_tag_info()
        .map_err(|_| ServiceResult::missing_required_parameter())?;
    // Time is always 4 bytes: hour, minute, second, hundredths
    if decoder.remaining() < 4 {
        return Err(ServiceResult::missing_required_parameter());
    }
    let bytes = decoder
        .read_bytes(4)
        .map_err(|_| ServiceResult::missing_required_parameter())?;
    Ok(BACnetTime {
        hour: bytes[0],
        minute: bytes[1],
        second: bytes[2],
        hundredths: bytes[3],
    })
}

// ============================================================================
// ReadRange Handler
// ============================================================================

/// ReadRange service handler.
///
/// Supports reading log buffer entries from TrendLog objects by
/// position, sequence number, or timestamp.
pub struct ReadRangeHandler;

impl ReadRangeHandler {
    pub fn new() -> Self {
        Self
    }
}

impl ConfirmedServiceHandler for ReadRangeHandler {
    fn service_choice(&self) -> ConfirmedService {
        ConfirmedService::ReadRange
    }

    fn handle(&self, data: &[u8], ctx: &ServiceContext) -> ServiceResult {
        // Decode the request
        let request = match ReadRangeRequest::decode(data) {
            Ok(req) => req,
            Err(e) => return e,
        };

        // Look up the object in the registry
        let object = match ctx.objects.get(&request.object_id) {
            Some(obj) => obj,
            None => return ServiceResult::unknown_object(),
        };

        // ReadRange is only applicable to TrendLog (and similar) objects
        if request.object_id.object_type != ObjectType::TrendLog {
            return ServiceResult::Error {
                error_class: ErrorClass::Services,
                error_code: ErrorCode::ServiceRequestDenied,
            };
        }

        // ReadRange is only applicable to the LogBuffer property
        if request.property_id != PropertyId::LogBuffer {
            return ServiceResult::Error {
                error_class: ErrorClass::Property,
                error_code: ErrorCode::PropertyIsNotAList,
            };
        }

        // Downcast to TrendLog — we know the object type is TrendLog.
        // Since we don't have Any-based downcasting, we read the log buffer
        // via the object's read_property interface, but for ReadRange we need
        // the TrendLog's specialized range methods.
        //
        // Strategy: Try to get the TrendLog from the registry using a trait object.
        // Since our objects are trait objects behind Arc<dyn BACnetObject>, we
        // need to use the property-based approach for now.
        //
        // For the simulator, we use the object's read_property to get the full
        // buffer, then apply range filtering on the BACnetValue::Array.
        let log_buffer = match object.read_property(PropertyId::LogBuffer) {
            Ok(val) => val,
            Err(_) => {
                return ServiceResult::Error {
                    error_class: ErrorClass::Property,
                    error_code: ErrorCode::UnknownProperty,
                }
            }
        };

        let all_records = match &log_buffer {
            BACnetValue::Array(arr) => arr,
            _ => {
                return ServiceResult::Error {
                    error_class: ErrorClass::Property,
                    error_code: ErrorCode::PropertyIsNotAList,
                }
            }
        };

        // Apply range filtering
        let (result_items, first_seq, more_follows) = apply_range(all_records, &request.range);

        // Encode the ReadRange-ACK response
        let response = encode_read_range_ack(
            &request.object_id,
            &request.property_id,
            &result_items,
            first_seq,
            more_follows,
        );

        ServiceResult::ComplexAck(response)
    }

    fn name(&self) -> &'static str {
        "ReadRange"
    }

    fn min_data_length(&self) -> usize {
        7 // Minimum: object_id (6) + property_id (1)
    }
}

/// Apply range filtering on the BACnetValue array representation of log records.
///
/// Each record in the array is a BACnetValue::List of [Date, Time, Value, StatusFlags].
fn apply_range<'a>(
    records: &'a [BACnetValue],
    range: &RangeType,
) -> (Vec<&'a BACnetValue>, u32, bool) {
    let total = records.len();

    match range {
        RangeType::None => {
            // Return all records
            let items: Vec<&BACnetValue> = records.iter().collect();
            let len = items.len() as u32;
            (items, len, false)
        }
        RangeType::ByPosition {
            reference_index,
            count,
        } => {
            if total == 0 || *count == 0 {
                return (Vec::new(), 0, false);
            }

            if *count > 0 {
                let start = ((*reference_index).max(1) as usize - 1).min(total);
                let take = (*count as usize).min(total - start);
                let items: Vec<&BACnetValue> = records[start..start + take].iter().collect();
                let more = (start + take) < total;
                let len = items.len() as u32;
                (items, len, more)
            } else {
                let end = ((*reference_index).max(1) as usize).min(total);
                let take = ((-*count) as usize).min(end);
                let start = end - take;
                let items: Vec<&BACnetValue> = records[start..end].iter().collect();
                let more = start > 0;
                let len = items.len() as u32;
                (items, len, more)
            }
        }
        RangeType::BySequenceNumber {
            reference_seq,
            count,
        } => {
            // For sequence-based access, the sequence numbers are implicit
            // (the array index maps to the record order). We approximate by
            // treating array index 0 as the first available sequence.
            // In practice, the TrendLog's read_all_records returns them in order.
            if total == 0 || *count == 0 {
                return (Vec::new(), 0, false);
            }

            let start = (*reference_seq as usize).min(total);
            if *count > 0 {
                let take = (*count as usize).min(total - start);
                let items: Vec<&BACnetValue> = records[start..start + take].iter().collect();
                let more = (start + take) < total;
                let len = items.len() as u32;
                (items, len, more)
            } else {
                let end = start.min(total);
                let take = ((-*count) as usize).min(end);
                let s = end - take;
                let items: Vec<&BACnetValue> = records[s..end].iter().collect();
                let more = s > 0;
                let len = items.len() as u32;
                (items, len, more)
            }
        }
        RangeType::ByTime {
            reference_date,
            reference_time,
            count,
        } => {
            // For time-based filtering, each record is [Date, Time, Value, StatusFlags].
            // We compare the Date+Time fields.
            if total == 0 || *count == 0 {
                return (Vec::new(), 0, false);
            }

            let ref_sortable = timestamp_sortable(reference_date, reference_time);

            if *count > 0 {
                let matching: Vec<(usize, &BACnetValue)> = records
                    .iter()
                    .enumerate()
                    .filter(|(_i, v)| record_timestamp_sortable(v) >= ref_sortable)
                    .collect();
                let take = (*count as usize).min(matching.len());
                let items: Vec<&BACnetValue> = matching[..take].iter().map(|(_i, v)| *v).collect();
                let more = take < matching.len();
                let len = items.len() as u32;
                (items, len, more)
            } else {
                let matching: Vec<(usize, &BACnetValue)> = records
                    .iter()
                    .enumerate()
                    .filter(|(_i, v)| record_timestamp_sortable(v) <= ref_sortable)
                    .collect();
                let take = ((-*count) as usize).min(matching.len());
                let start = matching.len() - take;
                let items: Vec<&BACnetValue> = matching[start..].iter().map(|(_i, v)| *v).collect();
                let more = start > 0;
                let len = items.len() as u32;
                (items, len, more)
            }
        }
    }
}

/// Extract a sortable timestamp from a record BACnetValue (List of [Date, Time, ...]).
fn record_timestamp_sortable(record: &BACnetValue) -> u64 {
    if let BACnetValue::List(items) = record {
        if items.len() >= 2 {
            if let (BACnetValue::Date(d), BACnetValue::Time(t)) = (&items[0], &items[1]) {
                return timestamp_sortable(d, t);
            }
        }
    }
    0
}

/// Convert BACnetDate + BACnetTime to a sortable u64.
fn timestamp_sortable(date: &BACnetDate, time: &BACnetTime) -> u64 {
    ((date.year as u64) << 33)
        | ((date.month as u64) << 29)
        | ((date.day as u64) << 24)
        | ((time.hour as u64) << 19)
        | ((time.minute as u64) << 13)
        | ((time.second as u64) << 7)
        | (time.hundredths as u64)
}

// ============================================================================
// Response Encoding
// ============================================================================

/// Encode a ReadRange-ACK per ASHRAE 135, Clause 21.18.
///
/// ```text
/// ReadRange-Ack ::= SEQUENCE {
///   objectIdentifier   [0] BACnetObjectIdentifier,
///   propertyIdentifier [1] BACnetPropertyIdentifier,
///   propertyArrayIndex [2] Unsigned OPTIONAL,
///   resultFlags        [3] BACnetResultFlags,
///   itemCount          [4] Unsigned,
///   itemData           [5] SEQUENCE OF ABSTRACT-SYNTAX.&Type,
///   firstSequenceNumber [6] Unsigned32 OPTIONAL
/// }
/// ```
fn encode_read_range_ack(
    object_id: &ObjectId,
    property_id: &PropertyId,
    items: &[&BACnetValue],
    _item_count: u32,
    more_follows: bool,
) -> Vec<u8> {
    let mut encoder = ApduEncoder::new();

    // [0] Object Identifier
    encoder.encode_context_object_identifier(0, *object_id);

    // [1] Property Identifier
    encoder.encode_context_enumerated(1, *property_id as u32);

    // [3] Result Flags (BACnet BIT STRING: firstItem, lastItem, moreItems)
    let first_item = !items.is_empty();
    let last_item = !more_follows;
    let more_items = more_follows;
    encoder.encode_context_bit_string(3, &[first_item, last_item, more_items]);

    // [4] Item Count
    encoder.encode_context_unsigned(4, items.len() as u32);

    // [5] Item Data — opening tag, then each item, closing tag
    encoder.encode_opening_tag(5);
    for item in items {
        encode_log_record_value(&mut encoder, item);
    }
    encoder.encode_closing_tag(5);

    encoder.into_bytes()
}

/// Encode a single log record value.
///
/// Each log record is a BACnetValue::List of [Date, Time, Datum, StatusFlags].
/// We encode it as a constructed value with the appropriate tags.
fn encode_log_record_value(encoder: &mut ApduEncoder, record: &BACnetValue) {
    if let BACnetValue::List(items) = record {
        if items.len() >= 3 {
            // Encode as inline sequence: date, time, value, optional status_flags
            encoder.encode_context_value(0, &items[0]); // timestamp date
            encoder.encode_context_value(1, &items[1]); // timestamp time
            encoder.encode_context_value(2, &items[2]); // log datum
            if items.len() >= 4 {
                encoder.encode_context_value(3, &items[3]); // status flags
            }
        }
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::object::property::{BACnetDate, BACnetTime};

    fn make_record_value(hour: u8, minute: u8, value: f32) -> BACnetValue {
        BACnetValue::List(vec![
            BACnetValue::Date(BACnetDate {
                year: 126,
                month: 2,
                day: 14,
                day_of_week: 255,
            }),
            BACnetValue::Time(BACnetTime {
                hour,
                minute,
                second: 0,
                hundredths: 0,
            }),
            BACnetValue::Real(value),
            BACnetValue::Unsigned(0),
        ])
    }

    #[test]
    fn test_apply_range_none() {
        let records: Vec<BACnetValue> = (0..5)
            .map(|i| make_record_value(10, i, i as f32))
            .collect();

        let (items, count, more) = apply_range(&records, &RangeType::None);
        assert_eq!(count, 5);
        assert!(!more);
        assert_eq!(items.len(), 5);
    }

    #[test]
    fn test_apply_range_by_position_forward() {
        let records: Vec<BACnetValue> = (0..10)
            .map(|i| make_record_value(10, i, i as f32))
            .collect();

        let range = RangeType::ByPosition {
            reference_index: 3,
            count: 4,
        };
        let (_items, count, more) = apply_range(&records, &range);
        assert_eq!(count, 4);
        assert!(more);
        // Should be records at index 2,3,4,5 (0-based)
    }

    #[test]
    fn test_apply_range_by_position_backward() {
        let records: Vec<BACnetValue> = (0..10)
            .map(|i| make_record_value(10, i, i as f32))
            .collect();

        let range = RangeType::ByPosition {
            reference_index: 8,
            count: -3,
        };
        let (_items, count, more) = apply_range(&records, &range);
        assert_eq!(count, 3);
        assert!(more);
    }

    #[test]
    fn test_apply_range_by_time() {
        let records: Vec<BACnetValue> = (0..10)
            .map(|i| make_record_value(10, i, i as f32))
            .collect();

        let range = RangeType::ByTime {
            reference_date: BACnetDate {
                year: 126,
                month: 2,
                day: 14,
                day_of_week: 255,
            },
            reference_time: BACnetTime {
                hour: 10,
                minute: 5,
                second: 0,
                hundredths: 0,
            },
            count: 3,
        };
        let (_items, count, more) = apply_range(&records, &range);
        assert_eq!(count, 3);
        assert!(more); // 5 matching records, took 3
    }

    #[test]
    fn test_apply_range_empty_buffer() {
        let records: Vec<BACnetValue> = Vec::new();
        let (items, count, more) = apply_range(&records, &RangeType::None);
        assert_eq!(count, 0);
        assert!(!more);
        assert!(items.is_empty());
    }

    #[test]
    fn test_encode_read_range_ack() {
        let object_id = ObjectId::new(ObjectType::TrendLog, 1);
        let property_id = PropertyId::LogBuffer;
        let records: Vec<BACnetValue> = (0..3)
            .map(|i| make_record_value(10, i, i as f32))
            .collect();
        let refs: Vec<&BACnetValue> = records.iter().collect();

        let encoded = encode_read_range_ack(&object_id, &property_id, &refs, 3, false);

        // Should be non-empty and contain the object identifier at the start
        assert!(!encoded.is_empty());
        // The first byte should be context tag 0 (object identifier)
        assert_eq!(encoded[0] & 0xF0, 0x00); // context tag 0
        assert_eq!(encoded[0] & 0x08, 0x08); // is context class
    }

    #[test]
    fn test_timestamp_sortable_ordering() {
        let d = BACnetDate {
            year: 126,
            month: 2,
            day: 14,
            day_of_week: 255,
        };
        let t1 = BACnetTime {
            hour: 10,
            minute: 0,
            second: 0,
            hundredths: 0,
        };
        let t2 = BACnetTime {
            hour: 10,
            minute: 30,
            second: 0,
            hundredths: 0,
        };

        assert!(timestamp_sortable(&d, &t1) < timestamp_sortable(&d, &t2));
    }

    #[test]
    fn test_record_timestamp_sortable_extraction() {
        let record = make_record_value(10, 30, 42.0);
        let sortable = record_timestamp_sortable(&record);
        assert!(sortable > 0);
    }

    #[test]
    fn test_handler_creation() {
        let handler = ReadRangeHandler::new();
        assert_eq!(handler.service_choice(), ConfirmedService::ReadRange);
        assert_eq!(handler.name(), "ReadRange");
    }
}

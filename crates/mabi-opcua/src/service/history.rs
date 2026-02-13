//! HistoryRead service handler.
//!
//! OPC UA Part 4, Section 5.10.3.
//!
//! Wires the existing `HistoryStore` backend to the OPC UA binary protocol.
//! Supports ReadRawModified (encoding_id i=650) and ReadProcessed (encoding_id i=655)
//! detail types. Used by TRAP Phase 5's `HistoryContinuationResolver` and
//! `GapRecoveryManager`.

use std::sync::Arc;

use async_trait::async_trait;
use bytes::{BufMut, Bytes, BytesMut};

use crate::codec::encoder::BinaryEncodable;
use crate::codec::decoder::BinaryDecodable;
use crate::codec::data_value::ExtensionObject;
use crate::error::OpcUaResult;
use crate::types::{NodeId, StatusCode};
use crate::services::history::{
    ReadRawModifiedDetails, ReadProcessedDetails, AggregateType, AggregateConfig,
};
use super::discovery::{RequestHeader, ResponseHeader};
use super::registry::{ServiceContext, ServiceHandler, ServiceResponse};

const HISTORY_READ_REQUEST_ID: u32 = 662;
const HISTORY_READ_RESPONSE_ID: u32 = 665;

// ExtensionObject encoding IDs for HistoryReadDetails
const READ_RAW_MODIFIED_ENCODING_ID: u32 = 650;
const READ_PROCESSED_ENCODING_ID: u32 = 655;

// ExtensionObject encoding ID for HistoryData result
const HISTORY_DATA_ENCODING_ID: u32 = 658;

/// Parsed history read details.
enum ParsedDetails {
    RawModified(ReadRawModifiedDetails),
    Processed(ReadProcessedDetails),
}

pub struct HistoryReadHandler;

#[async_trait]
impl ServiceHandler for HistoryReadHandler {
    fn request_type_id(&self) -> NodeId {
        NodeId::numeric(0, HISTORY_READ_REQUEST_ID)
    }

    async fn handle(
        &self,
        request_body: &[u8],
        context: &ServiceContext,
    ) -> OpcUaResult<ServiceResponse> {
        let mut buf = Bytes::copy_from_slice(request_body);
        let header = RequestHeader::decode(&mut buf)?;
        let _additional_header = ExtensionObject::decode(&mut buf)?;

        // HistoryReadDetails — ExtensionObject
        let details_ext = ExtensionObject::decode(&mut buf)?;
        let details = decode_history_read_details(&details_ext)?;

        // TimestampsToReturn (enum: 0=Source, 1=Server, 2=Both, 3=Neither)
        let _timestamps_to_return = u32::decode(&mut buf)?;

        // ReleaseContinuationPoints
        let release_continuation_points = bool::decode(&mut buf)?;

        // NodesToRead array (HistoryReadValueId)
        let count = i32::decode(&mut buf)?;

        let mut out = BytesMut::new();
        ResponseHeader::good(header.request_handle).encode(&mut out)?;

        // Results array
        out.put_i32_le(count);
        for _ in 0..count.max(0) {
            // HistoryReadValueId
            let node_id = NodeId::decode(&mut buf)?;
            let _index_range = String::decode(&mut buf)?;
            // DataEncoding (QualifiedName)
            let _encoding_ns = u16::decode(&mut buf)?;
            let _encoding_name = String::decode(&mut buf)?;
            // ContinuationPoint (ByteString)
            let cp_len = i32::decode(&mut buf)?;
            let continuation_point = if cp_len > 0 {
                let mut cp = vec![0u8; cp_len as usize];
                for b in cp.iter_mut() {
                    *b = u8::decode(&mut buf)?;
                }
                Some(cp)
            } else {
                None
            };

            // Handle release
            if release_continuation_points {
                if let Some(cp) = &continuation_point {
                    release_history_cp(cp, context);
                }
                // HistoryReadResult: StatusCode + null CP + null HistoryData
                StatusCode::GOOD.encode(&mut out)?;
                out.put_i32_le(-1); // null ByteString
                encode_null_extension_object(&mut out);
                continue;
            }

            // Continue from a previous read
            if let Some(cp) = &continuation_point {
                if let Some(cp_id) = decode_cp_id(cp) {
                    let result = context.history_store.continue_read(cp_id);
                    encode_history_result(&mut out, &result);
                    continue;
                }
            }

            // Fresh read
            match &details {
                Some(ParsedDetails::RawModified(raw_details)) => {
                    let result = context.history_store.read_raw(&node_id, raw_details);
                    encode_history_result(&mut out, &result);
                }
                Some(ParsedDetails::Processed(proc_details)) => {
                    let result = context.history_store.read_processed(&node_id, proc_details);
                    encode_history_result(&mut out, &result);
                }
                None => {
                    // Unsupported details type
                    StatusCode::BAD_HISTORY_OPERATION_UNSUPPORTED.encode(&mut out)?;
                    out.put_i32_le(-1);
                    encode_null_extension_object(&mut out);
                }
            }
        }

        // DiagnosticInfos (empty)
        out.put_i32_le(0);

        Ok(ServiceResponse {
            type_id: NodeId::numeric(0, HISTORY_READ_RESPONSE_ID),
            body: out.to_vec(),
        })
    }
}

/// Decode HistoryReadDetails from an ExtensionObject.
fn decode_history_read_details(ext: &ExtensionObject) -> OpcUaResult<Option<ParsedDetails>> {
    let type_id_num = ext.type_id.as_numeric().unwrap_or(0);
    let body = match &ext.body {
        Some(b) => b,
        None => return Ok(None),
    };

    let mut buf = Bytes::copy_from_slice(body);

    match type_id_num {
        READ_RAW_MODIFIED_ENCODING_ID => {
            let is_read_modified = bool::decode(&mut buf)?;
            let start_time = chrono::DateTime::<chrono::Utc>::decode(&mut buf)?;
            let end_time = chrono::DateTime::<chrono::Utc>::decode(&mut buf)?;
            let num_values_per_node = u32::decode(&mut buf)?;
            let return_bounds = bool::decode(&mut buf)?;

            Ok(Some(ParsedDetails::RawModified(ReadRawModifiedDetails {
                start_time,
                end_time,
                num_values_per_node,
                return_bounds,
                is_read_modified,
            })))
        }
        READ_PROCESSED_ENCODING_ID => {
            let start_time = chrono::DateTime::<chrono::Utc>::decode(&mut buf)?;
            let end_time = chrono::DateTime::<chrono::Utc>::decode(&mut buf)?;
            let processing_interval = f64::decode(&mut buf)?;

            // AggregateType array (NodeId[])
            let agg_count = i32::decode(&mut buf)?;
            let mut aggregate_type = AggregateType::Average; // default
            if agg_count > 0 {
                let agg_node_id = NodeId::decode(&mut buf)?;
                aggregate_type = aggregate_type_from_node_id(&agg_node_id);
                // Skip remaining aggregate types if more than one
                for _ in 1..agg_count {
                    let _ = NodeId::decode(&mut buf)?;
                }
            }

            // AggregateConfiguration — skip (use defaults)

            Ok(Some(ParsedDetails::Processed(ReadProcessedDetails {
                start_time,
                end_time,
                processing_interval_ms: processing_interval,
                aggregate_type,
                aggregate_config: AggregateConfig::default(),
            })))
        }
        _ => Ok(None), // Unsupported details type
    }
}

/// Map OPC UA aggregate type NodeId to AggregateType enum.
fn aggregate_type_from_node_id(node_id: &NodeId) -> AggregateType {
    match node_id.as_numeric() {
        Some(2342) => AggregateType::Average,
        Some(2346) => AggregateType::Minimum,
        Some(2347) => AggregateType::Maximum,
        Some(2352) => AggregateType::Count,
        Some(2355) => AggregateType::Total,
        Some(2362) => AggregateType::Range,
        Some(2365) => AggregateType::StandardDeviation,
        Some(2368) => AggregateType::Variance,
        Some(2371) => AggregateType::Start,
        Some(2372) => AggregateType::End,
        Some(2374) => AggregateType::Delta,
        Some(2419) => AggregateType::TimeAverage,
        Some(2420) => AggregateType::TimeAverage, // TimeAverage2 → TimeAverage
        Some(2422) => AggregateType::WorstQuality,
        Some(2424) => AggregateType::PercentGood,
        Some(2425) => AggregateType::PercentBad,
        Some(11285) => AggregateType::DurationInState, // DurationGood
        Some(11286) => AggregateType::DurationInState, // DurationBad
        Some(11304) => AggregateType::NumberOfTransitions,
        _ => AggregateType::Average, // Default fallback
    }
}

/// Encode a HistoryReadResult into the output buffer.
fn encode_history_result(
    out: &mut BytesMut,
    result: &crate::services::history::HistoryReadResult,
) {
    result.status.encode(out).ok();

    // ContinuationPoint (ByteString)
    match &result.continuation_point {
        Some(cp) => {
            let cp_bytes = cp.id.to_le_bytes();
            out.put_i32_le(8); // ByteString length
            out.extend_from_slice(&cp_bytes);
        }
        None => {
            out.put_i32_le(-1); // null ByteString
        }
    }

    // HistoryData ExtensionObject (encoding_id i=658)
    // TypeId
    NodeId::numeric(0, HISTORY_DATA_ENCODING_ID).encode(out).ok();
    // Encoding byte: 0x01 = has binary body
    out.put_u8(0x01);

    // Encode body to temporary buffer to get length
    let mut body_buf = BytesMut::new();
    // DataValues array
    body_buf.put_i32_le(result.data_values.len() as i32);
    for dp in &result.data_values {
        dp.value.encode(&mut body_buf).ok();
    }

    // Body length + body
    out.put_i32_le(body_buf.len() as i32);
    out.extend_from_slice(&body_buf);
}

/// Encode a null ExtensionObject.
fn encode_null_extension_object(out: &mut BytesMut) {
    NodeId::numeric(0, 0).encode(out).ok();
    out.put_u8(0x00); // No body
}

/// Decode a continuation point ID from 8-byte LE ByteString.
fn decode_cp_id(cp: &[u8]) -> Option<u64> {
    if cp.len() == 8 {
        Some(u64::from_le_bytes([
            cp[0], cp[1], cp[2], cp[3],
            cp[4], cp[5], cp[6], cp[7],
        ]))
    } else {
        None
    }
}

/// Release a history continuation point.
fn release_history_cp(cp: &[u8], context: &ServiceContext) {
    if let Some(cp_id) = decode_cp_id(cp) {
        // The continue_read method removes the CP automatically.
        // For explicit release, we just call continue_read and discard the result.
        let _ = context.history_store.continue_read(cp_id);
    }
}

/// Register all history service handlers.
pub fn register_handlers(registry: &mut super::registry::ServiceRegistry) {
    registry.register(Arc::new(HistoryReadHandler));
}

use mabi_bacnet::object::property::{BACnetDate, BACnetTime};
use mabi_bacnet::prelude::{
    ApduDecoder, BACnetValue, ErrorClass, ErrorCode, IAmResponse, ObjectId, PropertyId,
};

use super::client::{ApduFrame, ReceivedPacket};
use super::contract;
use super::interop::{ActivePeerTranscript, Bacpypes3CanaryTranscript};

#[derive(Debug)]
pub struct ReadPropertyAck {
    pub object_id: ObjectId,
    pub property_id: PropertyId,
    pub array_index: Option<u32>,
    pub value: BACnetValue,
}

#[derive(Debug)]
pub struct PropertyMultipleItem {
    pub property_id: PropertyId,
    pub value: Option<BACnetValue>,
    pub error: Option<(u32, u32)>,
}

#[derive(Debug)]
pub struct PropertyMultipleObjectResult {
    pub object_id: ObjectId,
    pub items: Vec<PropertyMultipleItem>,
}

#[derive(Debug)]
pub struct ErrorAck {
    pub invoke_id: u8,
    pub service_choice: u8,
    pub error_class: u32,
    pub error_code: u32,
}

#[derive(Debug)]
pub struct AtomicReadFileStreamAck {
    pub eof: bool,
    pub start_position: i32,
    pub data: Vec<u8>,
}

#[derive(Debug)]
pub struct ReadRangeAckSummary {
    pub object_id: ObjectId,
    pub property_id: PropertyId,
    pub item_count: u32,
    pub more_follows: bool,
}

pub fn assert_profile_contract(id: &str, capabilities: &[&str]) {
    let profile = contract::profile(id);
    assert_eq!(
        profile.lane, "deterministic",
        "profile {id} should stay deterministic"
    );
    assert_eq!(
        profile.phase_introduced, "phase_1",
        "profile {id} should remain a phase_1 profile"
    );
    let expected: Vec<String> = capabilities
        .iter()
        .map(|capability| capability.to_string())
        .collect();
    assert_eq!(
        profile.capabilities, expected,
        "profile {id} capabilities drifted"
    );
}

pub fn assert_capability_integration_coverage(id: &str, expected: &str) {
    let capability = contract::capability(id);
    assert_eq!(
        capability.integration_coverage, expected,
        "capability {id} integration coverage drifted"
    );
}

pub fn assert_capability_interop_coverage(id: &str, expected: &str) {
    let capability = contract::capability(id);
    assert_eq!(
        capability.interop_coverage, expected,
        "capability {id} interop coverage drifted"
    );
}

pub fn assert_peer_lane(id: &str, expected: &str) {
    let peer = contract::peer(id);
    assert_eq!(
        peer.automation_lane, expected,
        "peer {id} automation lane drifted"
    );
}

pub fn assert_peer_ci_participation(id: &str, excluded_from_ci: bool) {
    let peer = contract::peer(id);
    assert_eq!(
        peer.excluded_from_current_ci, excluded_from_ci,
        "peer {id} CI participation drifted"
    );
}

#[derive(Debug, Clone, Copy)]
pub struct ActivePeerExpectations<'a> {
    pub peer: &'a str,
    pub require_discovery: bool,
    pub require_read: bool,
    pub require_write: bool,
    pub require_property_multiple: bool,
    pub expected_round_trip_value: Option<f64>,
}

pub fn assert_active_peer_transcript(
    transcript: &ActivePeerTranscript,
    expected_sut_addr: &str,
    expected_device_instance: u32,
    expectations: ActivePeerExpectations<'_>,
) {
    assert_eq!(transcript.peer, expectations.peer);
    assert_eq!(transcript.sut_addr, expected_sut_addr);
    assert_eq!(transcript.device_instance, expected_device_instance);

    if expectations.require_discovery {
        assert!(
            transcript.discovery_ok,
            "{} discovery should succeed",
            expectations.peer
        );
    }
    if expectations.require_read {
        assert!(
            transcript.read_ok,
            "{} read should succeed",
            expectations.peer
        );
    }
    if expectations.require_write {
        assert!(
            transcript.write_ok,
            "{} write should succeed",
            expectations.peer
        );
    }
    if expectations.require_property_multiple {
        assert!(
            transcript.property_multiple_ok,
            "{} property-multiple flow should succeed",
            expectations.peer
        );
    }
    if let Some(expected_round_trip_value) = expectations.expected_round_trip_value {
        assert!(
            (transcript.round_trip_value - expected_round_trip_value).abs() < 0.01,
            "unexpected {} round-trip value: {:?}",
            expectations.peer,
            transcript.round_trip_value
        );
    }
    assert!(
        transcript.errors.is_empty(),
        "{} transcript should not report errors: {:?}",
        expectations.peer,
        transcript.errors
    );
}

pub fn assert_bacpypes3_canary_transcript(
    transcript: &Bacpypes3CanaryTranscript,
    expected_sut_addr: &str,
    expected_device_instance: u32,
    expected_round_trip_value: f64,
) {
    assert_active_peer_transcript(
        transcript,
        expected_sut_addr,
        expected_device_instance,
        ActivePeerExpectations {
            peer: "bacpypes3",
            require_discovery: true,
            require_read: true,
            require_write: true,
            require_property_multiple: false,
            expected_round_trip_value: Some(expected_round_trip_value),
        },
    );
}

pub fn expect_i_am(packet: &ReceivedPacket, expected_device_instance: u32) {
    match &packet.apdu {
        Some(ApduFrame::UnconfirmedRequest {
            service_choice,
            data,
        }) => {
            assert_eq!(*service_choice, 0, "expected I-Am service");
            let iam = IAmResponse::decode(data).expect("I-Am payload should decode");
            assert_eq!(
                iam.device_identifier.instance, expected_device_instance,
                "unexpected device instance in I-Am"
            );
        }
        other => panic!("expected I-Am packet, got {other:?}"),
    }
}

pub fn expect_simple_ack(packet: &ReceivedPacket, invoke_id: u8, service_choice: u8) {
    match &packet.apdu {
        Some(ApduFrame::SimpleAck {
            invoke_id: got_invoke_id,
            service_choice: got_service,
        }) => {
            assert_eq!(*got_invoke_id, invoke_id, "unexpected invoke id");
            assert_eq!(
                *got_service, service_choice,
                "unexpected simple ack service"
            );
        }
        other => panic!("expected simple ack, got {other:?}"),
    }
}

pub fn expect_error(packet: &ReceivedPacket, invoke_id: u8, service_choice: u8) -> ErrorAck {
    match &packet.apdu {
        Some(ApduFrame::Error {
            invoke_id: got_invoke_id,
            service_choice: got_service,
            error_class,
            error_code,
        }) => {
            assert_eq!(*got_invoke_id, invoke_id, "unexpected invoke id");
            assert_eq!(*got_service, service_choice, "unexpected error service");
            ErrorAck {
                invoke_id: *got_invoke_id,
                service_choice: *got_service,
                error_class: *error_class,
                error_code: *error_code,
            }
        }
        other => panic!("expected error ack, got {other:?}"),
    }
}

pub fn decode_read_property_ack(packet: &ReceivedPacket) -> ReadPropertyAck {
    let service_data = match &packet.apdu {
        Some(ApduFrame::ComplexAck { data, .. }) => data.as_slice(),
        other => panic!("expected complex ack for ReadProperty, got {other:?}"),
    };

    let mut decoder = ApduDecoder::new(service_data);

    let (tag0, is_ctx0, _) = decoder.decode_tag_info().expect("object tag should decode");
    assert!(
        is_ctx0 && tag0 == 0,
        "expected object identifier context tag"
    );
    let object_id = decoder
        .decode_object_identifier()
        .expect("object id should decode");

    let (tag1, is_ctx1, len1) = decoder
        .decode_tag_info()
        .expect("property tag should decode");
    assert!(
        is_ctx1 && tag1 == 1,
        "expected property identifier context tag"
    );
    let property_raw = decoder
        .decode_unsigned(len1)
        .expect("property id should decode");
    let property_id = PropertyId::from_u32(property_raw).expect("property id should map");

    let mut array_index = None;
    if !decoder.is_empty() && !decoder.is_opening_tag(3) {
        let (tag2, is_ctx2, len2) = decoder.decode_tag_info().expect("array tag should decode");
        assert!(is_ctx2 && tag2 == 2, "expected array index context tag");
        array_index = Some(
            decoder
                .decode_unsigned(len2)
                .expect("array index should decode"),
        );
    }

    assert!(
        decoder.is_opening_tag(3),
        "expected property value opening tag"
    );
    decoder.read_u8().expect("opening tag should be consumed");
    let value = decode_value(&mut decoder);
    assert!(
        decoder.is_closing_tag(3),
        "expected property value closing tag"
    );

    ReadPropertyAck {
        object_id,
        property_id,
        array_index,
        value,
    }
}

pub fn decode_property_multiple_ack(packet: &ReceivedPacket) -> Vec<PropertyMultipleObjectResult> {
    let service_data = match &packet.apdu {
        Some(ApduFrame::ComplexAck { data, .. }) => data.as_slice(),
        other => panic!("expected complex ack for ReadPropertyMultiple, got {other:?}"),
    };

    let mut decoder = ApduDecoder::new(service_data);
    let mut results = Vec::new();

    while !decoder.is_empty() {
        let (tag0, is_ctx0, _) = decoder.decode_tag_info().expect("object tag should decode");
        assert!(
            is_ctx0 && tag0 == 0,
            "expected object identifier context tag"
        );
        let object_id = decoder
            .decode_object_identifier()
            .expect("object id should decode");

        assert!(
            decoder.is_opening_tag(1),
            "expected property list opening tag"
        );
        decoder
            .read_u8()
            .expect("property list opening tag should consume");

        let mut items = Vec::new();
        while !decoder.is_closing_tag(1) {
            let (prop_tag, is_ctx, prop_len) = decoder
                .decode_tag_info()
                .expect("property item tag should decode");
            assert!(is_ctx && prop_tag == 2, "expected property identifier tag");
            let property_id = PropertyId::from_u32(
                decoder
                    .decode_unsigned(prop_len)
                    .expect("property id should decode"),
            )
            .expect("property id should map");

            let mut array_index = None;
            if !decoder.is_empty() && !decoder.is_opening_tag(4) && !decoder.is_opening_tag(5) {
                let (tag3, is_ctx3, len3) = decoder
                    .decode_tag_info()
                    .expect("array index tag should decode");
                assert!(is_ctx3 && tag3 == 3, "expected array index tag");
                array_index = Some(decoder.decode_unsigned(len3).expect("array index decode"));
            }

            if decoder.is_opening_tag(4) {
                decoder.read_u8().expect("value opening tag should consume");
                let value = decode_value(&mut decoder);
                assert!(decoder.is_closing_tag(4), "expected value closing tag");
                decoder.read_u8().expect("value closing tag should consume");
                items.push(PropertyMultipleItem {
                    property_id,
                    value: Some(value),
                    error: None,
                });
            } else if decoder.is_opening_tag(5) {
                decoder.read_u8().expect("error opening tag should consume");
                let error_class = decode_enumerated(&mut decoder);
                let error_code = decode_enumerated(&mut decoder);
                assert!(decoder.is_closing_tag(5), "expected error closing tag");
                decoder.read_u8().expect("error closing tag should consume");
                let _ = array_index;
                items.push(PropertyMultipleItem {
                    property_id,
                    value: None,
                    error: Some((error_class, error_code)),
                });
            } else {
                panic!("expected property value or error branch");
            }
        }

        decoder
            .read_u8()
            .expect("property list closing tag should consume");
        results.push(PropertyMultipleObjectResult { object_id, items });
    }

    results
}

pub fn decode_atomic_write_stream_ack(packet: &ReceivedPacket) -> i32 {
    let data = match &packet.apdu {
        Some(ApduFrame::ComplexAck { data, .. }) => data.as_slice(),
        other => panic!("expected complex ack for AtomicWriteFile, got {other:?}"),
    };

    let mut decoder = ApduDecoder::new(data);
    let (tag, is_context, len) = decoder.decode_tag_info().expect("ack tag should decode");
    assert!(is_context && tag == 0, "expected stream write context tag");
    decoder
        .decode_signed(len)
        .expect("write start position should decode")
}

pub fn decode_atomic_read_stream_ack(packet: &ReceivedPacket) -> AtomicReadFileStreamAck {
    let data = match &packet.apdu {
        Some(ApduFrame::ComplexAck { data, .. }) => data.as_slice(),
        other => panic!("expected complex ack for AtomicReadFile, got {other:?}"),
    };

    decode_atomic_read_stream_ack_data(data)
}

pub fn decode_atomic_read_stream_ack_data(data: &[u8]) -> AtomicReadFileStreamAck {
    let mut decoder = ApduDecoder::new(data);
    let (bool_tag, is_context, bool_len) =
        decoder.decode_tag_info().expect("EOF tag should decode");
    assert!(!is_context && bool_tag == 1, "expected application boolean");
    let eof = bool_len != 0;

    assert!(decoder.is_opening_tag(0), "expected stream opening tag");
    decoder
        .read_u8()
        .expect("stream opening tag should consume");

    let (tag, is_context, len) = decoder
        .decode_tag_info()
        .expect("start position should decode");
    assert!(
        !is_context && tag == 3,
        "expected signed integer start position"
    );
    let start_position = decoder.decode_signed(len).expect("start position decode");

    let (octet_tag, octet_ctx, octet_len) = decoder
        .decode_tag_info()
        .expect("file data tag should decode");
    assert!(
        !octet_ctx && octet_tag == 6,
        "expected octet string file payload"
    );
    let data = decoder
        .read_bytes(octet_len)
        .expect("file payload should decode")
        .to_vec();

    assert!(decoder.is_closing_tag(0), "expected stream closing tag");

    AtomicReadFileStreamAck {
        eof,
        start_position,
        data,
    }
}

pub fn decode_read_range_ack(packet: &ReceivedPacket) -> ReadRangeAckSummary {
    let data = match &packet.apdu {
        Some(ApduFrame::ComplexAck { data, .. }) => data.as_slice(),
        other => panic!("expected complex ack for ReadRange, got {other:?}"),
    };

    let mut decoder = ApduDecoder::new(data);

    let (tag0, is_ctx0, _) = decoder.decode_tag_info().expect("object tag should decode");
    assert!(is_ctx0 && tag0 == 0, "expected object id context tag");
    let object_id = decoder
        .decode_object_identifier()
        .expect("object id should decode");

    let (tag1, is_ctx1, len1) = decoder
        .decode_tag_info()
        .expect("property tag should decode");
    assert!(is_ctx1 && tag1 == 1, "expected property id context tag");
    let property_id = PropertyId::from_u32(
        decoder
            .decode_unsigned(len1)
            .expect("property id should decode"),
    )
    .expect("property id should map");

    let (tag3, is_ctx3, len3) = decoder
        .decode_tag_info()
        .expect("result flags should decode");
    assert!(is_ctx3 && tag3 == 3, "expected result flags context tag");
    let flags = decoder
        .decode_bit_string(len3)
        .expect("result flags should decode");
    let more_follows = flags.get(2).copied().unwrap_or(false);

    let (tag4, is_ctx4, len4) = decoder.decode_tag_info().expect("item count should decode");
    assert!(is_ctx4 && tag4 == 4, "expected item count context tag");
    let item_count = decoder
        .decode_unsigned(len4)
        .expect("item count should decode");

    assert!(decoder.is_opening_tag(5), "expected item data opening tag");

    ReadRangeAckSummary {
        object_id,
        property_id,
        item_count,
        more_follows,
    }
}

pub fn decode_value(decoder: &mut ApduDecoder<'_>) -> BACnetValue {
    let (tag, is_context, len) = decoder.decode_tag_info().expect("value tag should decode");
    if is_context {
        return BACnetValue::Unsigned(
            decoder
                .decode_unsigned(len)
                .expect("context-tagged value should decode"),
        );
    }

    match tag {
        0 => BACnetValue::Null,
        1 => BACnetValue::Boolean(len != 0),
        2 => BACnetValue::Unsigned(decoder.decode_unsigned(len).expect("unsigned decode")),
        3 => BACnetValue::Signed(decoder.decode_signed(len).expect("signed decode")),
        4 => BACnetValue::Real(decoder.decode_real().expect("real decode")),
        5 => BACnetValue::Double(decoder.decode_double().expect("double decode")),
        6 => BACnetValue::OctetString(
            decoder
                .read_bytes(len)
                .expect("octet string decode")
                .to_vec(),
        ),
        7 => BACnetValue::CharacterString(
            decoder.decode_character_string(len).expect("string decode"),
        ),
        8 => BACnetValue::BitString(decoder.decode_bit_string(len).expect("bit string decode")),
        9 => BACnetValue::Enumerated(decoder.decode_unsigned(len).expect("enumerated decode")),
        10 => {
            let bytes = decoder.read_bytes(len).expect("date decode");
            BACnetValue::Date(BACnetDate {
                year: bytes[0],
                month: bytes[1],
                day: bytes[2],
                day_of_week: bytes[3],
            })
        }
        11 => {
            let bytes = decoder.read_bytes(len).expect("time decode");
            BACnetValue::Time(BACnetTime {
                hour: bytes[0],
                minute: bytes[1],
                second: bytes[2],
                hundredths: bytes[3],
            })
        }
        12 => BACnetValue::ObjectIdentifier(
            decoder
                .decode_object_identifier()
                .expect("object identifier decode"),
        ),
        other => panic!("unsupported BACnet value tag {other} in deterministic integration test"),
    }
}

fn decode_enumerated(decoder: &mut ApduDecoder<'_>) -> u32 {
    let (tag, is_context, len) = decoder
        .decode_tag_info()
        .expect("enumerated tag should decode");
    assert!(!is_context && tag == 9, "expected application enumerated");
    decoder
        .decode_unsigned(len)
        .expect("enumerated value should decode")
}

pub fn assert_error_code(error: &ErrorAck, expected_class: ErrorClass, expected_code: ErrorCode) {
    assert_eq!(
        error.error_class, expected_class as u32,
        "unexpected error class"
    );
    assert_eq!(
        error.error_code, expected_code as u32,
        "unexpected error code"
    );
}

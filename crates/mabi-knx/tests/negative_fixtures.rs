use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use mabi_knx::{
    ConnectionStateResponse, DptId, DptRegistry, DptValue, Hpai, KnxError, KnxFrame,
    ReceivedValidation, SequenceTracker, ServiceType,
};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct VerificationContract {
    capabilities: Vec<ContractEntry>,
    profiles: Vec<ContractEntry>,
}

#[derive(Debug, Deserialize)]
struct ContractEntry {
    id: String,
}

#[derive(Debug, Deserialize)]
struct FixtureCatalog {
    version: u32,
    fixtures: Vec<NegativeFixture>,
}

#[derive(Debug, Deserialize)]
struct NegativeFixture {
    id: String,
    category: String,
    replay_kind: String,
    input_hex: Option<String>,
    expected_error_kind: String,
    expected_message_contains: String,
    profile_ids: Vec<String>,
    capability_ids: Vec<String>,
    channel_id: Option<u8>,
    status: Option<u8>,
    sequence_case: Option<String>,
    service_type: Option<String>,
    dpt: Option<String>,
}

const CONTRACT: &str = include_str!("../../../docs/knx-simulator/verification-contract.yaml");

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("mabi-knx crate should live two levels below repo root")
        .to_path_buf()
}

fn fixtures_root() -> PathBuf {
    repo_root().join("verification/knx/fixtures")
}

fn fixture_catalog_path() -> PathBuf {
    fixtures_root().join("catalog.toml")
}

fn load_contract() -> VerificationContract {
    serde_yaml::from_str(CONTRACT).expect("KNX verification contract should parse")
}

fn load_fixture_catalog() -> FixtureCatalog {
    toml::from_str(
        &fs::read_to_string(fixture_catalog_path()).expect("negative fixture catalog should exist"),
    )
    .expect("negative fixture catalog should parse")
}

fn ids(entries: &[ContractEntry]) -> HashSet<&str> {
    entries.iter().map(|entry| entry.id.as_str()).collect()
}

fn hex_bytes(input: &str) -> Vec<u8> {
    input
        .split_whitespace()
        .map(|part| u8::from_str_radix(part, 16).expect("fixture hex byte should parse"))
        .collect()
}

fn error_kind(error: &KnxError) -> &'static str {
    match error {
        KnxError::InvalidHeader(_) => "InvalidHeader",
        KnxError::InvalidProtocolVersion { .. } => "InvalidProtocolVersion",
        KnxError::UnknownServiceType(_) => "UnknownServiceType",
        KnxError::FrameLengthMismatch { .. } => "FrameLengthMismatch",
        KnxError::InvalidHpai(_) => "InvalidHpai",
        KnxError::DptDecoding { .. } => "DptDecoding",
        KnxError::InvalidChannel(_) => "InvalidChannel",
        _ => "Other",
    }
}

fn assert_knx_error(error: KnxError, fixture: &NegativeFixture) {
    assert_eq!(
        error_kind(&error),
        fixture.expected_error_kind,
        "fixture {} returned wrong error kind: {error}",
        fixture.id
    );
    assert!(
        error
            .to_string()
            .contains(&fixture.expected_message_contains),
        "fixture {} error `{}` did not contain `{}`",
        fixture.id,
        error,
        fixture.expected_message_contains
    );
}

fn service_from_name(name: &str) -> ServiceType {
    match name {
        "RoutingIndication" => ServiceType::RoutingIndication,
        "DeviceConfigurationRequest" => ServiceType::DeviceConfigurationRequest,
        other => panic!("unsupported service fixture type {other}"),
    }
}

#[test]
fn negative_fixture_catalog_matches_contract_and_categories() {
    let contract = load_contract();
    let catalog = load_fixture_catalog();
    assert_eq!(catalog.version, 1);

    let profile_ids = ids(&contract.profiles);
    let capability_ids = ids(&contract.capabilities);
    let mut categories = HashSet::new();

    for fixture in &catalog.fixtures {
        assert!(!fixture.id.trim().is_empty());
        assert!(!fixture.category.trim().is_empty());
        assert!(!fixture.replay_kind.trim().is_empty());
        assert!(!fixture.expected_error_kind.trim().is_empty());
        assert!(!fixture.expected_message_contains.trim().is_empty());
        assert!(!fixture.profile_ids.is_empty());
        assert!(!fixture.capability_ids.is_empty());
        categories.insert(fixture.category.as_str());

        let category_dir = fixtures_root().join(&fixture.category);
        assert!(
            category_dir.is_dir(),
            "fixture category directory missing for {}",
            fixture.category
        );

        for profile_id in &fixture.profile_ids {
            assert!(
                profile_ids.contains(profile_id.as_str()),
                "fixture {} references unknown profile {}",
                fixture.id,
                profile_id
            );
        }
        for capability_id in &fixture.capability_ids {
            assert!(
                capability_ids.contains(capability_id.as_str()),
                "fixture {} references unknown capability {}",
                fixture.id,
                capability_id
            );
        }
    }

    for expected in ["malformed", "hpai", "tunnel", "sequence", "service", "dpt"] {
        assert!(
            categories.contains(expected),
            "negative fixture catalog missing {expected} category"
        );
    }
}

#[test]
fn static_negative_fixtures_replay_against_knx_core() {
    let catalog = load_fixture_catalog();
    for fixture in &catalog.fixtures {
        match fixture.replay_kind.as_str() {
            "knx_frame_decode" => {
                let input = hex_bytes(fixture.input_hex.as_ref().expect("input_hex required"));
                let error = KnxFrame::decode(&input).expect_err("fixture must fail frame decode");
                assert_knx_error(error, fixture);
            }
            "hpai_decode" => {
                let input = hex_bytes(fixture.input_hex.as_ref().expect("input_hex required"));
                let error = Hpai::decode(&input).expect_err("fixture must fail HPAI decode");
                assert_knx_error(error, fixture);
            }
            "connection_state_response" => {
                let channel_id = fixture.channel_id.expect("channel_id required");
                let status = fixture.status.expect("status required");
                let response = ConnectionStateResponse::decode(&[channel_id, status])
                    .expect("connection state response should decode");
                assert_eq!(response.channel_id, channel_id);
                assert_eq!(response.status, status);
                assert_eq!(fixture.expected_error_kind, "KnxConnectionId");
                assert!(
                    format!("status {status:#04x}").contains(&fixture.expected_message_contains),
                    "fixture {} expected stable status text",
                    fixture.id
                );
            }
            "typed_error" => {
                let channel_id = fixture.channel_id.expect("channel_id required");
                assert_knx_error(KnxError::InvalidChannel(channel_id), fixture);
            }
            "sequence_validation" => replay_sequence_fixture(fixture),
            "service_support" => {
                let service = service_from_name(
                    fixture
                        .service_type
                        .as_deref()
                        .expect("service_type required"),
                );
                assert!(
                    service.is_routing()
                        || matches!(service, ServiceType::DeviceConfigurationRequest),
                    "fixture {} should only encode known unsupported default-lane services",
                    fixture.id
                );
                assert_eq!(fixture.expected_error_kind, "UnsupportedFeature");
                assert!(
                    "unsupported_feature".contains(&fixture.expected_message_contains),
                    "fixture {} expected unsupported feature classification",
                    fixture.id
                );
            }
            "dpt_decode" => {
                let (codec, input) = dpt_codec_and_input(fixture);
                let error = codec
                    .decode(&input)
                    .expect_err("fixture must fail DPT decode");
                assert_knx_error(error, fixture);
            }
            "dpt_decode_fallback_value" => {
                let (codec, input) = dpt_codec_and_input(fixture);
                let value = codec.decode(&input).expect("fallback value should decode");
                assert_eq!(fixture.expected_error_kind, "FallbackValue");
                assert!(
                    matches!(value, DptValue::U8(255)),
                    "fixture {} expected fallback U8(255), got {value:?}",
                    fixture.id
                );
                assert!(
                    format!("{value:?}").contains(&fixture.expected_message_contains),
                    "fixture {} fallback value text mismatch",
                    fixture.id
                );
            }
            "dpt_main_type_fallback" => {
                let registry = DptRegistry::new();
                let dpt: DptId = fixture
                    .dpt
                    .as_deref()
                    .expect("dpt required")
                    .parse()
                    .expect("fixture DPT should parse");
                let codec = registry
                    .get(&dpt)
                    .expect("main type fallback codec should exist");
                let input = hex_bytes(fixture.input_hex.as_ref().expect("input_hex required"));
                let value = codec.decode(&input).expect("fallback codec should decode");
                assert_eq!(fixture.expected_error_kind, "FallbackCodec");
                assert_eq!(codec.name(), fixture.expected_message_contains);
                assert!(matches!(value, DptValue::F16(_)));
            }
            other => panic!("unsupported replay kind {other} for fixture {}", fixture.id),
        }
    }
}

fn dpt_codec_and_input(
    fixture: &NegativeFixture,
) -> (std::sync::Arc<mabi_knx::BoxedDptCodec>, Vec<u8>) {
    let registry = DptRegistry::new();
    let dpt: DptId = fixture
        .dpt
        .as_deref()
        .expect("dpt required")
        .parse()
        .expect("fixture DPT should parse");
    let codec = registry
        .get_or_err(&dpt)
        .expect("fixture DPT codec should exist");
    let input = hex_bytes(fixture.input_hex.as_ref().expect("input_hex required"));
    (codec, input)
}

fn replay_sequence_fixture(fixture: &NegativeFixture) {
    let sequence_message = match fixture
        .sequence_case
        .as_deref()
        .expect("sequence_case required")
    {
        "duplicate" => {
            let tracker = SequenceTracker::new();
            assert!(matches!(
                tracker.validate_received(0),
                ReceivedValidation::Valid { sequence: 0 }
            ));
            assert!(matches!(
                tracker.validate_received(0),
                ReceivedValidation::Duplicate {
                    sequence: 0,
                    expected: 1
                }
            ));
            assert_eq!(fixture.expected_error_kind, "Duplicate");
            "duplicate sequence"
        }
        "out_of_order" => {
            let tracker = SequenceTracker::new();
            assert!(matches!(
                tracker.validate_received(0),
                ReceivedValidation::Valid { sequence: 0 }
            ));
            assert!(matches!(
                tracker.validate_received(2),
                ReceivedValidation::OutOfOrder {
                    sequence: 2,
                    expected: 1,
                    distance: 1
                }
            ));
            assert_eq!(fixture.expected_error_kind, "OutOfOrder");
            "out-of-order sequence"
        }
        "wraparound" => {
            let tracker = SequenceTracker::new();
            for expected in 0..=255 {
                assert_eq!(tracker.next_sno(), expected);
            }
            assert_eq!(tracker.next_sno(), 0);
            assert_eq!(fixture.expected_error_kind, "Wraparound");
            "wraparound"
        }
        "fatal_desync" => {
            let tracker = SequenceTracker::new();
            assert!(matches!(
                tracker.validate_received(5),
                ReceivedValidation::FatalDesync {
                    sequence: 5,
                    expected: 0,
                    distance: 5
                }
            ));
            assert_eq!(fixture.expected_error_kind, "FatalDesync");
            "fatal sequence desync"
        }
        other => panic!("unsupported sequence fixture case {other}"),
    };

    assert!(
        sequence_message.contains(&fixture.expected_message_contains.to_ascii_lowercase())
            || sequence_message.contains(&fixture.expected_message_contains),
        "fixture {} sequence expectation should include stable text",
        fixture.id
    );
}

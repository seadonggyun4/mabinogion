mod support;

use std::net::SocketAddr;
use std::time::Duration;

use mabi_bacnet::object::BACnetObject;
use mabi_bacnet::prelude::{
    BACnetServer, BACnetValue, ConfirmedService, ErrorClass, ErrorCode, ObjectId, ObjectRegistry,
    ObjectType, PropertyId, UnconfirmedService,
};

use support::assertions::{
    assert_capability_integration_coverage, assert_error_code, assert_profile_contract,
    decode_property_multiple_ack, decode_read_property_ack, expect_error, expect_i_am,
    expect_simple_ack, ReadPropertyAck,
};
use support::client::{
    encode_read_property_multiple_request, encode_read_property_request,
    encode_read_property_request_with_array_index, encode_who_is_all,
    encode_write_property_multiple_request, encode_write_property_request, LoopbackClient,
};
use support::fixtures::{loopback_server_config, property_fixture};
use support::server_harness::BacnetServerHarness;

async fn read_property(
    client: &LoopbackClient,
    server_addr: SocketAddr,
    invoke_id: u8,
    object_id: ObjectId,
    property_id: PropertyId,
) -> ReadPropertyAck {
    client
        .send_confirmed_request(
            server_addr,
            ConfirmedService::ReadProperty,
            invoke_id,
            encode_read_property_request(object_id, property_id),
            false,
        )
        .await
        .expect("ReadProperty should send");
    decode_read_property_ack(&client.recv_packet(Duration::from_secs(2)).await)
}

async fn read_property_at(
    client: &LoopbackClient,
    server_addr: SocketAddr,
    invoke_id: u8,
    object_id: ObjectId,
    property_id: PropertyId,
    array_index: u32,
) -> ReadPropertyAck {
    client
        .send_confirmed_request(
            server_addr,
            ConfirmedService::ReadProperty,
            invoke_id,
            encode_read_property_request_with_array_index(object_id, property_id, array_index),
            false,
        )
        .await
        .expect("ReadProperty with array index should send");
    decode_read_property_ack(&client.recv_packet(Duration::from_secs(2)).await)
}

#[tokio::test]
async fn basic_ip_profile_discovers_the_device_over_loopback() {
    assert_profile_contract("basic_ip", &["discovery"]);
    assert_capability_integration_coverage("discovery", "deterministic");

    let server = BACnetServer::new(loopback_server_config(4101), property_fixture().registry);
    let harness = BacnetServerHarness::start(server).await;
    let client = LoopbackClient::bind().await;

    client
        .send_unconfirmed_request(
            harness.addr(),
            UnconfirmedService::WhoIs,
            encode_who_is_all(),
        )
        .await
        .expect("Who-Is should send");

    let packet = client.recv_packet(Duration::from_secs(2)).await;
    expect_i_am(&packet, 4101);

    harness.shutdown().await;
}

#[tokio::test]
async fn empty_registry_default_device_metadata_is_explorer_readable() {
    assert_profile_contract("basic_ip", &["discovery"]);
    assert_capability_integration_coverage("discovery", "deterministic");
    assert_capability_integration_coverage("property_io", "deterministic");

    let config = loopback_server_config(4104);
    let device_id = ObjectId::new(ObjectType::Device, config.device_instance);
    let expected_device_name = config.device_name.clone();
    let expected_model_name = config.model_name.clone();
    let expected_vendor_id = config.vendor_id;
    let server = BACnetServer::new(config, ObjectRegistry::new());
    let harness = BacnetServerHarness::start(server).await;
    let client = LoopbackClient::bind().await;

    // phase1.empty_registry_whois: core discovery is automatic.
    client
        .send_unconfirmed_request(
            harness.addr(),
            UnconfirmedService::WhoIs,
            encode_who_is_all(),
        )
        .await
        .expect("Who-Is should send");
    let packet = client.recv_packet(Duration::from_secs(2)).await;
    expect_i_am(&packet, device_id.instance);

    // phase1.device_object_name: empty-registry UX still exposes the Device name.
    let object_name = read_property(
        &client,
        harness.addr(),
        10,
        device_id,
        PropertyId::ObjectName,
    )
    .await;
    assert_eq!(object_name.object_id, device_id);
    assert_eq!(object_name.property_id, PropertyId::ObjectName);
    assert_eq!(
        object_name.value,
        BACnetValue::CharacterString(expected_device_name)
    );

    // phase1.object_list_full: full-array encoding includes the mandatory Device object.
    let object_list = read_property(
        &client,
        harness.addr(),
        11,
        device_id,
        PropertyId::ObjectList,
    )
    .await;
    assert_eq!(object_list.object_id, device_id);
    assert_eq!(object_list.property_id, PropertyId::ObjectList);
    assert_eq!(
        object_list.value,
        BACnetValue::Array(vec![BACnetValue::ObjectIdentifier(device_id)])
    );

    // phase1.object_list_indexed: BACnet array semantics use 0=count and 1-based elements.
    let object_list_count = read_property_at(
        &client,
        harness.addr(),
        12,
        device_id,
        PropertyId::ObjectList,
        0,
    )
    .await;
    assert_eq!(object_list_count.array_index, Some(0));
    assert_eq!(object_list_count.value, BACnetValue::Unsigned(1));

    let first_object = read_property_at(
        &client,
        harness.addr(),
        13,
        device_id,
        PropertyId::ObjectList,
        1,
    )
    .await;
    assert_eq!(first_object.array_index, Some(1));
    assert_eq!(first_object.value, BACnetValue::ObjectIdentifier(device_id));

    client
        .send_confirmed_request(
            harness.addr(),
            ConfirmedService::ReadProperty,
            14,
            encode_read_property_request_with_array_index(device_id, PropertyId::ObjectList, 2),
            false,
        )
        .await
        .expect("invalid Object_List index request should send");
    let invalid_index_packet = client.recv_packet(Duration::from_secs(2)).await;
    let error = expect_error(
        &invalid_index_packet,
        14,
        ConfirmedService::ReadProperty as u8,
    );
    assert_error_code(&error, ErrorClass::Property, ErrorCode::InvalidArrayIndex);

    // phase1.protocol_metadata: explorer-readable protocol metadata decodes cleanly.
    let services = read_property(
        &client,
        harness.addr(),
        15,
        device_id,
        PropertyId::ProtocolServicesSupported,
    )
    .await;
    match services.value {
        BACnetValue::BitString(bits) => assert!(
            bits.iter().any(|bit| *bit),
            "Protocol_Services_Supported should expose at least one service"
        ),
        other => panic!("expected Protocol_Services_Supported bitstring, got {other:?}"),
    }

    let object_types = read_property(
        &client,
        harness.addr(),
        16,
        device_id,
        PropertyId::ProtocolObjectTypesSupported,
    )
    .await;
    match object_types.value {
        BACnetValue::BitString(bits) => assert!(
            bits.iter().any(|bit| *bit),
            "Protocol_Object_Types_Supported should expose at least one object type"
        ),
        other => panic!("expected Protocol_Object_Types_Supported bitstring, got {other:?}"),
    }

    let vendor_name = read_property(
        &client,
        harness.addr(),
        17,
        device_id,
        PropertyId::VendorName,
    )
    .await;
    assert!(
        matches!(vendor_name.value, BACnetValue::CharacterString(ref name) if !name.is_empty())
    );

    let vendor_identifier = read_property(
        &client,
        harness.addr(),
        18,
        device_id,
        PropertyId::VendorIdentifier,
    )
    .await;
    assert_eq!(
        vendor_identifier.value,
        BACnetValue::Unsigned(expected_vendor_id as u32)
    );

    let model_name = read_property(
        &client,
        harness.addr(),
        19,
        device_id,
        PropertyId::ModelName,
    )
    .await;
    assert_eq!(
        model_name.value,
        BACnetValue::CharacterString(expected_model_name)
    );

    for (invoke_id, property_id) in [
        (20, PropertyId::FirmwareRevision),
        (21, PropertyId::ApplicationSoftwareVersion),
    ] {
        let version_value =
            read_property(&client, harness.addr(), invoke_id, device_id, property_id)
                .await
                .value;
        assert!(
            matches!(version_value, BACnetValue::CharacterString(ref version) if !version.is_empty()),
            "{property_id} should be a non-empty string"
        );
    }

    harness.shutdown().await;
}

#[tokio::test]
async fn property_io_profile_round_trips_and_surfaces_errors() {
    assert_profile_contract("property_io", &["property_io"]);
    assert_capability_integration_coverage("property_io", "deterministic");

    let fixture = property_fixture();
    let analog_output_id = fixture.analog_output.object_identifier();
    let server = BACnetServer::new(loopback_server_config(4102), fixture.registry);
    let harness = BacnetServerHarness::start(server).await;
    let client = LoopbackClient::bind().await;

    client
        .send_confirmed_request(
            harness.addr(),
            ConfirmedService::WriteProperty,
            1,
            encode_write_property_request(
                analog_output_id,
                PropertyId::PresentValue,
                &BACnetValue::Real(42.5),
            ),
            false,
        )
        .await
        .expect("WriteProperty should send");
    let write_ack = client.recv_packet(Duration::from_secs(2)).await;
    expect_simple_ack(&write_ack, 1, ConfirmedService::WriteProperty as u8);

    client
        .send_confirmed_request(
            harness.addr(),
            ConfirmedService::ReadProperty,
            2,
            encode_read_property_request(analog_output_id, PropertyId::PresentValue),
            false,
        )
        .await
        .expect("ReadProperty should send");
    let read_ack = client.recv_packet(Duration::from_secs(2)).await;
    let decoded = decode_read_property_ack(&read_ack);
    assert_eq!(decoded.object_id, analog_output_id);
    assert_eq!(decoded.property_id, PropertyId::PresentValue);
    assert_eq!(decoded.value, BACnetValue::Real(42.5));

    client
        .send_confirmed_request(
            harness.addr(),
            ConfirmedService::ReadProperty,
            3,
            encode_read_property_request(
                ObjectId::new(ObjectType::AnalogOutput, 999),
                PropertyId::PresentValue,
            ),
            false,
        )
        .await
        .expect("invalid ReadProperty should send");
    let error_packet = client.recv_packet(Duration::from_secs(2)).await;
    let error = expect_error(&error_packet, 3, ConfirmedService::ReadProperty as u8);
    assert_error_code(&error, ErrorClass::Object, ErrorCode::UnknownObject);

    client
        .send_confirmed_request(
            harness.addr(),
            ConfirmedService::WriteProperty,
            4,
            encode_write_property_request(
                analog_output_id,
                PropertyId::ObjectIdentifier,
                &BACnetValue::ObjectIdentifier(analog_output_id),
            ),
            false,
        )
        .await
        .expect("read-only WriteProperty should send");
    let readonly_error = client.recv_packet(Duration::from_secs(2)).await;
    let error = expect_error(&readonly_error, 4, ConfirmedService::WriteProperty as u8);
    assert_error_code(&error, ErrorClass::Property, ErrorCode::WriteAccessDenied);

    harness.shutdown().await;
}

#[tokio::test]
async fn property_multiple_profile_keeps_mixed_results_deterministic() {
    assert_profile_contract("property_multiple", &["property_multiple"]);
    assert_capability_integration_coverage("property_multiple", "deterministic");

    let fixture = property_fixture();
    let analog_output_id = fixture.analog_output.object_identifier();
    let server = BACnetServer::new(loopback_server_config(4103), fixture.registry);
    let harness = BacnetServerHarness::start(server).await;
    let client = LoopbackClient::bind().await;

    client
        .send_confirmed_request(
            harness.addr(),
            ConfirmedService::ReadPropertyMultiple,
            5,
            encode_read_property_multiple_request(&[
                (analog_output_id, vec![PropertyId::PresentValue]),
                (
                    ObjectId::new(ObjectType::AnalogOutput, 999),
                    vec![PropertyId::PresentValue],
                ),
            ]),
            false,
        )
        .await
        .expect("ReadPropertyMultiple should send");
    let rpm_packet = client.recv_packet(Duration::from_secs(2)).await;
    let results = decode_property_multiple_ack(&rpm_packet);
    assert_eq!(results.len(), 2);
    assert_eq!(results[0].object_id, analog_output_id);
    assert_eq!(results[0].items[0].value, Some(BACnetValue::Real(12.5)));
    assert_eq!(
        results[1].items[0].error,
        Some((ErrorClass::Object as u32, ErrorCode::UnknownObject as u32))
    );

    client
        .send_confirmed_request(
            harness.addr(),
            ConfirmedService::WritePropertyMultiple,
            6,
            encode_write_property_multiple_request(
                analog_output_id,
                &[
                    (PropertyId::PresentValue, BACnetValue::Real(21.0)),
                    (
                        PropertyId::ObjectIdentifier,
                        BACnetValue::ObjectIdentifier(analog_output_id),
                    ),
                ],
            ),
            false,
        )
        .await
        .expect("WritePropertyMultiple should send");
    let wpm_error_packet = client.recv_packet(Duration::from_secs(2)).await;
    let error = expect_error(
        &wpm_error_packet,
        6,
        ConfirmedService::WritePropertyMultiple as u8,
    );
    assert_error_code(&error, ErrorClass::Property, ErrorCode::WriteAccessDenied);

    harness.shutdown().await;
}

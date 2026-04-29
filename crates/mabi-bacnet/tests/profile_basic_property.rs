mod support;

use std::time::Duration;

use mabi_bacnet::prelude::{
    BACnetValue, ConfirmedService, ErrorClass, ErrorCode, ObjectId, ObjectType, PropertyId,
    UnconfirmedService, BACnetServer,
};
use mabi_bacnet::object::BACnetObject;

use support::assertions::{
    assert_capability_integration_coverage, assert_error_code, assert_profile_contract,
    decode_property_multiple_ack, decode_read_property_ack, expect_error, expect_i_am,
    expect_simple_ack,
};
use support::client::{
    encode_read_property_multiple_request, encode_read_property_request,
    encode_who_is_all, encode_write_property_multiple_request, encode_write_property_request,
    LoopbackClient,
};
use support::fixtures::{loopback_server_config, property_fixture};
use support::server_harness::BacnetServerHarness;

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

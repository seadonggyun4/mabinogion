mod support;

use std::time::Duration;

use mabi_bacnet::object::property::{BACnetDate, BACnetTime};
use mabi_bacnet::prelude::{
    ConfirmedService, CommunicationControlState, ObjectType, PropertyId, ReinitializedState,
    UnconfirmedService, BACnetServer,
};

use support::assertions::{
    assert_capability_integration_coverage, assert_profile_contract, decode_read_property_ack,
    expect_simple_ack,
};
use support::client::{
    encode_create_object_request, encode_dcc_request, encode_delete_object_request,
    encode_read_property_request, encode_reinitialize_request, encode_subscribe_cov_request,
    encode_time_sync_request, LoopbackClient,
};
use support::fixtures::{cov_fixture, loopback_server_config, property_fixture};
use support::server_harness::BacnetServerHarness;

#[tokio::test]
async fn cov_flow_profile_subscribes_notifies_and_cancels() {
    assert_profile_contract("cov_flow", &["cov"]);
    assert_capability_integration_coverage("cov", "deterministic");

    let fixture = cov_fixture();
    let mut config = loopback_server_config(4201);
    config.cov_check_interval = Duration::from_millis(20);
    let server = BACnetServer::new(config, fixture.registry);
    let harness = BacnetServerHarness::start(server).await;
    let client = LoopbackClient::bind().await;

    client
        .send_confirmed_request(
            harness.addr(),
            ConfirmedService::SubscribeCov,
            11,
            encode_subscribe_cov_request(7, fixture.object_id, Some(false), Some(1)),
            false,
        )
        .await
        .expect("SubscribeCOV should send");
    let ack = client.recv_packet(Duration::from_secs(2)).await;
    expect_simple_ack(&ack, 11, ConfirmedService::SubscribeCov as u8);

    fixture.analog_input.set_value(20.0);
    let notification = client.recv_packet(Duration::from_secs(2)).await;
    match notification.apdu {
        Some(support::client::ApduFrame::UnconfirmedRequest { service_choice, .. }) => {
            assert_eq!(
                service_choice,
                UnconfirmedService::UnconfirmedCovNotification as u8
            );
        }
        other => panic!("expected unconfirmed COV notification, got {other:?}"),
    }

    client
        .send_confirmed_request(
            harness.addr(),
            ConfirmedService::SubscribeCov,
            12,
            encode_subscribe_cov_request(7, fixture.object_id, None, None),
            false,
        )
        .await
        .expect("SubscribeCOV cancel should send");
    let cancel_ack = client.recv_packet(Duration::from_secs(2)).await;
    expect_simple_ack(&cancel_ack, 12, ConfirmedService::SubscribeCov as u8);

    fixture.analog_input.set_value(21.0);
    client.expect_no_packet(Duration::from_millis(250)).await;

    harness.shutdown().await;
}

#[tokio::test]
async fn create_delete_profile_creates_network_visible_objects() {
    assert_profile_contract("create_delete", &["create_delete"]);
    assert_capability_integration_coverage("create_delete", "deterministic");

    let server = BACnetServer::new(loopback_server_config(4202), property_fixture().registry);
    let harness = BacnetServerHarness::start(server).await;
    let client = LoopbackClient::bind().await;

    client
        .send_confirmed_request(
            harness.addr(),
            ConfirmedService::CreateObject,
            13,
            encode_create_object_request(ObjectType::AnalogInput),
            false,
        )
        .await
        .expect("CreateObject should send");
    let create_ack = client.recv_packet(Duration::from_secs(2)).await;
    match &create_ack.apdu {
        Some(support::client::ApduFrame::ComplexAck { invoke_id, .. }) => assert_eq!(*invoke_id, 13),
        other => panic!("expected create-object complex ack, got {other:?}"),
    }

    let created_id = mabi_bacnet::prelude::ObjectId::new(ObjectType::AnalogInput, 0);
    assert!(
        harness.server().objects().contains(&created_id),
        "created object should be visible through the canonical registry"
    );

    client
        .send_confirmed_request(
            harness.addr(),
            ConfirmedService::ReadProperty,
            14,
            encode_read_property_request(created_id, PropertyId::ObjectName),
            false,
        )
        .await
        .expect("ReadProperty for created object should send");
    let read_ack = client.recv_packet(Duration::from_secs(2)).await;
    let read = decode_read_property_ack(&read_ack);
    assert_eq!(read.object_id, created_id);

    client
        .send_confirmed_request(
            harness.addr(),
            ConfirmedService::DeleteObject,
            15,
            encode_delete_object_request(created_id),
            false,
        )
        .await
        .expect("DeleteObject should send");
    let delete_ack = client.recv_packet(Duration::from_secs(2)).await;
    expect_simple_ack(&delete_ack, 15, ConfirmedService::DeleteObject as u8);
    assert!(
        !harness.server().objects().contains(&created_id),
        "deleted object should be removed from the canonical registry"
    );

    harness.shutdown().await;
}

#[tokio::test]
async fn device_control_profile_mutates_and_resets_device_state() {
    assert_profile_contract("device_control", &["device_control_time_sync"]);
    assert_capability_integration_coverage("device_control_time_sync", "deterministic");

    let server = BACnetServer::new(loopback_server_config(4203), property_fixture().registry);
    let harness = BacnetServerHarness::start(server).await;
    let client = LoopbackClient::bind().await;

    let device_id = mabi_bacnet::prelude::ObjectId::new(ObjectType::Device, 4203);
    let device = harness
        .server()
        .objects()
        .get(&device_id)
        .expect("device object should exist");
    let device = device
        .as_any()
        .downcast_ref::<mabi_bacnet::prelude::DeviceObject>()
        .expect("device object should downcast");

    client
        .send_confirmed_request(
            harness.addr(),
            ConfirmedService::DeviceCommunicationControl,
            16,
            encode_dcc_request(1),
            false,
        )
        .await
        .expect("DCC should send");
    let dcc_ack = client.recv_packet(Duration::from_secs(2)).await;
    expect_simple_ack(
        &dcc_ack,
        16,
        ConfirmedService::DeviceCommunicationControl as u8,
    );
    assert_eq!(
        device.communication_control(),
        CommunicationControlState::Disabled
    );

    client
        .send_unconfirmed_request(
            harness.addr(),
            UnconfirmedService::TimeSynchronization,
            encode_time_sync_request(
                BACnetDate {
                    year: 130,
                    month: 1,
                    day: 1,
                    day_of_week: 1,
                },
                BACnetTime {
                    hour: 0,
                    minute: 0,
                    second: 0,
                    hundredths: 0,
                },
            ),
        )
        .await
        .expect("TimeSynchronization should send");
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert_ne!(device.time_offset_secs(), 0);

    client
        .send_confirmed_request(
            harness.addr(),
            ConfirmedService::ReinitializeDevice,
            17,
            encode_reinitialize_request(ReinitializedState::Coldstart as u32),
            false,
        )
        .await
        .expect("ReinitializeDevice should send");
    let reinit_ack = client.recv_packet(Duration::from_secs(2)).await;
    expect_simple_ack(
        &reinit_ack,
        17,
        ConfirmedService::ReinitializeDevice as u8,
    );
    assert_eq!(
        device.communication_control(),
        CommunicationControlState::Enabled
    );
    assert_eq!(device.time_offset_secs(), 0);

    harness.shutdown().await;
}

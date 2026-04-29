mod support;

use std::time::Duration;

use mabi_bacnet::network::bvlc::BvlcHeader;
use mabi_bacnet::network::BvlcResultCode;
use mabi_bacnet::prelude::{
    BbmdConfig, BvlcFunction, BvlcMessage, ConfirmedService, ObjectType, PropertyId, TsmConfig,
    BACnetServer,
};

use support::assertions::{
    assert_capability_integration_coverage, assert_profile_contract, decode_atomic_read_stream_ack_data,
};
use support::client::{
    encode_atomic_read_file_stream_request, encode_read_property_request, encode_who_is_all,
    LoopbackClient,
};
use support::fixtures::{loopback_server_config, segmentation_fixture};
use support::server_harness::BacnetServerHarness;

#[tokio::test]
async fn segmentation_profile_reassembles_segmented_complex_acks() {
    assert_profile_contract("segmentation", &["segmentation"]);
    assert_capability_integration_coverage("segmentation", "deterministic");

    let fixture = segmentation_fixture();
    let mut config = loopback_server_config(4401);
    config.max_apdu_length = 48;
    let server = BACnetServer::new(config, fixture.registry);
    let harness = BacnetServerHarness::start(server).await;
    let client = LoopbackClient::bind().await;

    client
        .send_confirmed_request(
            harness.addr(),
            ConfirmedService::AtomicReadFile,
            31,
            encode_atomic_read_file_stream_request(fixture.file_object_id.instance, 0, 200),
            true,
        )
        .await
        .expect("segmented AtomicReadFile should send");
    let first_packet = client.recv_packet(Duration::from_secs(2)).await;
    let segmented = client
        .collect_segmented_response(first_packet, Duration::from_secs(2))
        .await;
    assert_eq!(segmented.invoke_id, 31);
    assert_eq!(segmented.service_choice, ConfirmedService::AtomicReadFile as u8);
    assert!(
        segmented.segment_count > 1,
        "response should be segmented under a low max APDU length"
    );
    let ack = decode_atomic_read_stream_ack_data(&segmented.service_data);
    assert_eq!(ack.start_position, 0);
    assert_eq!(ack.data.len(), 200);

    harness.shutdown().await;
}

#[tokio::test]
async fn bbmd_fdr_profile_registers_foreign_devices_without_docker() {
    assert_profile_contract("bbmd_fdr", &["bbmd_foreign_device"]);
    assert_capability_integration_coverage("bbmd_foreign_device", "deterministic");

    let server = BACnetServer::new(loopback_server_config(4402), segmentation_fixture().registry)
        .with_bbmd_config(BbmdConfig::enabled());
    let harness = BacnetServerHarness::start(server).await;
    let client = LoopbackClient::bind().await;

    let register_msg = BvlcMessage {
        header: BvlcHeader::new(BvlcFunction::RegisterForeignDevice, 6),
        npdu: vec![0x00, 0x3C],
        original_source: None,
        result_code: None,
    };
    client
        .send_bvlc_message(harness.addr(), register_msg)
        .await
        .expect("foreign-device registration should send");
    let result_packet = client.recv_packet(Duration::from_secs(2)).await;
    assert_eq!(result_packet.bvlc.header.function, BvlcFunction::Result);
    assert_eq!(result_packet.bvlc.result_code, Some(BvlcResultCode::Success));
    let foreign_addr = match client.local_addr() {
        std::net::SocketAddr::V4(addr) => addr,
        other => panic!("expected IPv4 client address, got {other}"),
    };
    assert!(
        harness.server().bbmd().fdt().is_registered(&foreign_addr),
        "BBMD should retain the foreign-device registration"
    );

    client
        .send_unconfirmed_request(
            harness.addr(),
            mabi_bacnet::prelude::UnconfirmedService::WhoIs,
            encode_who_is_all(),
        )
        .await
        .expect("Who-Is should still work after foreign-device registration");
    let iam_packet = client.recv_packet(Duration::from_secs(2)).await;
    match iam_packet.apdu {
        Some(support::client::ApduFrame::UnconfirmedRequest { service_choice, .. }) => {
            assert_eq!(service_choice, 0);
        }
        other => panic!("expected I-Am response, got {other:?}"),
    }

    harness.shutdown().await;
}

#[tokio::test]
async fn tsm_resilience_profile_returns_cached_duplicate_responses() {
    assert_profile_contract("tsm_resilience", &["tsm_duplicate_handling"]);
    assert_capability_integration_coverage("tsm_duplicate_handling", "deterministic");

    let mut config = loopback_server_config(4403);
    config.max_apdu_length = 256;
    let server = BACnetServer::new(config, support::fixtures::property_fixture().registry)
        .with_tsm_config(TsmConfig::default());
    let harness = BacnetServerHarness::start(server).await;
    let client = LoopbackClient::bind().await;

    let request = encode_read_property_request(
        mabi_bacnet::prelude::ObjectId::new(ObjectType::Device, 4403),
        PropertyId::ObjectName,
    );
    client
        .send_confirmed_request(
            harness.addr(),
            ConfirmedService::ReadProperty,
            41,
            request.clone(),
            false,
        )
        .await
        .expect("first ReadProperty should send");
    let first = client.recv_packet(Duration::from_secs(2)).await;

    client
        .send_confirmed_request(
            harness.addr(),
            ConfirmedService::ReadProperty,
            41,
            request,
            false,
        )
        .await
        .expect("duplicate ReadProperty should send");
    let second = client.recv_packet(Duration::from_secs(2)).await;

    assert_eq!(format!("{:?}", first.apdu), format!("{:?}", second.apdu));
    let stats = harness.server().tsm().statistics();
    assert!(
        stats.duplicate_count >= 1,
        "TSM should record duplicate request handling"
    );

    harness.shutdown().await;
}

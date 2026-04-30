mod support;

use std::time::Duration;

use mabi_bacnet::prelude::{
    BACnetServer, BACnetValue, ConfirmedService, PropertyId, ReadRangeRequest,
};

use support::assertions::{
    assert_capability_integration_coverage, assert_profile_contract, decode_atomic_read_stream_ack,
    decode_atomic_write_stream_ack, decode_read_property_ack, decode_read_range_ack,
};
use support::client::{
    encode_atomic_read_file_stream_request, encode_atomic_write_file_stream_request,
    encode_read_property_request, encode_read_range_by_position_request, LoopbackClient,
};
use support::fixtures::{
    file_and_trend_fixture, loopback_server_config, make_date, make_time, schedule_calendar_fixture,
};
use support::server_harness::BacnetServerHarness;

#[tokio::test]
async fn file_and_trend_profile_supports_file_io_and_read_range() {
    assert_profile_contract("file_and_trend", &["file_access", "read_range_trend_log"]);
    assert_capability_integration_coverage("file_access", "deterministic");
    assert_capability_integration_coverage("read_range_trend_log", "deterministic");

    let fixture = file_and_trend_fixture();
    let file_object_id = fixture.file_object_id;
    let trend_log_id = fixture.trend_log_id;
    let file = fixture.file.clone();
    let server = BACnetServer::new(loopback_server_config(4301), fixture.registry);
    let harness = BacnetServerHarness::start(server).await;
    let client = LoopbackClient::bind().await;

    client
        .send_confirmed_request(
            harness.addr(),
            ConfirmedService::AtomicWriteFile,
            21,
            encode_atomic_write_file_stream_request(file_object_id.instance, 0, b"abc"),
            false,
        )
        .await
        .expect("AtomicWriteFile should send");
    let write_ack = client.recv_packet(Duration::from_secs(2)).await;
    let start_position = decode_atomic_write_stream_ack(&write_ack);
    assert_eq!(start_position, 0);
    let (stored_bytes, eof) = file.read_stream(0, 3);
    assert!(eof);
    assert_eq!(stored_bytes, b"abc");

    client
        .send_confirmed_request(
            harness.addr(),
            ConfirmedService::AtomicReadFile,
            22,
            encode_atomic_read_file_stream_request(file_object_id.instance, 0, 3),
            false,
        )
        .await
        .expect("AtomicReadFile should send");
    let read_ack = client.recv_packet(Duration::from_secs(2)).await;
    let stream_ack = decode_atomic_read_stream_ack(&read_ack);
    assert_eq!(stream_ack.start_position, 0);
    assert!(stream_ack.eof);
    assert_eq!(stream_ack.data, b"abc");

    let read_range_request =
        encode_read_range_by_position_request(trend_log_id, PropertyId::LogBuffer, 1, 2);
    let decoded_request = ReadRangeRequest::decode(&read_range_request).unwrap_or_else(|err| {
        panic!(
            "loopback read-range helper should stay aligned with the server decoder: bytes={read_range_request:02X?}, err={err:?}"
        )
    });
    assert_eq!(decoded_request.object_id, trend_log_id);
    assert_eq!(decoded_request.property_id, PropertyId::LogBuffer);

    client
        .send_confirmed_request(
            harness.addr(),
            ConfirmedService::ReadRange,
            23,
            read_range_request,
            false,
        )
        .await
        .expect("ReadRange should send");
    let read_range_ack = client.recv_packet(Duration::from_secs(2)).await;
    let summary = decode_read_range_ack(&read_range_ack);
    assert_eq!(summary.object_id, trend_log_id);
    assert_eq!(summary.property_id, PropertyId::LogBuffer);
    assert_eq!(summary.item_count, 2);
    assert!(!summary.more_follows);

    harness.shutdown().await;
}

#[tokio::test]
async fn schedule_calendar_profile_reads_evaluated_present_values() {
    assert_profile_contract("schedule_calendar", &["schedule_calendar"]);
    assert_capability_integration_coverage("schedule_calendar", "deterministic");

    let fixture = schedule_calendar_fixture();
    fixture
        .schedule
        .evaluate(&make_date(2026, 12, 25, 5), &make_time(10, 0, 0));
    fixture.calendar.evaluate(&make_date(2026, 12, 25, 5));
    let schedule_id = fixture.schedule_id;
    let calendar_id = fixture.calendar_id;
    let server = BACnetServer::new(loopback_server_config(4302), fixture.registry);
    let harness = BacnetServerHarness::start(server).await;
    let client = LoopbackClient::bind().await;

    client
        .send_confirmed_request(
            harness.addr(),
            ConfirmedService::ReadProperty,
            24,
            encode_read_property_request(schedule_id, PropertyId::PresentValue),
            false,
        )
        .await
        .expect("Schedule ReadProperty should send");
    let schedule_ack = client.recv_packet(Duration::from_secs(2)).await;
    let schedule_value = decode_read_property_ack(&schedule_ack);
    assert_eq!(schedule_value.value, BACnetValue::Real(55.0));

    client
        .send_confirmed_request(
            harness.addr(),
            ConfirmedService::ReadProperty,
            25,
            encode_read_property_request(calendar_id, PropertyId::PresentValue),
            false,
        )
        .await
        .expect("Calendar ReadProperty should send");
    let calendar_ack = client.recv_packet(Duration::from_secs(2)).await;
    let calendar_value = decode_read_property_ack(&calendar_ack);
    assert_eq!(calendar_value.value, BACnetValue::Boolean(true));

    harness.shutdown().await;
}

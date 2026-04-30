mod support;

use std::time::Duration;

use mabi_knx::tunnel::{ConnectionRequestInfo, KnxLayer};
use mabi_knx::{
    Apci, CemiFrame, ConnectRequest, DptId, DptRegistry, DptValue, GroupAddress, IndividualAddress,
    KnxFrame, MessageCode, ServiceType, TunnellingAck, TunnellingRequest,
};
use tokio::time::timeout;

use support::assertions::{decode_ok_ack, decode_successful_connect};
use support::contract::assert_profile_lane;
use support::fixtures::{
    standard_group_table, COUNTER, FLOAT, HVAC, RGB, SCALING, SIGNED_COUNTER, SWITCH, TEMPERATURE,
    TEXT,
};
use support::frame_client::FrameClient;
use support::server_harness::ServerHarness;
use support::TestResult;

async fn connect_tunnel(client: &FrameClient, server_addr: std::net::SocketAddr) -> TestResult<u8> {
    let connect = ConnectRequest::new(
        client.local_hpai()?,
        client.local_hpai()?,
        ConnectionRequestInfo::tunnel(KnxLayer::LinkLayer),
    );
    let response = client
        .request_response(
            server_addr,
            KnxFrame::new(ServiceType::ConnectRequest, connect.encode()),
            ServiceType::ConnectResponse,
        )
        .await?;
    Ok(decode_successful_connect(&response)?.channel_id)
}

async fn send_group_write(
    client: &FrameClient,
    server_addr: std::net::SocketAddr,
    channel_id: u8,
    sequence: u8,
    address: GroupAddress,
    data: Vec<u8>,
) -> TestResult {
    let mut cemi = CemiFrame::group_value_write(IndividualAddress::new(1, 1, 10), address, data);
    cemi.message_code = MessageCode::LDataReq;
    let request = TunnellingRequest::new(channel_id, sequence, cemi);
    client
        .send_frame(
            server_addr,
            KnxFrame::new(ServiceType::TunnellingRequest, request.encode()),
        )
        .await?;
    let ack = client.recv_frame().await?;
    decode_ok_ack(&ack, channel_id, sequence)?;
    drain_optional_ldata_con(client, server_addr, channel_id).await?;
    Ok(())
}

async fn drain_optional_ldata_con(
    client: &FrameClient,
    server_addr: std::net::SocketAddr,
    channel_id: u8,
) -> TestResult {
    let frame = match timeout(
        Duration::from_millis(250),
        client.recv_until(|frame| {
            if frame.service_type != ServiceType::TunnellingRequest {
                return Ok(false);
            }
            let request = TunnellingRequest::decode(&frame.body)?;
            Ok(request.channel_id == channel_id
                && request.cemi.message_code == MessageCode::LDataCon)
        }),
    )
    .await
    {
        Ok(frame) => frame?,
        Err(_) => return Ok(()),
    };
    let request = TunnellingRequest::decode(&frame.body)?;
    let ack = TunnellingAck::ok(request.channel_id, request.sequence_counter);
    client
        .send_frame(
            server_addr,
            KnxFrame::new(ServiceType::TunnellingAck, ack.encode()),
        )
        .await?;
    Ok(())
}

async fn read_group_response(
    client: &FrameClient,
    server_addr: std::net::SocketAddr,
    channel_id: u8,
    sequence: u8,
    address: GroupAddress,
) -> TestResult<CemiFrame> {
    let mut cemi = CemiFrame::group_value_read(IndividualAddress::new(1, 1, 10), address);
    cemi.message_code = MessageCode::LDataReq;
    let request = TunnellingRequest::new(channel_id, sequence, cemi);
    client
        .send_frame(
            server_addr,
            KnxFrame::new(ServiceType::TunnellingRequest, request.encode()),
        )
        .await?;
    let ack = client.recv_frame().await?;
    decode_ok_ack(&ack, channel_id, sequence)?;

    let frame = client
        .recv_until(|frame| {
            if frame.service_type != ServiceType::TunnellingRequest {
                return Ok(false);
            }
            let request = TunnellingRequest::decode(&frame.body)?;
            Ok(matches!(request.cemi.apci, Apci::GroupValueResponse)
                && request.cemi.destination_group() == Some(address))
        })
        .await?;
    let request = TunnellingRequest::decode(&frame.body)?;

    let ack = TunnellingAck::ok(request.channel_id, request.sequence_counter);
    client
        .send_frame(
            server_addr,
            KnxFrame::new(ServiceType::TunnellingAck, ack.encode()),
        )
        .await?;
    drain_optional_ldata_con(client, server_addr, channel_id).await?;

    Ok(request.cemi)
}

fn assert_dpt_value_matches(actual: &DptValue, expected: &DptValue, dpt: &DptId) -> TestResult {
    match (actual, expected) {
        (DptValue::F16(actual), DptValue::F16(expected)) => {
            if (actual - expected).abs() <= 0.1 {
                Ok(())
            } else {
                Err(format!(
                    "DPT {} F16 semantic drift: actual={} expected={}",
                    dpt, actual, expected
                )
                .into())
            }
        }
        (DptValue::F32(actual), DptValue::F32(expected)) => {
            if (actual - expected).abs() <= 0.001 {
                Ok(())
            } else {
                Err(format!(
                    "DPT {} F32 semantic drift: actual={} expected={}",
                    dpt, actual, expected
                )
                .into())
            }
        }
        _ if actual == expected => Ok(()),
        _ => Err(format!(
            "DPT {} semantic drift: actual={:?} expected={:?}",
            dpt, actual, expected
        )
        .into()),
    }
}

fn assert_semantic_if_public(actual: &DptValue, expected: &DptValue, dpt: &DptId) -> TestResult {
    if dpt.main == 20 {
        if matches!(expected, DptValue::U8(3)) && actual.to_string() == "Economy" {
            return Ok(());
        }
        return Err(format!(
            "DPT {} HVAC semantic drift: actual={} expected raw mode {:?}",
            dpt, actual, expected
        )
        .into());
    }
    assert_dpt_value_matches(actual, expected, dpt)
}

#[tokio::test]
async fn group_io_profile_smoke() -> TestResult {
    assert_profile_lane("group_io", "deterministic")?;

    let table = standard_group_table()?;
    let harness = ServerHarness::start_default(table.clone()).await?;
    let client = FrameClient::bind_loopback().await?;
    let channel_id = connect_tunnel(&client, harness.addr).await?;
    let address: GroupAddress = SWITCH.parse()?;

    send_group_write(&client, harness.addr, channel_id, 0, address, vec![1]).await?;
    assert_eq!(table.read(&address)?, vec![1]);

    let response = read_group_response(&client, harness.addr, channel_id, 1, address).await?;
    assert!(matches!(response.apci, Apci::GroupValueResponse));
    assert_eq!(response.data, vec![1]);

    harness.shutdown().await
}

#[tokio::test]
async fn dpt_matrix_profile_smoke() -> TestResult {
    assert_profile_lane("dpt_matrix", "deterministic")?;

    let table = standard_group_table()?;
    let harness = ServerHarness::start_default(table.clone()).await?;
    let client = FrameClient::bind_loopback().await?;
    let channel_id = connect_tunnel(&client, harness.addr).await?;
    let registry = DptRegistry::new();

    let cases = vec![
        (SWITCH, DptId::new(1, 1), DptValue::Bool(true)),
        (SCALING, DptId::new(5, 1), DptValue::F16(42.0)),
        (TEMPERATURE, DptId::new(9, 1), DptValue::F16(21.5)),
        (COUNTER, DptId::new(12, 1), DptValue::U32(1234)),
        (SIGNED_COUNTER, DptId::new(13, 1), DptValue::I32(-55)),
        (FLOAT, DptId::new(14, 56), DptValue::F32(12.25)),
        (
            TEXT,
            DptId::new(16, 1),
            DptValue::String("hello".to_string()),
        ),
        (HVAC, DptId::new(20, 102), DptValue::U8(3)),
        (RGB, DptId::new(232, 600), DptValue::rgb(12, 34, 56)),
    ];

    for (index, (address, dpt, value)) in cases.into_iter().enumerate() {
        let address: GroupAddress = address.parse()?;
        let raw = registry.get_or_err(&dpt)?.encode(&value)?;
        let write_sequence = (index * 2) as u8;
        let read_sequence = write_sequence.wrapping_add(1);
        send_group_write(
            &client,
            harness.addr,
            channel_id,
            write_sequence,
            address,
            raw.clone(),
        )
        .await?;
        let stored_raw = table.read(&address)?;
        assert_eq!(
            stored_raw, raw,
            "DPT {} raw tunnel write did not update the table",
            dpt
        );
        let decoded = registry.get_or_err(&dpt)?.decode(&stored_raw)?;
        assert_semantic_if_public(&decoded, &value, &dpt)?;
        let response =
            read_group_response(&client, harness.addr, channel_id, read_sequence, address).await?;
        assert_eq!(
            response.data, raw,
            "DPT {} read response raw value drifted",
            dpt
        );
        let decoded = registry.get_or_err(&dpt)?.decode(&response.data)?;
        assert_semantic_if_public(&decoded, &value, &dpt)?;
    }

    harness.shutdown().await
}

mod support;

use std::time::Duration;

use mabi_knx::tunnel::{ConnectionRequestInfo, KnxLayer};
use mabi_knx::{
    CemiFrame, ConnectRequest, ConnectionStateRequest, GroupAddress, IndividualAddress, KnxFrame,
    KnxServerConfig, MessageCode, ServerEvent, ServiceType, TunnellingRequest,
};
use tokio::time::{sleep, timeout};

use support::assertions::{decode_ok_ack, decode_successful_connect};
use support::contract::assert_profile_lane;
use support::fixtures::standard_group_table;
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

async fn send_sequence(
    client: &FrameClient,
    server_addr: std::net::SocketAddr,
    channel_id: u8,
    sequence: u8,
) -> TestResult<Option<KnxFrame>> {
    let address: GroupAddress = "1/0/1".parse()?;
    let mut cemi = CemiFrame::group_value_write(IndividualAddress::new(1, 1, 10), address, vec![1]);
    cemi.message_code = MessageCode::LDataReq;
    let request = TunnellingRequest::new(channel_id, sequence, cemi);
    client
        .send_frame(
            server_addr,
            KnxFrame::new(ServiceType::TunnellingRequest, request.encode()),
        )
        .await?;

    match timeout(Duration::from_millis(500), client.recv_frame()).await {
        Ok(frame) => Ok(Some(frame?)),
        Err(_) => Ok(None),
    }
}

#[tokio::test]
async fn tunnel_resilience_profile_smoke() -> TestResult {
    assert_profile_lane("tunnel_resilience", "deterministic")?;

    let mut config = KnxServerConfig::default();
    config.tunnel_behavior.ldata_con_enabled = false;
    let harness = ServerHarness::start_with_table(config, standard_group_table()?).await?;
    let mut events = harness.subscribe();
    let client = FrameClient::bind_loopback().await?;
    let channel_id = connect_tunnel(&client, harness.addr).await?;

    let valid_ack = send_sequence(&client, harness.addr, channel_id, 0)
        .await?
        .ok_or("valid sequence did not ACK")?;
    decode_ok_ack(&valid_ack, channel_id, 0)?;

    let duplicate_ack = send_sequence(&client, harness.addr, channel_id, 0)
        .await?
        .ok_or("duplicate sequence did not ACK")?;
    decode_ok_ack(&duplicate_ack, channel_id, 0)?;

    let out_of_order_ack = send_sequence(&client, harness.addr, channel_id, 2)
        .await?
        .ok_or("out-of-order sequence did not ACK")?;
    decode_ok_ack(&out_of_order_ack, channel_id, 2)?;

    let connection_metrics = harness
        .server
        .connection_metrics()
        .into_iter()
        .find(|metrics| metrics.channel_id == channel_id)
        .ok_or("missing connection metrics for active tunnel")?;
    assert!(connection_metrics.duplicates_detected >= 1);
    assert!(connection_metrics.out_of_order_detected >= 1);

    let fatal_ack = send_sequence(&client, harness.addr, channel_id, 9).await?;
    assert!(fatal_ack.is_none(), "fatal desync must not be ACKed");

    timeout(Duration::from_secs(2), async {
        loop {
            if let ServerEvent::ClientDisconnected { channel_id: id } = events.recv().await? {
                if id == channel_id {
                    return Ok::<_, tokio::sync::broadcast::error::RecvError>(());
                }
            }
        }
    })
    .await??;
    assert_eq!(harness.server.connection_count(), 0);

    harness.shutdown().await
}

#[tokio::test]
async fn heartbeat_timeout_profile_smoke() -> TestResult {
    assert_profile_lane("heartbeat_timeout", "deterministic")?;

    let config = KnxServerConfig {
        connection_timeout_secs: 1,
        heartbeat_interval_secs: 1,
        ..Default::default()
    };
    let harness = ServerHarness::start_with_table(config, standard_group_table()?).await?;
    let client = FrameClient::bind_loopback().await?;
    let channel_id = connect_tunnel(&client, harness.addr).await?;

    let state = ConnectionStateRequest::new(channel_id, client.local_hpai()?);
    let response = client
        .request_response(
            harness.addr,
            KnxFrame::new(ServiceType::ConnectionStateRequest, state.encode()),
            ServiceType::ConnectionStateResponse,
        )
        .await?;
    assert_eq!(response.body, vec![channel_id, 0x00]);

    sleep(Duration::from_millis(1_150)).await;
    let timed_out = harness
        .server
        .get_connection(channel_id)
        .map(|connection| connection.is_timed_out())
        .unwrap_or(false);
    assert!(timed_out);

    let unknown = ConnectionStateRequest::new(channel_id.wrapping_add(1), client.local_hpai()?);
    let response = client
        .request_response(
            harness.addr,
            KnxFrame::new(ServiceType::ConnectionStateRequest, unknown.encode()),
            ServiceType::ConnectionStateResponse,
        )
        .await?;
    assert_eq!(response.body, vec![channel_id.wrapping_add(1), 0x21]);

    harness.shutdown().await
}

mod support;

use std::time::Duration;

use mabi_knx::tunnel::{ConnectionRequestInfo, KnxLayer};
use mabi_knx::{
    ConnectRequest, ConnectionStateRequest, DisconnectRequest, Hpai, KnxFrame, ServerEvent,
    ServiceType,
};
use tokio::time::timeout;

use support::assertions::{
    assert_dibs_include_device_and_tunnelling, assert_service, decode_disconnect,
    decode_successful_connect,
};
use support::contract::assert_profile_lane;
use support::fixtures::standard_group_table;
use support::frame_client::FrameClient;
use support::server_harness::ServerHarness;
use support::TestResult;

#[tokio::test]
async fn basic_discovery_profile_smoke() -> TestResult {
    assert_profile_lane("basic_discovery", "deterministic")?;

    let harness = ServerHarness::start_default(standard_group_table()?).await?;
    let client = FrameClient::bind_loopback().await?;

    let search = client
        .request_response(
            harness.addr,
            KnxFrame::empty(ServiceType::SearchRequest),
            ServiceType::SearchResponse,
        )
        .await?;
    let _hpai = Hpai::decode(&search.body)?;
    assert_dibs_include_device_and_tunnelling(&search.body[8..])?;

    let description = client
        .request_response(
            harness.addr,
            KnxFrame::empty(ServiceType::DescriptionRequest),
            ServiceType::DescriptionResponse,
        )
        .await?;
    assert_dibs_include_device_and_tunnelling(&description.body)?;

    harness.shutdown().await
}

#[tokio::test]
async fn tunnel_lifecycle_profile_smoke() -> TestResult {
    assert_profile_lane("tunnel_lifecycle", "deterministic")?;

    let harness = ServerHarness::start_default(standard_group_table()?).await?;
    let mut events = harness.subscribe();
    let client = FrameClient::bind_loopback().await?;

    let connect = ConnectRequest::new(
        client.local_hpai()?,
        client.local_hpai()?,
        ConnectionRequestInfo::tunnel(KnxLayer::LinkLayer),
    );
    let response = client
        .request_response(
            harness.addr,
            KnxFrame::new(ServiceType::ConnectRequest, connect.encode()),
            ServiceType::ConnectResponse,
        )
        .await?;
    let connect = decode_successful_connect(&response)?;
    let channel_id = connect.channel_id;
    assert_eq!(harness.server.connection_count(), 1);

    let connected = timeout(Duration::from_secs(2), async {
        loop {
            if let ServerEvent::ClientConnected { channel_id: id, .. } = events.recv().await? {
                if id == channel_id {
                    return Ok::<_, tokio::sync::broadcast::error::RecvError>(());
                }
            }
        }
    })
    .await?;
    connected?;

    let state = ConnectionStateRequest::new(channel_id, client.local_hpai()?);
    let response = client
        .request_response(
            harness.addr,
            KnxFrame::new(ServiceType::ConnectionStateRequest, state.encode()),
            ServiceType::ConnectionStateResponse,
        )
        .await?;
    assert_service(&response, ServiceType::ConnectionStateResponse)?;
    assert_eq!(response.body, vec![channel_id, 0x00]);

    let disconnect = DisconnectRequest::new(channel_id, client.local_hpai()?);
    let response = client
        .request_response(
            harness.addr,
            KnxFrame::new(ServiceType::DisconnectRequest, disconnect.encode()),
            ServiceType::DisconnectResponse,
        )
        .await?;
    decode_disconnect(&response, channel_id)?;
    assert_eq!(harness.server.connection_count(), 0);

    let removed = timeout(Duration::from_secs(2), async {
        loop {
            if let ServerEvent::ClientDisconnected { channel_id: id } = events.recv().await? {
                if id == channel_id {
                    return Ok::<_, tokio::sync::broadcast::error::RecvError>(());
                }
            }
        }
    })
    .await?;
    removed?;

    let state = ConnectionStateRequest::new(channel_id, client.local_hpai()?);
    let response = client
        .request_response(
            harness.addr,
            KnxFrame::new(ServiceType::ConnectionStateRequest, state.encode()),
            ServiceType::ConnectionStateResponse,
        )
        .await?;
    assert_eq!(response.body, vec![channel_id, 0x21]);

    harness.shutdown().await
}

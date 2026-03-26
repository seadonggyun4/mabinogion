mod support;

use std::net::{SocketAddr, TcpListener as StdTcpListener};
use std::sync::Arc;
use std::time::Duration;

use futures::{SinkExt, StreamExt};
use tokio::time::timeout;
use tokio_util::codec::Framed;

use mabi_modbus::rtu;
use mabi_modbus::rtu::{
    transport::ChannelConfig, ChannelTransport, ModbusRtuServer, RtuFrame, RtuTransport,
};
use mabi_modbus::tcp::{
    MbapCodec, MbapFrame, ModbusTcpServerV2, PerformancePreset, ServerConfigV2,
};

use support::{build_device, DeviceSpec};

fn reserve_tcp_addr() -> SocketAddr {
    let listener = StdTcpListener::bind("127.0.0.1:0").expect("should reserve loopback port");
    let addr = listener
        .local_addr()
        .expect("reserved listener should have address");
    drop(listener);
    addr
}

async fn connect_with_retry(bind_address: SocketAddr) -> tokio::net::TcpStream {
    for _ in 0..40 {
        match tokio::net::TcpStream::connect(bind_address).await {
            Ok(stream) => return stream,
            Err(_) => tokio::time::sleep(Duration::from_millis(10)).await,
        }
    }

    tokio::net::TcpStream::connect(bind_address)
        .await
        .expect("TCP server should accept loopback connections")
}

#[tokio::test]
async fn tcp_high_throughput_preserves_fc03_response() {
    let bind_address = reserve_tcp_addr();
    let config = ServerConfigV2 {
        bind_address,
        connection_timeout: Duration::from_millis(250),
        request_timeout: Duration::from_millis(250),
        shutdown_timeout: Duration::from_millis(500),
        performance_preset: PerformancePreset::HighThroughput,
        ..Default::default()
    };

    let server = Arc::new(ModbusTcpServerV2::new(config));
    let device = build_device(DeviceSpec::dense(1));
    device
        .registers()
        .write_holding_register(0, 0x1234)
        .unwrap();
    server.add_device(device);

    let run_server = server.clone();
    let server_task = tokio::spawn(async move {
        let _ = run_server.run().await;
    });

    let stream = connect_with_retry(bind_address).await;
    let mut framed = Framed::new(stream, MbapCodec::new());
    framed
        .send(MbapFrame::new(7, 1, vec![0x03, 0x00, 0x00, 0x00, 0x01]))
        .await
        .expect("request should be sent");

    let response = timeout(Duration::from_millis(250), framed.next())
        .await
        .expect("response should arrive")
        .expect("connection should stay open")
        .expect("response frame should decode");

    assert_eq!(response.pdu, vec![0x03, 0x02, 0x12, 0x34]);

    server.shutdown();
    let _ = timeout(Duration::from_secs(1), server_task).await;
}

#[tokio::test]
async fn rtu_high_throughput_preserves_fc03_response() {
    let config = rtu::RtuServerConfig::for_testing()
        .with_transport(rtu::TransportConfig::Channel(ChannelConfig::default()))
        .with_performance_preset(rtu::PerformancePreset::HighThroughput);
    let server = Arc::new(ModbusRtuServer::new(config));

    let device = build_device(DeviceSpec::dense(1));
    device
        .registers()
        .write_holding_register(0, 0x4321)
        .unwrap();
    server.add_device(device);

    let (server_transport, mut client_transport) = ChannelTransport::pair(ChannelConfig::default());
    let run_server = server.clone();
    let server_task = tokio::spawn(async move {
        let _ = run_server.run_with_transport(server_transport).await;
    });

    client_transport
        .write(&RtuFrame::new(1, vec![0x03, 0x00, 0x00, 0x00, 0x01]).encode())
        .await
        .expect("RTU request should be sent");

    let mut buffer = [0u8; 256];
    let size = timeout(
        Duration::from_millis(250),
        client_transport.read(&mut buffer),
    )
    .await
    .expect("response should arrive")
    .expect("transport read should succeed");
    let response = RtuFrame::decode(&buffer[..size]).expect("RTU response should decode");

    assert_eq!(response.pdu, vec![0x03, 0x02, 0x43, 0x21]);

    server.shutdown();
    let _ = timeout(Duration::from_secs(1), server_task).await;
}

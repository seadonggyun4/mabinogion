#![allow(dead_code)]

pub mod transport_harness;

use std::net::{SocketAddr, TcpListener as StdTcpListener};
use std::time::Duration;

use futures::{SinkExt, StreamExt};
use mabi_modbus::context::{BroadcastPolicy, SharedAddressSpace};
use mabi_modbus::handler::ExceptionCode;
use mabi_modbus::profile::{DatastoreKind, UnitProfile};
use mabi_modbus::register::RegisterStore;
use mabi_modbus::registers::RegisterStoreConfig;
use mabi_modbus::rtu::{
    transport::ChannelConfig, ChannelTransport, ModbusRtuServer, RtuFrame, RtuServerConfig,
    RtuTransport,
};
use mabi_modbus::service::{ModbusService, ServiceOutcome, ServiceRequest, StandardModbusService};
use mabi_modbus::tcp::{MbapCodec, MbapFrame, ModbusTcpServerV2, ServerConfigV2};
use mabi_modbus::{DeviceContext, ModbusDevice, RequestPdu, ServerContext};
use tokio::net::TcpStream;
use tokio::time::{sleep, timeout};
use tokio_util::codec::Framed;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackendKind {
    Dense,
    Sparse,
}

#[derive(Debug, Clone)]
pub struct DeviceSpec {
    pub unit_id: u8,
    pub backend: BackendKind,
    pub broadcast_enabled: bool,
}

impl DeviceSpec {
    pub fn dense(unit_id: u8) -> Self {
        Self {
            unit_id,
            backend: BackendKind::Dense,
            broadcast_enabled: true,
        }
    }

    pub fn sparse(unit_id: u8) -> Self {
        Self {
            unit_id,
            backend: BackendKind::Sparse,
            broadcast_enabled: true,
        }
    }

    pub fn with_broadcast(mut self, enabled: bool) -> Self {
        self.broadcast_enabled = enabled;
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransportOutcome {
    Response(Vec<u8>),
    NoResponse,
}

pub fn build_device(spec: DeviceSpec) -> ModbusDevice {
    let datastore = match spec.backend {
        BackendKind::Dense => DatastoreKind::dense_from_counts(256, 256, 256, 256),
        BackendKind::Sparse => DatastoreKind::Sparse {
            config: RegisterStoreConfig::minimal(),
        },
    };

    let mut profile =
        UnitProfile::new(spec.unit_id, format!("unit-{}", spec.unit_id)).with_datastore(datastore);
    profile.broadcast_enabled = spec.broadcast_enabled;

    ModbusDevice::from_profile(&profile).expect("device profile should build")
}

pub fn default_space() -> SharedAddressSpace {
    std::sync::Arc::new(RegisterStore::new(256, 256, 256, 256))
}

pub fn build_context(
    unit_contexts: impl IntoIterator<Item = std::sync::Arc<DeviceContext>>,
    policy: BroadcastPolicy,
) -> std::sync::Arc<ServerContext> {
    let context = std::sync::Arc::new(ServerContext::new(default_space()));
    context.set_broadcast_policy(policy);
    for device in unit_contexts {
        context.register(device);
    }
    context
}

pub fn run_direct_request(
    unit_contexts: impl IntoIterator<Item = std::sync::Arc<DeviceContext>>,
    policy: BroadcastPolicy,
    unit_id: u8,
    pdu: Vec<u8>,
) -> Result<Vec<u8>, ExceptionCode> {
    let context = build_context(unit_contexts, policy);
    let request_pdu = RequestPdu::new(pdu).map_err(|_| ExceptionCode::IllegalDataValue)?;
    let request = if unit_id == 0 {
        ServiceRequest::broadcast(1, request_pdu, context.broadcast_targets())
    } else {
        let target = context
            .target_for_unit(unit_id)
            .ok_or(ExceptionCode::GatewayTargetDeviceFailedToRespond)?;
        ServiceRequest::new(unit_id, 1, request_pdu, target)
    };

    match StandardModbusService::default().call(&request) {
        ServiceOutcome::Reply(response) => Ok(response.into_bytes()),
        ServiceOutcome::Exception(code) => Err(code),
        ServiceOutcome::Ignore => Err(ExceptionCode::GatewayTargetDeviceFailedToRespond),
    }
}

pub async fn run_tcp_request(
    devices: Vec<ModbusDevice>,
    policy: BroadcastPolicy,
    unit_id: u8,
    pdu: Vec<u8>,
) -> TransportOutcome {
    let bind_address = reserve_tcp_addr();
    let config = ServerConfigV2 {
        bind_address,
        connection_timeout: Duration::from_millis(250),
        request_timeout: Duration::from_millis(250),
        shutdown_timeout: Duration::from_millis(500),
        ..Default::default()
    };

    let server = std::sync::Arc::new(ModbusTcpServerV2::new(config));
    server.set_broadcast_policy(policy);
    for device in devices {
        server.add_device(device);
    }

    let task_server = server.clone();
    let server_task = tokio::spawn(async move {
        let _ = task_server.run().await;
    });

    let stream = connect_with_retry(bind_address).await;
    let mut framed = Framed::new(stream, MbapCodec::new());
    framed
        .send(MbapFrame::new(1, unit_id, pdu))
        .await
        .expect("request should be sent");

    let outcome = match timeout(Duration::from_millis(200), framed.next()).await {
        Ok(Some(Ok(frame))) => TransportOutcome::Response(frame.pdu),
        Ok(Some(Err(error))) => panic!("unexpected TCP decode error: {error}"),
        Ok(None) | Err(_) => TransportOutcome::NoResponse,
    };

    server.shutdown();
    let _ = timeout(Duration::from_secs(1), server_task).await;
    outcome
}

pub async fn run_rtu_request(
    devices: Vec<ModbusDevice>,
    policy: BroadcastPolicy,
    unit_ids: Vec<u8>,
    unit_id: u8,
    pdu: Vec<u8>,
) -> TransportOutcome {
    let config = RtuServerConfig::for_testing()
        .with_unit_ids(unit_ids)
        .with_broadcast(true)
        .with_transport(mabi_modbus::rtu::TransportConfig::Channel(
            ChannelConfig::default(),
        ));
    let server = std::sync::Arc::new(ModbusRtuServer::new(config));
    server.set_broadcast_policy(policy);
    for device in devices {
        server.add_device(device);
    }

    let (server_transport, mut client_transport) = ChannelTransport::pair(ChannelConfig::default());
    let task_server = server.clone();
    let server_task = tokio::spawn(async move {
        let _ = task_server.run_with_transport(server_transport).await;
    });

    let request = RtuFrame::new(unit_id, pdu).encode();
    client_transport
        .write(&request)
        .await
        .expect("RTU request should be sent");

    let mut buffer = [0u8; 512];
    let outcome = match timeout(
        Duration::from_millis(200),
        client_transport.read(&mut buffer),
    )
    .await
    {
        Ok(Ok(0)) | Err(_) => TransportOutcome::NoResponse,
        Ok(Ok(n)) => {
            let frame = RtuFrame::decode(&buffer[..n]).expect("RTU response should decode");
            if frame.pdu.is_empty() {
                TransportOutcome::NoResponse
            } else {
                TransportOutcome::Response(frame.pdu)
            }
        }
        Ok(Err(error)) => panic!("unexpected RTU transport error: {error}"),
    };

    server.shutdown();
    let _ = timeout(Duration::from_secs(1), server_task).await;
    outcome
}

pub fn assert_exception_pdu(pdu: &[u8], function_code: u8, exception: ExceptionCode) {
    assert_eq!(pdu, &[function_code | 0x80, exception as u8]);
}

async fn connect_with_retry(bind_address: SocketAddr) -> TcpStream {
    for _ in 0..40 {
        match TcpStream::connect(bind_address).await {
            Ok(stream) => return stream,
            Err(_) => sleep(Duration::from_millis(10)).await,
        }
    }
    TcpStream::connect(bind_address)
        .await
        .expect("TCP server should accept loopback connections")
}

fn reserve_tcp_addr() -> SocketAddr {
    let listener = StdTcpListener::bind("127.0.0.1:0").expect("should reserve loopback port");
    let addr = listener
        .local_addr()
        .expect("reserved listener should have address");
    drop(listener);
    addr
}

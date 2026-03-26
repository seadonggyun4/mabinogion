use std::net::{SocketAddr, TcpListener as StdTcpListener};
use std::sync::Arc;
use std::time::{Duration, Instant};

use futures::{SinkExt, StreamExt};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::task::JoinHandle;
use tokio::time::{sleep, timeout};
use tokio_util::codec::Framed;

use mabi_modbus::config::ModbusDeviceConfig;
use mabi_modbus::device::ModbusDevice;
use mabi_modbus::rtu::transport::{ChannelConfig, TcpBridgeConfig, TcpBridgeTransport};
use mabi_modbus::rtu::{
    ChannelTransport, ModbusRtuServer, PerformancePreset as RtuPerformancePreset, RtuFrame,
    RtuServerConfig, RtuTransport, TransportConfig,
};
use mabi_modbus::tcp::{
    MbapCodec, MbapFrame, ModbusTcpServerV2, PerformancePreset as TcpPerformancePreset,
    ServerConfigV2,
};

pub fn reserve_tcp_addr() -> SocketAddr {
    let listener = StdTcpListener::bind("127.0.0.1:0").expect("should reserve loopback port");
    let addr = listener
        .local_addr()
        .expect("reserved listener should have address");
    drop(listener);
    addr
}

pub async fn connect_with_retry(bind_address: SocketAddr) -> TcpStream {
    for _ in 0..40 {
        match TcpStream::connect(bind_address).await {
            Ok(stream) => return stream,
            Err(_) => sleep(Duration::from_millis(10)).await,
        }
    }

    TcpStream::connect(bind_address)
        .await
        .expect("server should accept loopback connections")
}

pub fn build_dense_device(unit_id: u8) -> ModbusDevice {
    let device = ModbusDevice::new(ModbusDeviceConfig::new(
        unit_id,
        format!("Bench Unit {unit_id}"),
    ));
    let registers = device.registers();
    for address in 0..64u16 {
        registers
            .write_holding_register(address, (unit_id as u16) << 8 | address)
            .expect("bench register should initialize");
    }
    device
}

pub struct TcpServerHarness {
    server: Arc<ModbusTcpServerV2>,
    handle: JoinHandle<()>,
    addr: SocketAddr,
}

impl TcpServerHarness {
    pub async fn start(preset: TcpPerformancePreset, unit_ids: &[u8]) -> Self {
        let addr = reserve_tcp_addr();
        let server = Arc::new(ModbusTcpServerV2::new(ServerConfigV2 {
            bind_address: addr,
            connection_timeout: Duration::from_secs(5),
            request_timeout: Duration::from_secs(1),
            shutdown_timeout: Duration::from_secs(1),
            performance_preset: preset,
            ..Default::default()
        }));

        for unit_id in unit_ids {
            server.add_device(build_dense_device(*unit_id));
        }

        let run_server = server.clone();
        let handle = tokio::spawn(async move {
            let _ = run_server.run().await;
        });

        Self {
            server,
            handle,
            addr,
        }
    }

    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    pub async fn shutdown(self) {
        self.server.shutdown();
        let _ = timeout(Duration::from_secs(2), self.handle).await;
    }
}

pub struct TcpRoundTripClient {
    framed: Framed<TcpStream, MbapCodec>,
    next_tid: u16,
}

impl TcpRoundTripClient {
    pub async fn connect(addr: SocketAddr) -> Self {
        let stream = connect_with_retry(addr).await;
        Self {
            framed: Framed::new(stream, MbapCodec::new()),
            next_tid: 1,
        }
    }

    pub async fn read_holding_register(
        &mut self,
        unit_id: u8,
        address: u16,
    ) -> Result<u16, String> {
        let transaction_id = self.next_tid;
        self.next_tid = self.next_tid.wrapping_add(1).max(1);

        self.framed
            .send(MbapFrame::new(
                transaction_id,
                unit_id,
                vec![0x03, (address >> 8) as u8, address as u8, 0x00, 0x01],
            ))
            .await
            .map_err(|error| error.to_string())?;

        let frame = timeout(Duration::from_secs(1), self.framed.next())
            .await
            .map_err(|_| "TCP response timed out".to_string())?
            .ok_or_else(|| "TCP connection closed".to_string())?
            .map_err(|error| error.to_string())?;

        if frame.pdu.len() != 4 || frame.pdu[0] != 0x03 || frame.pdu[1] != 0x02 {
            return Err(format!("unexpected TCP response PDU: {:02X?}", frame.pdu));
        }

        Ok(u16::from_be_bytes([frame.pdu[2], frame.pdu[3]]))
    }
}

pub struct RtuChannelHarness {
    server: Arc<ModbusRtuServer>,
    handle: JoinHandle<()>,
    client: ChannelTransport,
}

impl RtuChannelHarness {
    pub async fn start(preset: RtuPerformancePreset, unit_ids: &[u8]) -> Self {
        let config = RtuServerConfig::for_testing()
            .with_transport(TransportConfig::Channel(ChannelConfig::default()))
            .with_unit_ids(unit_ids.to_vec())
            .with_performance_preset(preset)
            .with_response_delay_simulation(false);
        let server = Arc::new(ModbusRtuServer::new(config));

        for unit_id in unit_ids {
            server.add_device(build_dense_device(*unit_id));
        }

        let (server_transport, client) = ChannelTransport::pair(ChannelConfig::default());
        let run_server = server.clone();
        let handle = tokio::spawn(async move {
            let _ = run_server.run_with_transport(server_transport).await;
        });

        Self {
            server,
            handle,
            client,
        }
    }

    pub async fn read_holding_register(
        &mut self,
        unit_id: u8,
        address: u16,
    ) -> Result<u16, String> {
        self.client
            .write(
                &RtuFrame::new(
                    unit_id,
                    vec![0x03, (address >> 8) as u8, address as u8, 0x00, 0x01],
                )
                .encode(),
            )
            .await
            .map_err(|error| error.to_string())?;

        let mut buffer = [0u8; 256];
        let size = timeout(Duration::from_secs(1), self.client.read(&mut buffer))
            .await
            .map_err(|_| "RTU channel response timed out".to_string())?
            .map_err(|error| error.to_string())?;
        let frame = RtuFrame::decode(&buffer[..size]).map_err(|error| error.to_string())?;

        if frame.pdu.len() != 4 || frame.pdu[0] != 0x03 || frame.pdu[1] != 0x02 {
            return Err(format!("unexpected RTU response PDU: {:02X?}", frame.pdu));
        }

        Ok(u16::from_be_bytes([frame.pdu[2], frame.pdu[3]]))
    }

    pub async fn shutdown(self) {
        self.server.shutdown();
        let _ = timeout(Duration::from_secs(2), self.handle).await;
    }
}

pub struct RtuTcpBridgeHarness {
    server: Arc<ModbusRtuServer>,
    handle: JoinHandle<()>,
    stream: TcpStream,
}

impl RtuTcpBridgeHarness {
    pub async fn start(preset: RtuPerformancePreset, unit_ids: &[u8]) -> Self {
        let bind_address = reserve_tcp_addr();
        let bridge_config = TcpBridgeConfig {
            bind_address,
            connection_timeout: Duration::from_secs(5),
            ..Default::default()
        };
        let config = RtuServerConfig::default()
            .with_transport(TransportConfig::TcpBridge(bridge_config.clone()))
            .with_unit_ids(unit_ids.to_vec())
            .with_performance_preset(preset)
            .with_response_delay_simulation(false);
        let server = Arc::new(ModbusRtuServer::new(config));

        for unit_id in unit_ids {
            server.add_device(build_dense_device(*unit_id));
        }

        let transport = TcpBridgeTransport::bind(bridge_config)
            .await
            .expect("TCP bridge transport should bind");
        let run_server = server.clone();
        let handle = tokio::spawn(async move {
            let _ = run_server.run_with_transport(transport).await;
        });

        let stream = connect_with_retry(bind_address).await;

        Self {
            server,
            handle,
            stream,
        }
    }

    pub async fn read_holding_register(
        &mut self,
        unit_id: u8,
        address: u16,
    ) -> Result<u16, String> {
        let request = RtuFrame::new(
            unit_id,
            vec![0x03, (address >> 8) as u8, address as u8, 0x00, 0x01],
        )
        .encode();
        self.stream
            .write_all(&request)
            .await
            .map_err(|error| error.to_string())?;

        let mut buffer = [0u8; 256];
        let size = timeout(Duration::from_secs(1), self.stream.read(&mut buffer))
            .await
            .map_err(|_| "RTU bridge response timed out".to_string())?
            .map_err(|error| error.to_string())?;
        let frame = RtuFrame::decode(&buffer[..size]).map_err(|error| error.to_string())?;

        if frame.pdu.len() != 4 || frame.pdu[0] != 0x03 || frame.pdu[1] != 0x02 {
            return Err(format!(
                "unexpected RTU bridge response PDU: {:02X?}",
                frame.pdu
            ));
        }

        Ok(u16::from_be_bytes([frame.pdu[2], frame.pdu[3]]))
    }

    pub async fn shutdown(self) {
        self.server.shutdown();
        let _ = timeout(Duration::from_secs(2), self.handle).await;
    }
}

#[derive(Debug, Clone, Copy)]
#[allow(dead_code)]
pub struct LatencySummary {
    pub avg_us: u64,
    pub p50_us: u64,
    pub p95_us: u64,
    pub p99_us: u64,
}

#[allow(dead_code)]
pub fn summarize_latencies(samples_us: &mut [u64]) -> LatencySummary {
    samples_us.sort_unstable();
    let sum: u128 = samples_us.iter().copied().map(u128::from).sum();
    let avg_us = (sum / samples_us.len().max(1) as u128) as u64;
    let percentile = |p: usize| -> u64 {
        let idx = ((samples_us.len().saturating_sub(1)) * p) / 100;
        samples_us[idx]
    };

    LatencySummary {
        avg_us,
        p50_us: percentile(50),
        p95_us: percentile(95),
        p99_us: percentile(99),
    }
}

#[allow(dead_code)]
pub async fn measure_async_latencies<F, Fut>(
    samples: usize,
    mut op: F,
) -> Result<LatencySummary, String>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<u16, String>>,
{
    let mut latencies = Vec::with_capacity(samples);
    for _ in 0..samples {
        let started = Instant::now();
        let _ = op().await?;
        latencies.push(started.elapsed().as_micros() as u64);
    }

    Ok(summarize_latencies(latencies.as_mut_slice()))
}

#[allow(dead_code)]
pub async fn measure_tcp_connection_churn(
    addr: SocketAddr,
    unit_ids: &[u8],
    samples: usize,
) -> Result<LatencySummary, String> {
    let mut latencies = Vec::with_capacity(samples);
    for step in 0..samples {
        let started = Instant::now();
        let unit_id = unit_ids[step % unit_ids.len()];
        let address = (step as u16) & 0x001f;
        let mut client = TcpRoundTripClient::connect(addr).await;
        let _ = client.read_holding_register(unit_id, address).await?;
        drop(client);
        latencies.push(started.elapsed().as_micros() as u64);
    }

    Ok(summarize_latencies(latencies.as_mut_slice()))
}

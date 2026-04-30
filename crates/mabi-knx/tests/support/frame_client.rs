use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use std::time::Duration;

use mabi_knx::{Hpai, KnxFrame, KnxResult, ServiceType};
use tokio::net::UdpSocket;
use tokio::time::timeout;

use super::TestResult;

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(2);

pub struct FrameClient {
    socket: UdpSocket,
}

impl FrameClient {
    pub async fn bind_loopback() -> TestResult<Self> {
        let socket =
            UdpSocket::bind(SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0))).await?;
        Ok(Self { socket })
    }

    pub fn local_addr(&self) -> TestResult<SocketAddr> {
        Ok(self.socket.local_addr()?)
    }

    pub fn local_hpai(&self) -> TestResult<Hpai> {
        match self.local_addr()? {
            SocketAddr::V4(addr) => Ok(Hpai::udp_ipv4(*addr.ip(), addr.port())),
            SocketAddr::V6(_) => Err("KNX integration client must bind IPv4 loopback".into()),
        }
    }

    pub async fn send_frame(&self, server_addr: SocketAddr, frame: KnxFrame) -> TestResult {
        self.socket.send_to(&frame.encode(), server_addr).await?;
        Ok(())
    }

    pub async fn recv_frame(&self) -> TestResult<KnxFrame> {
        let mut buf = vec![0u8; 2048];
        let (len, _) = timeout(DEFAULT_TIMEOUT, self.socket.recv_from(&mut buf)).await??;
        Ok(KnxFrame::decode(&buf[..len])?)
    }

    pub async fn request_response(
        &self,
        server_addr: SocketAddr,
        request: KnxFrame,
        expected: ServiceType,
    ) -> TestResult<KnxFrame> {
        self.send_frame(server_addr, request).await?;
        let response = self.recv_frame().await?;
        if response.service_type != expected {
            return Err(format!(
                "expected {:?}, received {:?}",
                expected, response.service_type
            )
            .into());
        }
        Ok(response)
    }

    pub async fn recv_until<F>(&self, mut predicate: F) -> TestResult<KnxFrame>
    where
        F: FnMut(&KnxFrame) -> KnxResult<bool>,
    {
        let deadline = tokio::time::Instant::now() + DEFAULT_TIMEOUT;
        loop {
            if tokio::time::Instant::now() >= deadline {
                return Err("timed out waiting for matching KNX frame".into());
            }
            let frame = self.recv_frame().await?;
            if predicate(&frame)? {
                return Ok(frame);
            }
        }
    }
}

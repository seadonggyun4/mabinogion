//! Host Protocol Address Information (HPAI).
//!
//! HPAI is used in KNXnet/IP to specify endpoint addresses.

use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};

use bytes::{Buf, BufMut, BytesMut};

use crate::error::{KnxError, KnxResult};

/// HPAI structure size.
pub const HPAI_SIZE: u8 = 8;

/// Host protocol type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[repr(u8)]
pub enum HostProtocol {
    /// UDP over IPv4.
    #[default]
    Udp4 = 0x01,
    /// TCP over IPv4.
    Tcp4 = 0x02,
}

impl HostProtocol {
    /// Create from raw value.
    pub fn from_u8(value: u8) -> Option<Self> {
        match value {
            0x01 => Some(Self::Udp4),
            0x02 => Some(Self::Tcp4),
            _ => None,
        }
    }
}

impl From<HostProtocol> for u8 {
    fn from(hp: HostProtocol) -> Self {
        hp as u8
    }
}

/// Host Protocol Address Information.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hpai {
    /// Host protocol (UDP or TCP).
    pub protocol: HostProtocol,
    /// IP address.
    pub ip_address: Ipv4Addr,
    /// Port number.
    pub port: u16,
}

impl Hpai {
    /// Create a UDP/IPv4 HPAI.
    pub fn udp_ipv4(ip: Ipv4Addr, port: u16) -> Self {
        Self {
            protocol: HostProtocol::Udp4,
            ip_address: ip,
            port,
        }
    }

    /// Create a TCP/IPv4 HPAI.
    pub fn tcp_ipv4(ip: Ipv4Addr, port: u16) -> Self {
        Self {
            protocol: HostProtocol::Tcp4,
            ip_address: ip,
            port,
        }
    }

    /// Create from socket address.
    pub fn from_socket_addr(addr: SocketAddrV4, protocol: HostProtocol) -> Self {
        Self {
            protocol,
            ip_address: *addr.ip(),
            port: addr.port(),
        }
    }

    /// Create a NAT (0.0.0.0:0) HPAI.
    pub fn nat() -> Self {
        Self::udp_ipv4(Ipv4Addr::UNSPECIFIED, 0)
    }

    /// Check if this is a NAT HPAI.
    pub fn is_nat(&self) -> bool {
        self.ip_address.is_unspecified() && self.port == 0
    }

    /// Convert to socket address.
    pub fn to_socket_addr(&self) -> SocketAddrV4 {
        SocketAddrV4::new(self.ip_address, self.port)
    }

    /// Convert to generic socket address.
    pub fn to_socket_addr_v(&self) -> SocketAddr {
        SocketAddr::V4(self.to_socket_addr())
    }

    /// Encode to bytes.
    pub fn encode(&self) -> Vec<u8> {
        let mut buf = BytesMut::with_capacity(HPAI_SIZE as usize);
        self.encode_to(&mut buf);
        buf.to_vec()
    }

    /// Encode to buffer.
    pub fn encode_to(&self, buf: &mut BytesMut) {
        buf.put_u8(HPAI_SIZE);
        buf.put_u8(self.protocol.into());
        buf.put_slice(&self.ip_address.octets());
        buf.put_u16(self.port);
    }

    /// Decode from bytes.
    pub fn decode(data: &[u8]) -> KnxResult<Self> {
        if data.len() < HPAI_SIZE as usize {
            return Err(KnxError::frame_too_short(HPAI_SIZE as usize, data.len()));
        }

        let mut buf = data;

        let length = buf.get_u8();
        if length != HPAI_SIZE {
            return Err(KnxError::InvalidHpai(format!(
                "Invalid HPAI length: expected {}, got {}",
                HPAI_SIZE, length
            )));
        }

        let protocol_code = buf.get_u8();
        let protocol = HostProtocol::from_u8(protocol_code).ok_or_else(|| {
            KnxError::InvalidHpai(format!("Unknown protocol code: {:#04x}", protocol_code))
        })?;

        let ip_address = Ipv4Addr::new(buf.get_u8(), buf.get_u8(), buf.get_u8(), buf.get_u8());
        let port = buf.get_u16();

        Ok(Self {
            protocol,
            ip_address,
            port,
        })
    }

    /// Decode from buffer with remaining.
    pub fn decode_from(buf: &mut &[u8]) -> KnxResult<Self> {
        let hpai = Self::decode(*buf)?;
        *buf = &buf[HPAI_SIZE as usize..];
        Ok(hpai)
    }
}

impl Default for Hpai {
    fn default() -> Self {
        Self::nat()
    }
}

impl std::fmt::Display for Hpai {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let proto = match self.protocol {
            HostProtocol::Udp4 => "UDP",
            HostProtocol::Tcp4 => "TCP",
        };
        write!(f, "{}://{}:{}", proto, self.ip_address, self.port)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hpai_encode_decode() {
        let hpai = Hpai::udp_ipv4(Ipv4Addr::new(192, 168, 1, 100), 3671);

        let encoded = hpai.encode();
        assert_eq!(encoded.len(), HPAI_SIZE as usize);

        let decoded = Hpai::decode(&encoded).unwrap();
        assert_eq!(decoded.protocol, HostProtocol::Udp4);
        assert_eq!(decoded.ip_address, Ipv4Addr::new(192, 168, 1, 100));
        assert_eq!(decoded.port, 3671);
    }

    #[test]
    fn test_hpai_nat() {
        let hpai = Hpai::nat();
        assert!(hpai.is_nat());
        assert_eq!(hpai.ip_address, Ipv4Addr::UNSPECIFIED);
        assert_eq!(hpai.port, 0);
    }

    #[test]
    fn test_hpai_display() {
        let hpai = Hpai::udp_ipv4(Ipv4Addr::new(192, 168, 1, 100), 3671);
        assert_eq!(hpai.to_string(), "UDP://192.168.1.100:3671");
    }

    #[test]
    fn test_hpai_to_socket_addr() {
        let hpai = Hpai::udp_ipv4(Ipv4Addr::new(192, 168, 1, 100), 3671);
        let addr = hpai.to_socket_addr();
        assert_eq!(addr.ip(), &Ipv4Addr::new(192, 168, 1, 100));
        assert_eq!(addr.port(), 3671);
    }
}

//! Description Information Blocks (DIB).
//!
//! DIBs are used in KNXnet/IP discovery and description responses.

use std::net::Ipv4Addr;

use bytes::{Buf, BufMut, BytesMut};

use crate::address::IndividualAddress;
use crate::error::{KnxError, KnxResult};
use super::ServiceFamily;

/// DIB type codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum DibType {
    /// Device Information.
    DeviceInfo = 0x01,
    /// Supported Service Families.
    SupportedServiceFamilies = 0x02,
    /// IP Configuration.
    IpConfig = 0x03,
    /// Current IP Configuration.
    IpCurrentConfig = 0x04,
    /// KNX Addresses.
    KnxAddresses = 0x05,
    /// Secured Service Families.
    SecuredServiceFamilies = 0x06,
    /// Tunnelling Info.
    TunnellingInfo = 0x07,
    /// Extended Device Info.
    ExtendedDeviceInfo = 0x08,
    /// Manufacturer Data.
    ManufacturerData = 0xFE,
}

impl DibType {
    /// Create from raw value.
    pub fn from_u8(value: u8) -> Option<Self> {
        match value {
            0x01 => Some(Self::DeviceInfo),
            0x02 => Some(Self::SupportedServiceFamilies),
            0x03 => Some(Self::IpConfig),
            0x04 => Some(Self::IpCurrentConfig),
            0x05 => Some(Self::KnxAddresses),
            0x06 => Some(Self::SecuredServiceFamilies),
            0x07 => Some(Self::TunnellingInfo),
            0x08 => Some(Self::ExtendedDeviceInfo),
            0xFE => Some(Self::ManufacturerData),
            _ => None,
        }
    }
}

impl From<DibType> for u8 {
    fn from(dt: DibType) -> Self {
        dt as u8
    }
}

/// Description Information Block.
#[derive(Debug, Clone)]
pub enum Dib {
    /// Device Information.
    DeviceInfo(DeviceInfo),
    /// Supported Service Families.
    SupportedServiceFamilies(SupportedServiceFamilies),
    /// Generic/unknown DIB.
    Generic { dib_type: u8, data: Vec<u8> },
}

impl Dib {
    /// Get DIB type.
    pub fn dib_type(&self) -> u8 {
        match self {
            Self::DeviceInfo(_) => DibType::DeviceInfo as u8,
            Self::SupportedServiceFamilies(_) => DibType::SupportedServiceFamilies as u8,
            Self::Generic { dib_type, .. } => *dib_type,
        }
    }

    /// Encode to bytes.
    pub fn encode(&self) -> Vec<u8> {
        match self {
            Self::DeviceInfo(info) => info.encode(),
            Self::SupportedServiceFamilies(svc) => svc.encode(),
            Self::Generic { dib_type, data } => {
                let mut buf = BytesMut::with_capacity(2 + data.len());
                buf.put_u8((2 + data.len()) as u8);
                buf.put_u8(*dib_type);
                buf.put_slice(data);
                buf.to_vec()
            }
        }
    }

    /// Decode from bytes.
    pub fn decode(data: &[u8]) -> KnxResult<Self> {
        if data.len() < 2 {
            return Err(KnxError::frame_too_short(2, data.len()));
        }

        let length = data[0] as usize;
        let dib_type = data[1];

        if data.len() < length {
            return Err(KnxError::frame_too_short(length, data.len()));
        }

        match DibType::from_u8(dib_type) {
            Some(DibType::DeviceInfo) => {
                Ok(Self::DeviceInfo(DeviceInfo::decode(&data[..length])?))
            }
            Some(DibType::SupportedServiceFamilies) => Ok(Self::SupportedServiceFamilies(
                SupportedServiceFamilies::decode(&data[..length])?,
            )),
            _ => Ok(Self::Generic {
                dib_type,
                data: data[2..length].to_vec(),
            }),
        }
    }

    /// Decode multiple DIBs from buffer.
    pub fn decode_all(data: &[u8]) -> KnxResult<Vec<Self>> {
        let mut dibs = Vec::new();
        let mut remaining = data;

        while remaining.len() >= 2 {
            let length = remaining[0] as usize;
            if length < 2 || remaining.len() < length {
                break;
            }

            dibs.push(Self::decode(&remaining[..length])?);
            remaining = &remaining[length..];
        }

        Ok(dibs)
    }
}

/// Device Information DIB.
#[derive(Debug, Clone)]
pub struct DeviceInfo {
    /// KNX medium type.
    pub knx_medium: KnxMedium,
    /// Device status.
    pub device_status: u8,
    /// Individual address.
    pub individual_address: IndividualAddress,
    /// Project installation identifier.
    pub project_installation_id: u16,
    /// Device serial number (6 bytes).
    pub serial_number: [u8; 6],
    /// Multicast address.
    pub multicast_address: Ipv4Addr,
    /// MAC address (6 bytes).
    pub mac_address: [u8; 6],
    /// Device friendly name (max 30 chars).
    pub device_name: String,
}

impl DeviceInfo {
    /// DIB length.
    pub const LENGTH: u8 = 54;

    /// Create a default device info.
    pub fn new(name: impl Into<String>, individual_address: IndividualAddress) -> Self {
        Self {
            knx_medium: KnxMedium::Tp1,
            device_status: 0x00,
            individual_address,
            project_installation_id: 0x0000,
            serial_number: [0; 6],
            multicast_address: Ipv4Addr::new(224, 0, 23, 12),
            mac_address: [0; 6],
            device_name: name.into(),
        }
    }

    /// Set serial number.
    pub fn with_serial_number(mut self, serial: [u8; 6]) -> Self {
        self.serial_number = serial;
        self
    }

    /// Set MAC address.
    pub fn with_mac_address(mut self, mac: [u8; 6]) -> Self {
        self.mac_address = mac;
        self
    }

    /// Encode to bytes.
    pub fn encode(&self) -> Vec<u8> {
        let mut buf = BytesMut::with_capacity(Self::LENGTH as usize);

        buf.put_u8(Self::LENGTH);
        buf.put_u8(DibType::DeviceInfo as u8);
        buf.put_u8(self.knx_medium as u8);
        buf.put_u8(self.device_status);
        buf.put_u16(self.individual_address.encode());
        buf.put_u16(self.project_installation_id);
        buf.put_slice(&self.serial_number);
        buf.put_slice(&self.multicast_address.octets());
        buf.put_slice(&self.mac_address);

        // Device name (30 bytes, null-padded)
        let name_bytes = self.device_name.as_bytes();
        let name_len = name_bytes.len().min(30);
        buf.put_slice(&name_bytes[..name_len]);
        for _ in name_len..30 {
            buf.put_u8(0);
        }

        buf.to_vec()
    }

    /// Decode from bytes.
    pub fn decode(data: &[u8]) -> KnxResult<Self> {
        if data.len() < Self::LENGTH as usize {
            return Err(KnxError::frame_too_short(Self::LENGTH as usize, data.len()));
        }

        let mut buf = &data[2..]; // Skip length and type

        let knx_medium = KnxMedium::from_u8(buf.get_u8()).unwrap_or(KnxMedium::Tp1);
        let device_status = buf.get_u8();
        let individual_address = IndividualAddress::decode(buf.get_u16());
        let project_installation_id = buf.get_u16();

        let mut serial_number = [0u8; 6];
        serial_number.copy_from_slice(&buf[..6]);
        buf = &buf[6..];

        let multicast_address =
            Ipv4Addr::new(buf.get_u8(), buf.get_u8(), buf.get_u8(), buf.get_u8());

        let mut mac_address = [0u8; 6];
        mac_address.copy_from_slice(&buf[..6]);
        buf = &buf[6..];

        // Device name
        let name_end = buf[..30].iter().position(|&b| b == 0).unwrap_or(30);
        let device_name = String::from_utf8_lossy(&buf[..name_end]).to_string();

        Ok(Self {
            knx_medium,
            device_status,
            individual_address,
            project_installation_id,
            serial_number,
            multicast_address,
            mac_address,
            device_name,
        })
    }
}

/// KNX medium type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[repr(u8)]
pub enum KnxMedium {
    /// TP1 (Twisted Pair 1).
    #[default]
    Tp1 = 0x02,
    /// PL110 (Power Line 110).
    Pl110 = 0x04,
    /// RF (Radio Frequency).
    Rf = 0x10,
    /// IP.
    Ip = 0x20,
}

impl KnxMedium {
    /// Create from raw value.
    pub fn from_u8(value: u8) -> Option<Self> {
        match value {
            0x02 => Some(Self::Tp1),
            0x04 => Some(Self::Pl110),
            0x10 => Some(Self::Rf),
            0x20 => Some(Self::Ip),
            _ => None,
        }
    }
}

/// Supported Service Families DIB.
#[derive(Debug, Clone)]
pub struct SupportedServiceFamilies {
    /// List of supported service families with versions.
    pub families: Vec<ServiceFamilyEntry>,
}

/// Service family entry.
#[derive(Debug, Clone, Copy)]
pub struct ServiceFamilyEntry {
    /// Service family.
    pub family: ServiceFamily,
    /// Version.
    pub version: u8,
}

impl SupportedServiceFamilies {
    /// Create with default families.
    pub fn default_families() -> Self {
        Self {
            families: vec![
                ServiceFamilyEntry {
                    family: ServiceFamily::Core,
                    version: 1,
                },
                ServiceFamilyEntry {
                    family: ServiceFamily::DeviceManagement,
                    version: 1,
                },
                ServiceFamilyEntry {
                    family: ServiceFamily::Tunnelling,
                    version: 1,
                },
            ],
        }
    }

    /// Add a service family.
    pub fn with_family(mut self, family: ServiceFamily, version: u8) -> Self {
        self.families.push(ServiceFamilyEntry { family, version });
        self
    }

    /// Encode to bytes.
    pub fn encode(&self) -> Vec<u8> {
        let length = 2 + self.families.len() * 2;
        let mut buf = BytesMut::with_capacity(length);

        buf.put_u8(length as u8);
        buf.put_u8(DibType::SupportedServiceFamilies as u8);

        for entry in &self.families {
            buf.put_u8(entry.family.id());
            buf.put_u8(entry.version);
        }

        buf.to_vec()
    }

    /// Decode from bytes.
    pub fn decode(data: &[u8]) -> KnxResult<Self> {
        if data.len() < 2 {
            return Err(KnxError::frame_too_short(2, data.len()));
        }

        let length = data[0] as usize;
        if data.len() < length {
            return Err(KnxError::frame_too_short(length, data.len()));
        }

        let mut families = Vec::new();
        let mut buf = &data[2..length];

        while buf.len() >= 2 {
            let family_id = buf.get_u8();
            let version = buf.get_u8();
            families.push(ServiceFamilyEntry {
                family: ServiceFamily::from_id(family_id),
                version,
            });
        }

        Ok(Self { families })
    }

    /// Check if a service family is supported.
    pub fn supports(&self, family: ServiceFamily) -> bool {
        self.families.iter().any(|e| e.family == family)
    }

    /// Get version for a service family.
    pub fn version(&self, family: ServiceFamily) -> Option<u8> {
        self.families
            .iter()
            .find(|e| e.family == family)
            .map(|e| e.version)
    }
}

impl Default for SupportedServiceFamilies {
    fn default() -> Self {
        Self::default_families()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_device_info_encode_decode() {
        let info = DeviceInfo::new("Test Device", IndividualAddress::new(1, 2, 3))
            .with_serial_number([0x01, 0x02, 0x03, 0x04, 0x05, 0x06])
            .with_mac_address([0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF]);

        let encoded = info.encode();
        assert_eq!(encoded.len(), DeviceInfo::LENGTH as usize);

        let decoded = DeviceInfo::decode(&encoded).unwrap();
        assert_eq!(decoded.device_name, "Test Device");
        assert_eq!(decoded.individual_address.to_string(), "1.2.3");
        assert_eq!(decoded.serial_number, [0x01, 0x02, 0x03, 0x04, 0x05, 0x06]);
    }

    #[test]
    fn test_supported_service_families() {
        let families = SupportedServiceFamilies::default_families();

        assert!(families.supports(ServiceFamily::Core));
        assert!(families.supports(ServiceFamily::Tunnelling));
        assert!(!families.supports(ServiceFamily::Routing));

        let encoded = families.encode();
        let decoded = SupportedServiceFamilies::decode(&encoded).unwrap();

        assert_eq!(decoded.families.len(), families.families.len());
    }

    #[test]
    fn test_dib_decode_all() {
        let device_info = DeviceInfo::new("Test", IndividualAddress::new(1, 1, 1));
        let families = SupportedServiceFamilies::default_families();

        let mut data = device_info.encode();
        data.extend(families.encode());

        let dibs = Dib::decode_all(&data).unwrap();
        assert_eq!(dibs.len(), 2);

        assert!(matches!(dibs[0], Dib::DeviceInfo(_)));
        assert!(matches!(dibs[1], Dib::SupportedServiceFamilies(_)));
    }
}

//! NPDU (Network Protocol Data Unit) implementation.
//!
//! The NPDU provides network layer services for BACnet, including
//! routing information and message priority.

use bytes::{Buf, BufMut, BytesMut};

/// NPDU version (always 0x01 for BACnet).
pub const NPDU_VERSION: u8 = 0x01;

/// Message priority levels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
#[repr(u8)]
pub enum Priority {
    /// Normal priority (default).
    #[default]
    Normal = 0,
    /// Urgent priority.
    Urgent = 1,
    /// Critical equipment message.
    CriticalEquipment = 2,
    /// Life safety message.
    LifeSafety = 3,
}

impl Priority {
    /// Create from 2-bit value.
    pub fn from_bits(bits: u8) -> Self {
        match bits & 0x03 {
            0 => Self::Normal,
            1 => Self::Urgent,
            2 => Self::CriticalEquipment,
            3 => Self::LifeSafety,
            _ => Self::Normal,
        }
    }
}

/// Network layer message types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum NetworkMessageType {
    /// Who-Is-Router-To-Network
    WhoIsRouterToNetwork = 0x00,
    /// I-Am-Router-To-Network
    IAmRouterToNetwork = 0x01,
    /// I-Could-Be-Router-To-Network
    ICouldBeRouterToNetwork = 0x02,
    /// Reject-Message-To-Network
    RejectMessageToNetwork = 0x03,
    /// Router-Busy-To-Network
    RouterBusyToNetwork = 0x04,
    /// Router-Available-To-Network
    RouterAvailableToNetwork = 0x05,
    /// Initialize-Routing-Table
    InitializeRoutingTable = 0x06,
    /// Initialize-Routing-Table-Ack
    InitializeRoutingTableAck = 0x07,
    /// Establish-Connection-To-Network
    EstablishConnectionToNetwork = 0x08,
    /// Disconnect-Connection-To-Network
    DisconnectConnectionToNetwork = 0x09,
    /// Challenge-Request
    ChallengeRequest = 0x0A,
    /// Security-Payload
    SecurityPayload = 0x0B,
    /// Security-Response
    SecurityResponse = 0x0C,
    /// Request-Key-Update
    RequestKeyUpdate = 0x0D,
    /// Update-Key-Set
    UpdateKeySet = 0x0E,
    /// Update-Distribution-Key
    UpdateDistributionKey = 0x0F,
    /// Request-Master-Key
    RequestMasterKey = 0x10,
    /// Set-Master-Key
    SetMasterKey = 0x11,
    /// What-Is-Network-Number
    WhatIsNetworkNumber = 0x12,
    /// Network-Number-Is
    NetworkNumberIs = 0x13,
}

impl NetworkMessageType {
    pub fn from_u8(value: u8) -> Option<Self> {
        match value {
            0x00 => Some(Self::WhoIsRouterToNetwork),
            0x01 => Some(Self::IAmRouterToNetwork),
            0x02 => Some(Self::ICouldBeRouterToNetwork),
            0x03 => Some(Self::RejectMessageToNetwork),
            0x04 => Some(Self::RouterBusyToNetwork),
            0x05 => Some(Self::RouterAvailableToNetwork),
            0x06 => Some(Self::InitializeRoutingTable),
            0x07 => Some(Self::InitializeRoutingTableAck),
            0x08 => Some(Self::EstablishConnectionToNetwork),
            0x09 => Some(Self::DisconnectConnectionToNetwork),
            0x12 => Some(Self::WhatIsNetworkNumber),
            0x13 => Some(Self::NetworkNumberIs),
            _ => None,
        }
    }
}

/// NPDU control flags.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct NpduControl {
    /// This is a network layer message (not application layer).
    pub network_layer_message: bool,
    /// Destination specifier present (DNET, DLEN, DADR, Hop Count).
    pub destination_specifier: bool,
    /// Source specifier present (SNET, SLEN, SADR).
    pub source_specifier: bool,
    /// Reply expected.
    pub expecting_reply: bool,
    /// Message priority.
    pub priority: Priority,
}

impl NpduControl {
    /// Create a new NPDU control with defaults.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set expecting reply flag.
    pub fn with_expecting_reply(mut self, expecting: bool) -> Self {
        self.expecting_reply = expecting;
        self
    }

    /// Set priority.
    pub fn with_priority(mut self, priority: Priority) -> Self {
        self.priority = priority;
        self
    }

    /// Set destination specifier flag.
    pub fn with_destination(mut self, has_dest: bool) -> Self {
        self.destination_specifier = has_dest;
        self
    }

    /// Set source specifier flag.
    pub fn with_source(mut self, has_source: bool) -> Self {
        self.source_specifier = has_source;
        self
    }

    /// Encode control byte.
    pub fn encode(&self) -> u8 {
        let mut ctrl: u8 = 0;

        if self.network_layer_message {
            ctrl |= 0x80;
        }
        if self.destination_specifier {
            ctrl |= 0x20;
        }
        if self.source_specifier {
            ctrl |= 0x08;
        }
        if self.expecting_reply {
            ctrl |= 0x04;
        }
        ctrl |= (self.priority as u8) & 0x03;

        ctrl
    }

    /// Decode control byte.
    pub fn decode(byte: u8) -> Self {
        Self {
            network_layer_message: byte & 0x80 != 0,
            destination_specifier: byte & 0x20 != 0,
            source_specifier: byte & 0x08 != 0,
            expecting_reply: byte & 0x04 != 0,
            priority: Priority::from_bits(byte),
        }
    }
}

/// BACnet network address.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct BACnetAddress {
    /// Network number (0 = local network, 0xFFFF = broadcast).
    pub network: u16,
    /// MAC address (length 0 = broadcast on network).
    pub address: Vec<u8>,
}

impl BACnetAddress {
    /// Create a new BACnet address.
    pub fn new(network: u16, address: Vec<u8>) -> Self {
        Self { network, address }
    }

    /// Create a local network address.
    pub fn local(address: Vec<u8>) -> Self {
        Self {
            network: 0,
            address,
        }
    }

    /// Create a broadcast address.
    pub fn broadcast() -> Self {
        Self {
            network: 0xFFFF,
            address: Vec::new(),
        }
    }

    /// Check if this is a broadcast address.
    pub fn is_broadcast(&self) -> bool {
        self.network == 0xFFFF || self.address.is_empty()
    }

    /// Encode to bytes.
    pub fn encode(&self, buf: &mut BytesMut) {
        buf.put_u16(self.network);
        buf.put_u8(self.address.len() as u8);
        buf.put_slice(&self.address);
    }

    /// Decode from bytes.
    pub fn decode(data: &mut &[u8]) -> Result<Self, NpduError> {
        if data.len() < 3 {
            return Err(NpduError::TooShort);
        }

        let network = data.get_u16();
        let len = data.get_u8() as usize;

        if data.len() < len {
            return Err(NpduError::TooShort);
        }

        let mut address = vec![0u8; len];
        data.copy_to_slice(&mut address);

        Ok(Self { network, address })
    }
}

/// NPDU (Network Protocol Data Unit).
#[derive(Debug, Clone)]
pub struct Npdu {
    /// Control information.
    pub control: NpduControl,
    /// Destination address (if destination_specifier is set).
    pub destination: Option<BACnetAddress>,
    /// Source address (if source_specifier is set).
    pub source: Option<BACnetAddress>,
    /// Hop count (if destination_specifier is set).
    pub hop_count: Option<u8>,
    /// Network message type (if network_layer_message is set).
    pub network_message_type: Option<NetworkMessageType>,
    /// Vendor ID (if network_layer_message with vendor-specific type).
    pub vendor_id: Option<u16>,
    /// APDU or network message data.
    pub data: Vec<u8>,
}

impl Npdu {
    /// Create a simple NPDU containing only an APDU.
    pub fn simple(apdu: Vec<u8>) -> Self {
        Self {
            control: NpduControl {
                expecting_reply: true,
                ..Default::default()
            },
            destination: None,
            source: None,
            hop_count: None,
            network_message_type: None,
            vendor_id: None,
            data: apdu,
        }
    }

    /// Create an NPDU with no reply expected.
    pub fn no_reply(apdu: Vec<u8>) -> Self {
        Self {
            control: NpduControl::default(),
            destination: None,
            source: None,
            hop_count: None,
            network_message_type: None,
            vendor_id: None,
            data: apdu,
        }
    }

    /// Create an NPDU with destination address.
    pub fn with_destination(mut self, dest: BACnetAddress) -> Self {
        self.destination = Some(dest);
        self.control.destination_specifier = true;
        self.hop_count = Some(255);
        self
    }

    /// Create an NPDU with source address.
    pub fn with_source(mut self, source: BACnetAddress) -> Self {
        self.source = Some(source);
        self.control.source_specifier = true;
        self
    }

    /// Set hop count.
    pub fn with_hop_count(mut self, hop_count: u8) -> Self {
        self.hop_count = Some(hop_count);
        self
    }

    /// Encode NPDU to bytes.
    pub fn encode(&self) -> Vec<u8> {
        let mut buf = BytesMut::with_capacity(64 + self.data.len());

        buf.put_u8(NPDU_VERSION);
        buf.put_u8(self.control.encode());

        if let Some(ref dest) = self.destination {
            dest.encode(&mut buf);
        }

        if let Some(ref src) = self.source {
            src.encode(&mut buf);
        }

        if self.control.destination_specifier {
            buf.put_u8(self.hop_count.unwrap_or(255));
        }

        if self.control.network_layer_message {
            if let Some(msg_type) = self.network_message_type {
                buf.put_u8(msg_type as u8);
                if (msg_type as u8) >= 0x80 {
                    if let Some(vendor_id) = self.vendor_id {
                        buf.put_u16(vendor_id);
                    }
                }
            }
        }

        buf.put_slice(&self.data);

        buf.to_vec()
    }

    /// Decode NPDU from bytes.
    pub fn decode(data: &[u8]) -> Result<Self, NpduError> {
        if data.len() < 2 {
            return Err(NpduError::TooShort);
        }

        let mut cursor = data;

        let version = cursor.get_u8();
        if version != NPDU_VERSION {
            return Err(NpduError::InvalidVersion(version));
        }

        let control = NpduControl::decode(cursor.get_u8());

        let destination = if control.destination_specifier {
            Some(BACnetAddress::decode(&mut cursor)?)
        } else {
            None
        };

        let source = if control.source_specifier {
            Some(BACnetAddress::decode(&mut cursor)?)
        } else {
            None
        };

        let hop_count = if control.destination_specifier {
            if cursor.is_empty() {
                return Err(NpduError::TooShort);
            }
            Some(cursor.get_u8())
        } else {
            None
        };

        let (network_message_type, vendor_id) = if control.network_layer_message {
            if cursor.is_empty() {
                return Err(NpduError::TooShort);
            }
            let msg_type_byte = cursor.get_u8();
            let msg_type = NetworkMessageType::from_u8(msg_type_byte);

            let vendor = if msg_type_byte >= 0x80 && cursor.len() >= 2 {
                Some(cursor.get_u16())
            } else {
                None
            };

            (msg_type, vendor)
        } else {
            (None, None)
        };

        Ok(Self {
            control,
            destination,
            source,
            hop_count,
            network_message_type,
            vendor_id,
            data: cursor.to_vec(),
        })
    }

    /// Get the APDU data (returns empty slice for network layer messages).
    pub fn apdu(&self) -> &[u8] {
        if self.control.network_layer_message {
            &[]
        } else {
            &self.data
        }
    }

    /// Check if this NPDU expects a reply.
    pub fn expects_reply(&self) -> bool {
        self.control.expecting_reply
    }

    /// Check if this is a network layer message.
    pub fn is_network_message(&self) -> bool {
        self.control.network_layer_message
    }

    /// Get the effective destination network.
    pub fn destination_network(&self) -> Option<u16> {
        self.destination.as_ref().map(|d| d.network)
    }
}

/// NPDU errors.
#[derive(Debug, thiserror::Error)]
pub enum NpduError {
    #[error("NPDU too short")]
    TooShort,

    #[error("Invalid NPDU version: 0x{0:02X} (expected 0x01)")]
    InvalidVersion(u8),

    #[error("Invalid control byte")]
    InvalidControl,

    #[error("Invalid address format")]
    InvalidAddress,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_npdu_control_encode_decode() {
        let control = NpduControl {
            network_layer_message: false,
            destination_specifier: false,
            source_specifier: false,
            expecting_reply: true,
            priority: Priority::Normal,
        };

        let encoded = control.encode();
        let decoded = NpduControl::decode(encoded);

        assert_eq!(decoded, control);
    }

    #[test]
    fn test_npdu_control_with_all_flags() {
        let control = NpduControl {
            network_layer_message: true,
            destination_specifier: true,
            source_specifier: true,
            expecting_reply: true,
            priority: Priority::LifeSafety,
        };

        let encoded = control.encode();
        assert_eq!(encoded, 0x80 | 0x20 | 0x08 | 0x04 | 0x03);

        let decoded = NpduControl::decode(encoded);
        assert_eq!(decoded, control);
    }

    #[test]
    fn test_simple_npdu_encode_decode() {
        let apdu = vec![0x10, 0x00]; // Unconfirmed I-Am
        let npdu = Npdu::simple(apdu.clone());

        let encoded = npdu.encode();
        let decoded = Npdu::decode(&encoded).unwrap();

        assert_eq!(decoded.data, apdu);
        assert!(decoded.expects_reply());
        assert!(!decoded.is_network_message());
    }

    #[test]
    fn test_npdu_with_destination() {
        let apdu = vec![0x10, 0x08]; // Unconfirmed Who-Is
        let dest = BACnetAddress::broadcast();
        let npdu = Npdu::simple(apdu.clone()).with_destination(dest);

        let encoded = npdu.encode();
        let decoded = Npdu::decode(&encoded).unwrap();

        assert!(decoded.control.destination_specifier);
        assert!(decoded.destination.is_some());
        assert!(decoded.destination.unwrap().is_broadcast());
    }

    #[test]
    fn test_bacnet_address_encode_decode() {
        let addr = BACnetAddress::new(100, vec![0x0A, 0x0B, 0x0C, 0x0D, 0xBA, 0xC0]);
        let mut buf = BytesMut::new();
        addr.encode(&mut buf);

        let mut cursor = &buf[..];
        let decoded = BACnetAddress::decode(&mut cursor).unwrap();

        assert_eq!(decoded, addr);
    }

    #[test]
    fn test_priority_from_bits() {
        assert_eq!(Priority::from_bits(0), Priority::Normal);
        assert_eq!(Priority::from_bits(1), Priority::Urgent);
        assert_eq!(Priority::from_bits(2), Priority::CriticalEquipment);
        assert_eq!(Priority::from_bits(3), Priority::LifeSafety);
    }
}

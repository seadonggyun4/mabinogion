//! Common EMI (cEMI) frame handling.
//!
//! cEMI is the standardized interface for KNX communication,
//! used in KNXnet/IP tunneling and routing.

use bytes::{Buf, BufMut, BytesMut};

use crate::address::{AddressType, GroupAddress, IndividualAddress};
use crate::error::{KnxError, KnxResult};

// ============================================================================
// Message Code
// ============================================================================

/// cEMI message code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum MessageCode {
    /// L_Data.req - Request to send data.
    LDataReq = 0x11,
    /// L_Data.con - Confirmation of data sent.
    LDataCon = 0x2E,
    /// L_Data.ind - Indication of received data.
    LDataInd = 0x29,
    /// L_Busmon.ind - Bus monitor indication.
    LBusmonInd = 0x2B,
    /// L_Raw.req - Raw request.
    LRawReq = 0x10,
    /// L_Raw.ind - Raw indication.
    LRawInd = 0x2D,
    /// L_Raw.con - Raw confirmation.
    LRawCon = 0x2F,
    /// M_PropRead.req - Property read request.
    MPropReadReq = 0xFC,
    /// M_PropRead.con - Property read confirmation.
    MPropReadCon = 0xFB,
    /// M_PropWrite.req - Property write request.
    MPropWriteReq = 0xF6,
    /// M_PropWrite.con - Property write confirmation.
    MPropWriteCon = 0xF5,
    /// M_Reset.req - Reset request.
    MResetReq = 0xF1,
    /// M_Reset.ind - Reset indication.
    MResetInd = 0xF0,
}

impl MessageCode {
    /// Try to create from raw byte.
    pub fn try_from_u8(value: u8) -> Option<Self> {
        match value {
            0x11 => Some(Self::LDataReq),
            0x2E => Some(Self::LDataCon),
            0x29 => Some(Self::LDataInd),
            0x2B => Some(Self::LBusmonInd),
            0x10 => Some(Self::LRawReq),
            0x2D => Some(Self::LRawInd),
            0x2F => Some(Self::LRawCon),
            0xFC => Some(Self::MPropReadReq),
            0xFB => Some(Self::MPropReadCon),
            0xF6 => Some(Self::MPropWriteReq),
            0xF5 => Some(Self::MPropWriteCon),
            0xF1 => Some(Self::MResetReq),
            0xF0 => Some(Self::MResetInd),
            _ => None,
        }
    }

    /// Check if this is a data message.
    pub fn is_data(&self) -> bool {
        matches!(self, Self::LDataReq | Self::LDataCon | Self::LDataInd)
    }

    /// Check if this is a request.
    pub fn is_request(&self) -> bool {
        matches!(
            self,
            Self::LDataReq
                | Self::LRawReq
                | Self::MPropReadReq
                | Self::MPropWriteReq
                | Self::MResetReq
        )
    }

    /// Check if this is an indication.
    pub fn is_indication(&self) -> bool {
        matches!(
            self,
            Self::LDataInd | Self::LBusmonInd | Self::LRawInd | Self::MResetInd
        )
    }

    /// Check if this is a confirmation.
    pub fn is_confirmation(&self) -> bool {
        matches!(
            self,
            Self::LDataCon | Self::LRawCon | Self::MPropReadCon | Self::MPropWriteCon
        )
    }

    /// Check if this is a property service message.
    pub fn is_property_service(&self) -> bool {
        matches!(
            self,
            Self::MPropReadReq | Self::MPropReadCon | Self::MPropWriteReq | Self::MPropWriteCon
        )
    }

    /// Check if this is a reset service message.
    pub fn is_reset_service(&self) -> bool {
        matches!(self, Self::MResetReq | Self::MResetInd)
    }

    /// Get the corresponding confirmation message code for a request.
    pub fn to_confirmation(&self) -> Option<Self> {
        match self {
            Self::LDataReq => Some(Self::LDataCon),
            Self::LRawReq => Some(Self::LRawCon),
            Self::MPropReadReq => Some(Self::MPropReadCon),
            Self::MPropWriteReq => Some(Self::MPropWriteCon),
            Self::MResetReq => Some(Self::MResetInd),
            _ => None,
        }
    }
}

impl From<MessageCode> for u8 {
    fn from(code: MessageCode) -> Self {
        code as u8
    }
}

// ============================================================================
// Priority
// ============================================================================

/// KNX telegram priority.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[repr(u8)]
pub enum Priority {
    /// System priority (highest).
    System = 0,
    /// Normal priority.
    #[default]
    Normal = 1,
    /// Urgent priority.
    Urgent = 2,
    /// Low priority (lowest).
    Low = 3,
}

impl Priority {
    /// Create from raw value (2 bits).
    pub fn from_bits(value: u8) -> Self {
        match value & 0x03 {
            0 => Self::System,
            1 => Self::Normal,
            2 => Self::Urgent,
            _ => Self::Low,
        }
    }

    /// Convert to raw bits.
    pub fn to_bits(self) -> u8 {
        self as u8
    }
}

// ============================================================================
// APCI (Application Layer Protocol Control Information)
// ============================================================================

/// APCI - Application Protocol Control Information.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Apci {
    /// Group Value Read.
    GroupValueRead,
    /// Group Value Response.
    GroupValueResponse,
    /// Group Value Write.
    GroupValueWrite,
    /// Individual Address Write.
    IndividualAddressWrite,
    /// Individual Address Read.
    IndividualAddressRead,
    /// Individual Address Response.
    IndividualAddressResponse,
    /// ADC Read.
    AdcRead,
    /// ADC Response.
    AdcResponse,
    /// Memory Read.
    MemoryRead,
    /// Memory Response.
    MemoryResponse,
    /// Memory Write.
    MemoryWrite,
    /// User Message.
    UserMessage,
    /// Device Descriptor Read.
    DeviceDescriptorRead,
    /// Device Descriptor Response.
    DeviceDescriptorResponse,
    /// Restart.
    Restart,
    /// Escape (for extended APCI).
    Escape,
    /// Unknown APCI.
    Unknown(u16),
}

impl Apci {
    /// Decode APCI from raw 10-bit value.
    pub fn decode(value: u16) -> Self {
        // APCI encoding uses bits for different services
        // Group Value services are in range 0x0000-0x00FF
        // Check specific patterns

        // Mask to the 10-bit APCI
        let apci = value & 0x03FF;

        match apci {
            0x0000 => Self::GroupValueRead,
            0x0040..=0x007F => Self::GroupValueResponse,
            0x0080..=0x00FF => Self::GroupValueWrite,
            0x0100..=0x013F => Self::IndividualAddressWrite,
            0x0140..=0x017F => Self::IndividualAddressRead,
            0x0180..=0x01BF => Self::IndividualAddressResponse,
            0x01C0..=0x01FF => Self::AdcRead,
            0x0200..=0x023F => Self::AdcResponse,
            0x0240..=0x027F => Self::MemoryRead,
            0x0280..=0x02BF => Self::MemoryResponse,
            0x02C0..=0x02FF => Self::MemoryWrite,
            0x0300..=0x033F => Self::UserMessage,
            0x0340..=0x037F => Self::DeviceDescriptorRead,
            0x0380..=0x03BF => Self::DeviceDescriptorResponse,
            0x03C0..=0x03DF => Self::Restart,
            0x03E0..=0x03FF => Self::Escape,
            _ => Self::Unknown(value),
        }
    }

    /// Decode APCI simplified - just match the base values.
    pub fn decode_simple(value: u16) -> Self {
        // Use only upper bits to determine main type
        let base = value & 0x03C0; // Bits 9-6

        match base {
            0x0000 => Self::GroupValueRead,
            0x0040 => Self::GroupValueResponse,
            0x0080 => Self::GroupValueWrite,
            0x00C0 => Self::GroupValueWrite, // Continuation
            0x0100 => Self::IndividualAddressWrite,
            0x0140 => Self::IndividualAddressRead,
            0x0180 => Self::IndividualAddressResponse,
            0x01C0 => Self::AdcRead,
            0x0200 => Self::AdcResponse,
            0x0240 => Self::MemoryRead,
            0x0280 => Self::MemoryResponse,
            0x02C0 => Self::MemoryWrite,
            0x0300 => Self::UserMessage,
            0x0340 => Self::DeviceDescriptorRead,
            0x0380 => Self::DeviceDescriptorResponse,
            0x03C0 => Self::Restart,
            _ => Self::Unknown(value),
        }
    }

    /// Encode APCI to raw 10-bit value.
    pub fn encode(&self) -> u16 {
        match self {
            Self::GroupValueRead => 0x0000,
            Self::GroupValueResponse => 0x0040,
            Self::GroupValueWrite => 0x0080,
            Self::IndividualAddressWrite => 0x0100,
            Self::IndividualAddressRead => 0x0100,
            Self::IndividualAddressResponse => 0x0140,
            Self::AdcRead => 0x0180,
            Self::AdcResponse => 0x01C0,
            Self::MemoryRead => 0x0200,
            Self::MemoryResponse => 0x0240,
            Self::MemoryWrite => 0x0280,
            Self::UserMessage => 0x02C0,
            Self::DeviceDescriptorRead => 0x0300,
            Self::DeviceDescriptorResponse => 0x0340,
            Self::Restart => 0x0380,
            Self::Escape => 0x03C0,
            Self::Unknown(v) => *v,
        }
    }

    /// Check if this is a group value service.
    pub fn is_group_value(&self) -> bool {
        matches!(
            self,
            Self::GroupValueRead | Self::GroupValueResponse | Self::GroupValueWrite
        )
    }

    /// Check if this is a read request.
    pub fn is_read(&self) -> bool {
        matches!(
            self,
            Self::GroupValueRead
                | Self::IndividualAddressRead
                | Self::AdcRead
                | Self::MemoryRead
                | Self::DeviceDescriptorRead
        )
    }

    /// Check if this is a write.
    pub fn is_write(&self) -> bool {
        matches!(
            self,
            Self::GroupValueWrite | Self::IndividualAddressWrite | Self::MemoryWrite
        )
    }

    /// Check if this is a response.
    pub fn is_response(&self) -> bool {
        matches!(
            self,
            Self::GroupValueResponse
                | Self::IndividualAddressResponse
                | Self::AdcResponse
                | Self::MemoryResponse
                | Self::DeviceDescriptorResponse
        )
    }
}

// ============================================================================
// Additional Info
// ============================================================================

/// cEMI additional information type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum AdditionalInfoType {
    /// PL Medium Information.
    PlMediumInfo = 0x01,
    /// RF Medium Information.
    RfMediumInfo = 0x02,
    /// Bus Monitor Info.
    BusMonitorInfo = 0x03,
    /// Timestamp Relative.
    TimestampRelative = 0x04,
    /// Time Delay Until.
    TimeDelayUntil = 0x05,
    /// Extended Relative Timestamp.
    ExtendedTimestamp = 0x06,
    /// BiBat Information.
    BiBatInfo = 0x07,
    /// RF Multi Information.
    RfMultiInfo = 0x08,
    /// Preamble And Postamble.
    PreamblePostamble = 0x09,
    /// RF Fast Ack Information.
    RfFastAckInfo = 0x0A,
    /// Manufacturer Specific Data.
    ManufacturerData = 0xFE,
}

/// cEMI additional information.
#[derive(Debug, Clone)]
pub struct AdditionalInfo {
    pub info_type: u8,
    pub data: Vec<u8>,
}

impl AdditionalInfo {
    /// Create new additional info.
    pub fn new(info_type: u8, data: Vec<u8>) -> Self {
        Self { info_type, data }
    }

    /// Encode to bytes.
    pub fn encode(&self, buf: &mut BytesMut) {
        buf.put_u8(self.info_type);
        buf.put_u8(self.data.len() as u8);
        buf.put_slice(&self.data);
    }

    /// Encoded length.
    pub fn encoded_len(&self) -> usize {
        2 + self.data.len()
    }

    /// Decode from buffer.
    pub fn decode(buf: &mut &[u8]) -> KnxResult<Self> {
        if buf.len() < 2 {
            return Err(KnxError::frame_too_short(2, buf.len()));
        }

        let info_type = buf.get_u8();
        let length = buf.get_u8() as usize;

        if buf.len() < length {
            return Err(KnxError::frame_too_short(length, buf.len()));
        }

        let data = buf[..length].to_vec();
        *buf = &buf[length..];

        Ok(Self { info_type, data })
    }
}

// ============================================================================
// cEMI Frame
// ============================================================================

/// cEMI L_Data frame.
#[derive(Debug, Clone)]
pub struct CemiFrame {
    /// Message code.
    pub message_code: MessageCode,
    /// Additional information.
    pub additional_info: Vec<AdditionalInfo>,
    /// Source address.
    pub source: IndividualAddress,
    /// Destination address (raw).
    pub destination: u16,
    /// Address type.
    pub address_type: AddressType,
    /// Hop count (routing counter).
    pub hop_count: u8,
    /// Priority.
    pub priority: Priority,
    /// Confirm flag (for L_Data.con).
    pub confirm: bool,
    /// Acknowledge request.
    pub ack_request: bool,
    /// System broadcast.
    pub system_broadcast: bool,
    /// APCI.
    pub apci: Apci,
    /// Data (payload after APCI).
    pub data: Vec<u8>,
}

impl CemiFrame {
    /// Create a Group Value Write frame.
    pub fn group_value_write(
        source: IndividualAddress,
        destination: GroupAddress,
        data: Vec<u8>,
    ) -> Self {
        Self {
            message_code: MessageCode::LDataInd,
            additional_info: Vec::new(),
            source,
            destination: destination.raw(),
            address_type: AddressType::Group,
            hop_count: 6,
            priority: Priority::Low,
            confirm: false,
            ack_request: false,
            system_broadcast: false,
            apci: Apci::GroupValueWrite,
            data,
        }
    }

    /// Create a Group Value Read frame.
    pub fn group_value_read(source: IndividualAddress, destination: GroupAddress) -> Self {
        Self {
            message_code: MessageCode::LDataInd,
            additional_info: Vec::new(),
            source,
            destination: destination.raw(),
            address_type: AddressType::Group,
            hop_count: 6,
            priority: Priority::Low,
            confirm: false,
            ack_request: false,
            system_broadcast: false,
            apci: Apci::GroupValueRead,
            data: Vec::new(),
        }
    }

    /// Create a Group Value Response frame.
    pub fn group_value_response(
        source: IndividualAddress,
        destination: GroupAddress,
        data: Vec<u8>,
    ) -> Self {
        Self {
            message_code: MessageCode::LDataInd,
            additional_info: Vec::new(),
            source,
            destination: destination.raw(),
            address_type: AddressType::Group,
            hop_count: 6,
            priority: Priority::Low,
            confirm: false,
            ack_request: false,
            system_broadcast: false,
            apci: Apci::GroupValueResponse,
            data,
        }
    }

    /// Create a Bus Monitor indication frame (MC=0x2B).
    ///
    /// Bus monitor frames wrap the raw bus frame data with Additional Info TLV:
    /// - 0x03: Bus Monitor Status (error flags)
    /// - 0x04: Timestamp Relative (2 bytes, ms since last frame)
    /// - 0x06: Extended Timestamp (4 bytes, microseconds)
    pub fn bus_monitor_indication(raw_frame: &[u8], status: u8, timestamp_ms: u16) -> Self {
        let mut additional_info = Vec::new();

        // Bus Monitor Info (type 0x03): 1 byte status
        additional_info.push(AdditionalInfo::new(
            AdditionalInfoType::BusMonitorInfo as u8,
            vec![status],
        ));

        // Timestamp Relative (type 0x04): 2 bytes, milliseconds since last frame
        additional_info.push(AdditionalInfo::new(
            AdditionalInfoType::TimestampRelative as u8,
            timestamp_ms.to_be_bytes().to_vec(),
        ));

        Self {
            message_code: MessageCode::LBusmonInd,
            additional_info,
            source: IndividualAddress::new(0, 0, 0),
            destination: 0,
            address_type: AddressType::Group,
            hop_count: 7,
            priority: Priority::Low,
            confirm: false,
            ack_request: false,
            system_broadcast: false,
            apci: Apci::Unknown(0),
            data: raw_frame.to_vec(),
        }
    }

    /// Create a Bus Monitor indication with extended timestamp (microseconds).
    pub fn bus_monitor_indication_ext(
        raw_frame: &[u8],
        status: u8,
        timestamp_ms: u16,
        timestamp_us: u32,
    ) -> Self {
        let mut frame = Self::bus_monitor_indication(raw_frame, status, timestamp_ms);

        // Extended Relative Timestamp (type 0x06): 4 bytes, microseconds
        frame.additional_info.push(AdditionalInfo::new(
            AdditionalInfoType::ExtendedTimestamp as u8,
            timestamp_us.to_be_bytes().to_vec(),
        ));

        frame
    }

    /// Create an M_PropRead.con response frame.
    ///
    /// Responds to M_PropRead.req with the requested property value.
    /// The data format follows KNX cEMI property service encoding:
    /// [object_index(2) | property_id(2) | count_index(2) | data...]
    pub fn prop_read_con(
        object_index: u16,
        property_id: u16,
        count: u8,
        start_index: u8,
        value: Vec<u8>,
    ) -> Self {
        let mut data = Vec::with_capacity(6 + value.len());
        data.extend_from_slice(&object_index.to_be_bytes());
        data.extend_from_slice(&property_id.to_be_bytes());
        // Number of elements (4 bits) | start index (12 bits)
        let count_index = ((count as u16) << 12) | (start_index as u16 & 0x0FFF);
        data.extend_from_slice(&count_index.to_be_bytes());
        data.extend(value);

        Self {
            message_code: MessageCode::MPropReadCon,
            additional_info: Vec::new(),
            source: IndividualAddress::new(0, 0, 0),
            destination: 0,
            address_type: AddressType::Individual,
            hop_count: 7,
            priority: Priority::System,
            confirm: false,
            ack_request: false,
            system_broadcast: false,
            apci: Apci::Unknown(0),
            data,
        }
    }

    /// Create an M_PropWrite.con response frame.
    pub fn prop_write_con(
        object_index: u16,
        property_id: u16,
        count: u8,
        start_index: u8,
        success: bool,
    ) -> Self {
        let mut data = Vec::with_capacity(6);
        data.extend_from_slice(&object_index.to_be_bytes());
        data.extend_from_slice(&property_id.to_be_bytes());
        // On error: count=0 signals failure per KNX spec
        let actual_count = if success { count } else { 0 };
        let count_index = ((actual_count as u16) << 12) | (start_index as u16 & 0x0FFF);
        data.extend_from_slice(&count_index.to_be_bytes());

        Self {
            message_code: MessageCode::MPropWriteCon,
            additional_info: Vec::new(),
            source: IndividualAddress::new(0, 0, 0),
            destination: 0,
            address_type: AddressType::Individual,
            hop_count: 7,
            priority: Priority::System,
            confirm: !success,
            ack_request: false,
            system_broadcast: false,
            apci: Apci::Unknown(0),
            data,
        }
    }

    /// Create an M_Reset.ind frame (response to M_Reset.req).
    pub fn reset_ind() -> Self {
        Self {
            message_code: MessageCode::MResetInd,
            additional_info: Vec::new(),
            source: IndividualAddress::new(0, 0, 0),
            destination: 0,
            address_type: AddressType::Individual,
            hop_count: 7,
            priority: Priority::System,
            confirm: false,
            ack_request: false,
            system_broadcast: false,
            apci: Apci::Unknown(0),
            data: Vec::new(),
        }
    }

    /// Parse property service request data.
    ///
    /// Returns (object_index, property_id, count, start_index, remaining_data).
    pub fn parse_property_request(&self) -> Option<(u16, u16, u8, u8, &[u8])> {
        if self.data.len() < 6 {
            return None;
        }
        let object_index = u16::from_be_bytes([self.data[0], self.data[1]]);
        let property_id = u16::from_be_bytes([self.data[2], self.data[3]]);
        let count_index = u16::from_be_bytes([self.data[4], self.data[5]]);
        let count = (count_index >> 12) as u8;
        let start_index = (count_index & 0x0FFF) as u8;
        let remaining = &self.data[6..];
        Some((object_index, property_id, count, start_index, remaining))
    }

    /// Get destination as group address.
    pub fn destination_group(&self) -> Option<GroupAddress> {
        if self.address_type == AddressType::Group {
            Some(GroupAddress::from_raw(self.destination))
        } else {
            None
        }
    }

    /// Get destination as individual address.
    pub fn destination_individual(&self) -> Option<IndividualAddress> {
        if self.address_type == AddressType::Individual {
            Some(IndividualAddress::decode(self.destination))
        } else {
            None
        }
    }

    /// Set destination as group address.
    pub fn with_destination_group(mut self, addr: GroupAddress) -> Self {
        self.destination = addr.raw();
        self.address_type = AddressType::Group;
        self
    }

    /// Set destination as individual address.
    pub fn with_destination_individual(mut self, addr: IndividualAddress) -> Self {
        self.destination = addr.encode();
        self.address_type = AddressType::Individual;
        self
    }

    /// Set priority.
    pub fn with_priority(mut self, priority: Priority) -> Self {
        self.priority = priority;
        self
    }

    /// Set hop count.
    pub fn with_hop_count(mut self, hop_count: u8) -> Self {
        self.hop_count = hop_count.min(7);
        self
    }

    /// Encode to bytes.
    pub fn encode(&self) -> Vec<u8> {
        let mut buf = BytesMut::new();

        // Message code
        buf.put_u8(self.message_code.into());

        // Additional info length
        let add_info_len: usize = self.additional_info.iter().map(|i| i.encoded_len()).sum();
        buf.put_u8(add_info_len as u8);

        // Additional info
        for info in &self.additional_info {
            info.encode(&mut buf);
        }

        // Property service and reset frames use a different body format:
        // MC | add_info_len | [add_info...] | data
        // No Ctrl1/Ctrl2/addresses/NPDU — just raw property data.
        if self.message_code.is_property_service() || self.message_code.is_reset_service() {
            buf.put_slice(&self.data);
            return buf.to_vec();
        }

        // Bus monitor frames: MC | add_info_len | [add_info...] | raw_frame_data
        // The additional info carries status/timestamp, data is the raw bus frame.
        if self.message_code == MessageCode::LBusmonInd {
            buf.put_slice(&self.data);
            return buf.to_vec();
        }

        // Standard L_Data frame encoding (L_Data.req/con/ind, L_Raw)
        // Control byte 1
        let ctrl1 = 0x80 // Standard frame
            | (if self.ack_request { 0x00 } else { 0x20 }) // L/H (ack flag inverted)
            | (if !self.confirm { 0x00 } else { 0x01 }) // Confirm
            | ((self.priority.to_bits() & 0x03) << 2); // Priority
        buf.put_u8(ctrl1);

        // Control byte 2
        let ctrl2 = (if self.address_type == AddressType::Group {
            0x80
        } else {
            0x00
        }) | (self.hop_count & 0x07);
        buf.put_u8(ctrl2);

        // Source address
        buf.put_u16(self.source.encode());

        // Destination address
        buf.put_u16(self.destination);

        // NPDU
        let apci = self.apci.encode();

        // APCI internal value maps directly to first NPDU wire byte
        let apci_byte = apci as u8;

        if self.data.is_empty() {
            // No data - APCI only (e.g., GroupValueRead)
            buf.put_u8(1); // npdu_len = 1
            buf.put_u8(apci_byte);
        } else if self.data.len() == 1 && self.data[0] <= 0x3F {
            // Small data - embed in low 6 bits of APCI byte
            buf.put_u8(1); // npdu_len = 1
            buf.put_u8(apci_byte | (self.data[0] & 0x3F));
        } else {
            // Full data: npdu_len = 1 (APCI byte) + data.len()
            buf.put_u8((self.data.len() + 1) as u8);
            buf.put_u8(apci_byte);
            buf.put_slice(&self.data);
        }

        buf.to_vec()
    }

    /// Decode from bytes.
    pub fn decode(data: &[u8]) -> KnxResult<Self> {
        if data.len() < 2 {
            return Err(KnxError::frame_too_short(2, data.len()));
        }

        let mut buf = data;

        // Message code
        let message_code = MessageCode::try_from_u8(buf.get_u8())
            .ok_or_else(|| KnxError::UnknownMessageCode(data[0]))?;

        // Additional info
        let add_info_len = buf.get_u8() as usize;
        let mut additional_info = Vec::new();

        if add_info_len > 0 {
            if buf.len() < add_info_len {
                return Err(KnxError::frame_too_short(add_info_len, buf.len()));
            }
            let mut add_info_buf = &buf[..add_info_len];
            while !add_info_buf.is_empty() {
                additional_info.push(AdditionalInfo::decode(&mut add_info_buf)?);
            }
            buf = &buf[add_info_len..];
        }

        // Property service frames: MC | add_info_len | [add_info...] | data
        if message_code.is_property_service() || message_code.is_reset_service() {
            return Ok(Self {
                message_code,
                additional_info,
                source: IndividualAddress::new(0, 0, 0),
                destination: 0,
                address_type: AddressType::Individual,
                hop_count: 7,
                priority: Priority::System,
                confirm: false,
                ack_request: false,
                system_broadcast: false,
                apci: Apci::Unknown(0),
                data: buf.to_vec(),
            });
        }

        // Bus monitor frames: MC | add_info_len | [add_info...] | raw_frame_data
        if message_code == MessageCode::LBusmonInd {
            return Ok(Self {
                message_code,
                additional_info,
                source: IndividualAddress::new(0, 0, 0),
                destination: 0,
                address_type: AddressType::Group,
                hop_count: 7,
                priority: Priority::Low,
                confirm: false,
                ack_request: false,
                system_broadcast: false,
                apci: Apci::Unknown(0),
                data: buf.to_vec(),
            });
        }

        // Standard L_Data frame decoding
        if buf.len() < 7 {
            return Err(KnxError::frame_too_short(7, buf.len()));
        }

        // Control bytes
        let ctrl1 = buf.get_u8();
        let ctrl2 = buf.get_u8();

        let priority = Priority::from_bits((ctrl1 >> 2) & 0x03);
        let ack_request = ctrl1 & 0x20 == 0;
        let confirm = ctrl1 & 0x01 != 0;
        let system_broadcast = ctrl1 & 0x10 != 0;

        let address_type = AddressType::from_bit(ctrl2 & 0x80 != 0);
        let hop_count = ctrl2 & 0x07;

        // Addresses
        let source = IndividualAddress::decode(buf.get_u16());
        let destination = buf.get_u16();

        // NPDU
        // npdu_len counts bytes starting from the TPCI/APCI byte (inclusive)
        let npdu_len = buf.get_u8() as usize;
        if buf.len() < npdu_len {
            return Err(KnxError::frame_too_short(npdu_len, buf.len()));
        }

        // Support both the historical compact test encoding used by this crate
        // and the two-octet TPCI/APCI layout emitted by external stacks such as
        // XKNX. The latter uses a first byte with only TPCI/APCI high bits and a
        // second byte carrying the APCI service plus compact payload bits.
        let apci_byte1 = buf.get_u8();
        let (apci, frame_data) = if npdu_len >= 2 && (apci_byte1 & 0xFC) == 0 {
            let apci_byte2 = buf.get_u8();
            let apci_raw = (((apci_byte1 & 0x03) as u16) << 8) | apci_byte2 as u16;
            let apci = Apci::decode(apci_raw);
            let remaining_len = npdu_len - 2;
            let frame_data = if remaining_len == 0 {
                let small = apci_byte2 & 0x3F;
                if small != 0 {
                    vec![small]
                } else {
                    Vec::new()
                }
            } else {
                buf[..remaining_len].to_vec()
            };
            (apci, frame_data)
        } else {
            // Compact legacy layout: first NPDU byte maps directly to APCI.
            let apci = Apci::decode(apci_byte1 as u16);
            let frame_data = if npdu_len <= 1 {
                let small = apci_byte1 & 0x3F;
                if small != 0 {
                    vec![small]
                } else {
                    Vec::new()
                }
            } else {
                buf[..npdu_len - 1].to_vec()
            };
            (apci, frame_data)
        };

        Ok(Self {
            message_code,
            additional_info,
            source,
            destination,
            address_type,
            hop_count,
            priority,
            confirm,
            ack_request,
            system_broadcast,
            apci,
            data: frame_data,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_message_code() {
        assert_eq!(MessageCode::try_from_u8(0x11), Some(MessageCode::LDataReq));
        assert_eq!(MessageCode::try_from_u8(0x29), Some(MessageCode::LDataInd));
        assert!(MessageCode::LDataReq.is_data());
        assert!(MessageCode::LDataReq.is_request());
    }

    #[test]
    fn test_priority() {
        assert_eq!(Priority::from_bits(0), Priority::System);
        assert_eq!(Priority::from_bits(3), Priority::Low);
        assert_eq!(Priority::Normal.to_bits(), 1);
    }

    #[test]
    fn test_apci_encode_decode() {
        let tests = [
            Apci::GroupValueRead,
            Apci::GroupValueResponse,
            Apci::GroupValueWrite,
        ];

        for apci in tests {
            let encoded = apci.encode();
            let decoded = Apci::decode(encoded);
            assert_eq!(apci, decoded, "Failed for {:?}", apci);
        }
    }

    #[test]
    fn test_cemi_frame_group_value_write() {
        let frame = CemiFrame::group_value_write(
            IndividualAddress::new(1, 2, 3),
            GroupAddress::three_level(1, 2, 3),
            vec![0x01],
        );

        assert!(matches!(frame.apci, Apci::GroupValueWrite));
        assert_eq!(frame.source.to_string(), "1.2.3");
        assert_eq!(frame.destination_group().unwrap().to_string(), "1/2/3");
    }

    #[test]
    fn test_cemi_frame_encode_decode() {
        let original = CemiFrame::group_value_write(
            IndividualAddress::new(1, 2, 3),
            GroupAddress::three_level(1, 2, 100),
            vec![0x55],
        );

        let encoded = original.encode();
        let decoded = CemiFrame::decode(&encoded).unwrap();

        assert!(matches!(decoded.apci, Apci::GroupValueWrite));
        assert_eq!(decoded.source.to_string(), "1.2.3");
        assert_eq!(decoded.destination_group().unwrap().to_string(), "1/2/100");
        assert_eq!(decoded.data, vec![0x55]);
    }

    #[test]
    fn test_cemi_decode_external_two_octet_apci() {
        let decoded = CemiFrame::decode(&[
            0x11, // L_Data.req
            0x00, // additional info length
            0xBC, // control 1
            0xE0, // control 2, group destination
            0x11, 0x65, // source 1.1.101
            0x08, 0x02, // destination 1/0/2
            0x03, // TPCI/APCI plus one payload octet
            0x00, // TPCI + APCI high bits
            0x80, // GroupValueWrite
            0x80, // DPT5 payload
        ])
        .unwrap();

        assert_eq!(decoded.message_code, MessageCode::LDataReq);
        assert!(matches!(decoded.apci, Apci::GroupValueWrite));
        assert_eq!(decoded.source.to_string(), "1.1.101");
        assert_eq!(decoded.destination_group().unwrap().to_string(), "1/0/2");
        assert_eq!(decoded.data, vec![0x80]);
    }

    #[test]
    fn test_cemi_frame_group_value_read() {
        let frame = CemiFrame::group_value_read(
            IndividualAddress::new(1, 1, 1),
            GroupAddress::three_level(1, 0, 1),
        );

        let encoded = frame.encode();
        let decoded = CemiFrame::decode(&encoded).unwrap();

        assert!(matches!(decoded.apci, Apci::GroupValueRead));
        assert!(decoded.data.is_empty() || decoded.data == vec![0]);
    }

    #[test]
    fn test_message_code_confirmation_mapping() {
        assert_eq!(
            MessageCode::LDataReq.to_confirmation(),
            Some(MessageCode::LDataCon)
        );
        assert_eq!(
            MessageCode::LRawReq.to_confirmation(),
            Some(MessageCode::LRawCon)
        );
        assert_eq!(
            MessageCode::MPropReadReq.to_confirmation(),
            Some(MessageCode::MPropReadCon)
        );
        assert_eq!(
            MessageCode::MPropWriteReq.to_confirmation(),
            Some(MessageCode::MPropWriteCon)
        );
        assert_eq!(
            MessageCode::MResetReq.to_confirmation(),
            Some(MessageCode::MResetInd)
        );
        assert_eq!(MessageCode::LDataCon.to_confirmation(), None);
        assert_eq!(MessageCode::LDataInd.to_confirmation(), None);
    }

    #[test]
    fn test_message_code_categories() {
        assert!(MessageCode::LDataCon.is_confirmation());
        assert!(MessageCode::LRawCon.is_confirmation());
        assert!(MessageCode::MPropReadCon.is_confirmation());
        assert!(!MessageCode::LDataReq.is_confirmation());

        assert!(MessageCode::MPropReadReq.is_property_service());
        assert!(MessageCode::MPropWriteReq.is_property_service());
        assert!(MessageCode::MPropReadCon.is_property_service());
        assert!(!MessageCode::LDataReq.is_property_service());

        assert!(MessageCode::MResetReq.is_reset_service());
        assert!(MessageCode::MResetInd.is_reset_service());
        assert!(!MessageCode::LDataReq.is_reset_service());
    }

    #[test]
    fn test_bus_monitor_indication() {
        let raw_frame = vec![0x29, 0x00, 0xBC, 0x11, 0x01, 0x09, 0x01, 0x00, 0x80, 0x01];
        let frame = CemiFrame::bus_monitor_indication(&raw_frame, 0x00, 150);

        assert_eq!(frame.message_code, MessageCode::LBusmonInd);
        assert_eq!(frame.additional_info.len(), 2);

        // Bus Monitor Info (type 0x03)
        assert_eq!(
            frame.additional_info[0].info_type,
            AdditionalInfoType::BusMonitorInfo as u8
        );
        assert_eq!(frame.additional_info[0].data, vec![0x00]);

        // Timestamp Relative (type 0x04)
        assert_eq!(
            frame.additional_info[1].info_type,
            AdditionalInfoType::TimestampRelative as u8
        );
        assert_eq!(frame.additional_info[1].data, vec![0x00, 0x96]); // 150 in BE

        // Raw frame data
        assert_eq!(frame.data, raw_frame);

        // Verify encode/decode roundtrip
        let encoded = frame.encode();
        let decoded = CemiFrame::decode(&encoded).unwrap();
        assert_eq!(decoded.message_code, MessageCode::LBusmonInd);
        assert_eq!(decoded.additional_info.len(), 2);
        assert_eq!(decoded.data, raw_frame);
    }

    #[test]
    fn test_bus_monitor_indication_ext() {
        let raw_frame = vec![0x29, 0x00, 0xBC];
        let frame = CemiFrame::bus_monitor_indication_ext(&raw_frame, 0x80, 100, 123456);

        assert_eq!(frame.additional_info.len(), 3);
        // Extended Timestamp (type 0x06)
        assert_eq!(
            frame.additional_info[2].info_type,
            AdditionalInfoType::ExtendedTimestamp as u8
        );
        let ts_bytes = &frame.additional_info[2].data;
        let ts = u32::from_be_bytes([ts_bytes[0], ts_bytes[1], ts_bytes[2], ts_bytes[3]]);
        assert_eq!(ts, 123456);
    }

    #[test]
    fn test_prop_read_con() {
        let frame = CemiFrame::prop_read_con(0, 11, 1, 1, vec![0x00, 0x00, 0x00, 0x00, 0x00, 0x01]);
        assert_eq!(frame.message_code, MessageCode::MPropReadCon);

        // Parse the property request back
        let parsed = frame.parse_property_request().unwrap();
        assert_eq!(parsed.0, 0); // object_index
        assert_eq!(parsed.1, 11); // property_id (serial number)
        assert_eq!(parsed.2, 1); // count
        assert_eq!(parsed.3, 1); // start_index
        assert_eq!(parsed.4, &[0x00, 0x00, 0x00, 0x00, 0x00, 0x01]); // value

        // Verify encode/decode roundtrip
        let encoded = frame.encode();
        let decoded = CemiFrame::decode(&encoded).unwrap();
        assert_eq!(decoded.message_code, MessageCode::MPropReadCon);
        assert_eq!(decoded.data, frame.data);
    }

    #[test]
    fn test_prop_write_con_success() {
        let frame = CemiFrame::prop_write_con(0, 14, 1, 1, true);
        assert_eq!(frame.message_code, MessageCode::MPropWriteCon);
        assert!(!frame.confirm); // success: confirm=false

        let parsed = frame.parse_property_request().unwrap();
        assert_eq!(parsed.2, 1); // count=1 on success
    }

    #[test]
    fn test_prop_write_con_failure() {
        let frame = CemiFrame::prop_write_con(0, 14, 1, 1, false);
        assert!(frame.confirm); // failure: confirm=true

        let parsed = frame.parse_property_request().unwrap();
        assert_eq!(parsed.2, 0); // count=0 signals error per KNX spec
    }

    #[test]
    fn test_reset_ind() {
        let frame = CemiFrame::reset_ind();
        assert_eq!(frame.message_code, MessageCode::MResetInd);
        assert!(frame.data.is_empty());

        // Verify encode/decode roundtrip
        let encoded = frame.encode();
        let decoded = CemiFrame::decode(&encoded).unwrap();
        assert_eq!(decoded.message_code, MessageCode::MResetInd);
    }

    #[test]
    fn test_parse_property_request_too_short() {
        let frame = CemiFrame {
            message_code: MessageCode::MPropReadReq,
            additional_info: Vec::new(),
            source: IndividualAddress::new(0, 0, 0),
            destination: 0,
            address_type: AddressType::Individual,
            hop_count: 7,
            priority: Priority::System,
            confirm: false,
            ack_request: false,
            system_broadcast: false,
            apci: Apci::Unknown(0),
            data: vec![0x00, 0x00], // Only 2 bytes, need 6
        };

        assert!(frame.parse_property_request().is_none());
    }
}

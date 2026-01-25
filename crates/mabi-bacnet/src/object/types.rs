//! BACnet object types and identifiers.

use serde::{Deserialize, Serialize};
use std::fmt;

/// BACnet object types per ASHRAE 135.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[repr(u16)]
pub enum ObjectType {
    AnalogInput = 0,
    AnalogOutput = 1,
    AnalogValue = 2,
    BinaryInput = 3,
    BinaryOutput = 4,
    BinaryValue = 5,
    Calendar = 6,
    Command = 7,
    Device = 8,
    EventEnrollment = 9,
    File = 10,
    Group = 11,
    Loop = 12,
    MultiStateInput = 13,
    MultiStateOutput = 14,
    NotificationClass = 15,
    Program = 16,
    Schedule = 17,
    Averaging = 18,
    MultiStateValue = 19,
    TrendLog = 20,
    LifeSafetyPoint = 21,
    LifeSafetyZone = 22,
    Accumulator = 23,
    PulseConverter = 24,
    EventLog = 25,
    GlobalGroup = 26,
    TrendLogMultiple = 27,
    LoadControl = 28,
    StructuredView = 29,
    AccessDoor = 30,
    Timer = 31,
    AccessCredential = 32,
    AccessPoint = 33,
    AccessRights = 34,
    AccessUser = 35,
    AccessZone = 36,
    CredentialDataInput = 37,
    NetworkSecurity = 38,
    BitStringValue = 39,
    CharacterStringValue = 40,
    DatePatternValue = 41,
    DateValue = 42,
    DatetimePatternValue = 43,
    DatetimeValue = 44,
    IntegerValue = 45,
    LargeAnalogValue = 46,
    OctetStringValue = 47,
    PositiveIntegerValue = 48,
    TimePatternValue = 49,
    TimeValue = 50,
    NotificationForwarder = 51,
    AlertEnrollment = 52,
    Channel = 53,
    LightingOutput = 54,
    BinaryLightingOutput = 55,
    NetworkPort = 56,
    ElevatorGroup = 57,
    Escalator = 58,
    Lift = 59,
}

impl ObjectType {
    /// Create from u16 value.
    pub fn from_u16(value: u16) -> Option<Self> {
        match value {
            0 => Some(Self::AnalogInput),
            1 => Some(Self::AnalogOutput),
            2 => Some(Self::AnalogValue),
            3 => Some(Self::BinaryInput),
            4 => Some(Self::BinaryOutput),
            5 => Some(Self::BinaryValue),
            6 => Some(Self::Calendar),
            7 => Some(Self::Command),
            8 => Some(Self::Device),
            9 => Some(Self::EventEnrollment),
            10 => Some(Self::File),
            11 => Some(Self::Group),
            12 => Some(Self::Loop),
            13 => Some(Self::MultiStateInput),
            14 => Some(Self::MultiStateOutput),
            15 => Some(Self::NotificationClass),
            16 => Some(Self::Program),
            17 => Some(Self::Schedule),
            18 => Some(Self::Averaging),
            19 => Some(Self::MultiStateValue),
            20 => Some(Self::TrendLog),
            53 => Some(Self::Channel),
            54 => Some(Self::LightingOutput),
            56 => Some(Self::NetworkPort),
            _ => None,
        }
    }

    /// Check if this object type is writable.
    pub fn is_writable(&self) -> bool {
        matches!(
            self,
            Self::AnalogOutput
                | Self::AnalogValue
                | Self::BinaryOutput
                | Self::BinaryValue
                | Self::MultiStateOutput
                | Self::MultiStateValue
                | Self::Command
                | Self::Schedule
                | Self::Calendar
        )
    }

    /// Check if this object type supports COV.
    pub fn supports_cov(&self) -> bool {
        matches!(
            self,
            Self::AnalogInput
                | Self::AnalogOutput
                | Self::AnalogValue
                | Self::BinaryInput
                | Self::BinaryOutput
                | Self::BinaryValue
                | Self::MultiStateInput
                | Self::MultiStateOutput
                | Self::MultiStateValue
                | Self::LightingOutput
                | Self::Channel
        )
    }

    /// Get display name.
    pub fn display_name(&self) -> &'static str {
        match self {
            Self::AnalogInput => "Analog Input",
            Self::AnalogOutput => "Analog Output",
            Self::AnalogValue => "Analog Value",
            Self::BinaryInput => "Binary Input",
            Self::BinaryOutput => "Binary Output",
            Self::BinaryValue => "Binary Value",
            Self::Calendar => "Calendar",
            Self::Command => "Command",
            Self::Device => "Device",
            Self::EventEnrollment => "Event Enrollment",
            Self::File => "File",
            Self::Group => "Group",
            Self::Loop => "Loop",
            Self::MultiStateInput => "Multi-state Input",
            Self::MultiStateOutput => "Multi-state Output",
            Self::NotificationClass => "Notification Class",
            Self::Program => "Program",
            Self::Schedule => "Schedule",
            Self::Averaging => "Averaging",
            Self::MultiStateValue => "Multi-state Value",
            Self::TrendLog => "Trend Log",
            Self::LifeSafetyPoint => "Life Safety Point",
            Self::LifeSafetyZone => "Life Safety Zone",
            Self::Accumulator => "Accumulator",
            Self::PulseConverter => "Pulse Converter",
            Self::EventLog => "Event Log",
            Self::GlobalGroup => "Global Group",
            Self::TrendLogMultiple => "Trend Log Multiple",
            Self::LoadControl => "Load Control",
            Self::StructuredView => "Structured View",
            Self::AccessDoor => "Access Door",
            Self::Timer => "Timer",
            Self::AccessCredential => "Access Credential",
            Self::AccessPoint => "Access Point",
            Self::AccessRights => "Access Rights",
            Self::AccessUser => "Access User",
            Self::AccessZone => "Access Zone",
            Self::CredentialDataInput => "Credential Data Input",
            Self::NetworkSecurity => "Network Security",
            Self::BitStringValue => "BitString Value",
            Self::CharacterStringValue => "CharacterString Value",
            Self::DatePatternValue => "Date Pattern Value",
            Self::DateValue => "Date Value",
            Self::DatetimePatternValue => "DateTime Pattern Value",
            Self::DatetimeValue => "DateTime Value",
            Self::IntegerValue => "Integer Value",
            Self::LargeAnalogValue => "Large Analog Value",
            Self::OctetStringValue => "OctetString Value",
            Self::PositiveIntegerValue => "Positive Integer Value",
            Self::TimePatternValue => "Time Pattern Value",
            Self::TimeValue => "Time Value",
            Self::NotificationForwarder => "Notification Forwarder",
            Self::AlertEnrollment => "Alert Enrollment",
            Self::Channel => "Channel",
            Self::LightingOutput => "Lighting Output",
            Self::BinaryLightingOutput => "Binary Lighting Output",
            Self::NetworkPort => "Network Port",
            Self::ElevatorGroup => "Elevator Group",
            Self::Escalator => "Escalator",
            Self::Lift => "Lift",
        }
    }

    /// Get the abbreviation.
    pub fn abbreviation(&self) -> &'static str {
        match self {
            Self::AnalogInput => "AI",
            Self::AnalogOutput => "AO",
            Self::AnalogValue => "AV",
            Self::BinaryInput => "BI",
            Self::BinaryOutput => "BO",
            Self::BinaryValue => "BV",
            Self::MultiStateInput => "MSI",
            Self::MultiStateOutput => "MSO",
            Self::MultiStateValue => "MSV",
            Self::Device => "DEV",
            _ => "OBJ",
        }
    }
}

impl TryFrom<u16> for ObjectType {
    type Error = ();

    fn try_from(value: u16) -> Result<Self, Self::Error> {
        Self::from_u16(value).ok_or(())
    }
}

impl fmt::Display for ObjectType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.display_name())
    }
}

/// BACnet object identifier (type + instance).
///
/// Encoded as a 32-bit value with 10 bits for type and 22 bits for instance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ObjectId {
    /// Object type.
    pub object_type: ObjectType,
    /// Instance number (0 to 4194302).
    pub instance: u32,
}

impl ObjectId {
    /// Maximum instance number (2^22 - 2).
    pub const MAX_INSTANCE: u32 = 0x3FFFFF - 1;

    /// Wildcard instance for queries.
    pub const WILDCARD_INSTANCE: u32 = 0x3FFFFF;

    /// Create a new object identifier.
    pub fn new(object_type: ObjectType, instance: u32) -> Self {
        debug_assert!(
            instance <= Self::MAX_INSTANCE,
            "Instance {} exceeds maximum {}",
            instance,
            Self::MAX_INSTANCE
        );
        Self {
            object_type,
            instance,
        }
    }

    /// Create a device object identifier.
    pub fn device(instance: u32) -> Self {
        Self::new(ObjectType::Device, instance)
    }

    /// Encode as 32-bit value.
    pub fn encode(&self) -> u32 {
        ((self.object_type as u32) << 22) | (self.instance & 0x3FFFFF)
    }

    /// Decode from 32-bit value.
    pub fn decode(value: u32) -> Option<Self> {
        let type_code = (value >> 22) as u16;
        let instance = value & 0x3FFFFF;

        ObjectType::from_u16(type_code).map(|object_type| Self {
            object_type,
            instance,
        })
    }

    /// Check if this is a device object.
    pub fn is_device(&self) -> bool {
        self.object_type == ObjectType::Device
    }

    /// Get a string representation like "AI:1" or "BV:100".
    pub fn short_string(&self) -> String {
        format!("{}:{}", self.object_type.abbreviation(), self.instance)
    }
}

impl fmt::Display for ObjectId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}", self.object_type.display_name(), self.instance)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_object_type_from_u16() {
        assert_eq!(ObjectType::from_u16(0), Some(ObjectType::AnalogInput));
        assert_eq!(ObjectType::from_u16(8), Some(ObjectType::Device));
        assert_eq!(ObjectType::from_u16(19), Some(ObjectType::MultiStateValue));
    }

    #[test]
    fn test_object_type_writable() {
        assert!(!ObjectType::AnalogInput.is_writable());
        assert!(ObjectType::AnalogOutput.is_writable());
        assert!(ObjectType::AnalogValue.is_writable());
        assert!(!ObjectType::BinaryInput.is_writable());
        assert!(ObjectType::BinaryOutput.is_writable());
    }

    #[test]
    fn test_object_id_encode_decode() {
        let id = ObjectId::new(ObjectType::AnalogInput, 100);
        let encoded = id.encode();
        let decoded = ObjectId::decode(encoded).unwrap();

        assert_eq!(decoded.object_type, ObjectType::AnalogInput);
        assert_eq!(decoded.instance, 100);
    }

    #[test]
    fn test_object_id_max_instance() {
        let id = ObjectId::new(ObjectType::Device, ObjectId::MAX_INSTANCE);
        let encoded = id.encode();
        let decoded = ObjectId::decode(encoded).unwrap();

        assert_eq!(decoded.instance, ObjectId::MAX_INSTANCE);
    }

    #[test]
    fn test_object_id_short_string() {
        assert_eq!(
            ObjectId::new(ObjectType::AnalogInput, 1).short_string(),
            "AI:1"
        );
        assert_eq!(
            ObjectId::new(ObjectType::BinaryValue, 100).short_string(),
            "BV:100"
        );
    }
}

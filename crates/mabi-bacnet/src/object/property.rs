//! BACnet property management.
//!
//! This module provides property identifiers, values, and storage
//! for BACnet objects with concurrent access support.

use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use std::fmt;

use super::types::ObjectId;

/// BACnet property identifiers per ASHRAE 135.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[repr(u32)]
pub enum PropertyId {
    // Core properties
    AckedTransitions = 0,
    AckRequired = 1,
    Action = 2,
    ActionText = 3,
    ActiveText = 4,
    ActiveVtSessions = 5,
    AlarmValue = 6,
    AlarmValues = 7,
    All = 8,
    AllWritesSuccessful = 9,
    ApduSegmentTimeout = 10,
    ApduTimeout = 11,
    ApplicationSoftwareVersion = 12,
    Archive = 13,
    Bias = 14,
    ChangeOfStateCount = 15,
    ChangeOfStateTime = 16,
    NotificationClass = 17,
    ControlledVariableReference = 19,
    ControlledVariableUnits = 20,
    ControlledVariableValue = 21,
    CovIncrement = 22,
    DateList = 23,
    DaylightSavingsStatus = 24,
    Deadband = 25,
    DerivativeConstant = 26,
    DerivativeConstantUnits = 27,
    Description = 28,
    DescriptionOfHalt = 29,
    DeviceAddressBinding = 30,
    DeviceType = 31,
    EffectivePeriod = 32,
    ElapsedActiveTime = 33,
    ErrorLimit = 34,
    EventEnable = 35,
    EventState = 36,
    EventType = 37,
    ExceptionSchedule = 38,
    FaultValues = 39,
    FeedbackValue = 40,
    FileAccessMethod = 41,
    FileSize = 42,
    FileType = 43,
    FirmwareRevision = 44,
    HighLimit = 45,
    InactiveText = 46,
    InProcess = 47,
    InstanceOf = 48,
    IntegralConstant = 49,
    IntegralConstantUnits = 50,
    LimitEnable = 52,
    ListOfGroupMembers = 53,
    ListOfObjectPropertyReferences = 54,
    LocalDate = 56,
    LocalTime = 57,
    Location = 58,
    LowLimit = 59,
    ManipulatedVariableReference = 60,
    MaximumOutput = 61,
    MaxApduLengthAccepted = 62,
    MaxInfoFrames = 63,
    MaxMaster = 64,
    MaxPresValue = 65,
    MinimumOffTime = 66,
    MinimumOnTime = 67,
    MinimumOutput = 68,
    MinPresValue = 69,
    ModelName = 70,
    ModificationDate = 71,
    NotifyType = 72,
    NumberOfApduRetries = 73,
    NumberOfStates = 74,
    ObjectIdentifier = 75,
    ObjectList = 76,
    ObjectName = 77,
    ObjectPropertyReference = 78,
    ObjectType = 79,
    Optional = 80,
    OutOfService = 81,
    OutputUnits = 82,
    EventParameters = 83,
    Polarity = 84,
    PresentValue = 85,
    Priority = 86,
    PriorityArray = 87,
    PriorityForWriting = 88,
    ProcessIdentifier = 89,
    ProgramChange = 90,
    ProgramLocation = 91,
    ProgramState = 92,
    ProportionalConstant = 93,
    ProportionalConstantUnits = 94,
    ProtocolObjectTypesSupported = 96,
    ProtocolServicesSupported = 97,
    ProtocolVersion = 98,
    ReadOnly = 99,
    ReasonForHalt = 100,
    Recipient = 101,
    RecipientList = 102,
    Reliability = 103,
    RelinquishDefault = 104,
    Required = 105,
    Resolution = 106,
    SegmentationSupported = 107,
    Setpoint = 108,
    SetpointReference = 109,
    StateText = 110,
    StatusFlags = 111,
    SystemStatus = 112,
    TimeDelay = 113,
    TimeOfActiveTimeReset = 114,
    TimeOfStateCountReset = 115,
    TimeSynchronizationRecipients = 116,
    Units = 117,
    UpdateInterval = 118,
    UtcOffset = 119,
    VendorIdentifier = 120,
    VendorName = 121,
    VtClassesSupported = 122,
    WeeklySchedule = 123,
    AttemptedSamples = 124,
    AverageValue = 125,
    BufferSize = 126,
    ClientCovIncrement = 127,
    CovResubscriptionInterval = 128,
    EventTimeStamps = 130,
    LogBuffer = 131,
    LogDeviceObjectProperty = 132,
    Enable = 133,
    LogInterval = 134,
    MaximumValue = 135,
    MinimumValue = 136,
    NotificationThreshold = 137,
    PreviousNotifyRecord = 138,
    ProtocolRevision = 139,
    RecordsSinceNotification = 140,
    RecordCount = 141,
    StartTime = 142,
    StopTime = 143,
    StopWhenFull = 144,
    TotalRecordCount = 145,
    ValidSamples = 146,
    WindowInterval = 147,
    WindowSamples = 148,
    MaximumValueTimestamp = 149,
    MinimumValueTimestamp = 150,
    VarianceValue = 151,
    ActiveCovSubscriptions = 152,
    BackupFailureTimeout = 153,
    ConfigurationFiles = 154,
    DatabaseRevision = 155,
    DirectReading = 156,
    LastRestoreTime = 157,
    MaintenanceRequired = 158,
    MemberOf = 159,
    Mode = 160,
    OperationExpected = 161,
    Setting = 162,
    Silenced = 163,
    TrackingValue = 164,
    ZoneMembers = 165,
    LifeSafetyAlarmValues = 166,
    MaxSegmentsAccepted = 167,
    ProfileName = 168,
    AutoSlaveDiscovery = 169,
    ManualSlaveAddressBinding = 170,
    SlaveAddressBinding = 171,
    SlaveProxyEnable = 172,
    LastNotifyRecord = 173,
    ScheduleDefault = 174,
    AcceptedModes = 175,
    AdjustValue = 176,
    Count = 177,
    CountBeforeChange = 178,
    CountChangeTime = 179,
    CovPeriod = 180,
    InputReference = 181,
    LimitMonitoringInterval = 182,
    LoggingObject = 183,
    LoggingRecord = 184,
    Prescale = 185,
    PulseRate = 186,
    Scale = 187,
    ScaleFactor = 188,
    UpdateTime = 189,
    ValueBeforeChange = 190,
    ValueSet = 191,
    ValueChangeTime = 192,
}

impl PropertyId {
    /// Create from u32 value.
    pub fn from_u32(value: u32) -> Option<Self> {
        // Only handle common properties to keep the match manageable
        match value {
            75 => Some(Self::ObjectIdentifier),
            77 => Some(Self::ObjectName),
            79 => Some(Self::ObjectType),
            85 => Some(Self::PresentValue),
            28 => Some(Self::Description),
            111 => Some(Self::StatusFlags),
            36 => Some(Self::EventState),
            103 => Some(Self::Reliability),
            81 => Some(Self::OutOfService),
            117 => Some(Self::Units),
            69 => Some(Self::MinPresValue),
            65 => Some(Self::MaxPresValue),
            106 => Some(Self::Resolution),
            22 => Some(Self::CovIncrement),
            17 => Some(Self::NotificationClass),
            45 => Some(Self::HighLimit),
            59 => Some(Self::LowLimit),
            25 => Some(Self::Deadband),
            52 => Some(Self::LimitEnable),
            35 => Some(Self::EventEnable),
            72 => Some(Self::NotifyType),
            130 => Some(Self::EventTimeStamps),
            87 => Some(Self::PriorityArray),
            104 => Some(Self::RelinquishDefault),
            74 => Some(Self::NumberOfStates),
            110 => Some(Self::StateText),
            84 => Some(Self::Polarity),
            4 => Some(Self::ActiveText),
            46 => Some(Self::InactiveText),
            // Device properties
            112 => Some(Self::SystemStatus),
            121 => Some(Self::VendorName),
            120 => Some(Self::VendorIdentifier),
            70 => Some(Self::ModelName),
            44 => Some(Self::FirmwareRevision),
            12 => Some(Self::ApplicationSoftwareVersion),
            98 => Some(Self::ProtocolVersion),
            139 => Some(Self::ProtocolRevision),
            97 => Some(Self::ProtocolServicesSupported),
            96 => Some(Self::ProtocolObjectTypesSupported),
            76 => Some(Self::ObjectList),
            62 => Some(Self::MaxApduLengthAccepted),
            107 => Some(Self::SegmentationSupported),
            11 => Some(Self::ApduTimeout),
            73 => Some(Self::NumberOfApduRetries),
            155 => Some(Self::DatabaseRevision),
            152 => Some(Self::ActiveCovSubscriptions),
            _ => None,
        }
    }

    /// Check if this property is required for all objects.
    pub fn is_required(&self) -> bool {
        matches!(
            self,
            Self::ObjectIdentifier | Self::ObjectName | Self::ObjectType
        )
    }

    /// Check if this property is typically read-only.
    pub fn is_read_only(&self) -> bool {
        matches!(
            self,
            Self::ObjectIdentifier
                | Self::ObjectType
                | Self::ObjectList
                | Self::StatusFlags
                | Self::EventState
                | Self::ProtocolVersion
                | Self::ProtocolRevision
        )
    }

    /// Get display name.
    pub fn display_name(&self) -> &'static str {
        match self {
            Self::ObjectIdentifier => "Object_Identifier",
            Self::ObjectName => "Object_Name",
            Self::ObjectType => "Object_Type",
            Self::PresentValue => "Present_Value",
            Self::Description => "Description",
            Self::StatusFlags => "Status_Flags",
            Self::EventState => "Event_State",
            Self::Reliability => "Reliability",
            Self::OutOfService => "Out_Of_Service",
            Self::Units => "Units",
            Self::MinPresValue => "Min_Pres_Value",
            Self::MaxPresValue => "Max_Pres_Value",
            Self::CovIncrement => "COV_Increment",
            Self::HighLimit => "High_Limit",
            Self::LowLimit => "Low_Limit",
            Self::Deadband => "Deadband",
            Self::PriorityArray => "Priority_Array",
            Self::RelinquishDefault => "Relinquish_Default",
            Self::NumberOfStates => "Number_Of_States",
            Self::StateText => "State_Text",
            Self::Polarity => "Polarity",
            Self::ActiveText => "Active_Text",
            Self::InactiveText => "Inactive_Text",
            Self::SystemStatus => "System_Status",
            Self::VendorName => "Vendor_Name",
            Self::VendorIdentifier => "Vendor_Identifier",
            Self::ModelName => "Model_Name",
            Self::FirmwareRevision => "Firmware_Revision",
            Self::ApplicationSoftwareVersion => "Application_Software_Version",
            Self::ProtocolVersion => "Protocol_Version",
            Self::ProtocolRevision => "Protocol_Revision",
            Self::ObjectList => "Object_List",
            Self::MaxApduLengthAccepted => "Max_APDU_Length_Accepted",
            Self::SegmentationSupported => "Segmentation_Supported",
            Self::ApduTimeout => "APDU_Timeout",
            Self::NumberOfApduRetries => "Number_Of_APDU_Retries",
            Self::DatabaseRevision => "Database_Revision",
            Self::ActiveCovSubscriptions => "Active_COV_Subscriptions",
            _ => "Unknown",
        }
    }
}

impl fmt::Display for PropertyId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.display_name())
    }
}

/// BACnet event state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[repr(u32)]
pub enum EventState {
    #[default]
    Normal = 0,
    Fault = 1,
    Offnormal = 2,
    HighLimit = 3,
    LowLimit = 4,
    LifeSafetyAlarm = 5,
}

/// BACnet reliability.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[repr(u32)]
pub enum Reliability {
    #[default]
    NoFaultDetected = 0,
    NoSensor = 1,
    OverRange = 2,
    UnderRange = 3,
    OpenLoop = 4,
    ShortedLoop = 5,
    NoOutput = 6,
    UnreliableOther = 7,
    ProcessError = 8,
    MultiStateFault = 9,
    ConfigurationError = 10,
    CommunicationFailure = 12,
    MemberFault = 13,
    MonitoredObjectFault = 14,
    Tripped = 15,
}

/// BACnet polarity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[repr(u32)]
pub enum Polarity {
    #[default]
    Normal = 0,
    Reverse = 1,
}

/// BACnet segmentation support.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[repr(u32)]
pub enum SegmentationSupport {
    Both = 0,
    Transmit = 1,
    Receive = 2,
    #[default]
    None = 3,
}

/// Status flags for BACnet objects.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct StatusFlags {
    /// Object is in alarm.
    pub in_alarm: bool,
    /// Object has a fault.
    pub fault: bool,
    /// Object is overridden.
    pub overridden: bool,
    /// Object is out of service.
    pub out_of_service: bool,
}

impl StatusFlags {
    /// Create new status flags with all false.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the fault flag.
    pub fn with_fault(mut self, fault: bool) -> Self {
        self.fault = fault;
        self
    }

    /// Set the out of service flag.
    pub fn with_out_of_service(mut self, oos: bool) -> Self {
        self.out_of_service = oos;
        self
    }

    /// Convert to bit vector (for encoding).
    pub fn to_bits(&self) -> Vec<bool> {
        vec![self.in_alarm, self.fault, self.overridden, self.out_of_service]
    }

    /// Create from bit vector.
    pub fn from_bits(bits: &[bool]) -> Self {
        Self {
            in_alarm: bits.first().copied().unwrap_or(false),
            fault: bits.get(1).copied().unwrap_or(false),
            overridden: bits.get(2).copied().unwrap_or(false),
            out_of_service: bits.get(3).copied().unwrap_or(false),
        }
    }

    /// Encode as byte (bit-packed).
    pub fn encode(&self) -> u8 {
        let mut result = 0u8;
        if self.in_alarm {
            result |= 0x08;
        }
        if self.fault {
            result |= 0x04;
        }
        if self.overridden {
            result |= 0x02;
        }
        if self.out_of_service {
            result |= 0x01;
        }
        result
    }
}

/// BACnet date.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct BACnetDate {
    /// Year minus 1900 (255 = unspecified).
    pub year: u8,
    /// Month 1-14 (255 = unspecified, 13 = odd months, 14 = even months).
    pub month: u8,
    /// Day 1-34 (255 = unspecified, 32 = last day, 33 = odd days, 34 = even days).
    pub day: u8,
    /// Day of week 1-7 (1 = Monday, 255 = unspecified).
    pub day_of_week: u8,
}

impl Default for BACnetDate {
    fn default() -> Self {
        Self {
            year: 255,
            month: 255,
            day: 255,
            day_of_week: 255,
        }
    }
}

/// BACnet time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct BACnetTime {
    /// Hour 0-23 (255 = unspecified).
    pub hour: u8,
    /// Minute 0-59 (255 = unspecified).
    pub minute: u8,
    /// Second 0-59 (255 = unspecified).
    pub second: u8,
    /// Hundredths 0-99 (255 = unspecified).
    pub hundredths: u8,
}

impl Default for BACnetTime {
    fn default() -> Self {
        Self {
            hour: 255,
            minute: 255,
            second: 255,
            hundredths: 255,
        }
    }
}

/// BACnet property values.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum BACnetValue {
    /// Null value.
    Null,
    /// Boolean value.
    Boolean(bool),
    /// Unsigned integer (up to 32 bits).
    Unsigned(u32),
    /// Unsigned 64-bit integer.
    Unsigned64(u64),
    /// Signed integer.
    Signed(i32),
    /// Signed 64-bit integer.
    Signed64(i64),
    /// Single-precision float.
    Real(f32),
    /// Double-precision float.
    Double(f64),
    /// Octet string (raw bytes).
    OctetString(Vec<u8>),
    /// Character string (UTF-8).
    CharacterString(String),
    /// Bit string.
    BitString(Vec<bool>),
    /// Enumerated value.
    Enumerated(u32),
    /// Date.
    Date(BACnetDate),
    /// Time.
    Time(BACnetTime),
    /// Object identifier.
    ObjectIdentifier(ObjectId),
    /// Array of values.
    Array(Vec<BACnetValue>),
    /// List of values.
    List(Vec<BACnetValue>),
}

impl BACnetValue {
    /// Get as boolean if applicable.
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Self::Boolean(v) => Some(*v),
            Self::Enumerated(v) => Some(*v != 0),
            _ => None,
        }
    }

    /// Get as unsigned integer if applicable.
    pub fn as_unsigned(&self) -> Option<u32> {
        match self {
            Self::Unsigned(v) => Some(*v),
            Self::Enumerated(v) => Some(*v),
            Self::Boolean(v) => Some(if *v { 1 } else { 0 }),
            _ => None,
        }
    }

    /// Get as float if applicable.
    pub fn as_real(&self) -> Option<f32> {
        match self {
            Self::Real(v) => Some(*v),
            Self::Double(v) => Some(*v as f32),
            Self::Unsigned(v) => Some(*v as f32),
            Self::Signed(v) => Some(*v as f32),
            _ => None,
        }
    }

    /// Get as string if applicable.
    pub fn as_string(&self) -> Option<&str> {
        match self {
            Self::CharacterString(s) => Some(s),
            _ => None,
        }
    }

    /// Get as object identifier if applicable.
    pub fn as_object_id(&self) -> Option<ObjectId> {
        match self {
            Self::ObjectIdentifier(id) => Some(*id),
            _ => None,
        }
    }

    /// Check if value is null.
    pub fn is_null(&self) -> bool {
        matches!(self, Self::Null)
    }
}

impl fmt::Display for BACnetValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Null => write!(f, "null"),
            Self::Boolean(v) => write!(f, "{}", v),
            Self::Unsigned(v) => write!(f, "{}", v),
            Self::Unsigned64(v) => write!(f, "{}", v),
            Self::Signed(v) => write!(f, "{}", v),
            Self::Signed64(v) => write!(f, "{}", v),
            Self::Real(v) => write!(f, "{:.2}", v),
            Self::Double(v) => write!(f, "{:.4}", v),
            Self::OctetString(v) => write!(f, "OctetString[{}]", v.len()),
            Self::CharacterString(v) => write!(f, "\"{}\"", v),
            Self::BitString(v) => write!(f, "BitString[{}]", v.len()),
            Self::Enumerated(v) => write!(f, "Enum({})", v),
            Self::Date(_) => write!(f, "Date"),
            Self::Time(_) => write!(f, "Time"),
            Self::ObjectIdentifier(id) => write!(f, "{}", id),
            Self::Array(v) => write!(f, "Array[{}]", v.len()),
            Self::List(v) => write!(f, "List[{}]", v.len()),
        }
    }
}

/// Property storage with concurrent access support.
pub struct PropertyStore {
    properties: DashMap<PropertyId, BACnetValue>,
}

impl PropertyStore {
    /// Create a new empty property store.
    pub fn new() -> Self {
        Self {
            properties: DashMap::new(),
        }
    }

    /// Get a property value.
    pub fn get(&self, property_id: PropertyId) -> Option<BACnetValue> {
        self.properties.get(&property_id).map(|v| v.clone())
    }

    /// Set a property value.
    pub fn set(&self, property_id: PropertyId, value: BACnetValue) {
        self.properties.insert(property_id, value);
    }

    /// Remove a property.
    pub fn remove(&self, property_id: PropertyId) -> Option<BACnetValue> {
        self.properties.remove(&property_id).map(|(_, v)| v)
    }

    /// Check if a property exists.
    pub fn contains(&self, property_id: PropertyId) -> bool {
        self.properties.contains_key(&property_id)
    }

    /// List all property IDs.
    pub fn list(&self) -> Vec<PropertyId> {
        self.properties.iter().map(|r| *r.key()).collect()
    }

    /// Get the number of properties.
    pub fn len(&self) -> usize {
        self.properties.len()
    }

    /// Check if empty.
    pub fn is_empty(&self) -> bool {
        self.properties.is_empty()
    }

    /// Clear all properties.
    pub fn clear(&self) {
        self.properties.clear();
    }
}

impl Default for PropertyStore {
    fn default() -> Self {
        Self::new()
    }
}

/// Property access errors.
#[derive(Debug, thiserror::Error)]
pub enum PropertyError {
    #[error("Property not found: {0}")]
    NotFound(PropertyId),

    #[error("Write access denied for property: {0}")]
    WriteAccessDenied(PropertyId),

    #[error("Invalid data type for property: {0}")]
    InvalidDataType(PropertyId),

    #[error("Value out of range for property: {0}")]
    ValueOutOfRange(PropertyId),

    #[error("Property is read-only: {0}")]
    ReadOnly(PropertyId),

    #[error("Invalid array index: {0}")]
    InvalidArrayIndex(u32),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_property_store() {
        let store = PropertyStore::new();

        store.set(PropertyId::ObjectName, BACnetValue::CharacterString("Test".to_string()));
        store.set(PropertyId::PresentValue, BACnetValue::Real(25.5));

        assert!(store.contains(PropertyId::ObjectName));
        assert_eq!(
            store.get(PropertyId::ObjectName).unwrap().as_string(),
            Some("Test")
        );
        assert_eq!(
            store.get(PropertyId::PresentValue).unwrap().as_real(),
            Some(25.5)
        );
    }

    #[test]
    fn test_status_flags() {
        let flags = StatusFlags {
            in_alarm: true,
            fault: false,
            overridden: false,
            out_of_service: true,
        };

        let bits = flags.to_bits();
        assert_eq!(bits, vec![true, false, false, true]);

        let decoded = StatusFlags::from_bits(&bits);
        assert_eq!(decoded, flags);
    }

    #[test]
    fn test_bacnet_value_conversions() {
        assert_eq!(BACnetValue::Boolean(true).as_bool(), Some(true));
        assert_eq!(BACnetValue::Unsigned(42).as_unsigned(), Some(42));
        assert_eq!(BACnetValue::Real(3.14).as_real(), Some(3.14));
        assert_eq!(
            BACnetValue::CharacterString("hello".to_string()).as_string(),
            Some("hello")
        );
    }
}

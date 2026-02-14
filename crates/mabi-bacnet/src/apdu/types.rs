//! APDU type definitions.

/// APDU type codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ApduType {
    /// BACnet-Confirmed-Request-PDU
    ConfirmedRequest = 0,
    /// BACnet-Unconfirmed-Request-PDU
    UnconfirmedRequest = 1,
    /// BACnet-SimpleACK-PDU
    SimpleAck = 2,
    /// BACnet-ComplexACK-PDU
    ComplexAck = 3,
    /// BACnet-SegmentACK-PDU
    SegmentAck = 4,
    /// BACnet-Error-PDU
    Error = 5,
    /// BACnet-Reject-PDU
    Reject = 6,
    /// BACnet-Abort-PDU
    Abort = 7,
}

impl ApduType {
    /// Create from the first nibble of the type byte.
    pub fn from_nibble(nibble: u8) -> Option<Self> {
        match nibble {
            0 => Some(Self::ConfirmedRequest),
            1 => Some(Self::UnconfirmedRequest),
            2 => Some(Self::SimpleAck),
            3 => Some(Self::ComplexAck),
            4 => Some(Self::SegmentAck),
            5 => Some(Self::Error),
            6 => Some(Self::Reject),
            7 => Some(Self::Abort),
            _ => None,
        }
    }
}

/// Confirmed service choices.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ConfirmedService {
    // Alarm and Event Services
    AcknowledgeAlarm = 0,
    ConfirmedCovNotification = 1,
    ConfirmedEventNotification = 2,
    GetAlarmSummary = 3,
    GetEnrollmentSummary = 4,
    SubscribeCov = 5,
    // File Access Services
    AtomicReadFile = 6,
    AtomicWriteFile = 7,
    // Object Access Services
    AddListElement = 8,
    RemoveListElement = 9,
    CreateObject = 10,
    DeleteObject = 11,
    ReadProperty = 12,
    ReadPropertyConditional = 13,
    ReadPropertyMultiple = 14,
    WriteProperty = 15,
    WritePropertyMultiple = 16,
    // Remote Device Management Services
    DeviceCommunicationControl = 17,
    ConfirmedPrivateTransfer = 18,
    ConfirmedTextMessage = 19,
    ReinitializeDevice = 20,
    // Virtual Terminal Services
    VtOpen = 21,
    VtClose = 22,
    VtData = 23,
    // Security Services
    Authenticate = 24,
    RequestKey = 25,
    // Other Services
    ReadRange = 26,
    LifeSafetyOperation = 27,
    SubscribeCovProperty = 28,
    GetEventInformation = 29,
    SubscribeCovPropertyMultiple = 30,
    ConfirmedCovNotificationMultiple = 31,
}

impl ConfirmedService {
    /// Create from service code.
    pub fn from_u8(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::AcknowledgeAlarm),
            1 => Some(Self::ConfirmedCovNotification),
            2 => Some(Self::ConfirmedEventNotification),
            3 => Some(Self::GetAlarmSummary),
            4 => Some(Self::GetEnrollmentSummary),
            5 => Some(Self::SubscribeCov),
            6 => Some(Self::AtomicReadFile),
            7 => Some(Self::AtomicWriteFile),
            8 => Some(Self::AddListElement),
            9 => Some(Self::RemoveListElement),
            10 => Some(Self::CreateObject),
            11 => Some(Self::DeleteObject),
            12 => Some(Self::ReadProperty),
            13 => Some(Self::ReadPropertyConditional),
            14 => Some(Self::ReadPropertyMultiple),
            15 => Some(Self::WriteProperty),
            16 => Some(Self::WritePropertyMultiple),
            17 => Some(Self::DeviceCommunicationControl),
            18 => Some(Self::ConfirmedPrivateTransfer),
            19 => Some(Self::ConfirmedTextMessage),
            20 => Some(Self::ReinitializeDevice),
            21 => Some(Self::VtOpen),
            22 => Some(Self::VtClose),
            23 => Some(Self::VtData),
            24 => Some(Self::Authenticate),
            25 => Some(Self::RequestKey),
            26 => Some(Self::ReadRange),
            27 => Some(Self::LifeSafetyOperation),
            28 => Some(Self::SubscribeCovProperty),
            29 => Some(Self::GetEventInformation),
            _ => None,
        }
    }

    /// Get the service name.
    pub fn name(&self) -> &'static str {
        match self {
            Self::AcknowledgeAlarm => "AcknowledgeAlarm",
            Self::ConfirmedCovNotification => "ConfirmedCOVNotification",
            Self::ConfirmedEventNotification => "ConfirmedEventNotification",
            Self::GetAlarmSummary => "GetAlarmSummary",
            Self::GetEnrollmentSummary => "GetEnrollmentSummary",
            Self::SubscribeCov => "SubscribeCOV",
            Self::AtomicReadFile => "AtomicReadFile",
            Self::AtomicWriteFile => "AtomicWriteFile",
            Self::AddListElement => "AddListElement",
            Self::RemoveListElement => "RemoveListElement",
            Self::CreateObject => "CreateObject",
            Self::DeleteObject => "DeleteObject",
            Self::ReadProperty => "ReadProperty",
            Self::ReadPropertyConditional => "ReadPropertyConditional",
            Self::ReadPropertyMultiple => "ReadPropertyMultiple",
            Self::WriteProperty => "WriteProperty",
            Self::WritePropertyMultiple => "WritePropertyMultiple",
            Self::DeviceCommunicationControl => "DeviceCommunicationControl",
            Self::ConfirmedPrivateTransfer => "ConfirmedPrivateTransfer",
            Self::ConfirmedTextMessage => "ConfirmedTextMessage",
            Self::ReinitializeDevice => "ReinitializeDevice",
            Self::VtOpen => "VT-Open",
            Self::VtClose => "VT-Close",
            Self::VtData => "VT-Data",
            Self::Authenticate => "Authenticate",
            Self::RequestKey => "RequestKey",
            Self::ReadRange => "ReadRange",
            Self::LifeSafetyOperation => "LifeSafetyOperation",
            Self::SubscribeCovProperty => "SubscribeCOVProperty",
            Self::GetEventInformation => "GetEventInformation",
            Self::SubscribeCovPropertyMultiple => "SubscribeCOVPropertyMultiple",
            Self::ConfirmedCovNotificationMultiple => "ConfirmedCOVNotificationMultiple",
        }
    }
}

/// Unconfirmed service choices.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum UnconfirmedService {
    IAm = 0,
    IHave = 1,
    UnconfirmedCovNotification = 2,
    UnconfirmedEventNotification = 3,
    UnconfirmedPrivateTransfer = 4,
    UnconfirmedTextMessage = 5,
    TimeSynchronization = 6,
    WhoHas = 7,
    WhoIs = 8,
    UtcTimeSynchronization = 9,
    WriteGroup = 10,
    UnconfirmedCovNotificationMultiple = 11,
}

impl UnconfirmedService {
    /// Create from service code.
    pub fn from_u8(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::IAm),
            1 => Some(Self::IHave),
            2 => Some(Self::UnconfirmedCovNotification),
            3 => Some(Self::UnconfirmedEventNotification),
            4 => Some(Self::UnconfirmedPrivateTransfer),
            5 => Some(Self::UnconfirmedTextMessage),
            6 => Some(Self::TimeSynchronization),
            7 => Some(Self::WhoHas),
            8 => Some(Self::WhoIs),
            9 => Some(Self::UtcTimeSynchronization),
            10 => Some(Self::WriteGroup),
            11 => Some(Self::UnconfirmedCovNotificationMultiple),
            _ => None,
        }
    }

    /// Get the service name.
    pub fn name(&self) -> &'static str {
        match self {
            Self::IAm => "I-Am",
            Self::IHave => "I-Have",
            Self::UnconfirmedCovNotification => "UnconfirmedCOVNotification",
            Self::UnconfirmedEventNotification => "UnconfirmedEventNotification",
            Self::UnconfirmedPrivateTransfer => "UnconfirmedPrivateTransfer",
            Self::UnconfirmedTextMessage => "UnconfirmedTextMessage",
            Self::TimeSynchronization => "TimeSynchronization",
            Self::WhoHas => "Who-Has",
            Self::WhoIs => "Who-Is",
            Self::UtcTimeSynchronization => "UTCTimeSynchronization",
            Self::WriteGroup => "WriteGroup",
            Self::UnconfirmedCovNotificationMultiple => "UnconfirmedCOVNotificationMultiple",
        }
    }
}

/// BACnet error class.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum ErrorClass {
    Device = 0,
    Object = 1,
    Property = 2,
    Resources = 3,
    Security = 4,
    Services = 5,
    Vt = 6,
    Communication = 7,
}

impl ErrorClass {
    pub fn from_u32(value: u32) -> Option<Self> {
        match value {
            0 => Some(Self::Device),
            1 => Some(Self::Object),
            2 => Some(Self::Property),
            3 => Some(Self::Resources),
            4 => Some(Self::Security),
            5 => Some(Self::Services),
            6 => Some(Self::Vt),
            7 => Some(Self::Communication),
            _ => None,
        }
    }
}

/// BACnet error code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum ErrorCode {
    // General errors
    Other = 0,
    AuthenticationFailed = 1,
    ConfigurationInProgress = 2,
    DeviceBusy = 3,
    DynamicCreationNotSupported = 4,
    FileAccessDenied = 5,
    // Object errors
    InconsistentParameters = 7,
    InconsistentSelectionCriterion = 8,
    InvalidDataType = 9,
    InvalidFileAccessMethod = 10,
    InvalidFileStartPosition = 11,
    InvalidParameterDataType = 13,
    InvalidTimeStamp = 14,
    MissingRequiredParameter = 16,
    NoObjectsOfSpecifiedType = 17,
    NoSpaceForObject = 18,
    NoSpaceToAddListElement = 19,
    NoSpaceToWriteProperty = 20,
    NoVtSessionsAvailable = 21,
    PropertyIsNotAList = 22,
    ObjectDeletionNotPermitted = 23,
    ObjectIdentifierAlreadyExists = 24,
    OperationalProblem = 25,
    PasswordFailure = 26,
    ReadAccessDenied = 27,
    ServiceRequestDenied = 29,
    Timeout = 30,
    UnknownObject = 31,
    UnknownProperty = 32,
    UnknownVtClass = 34,
    UnknownVtSession = 35,
    UnsupportedObjectType = 36,
    ValueOutOfRange = 37,
    VtSessionAlreadyClosed = 38,
    VtSessionTerminationFailure = 39,
    WriteAccessDenied = 40,
    CharacterSetNotSupported = 41,
    InvalidArrayIndex = 42,
    CovSubscriptionFailed = 43,
    NotCovProperty = 44,
    OptionalFunctionalityNotSupported = 45,
    InvalidConfigurationData = 46,
    DatatypeNotSupported = 47,
    DuplicateName = 48,
    DuplicateObjectId = 49,
    PropertyIsNotAnArray = 50,
    AbortBufferOverflow = 51,
    AbortInvalidApduInThisState = 52,
    AbortPreemptedByHigherPriorityTask = 53,
    AbortSegmentationNotSupported = 54,
    AbortProprietary = 55,
    AbortOther = 56,
    InvalidTag = 57,
    NetworkDown = 58,
    RejectBufferOverflow = 59,
    RejectInconsistentParameters = 60,
    RejectInvalidParameterDataType = 61,
    RejectInvalidTag = 62,
    RejectMissingRequiredParameter = 63,
    RejectParameterOutOfRange = 64,
    RejectTooManyArguments = 65,
    RejectUndefinedEnumeration = 66,
    RejectUnrecognizedService = 67,
    RejectProprietary = 68,
    RejectOther = 69,
    UnknownDevice = 70,
    UnknownRoute = 71,
    ValueNotInitialized = 72,
    InvalidEventState = 73,
    NoAlarmConfigured = 74,
    LogBufferFull = 75,
    LoggedValuePurged = 76,
    NoPropertySpecified = 77,
    NotConfiguredForTriggeredLogging = 78,
    UnknownSubscription = 79,
    ParameterOutOfRange = 80,
    ListElementNotFound = 81,
    Busy = 82,
    CommunicationDisabled = 83,
}

impl ErrorCode {
    pub fn from_u32(value: u32) -> Option<Self> {
        match value {
            0 => Some(Self::Other),
            9 => Some(Self::InvalidDataType),
            27 => Some(Self::ReadAccessDenied),
            31 => Some(Self::UnknownObject),
            32 => Some(Self::UnknownProperty),
            37 => Some(Self::ValueOutOfRange),
            40 => Some(Self::WriteAccessDenied),
            42 => Some(Self::InvalidArrayIndex),
            43 => Some(Self::CovSubscriptionFailed),
            44 => Some(Self::NotCovProperty),
            47 => Some(Self::DatatypeNotSupported),
            70 => Some(Self::UnknownDevice),
            80 => Some(Self::ParameterOutOfRange),
            _ => None,
        }
    }
}

/// Reject reason codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum RejectReason {
    Other = 0,
    BufferOverflow = 1,
    InconsistentParameters = 2,
    InvalidParameterDataType = 3,
    InvalidTag = 4,
    MissingRequiredParameter = 5,
    ParameterOutOfRange = 6,
    TooManyArguments = 7,
    UndefinedEnumeration = 8,
    UnrecognizedService = 9,
}

impl RejectReason {
    pub fn from_u8(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::Other),
            1 => Some(Self::BufferOverflow),
            2 => Some(Self::InconsistentParameters),
            3 => Some(Self::InvalidParameterDataType),
            4 => Some(Self::InvalidTag),
            5 => Some(Self::MissingRequiredParameter),
            6 => Some(Self::ParameterOutOfRange),
            7 => Some(Self::TooManyArguments),
            8 => Some(Self::UndefinedEnumeration),
            9 => Some(Self::UnrecognizedService),
            _ => None,
        }
    }
}

/// Abort reason codes per ASHRAE 135 Clause 21.7.
///
/// The Abort PDU is used to terminate a transaction when a device
/// cannot process a request. Unlike Reject (which indicates a protocol
/// violation in the request), Abort indicates a server-side issue
/// (resource exhaustion, segmentation failure, internal error).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum AbortReason {
    /// Abort for a reason other than those listed.
    Other = 0,
    /// The buffer used to assemble segmented messages is too small.
    BufferOverflow = 1,
    /// The PDU received is not valid for the current state of the transaction.
    InvalidApduInThisState = 2,
    /// The request was preempted by a higher priority task.
    PreemptedByHigherPriorityTask = 3,
    /// The device does not support segmentation.
    SegmentationNotSupported = 4,
    /// A security-related error occurred.
    SecurityError = 5,
    /// Insufficient security for the requested operation.
    InsufficientSecurity = 6,
    /// Window size out of range (segmentation window negotiation failure).
    WindowSizeOutOfRange = 7,
    /// The APDU is too long for the device to process.
    ApplicationExceededReplyTime = 8,
    /// Out of resources (e.g., memory, file handles).
    OutOfResources = 9,
    /// The TSM (Transaction State Machine) timed out.
    TsmTimeout = 10,
    /// APDU too long for the device to accept.
    ApduTooLong = 11,
}

impl AbortReason {
    pub fn from_u8(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::Other),
            1 => Some(Self::BufferOverflow),
            2 => Some(Self::InvalidApduInThisState),
            3 => Some(Self::PreemptedByHigherPriorityTask),
            4 => Some(Self::SegmentationNotSupported),
            5 => Some(Self::SecurityError),
            6 => Some(Self::InsufficientSecurity),
            7 => Some(Self::WindowSizeOutOfRange),
            8 => Some(Self::ApplicationExceededReplyTime),
            9 => Some(Self::OutOfResources),
            10 => Some(Self::TsmTimeout),
            11 => Some(Self::ApduTooLong),
            _ => None,
        }
    }

    /// Get the human-readable name.
    pub fn name(&self) -> &'static str {
        match self {
            Self::Other => "other",
            Self::BufferOverflow => "buffer-overflow",
            Self::InvalidApduInThisState => "invalid-apdu-in-this-state",
            Self::PreemptedByHigherPriorityTask => "preempted-by-higher-priority-task",
            Self::SegmentationNotSupported => "segmentation-not-supported",
            Self::SecurityError => "security-error",
            Self::InsufficientSecurity => "insufficient-security",
            Self::WindowSizeOutOfRange => "window-size-out-of-range",
            Self::ApplicationExceededReplyTime => "application-exceeded-reply-time",
            Self::OutOfResources => "out-of-resources",
            Self::TsmTimeout => "tsm-timeout",
            Self::ApduTooLong => "apdu-too-long",
        }
    }
}

/// APDU errors.
#[derive(Debug, thiserror::Error)]
pub enum ApduError {
    #[error("APDU too short")]
    TooShort,

    #[error("Invalid APDU type: {0}")]
    InvalidType(u8),

    #[error("Unknown service: {0}")]
    UnknownService(u8),

    #[error("Invalid tag: {0}")]
    InvalidTag(u8),

    #[error("Decode error: {0}")]
    DecodeError(String),

    #[error("Encode error: {0}")]
    EncodeError(String),
}

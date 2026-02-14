//! BACnet EventEnrollment and NotificationClass object implementations.
//!
//! Per ASHRAE 135, Clause 12.12 (EventEnrollment) and Clause 12.16 (NotificationClass).
//!
//! ## EventEnrollment
//!
//! Represents an event monitoring configuration that watches a property of
//! another object and generates notifications when alarm conditions occur.
//! Supports event types: change-of-state, change-of-value, out-of-range,
//! command-failure, floating-limit, and others.
//!
//! ## NotificationClass
//!
//! Defines notification distribution rules: which recipients receive which
//! alarm transitions, priority mapping, and acknowledgment requirements.

use parking_lot::RwLock;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};

use super::property::{
    BACnetValue, EventState, PropertyError, PropertyId, PropertyStore, Reliability, StatusFlags,
};
use super::traits::BACnetObject;
use super::trend_log::DeviceObjectPropertyReference;
use super::types::{ObjectId, ObjectType};

// ============================================================================
// Event types & transitions
// ============================================================================

/// BACnet event type per ASHRAE 135, Clause 13.3.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum EventType {
    ChangeOfBitstring = 0,
    ChangeOfState = 1,
    ChangeOfValue = 2,
    CommandFailure = 3,
    FloatingLimit = 4,
    OutOfRange = 5,
    ChangeOfLifeSafety = 8,
    Extended = 9,
    BufferReady = 10,
    UnsignedRange = 11,
    AccessEvent = 13,
    DoubleOutOfRange = 14,
    SignedOutOfRange = 15,
    UnsignedOutOfRange = 16,
    ChangeOfCharacterstring = 17,
    ChangeOfStatusFlags = 18,
    ChangeOfReliability = 19,
    None = 20,
    ChangeOfDiscreteValue = 21,
    ChangeOfTimer = 22,
}

impl EventType {
    pub fn from_u32(v: u32) -> Option<Self> {
        match v {
            0 => Some(Self::ChangeOfBitstring),
            1 => Some(Self::ChangeOfState),
            2 => Some(Self::ChangeOfValue),
            3 => Some(Self::CommandFailure),
            4 => Some(Self::FloatingLimit),
            5 => Some(Self::OutOfRange),
            8 => Some(Self::ChangeOfLifeSafety),
            9 => Some(Self::Extended),
            10 => Some(Self::BufferReady),
            11 => Some(Self::UnsignedRange),
            13 => Some(Self::AccessEvent),
            14 => Some(Self::DoubleOutOfRange),
            15 => Some(Self::SignedOutOfRange),
            16 => Some(Self::UnsignedOutOfRange),
            17 => Some(Self::ChangeOfCharacterstring),
            18 => Some(Self::ChangeOfStatusFlags),
            19 => Some(Self::ChangeOfReliability),
            20 => Some(Self::None),
            21 => Some(Self::ChangeOfDiscreteValue),
            22 => Some(Self::ChangeOfTimer),
            _ => Option::None,
        }
    }
}

/// Event transition flags per ASHRAE 135.
/// Tracks which transitions have occurred and been acknowledged.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct EventTransitionBits {
    pub to_offnormal: bool,
    pub to_fault: bool,
    pub to_normal: bool,
}

impl EventTransitionBits {
    pub fn all() -> Self {
        Self {
            to_offnormal: true,
            to_fault: true,
            to_normal: true,
        }
    }

    pub fn none() -> Self {
        Self::default()
    }

    pub fn to_bits(&self) -> Vec<bool> {
        vec![self.to_offnormal, self.to_fault, self.to_normal]
    }

    pub fn from_bits(bits: &[bool]) -> Self {
        Self {
            to_offnormal: bits.first().copied().unwrap_or(false),
            to_fault: bits.get(1).copied().unwrap_or(false),
            to_normal: bits.get(2).copied().unwrap_or(false),
        }
    }
}

/// Notify type per ASHRAE 135.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum NotifyType {
    Alarm = 0,
    Event = 1,
    AckNotification = 2,
}

impl NotifyType {
    pub fn from_u32(v: u32) -> Option<Self> {
        match v {
            0 => Some(Self::Alarm),
            1 => Some(Self::Event),
            2 => Some(Self::AckNotification),
            _ => Option::None,
        }
    }
}

/// BACnet event notification per ASHRAE 135, Clause 13.8.
#[derive(Debug, Clone)]
pub struct EventNotification {
    /// Destination address.
    pub destination: SocketAddr,
    /// Process identifier at the recipient.
    pub process_id: u32,
    /// Initiating device identifier.
    pub initiating_device: ObjectId,
    /// Event-generating object identifier.
    pub event_object: ObjectId,
    /// Time stamp of the event.
    pub time_stamp: u64,
    /// Notification class instance.
    pub notification_class: u32,
    /// Priority (0-255, lower = higher priority).
    pub priority: u8,
    /// Event type.
    pub event_type: EventType,
    /// Message text (optional).
    pub message_text: Option<String>,
    /// Notify type (alarm, event, or ack).
    pub notify_type: NotifyType,
    /// Whether acknowledgment is required.
    pub ack_required: bool,
    /// Previous event state (from-state).
    pub from_state: EventState,
    /// Current event state (to-state).
    pub to_state: EventState,
    /// Whether this requires confirmed notification.
    pub confirmed: bool,
}

// ============================================================================
// EventEnrollment object
// ============================================================================

/// BACnet EventEnrollment object per ASHRAE 135, Clause 12.12.
///
/// Monitors a property of a referenced object and generates alarm
/// notifications based on configured conditions.
pub struct EventEnrollment {
    id: ObjectId,
    name: String,
    description: String,
    properties: PropertyStore,

    /// Event type for this enrollment.
    event_type: EventType,
    /// Current event state.
    event_state: RwLock<EventState>,
    /// Reference to the monitored object/property.
    object_property_reference: RwLock<DeviceObjectPropertyReference>,
    /// Notification class for distributing notifications.
    notification_class: RwLock<u32>,
    /// Event enable bits (which transitions generate notifications).
    event_enable: RwLock<EventTransitionBits>,
    /// Acked transitions (which transitions have been acknowledged).
    acked_transitions: RwLock<EventTransitionBits>,
    /// Notify type (alarm vs event).
    notify_type: RwLock<NotifyType>,
    /// Event timestamps [to-offnormal, to-fault, to-normal].
    event_timestamps: RwLock<[u64; 3]>,
    /// Time delay for state transition (seconds).
    time_delay: RwLock<u32>,

    // Out-of-range parameters (for OutOfRange event type)
    /// High limit threshold.
    high_limit: RwLock<f32>,
    /// Low limit threshold.
    low_limit: RwLock<f32>,
    /// Deadband (hysteresis).
    deadband: RwLock<f32>,
    /// Limit enable bits.
    limit_enable: RwLock<EventTransitionBits>,

    out_of_service: AtomicBool,
}

impl EventEnrollment {
    /// Create a new EventEnrollment.
    pub fn new(instance: u32, name: impl Into<String>, event_type: EventType) -> Self {
        let id = ObjectId::new(ObjectType::EventEnrollment, instance);
        let properties = PropertyStore::new();

        properties.set(
            PropertyId::Reliability,
            BACnetValue::Enumerated(Reliability::NoFaultDetected as u32),
        );

        Self {
            id,
            name: name.into(),
            description: String::new(),
            properties,
            event_type,
            event_state: RwLock::new(EventState::Normal),
            object_property_reference: RwLock::new(DeviceObjectPropertyReference {
                object_id: ObjectId::new(ObjectType::AnalogInput, 0),
                property_id: PropertyId::PresentValue,
                array_index: None,
                device_id: None,
            }),
            notification_class: RwLock::new(0),
            event_enable: RwLock::new(EventTransitionBits::all()),
            acked_transitions: RwLock::new(EventTransitionBits::all()),
            notify_type: RwLock::new(NotifyType::Alarm),
            event_timestamps: RwLock::new([0; 3]),
            time_delay: RwLock::new(0),
            high_limit: RwLock::new(100.0),
            low_limit: RwLock::new(0.0),
            deadband: RwLock::new(1.0),
            limit_enable: RwLock::new(EventTransitionBits {
                to_offnormal: true,
                to_fault: false,
                to_normal: true,
            }),
            out_of_service: AtomicBool::new(false),
        }
    }

    /// Builder: set description.
    pub fn with_description(mut self, desc: impl Into<String>) -> Self {
        self.description = desc.into();
        self
    }

    /// Builder: set monitored object property reference.
    pub fn with_object_property_reference(self, opr: DeviceObjectPropertyReference) -> Self {
        *self.object_property_reference.write() = opr;
        self
    }

    /// Builder: set notification class.
    pub fn with_notification_class(self, nc: u32) -> Self {
        *self.notification_class.write() = nc;
        self
    }

    /// Builder: set time delay.
    pub fn with_time_delay(self, delay: u32) -> Self {
        *self.time_delay.write() = delay;
        self
    }

    /// Builder: set high limit.
    pub fn with_high_limit(self, limit: f32) -> Self {
        *self.high_limit.write() = limit;
        self
    }

    /// Builder: set low limit.
    pub fn with_low_limit(self, limit: f32) -> Self {
        *self.low_limit.write() = limit;
        self
    }

    /// Builder: set deadband.
    pub fn with_deadband(self, db: f32) -> Self {
        *self.deadband.write() = db;
        self
    }

    /// Builder: set notify type.
    pub fn with_notify_type(self, nt: NotifyType) -> Self {
        *self.notify_type.write() = nt;
        self
    }

    /// Get the current event state.
    pub fn event_state(&self) -> EventState {
        *self.event_state.read()
    }

    /// Evaluate a monitored value and return a new EventState if a transition occurs.
    ///
    /// For `OutOfRange` event type, checks the value against high/low limits with deadband.
    /// Returns `Some(new_state)` if a transition should occur, `None` otherwise.
    pub fn evaluate(&self, value: f32) -> Option<EventState> {
        let current = *self.event_state.read();
        let high = *self.high_limit.read();
        let low = *self.low_limit.read();
        let db = *self.deadband.read();

        match self.event_type {
            EventType::OutOfRange => {
                match current {
                    EventState::Normal => {
                        if value > high {
                            Some(EventState::HighLimit)
                        } else if value < low {
                            Some(EventState::LowLimit)
                        } else {
                            None
                        }
                    }
                    EventState::HighLimit => {
                        if value < (high - db) && value >= low {
                            Some(EventState::Normal)
                        } else if value < low {
                            Some(EventState::LowLimit)
                        } else {
                            None
                        }
                    }
                    EventState::LowLimit => {
                        if value > (low + db) && value <= high {
                            Some(EventState::Normal)
                        } else if value > high {
                            Some(EventState::HighLimit)
                        } else {
                            None
                        }
                    }
                    _ => None,
                }
            }
            _ => {
                // For other event types, a simple offnormal/normal toggle
                // based on whether the value is within limits.
                match current {
                    EventState::Normal => {
                        if value > high || value < low {
                            Some(EventState::Offnormal)
                        } else {
                            None
                        }
                    }
                    EventState::Offnormal => {
                        if value >= low && value <= high {
                            Some(EventState::Normal)
                        } else {
                            None
                        }
                    }
                    _ => None,
                }
            }
        }
    }

    /// Apply a state transition.
    ///
    /// Updates the event state, timestamps, and acked_transitions.
    /// Returns `true` if the transition should generate a notification.
    pub fn transition_to(&self, new_state: EventState, timestamp: u64) -> bool {
        let old_state = {
            let mut state = self.event_state.write();
            let old = *state;
            *state = new_state;
            old
        };

        if old_state == new_state {
            return false;
        }

        // Update timestamps
        let mut timestamps = self.event_timestamps.write();
        match new_state {
            EventState::Normal => timestamps[2] = timestamp,
            EventState::Fault => timestamps[1] = timestamp,
            _ => timestamps[0] = timestamp, // offnormal, high/low limit
        }

        // Clear acked bit for this transition
        let mut acked = self.acked_transitions.write();
        match new_state {
            EventState::Normal => acked.to_normal = false,
            EventState::Fault => acked.to_fault = false,
            _ => acked.to_offnormal = false,
        }

        // Check if notifications are enabled for this transition
        let enable = self.event_enable.read();
        match new_state {
            EventState::Normal => enable.to_normal,
            EventState::Fault => enable.to_fault,
            _ => enable.to_offnormal,
        }
    }

    /// Acknowledge a transition.
    pub fn acknowledge(&self, transition: &str) -> Result<(), PropertyError> {
        let mut acked = self.acked_transitions.write();
        match transition {
            "to-offnormal" => {
                acked.to_offnormal = true;
                Ok(())
            }
            "to-fault" => {
                acked.to_fault = true;
                Ok(())
            }
            "to-normal" => {
                acked.to_normal = true;
                Ok(())
            }
            _ => Err(PropertyError::ValueOutOfRange(PropertyId::AckedTransitions)),
        }
    }

    /// Get the monitored object property reference.
    pub fn object_property_reference(&self) -> DeviceObjectPropertyReference {
        self.object_property_reference.read().clone()
    }

    /// Get notification class instance.
    pub fn notification_class(&self) -> u32 {
        *self.notification_class.read()
    }

    /// Get event type.
    pub fn event_type(&self) -> EventType {
        self.event_type
    }

    /// Check if ack is required.
    pub fn ack_required(&self) -> EventTransitionBits {
        // For simplicity, ack is required for all enabled transitions
        *self.event_enable.read()
    }

    /// Get the acked transitions.
    pub fn acked_transitions(&self) -> EventTransitionBits {
        *self.acked_transitions.read()
    }
}

impl BACnetObject for EventEnrollment {
    fn object_identifier(&self) -> ObjectId {
        self.id
    }

    fn object_name(&self) -> &str {
        &self.name
    }

    fn description(&self) -> Option<&str> {
        if self.description.is_empty() {
            None
        } else {
            Some(&self.description)
        }
    }

    fn read_property(&self, property_id: PropertyId) -> Result<BACnetValue, PropertyError> {
        match property_id {
            PropertyId::ObjectIdentifier => Ok(BACnetValue::ObjectIdentifier(self.id)),
            PropertyId::ObjectName => Ok(BACnetValue::CharacterString(self.name.clone())),
            PropertyId::ObjectType => {
                Ok(BACnetValue::Enumerated(ObjectType::EventEnrollment as u32))
            }
            PropertyId::Description => Ok(BACnetValue::CharacterString(self.description.clone())),
            PropertyId::EventType => Ok(BACnetValue::Enumerated(self.event_type as u32)),
            PropertyId::EventState => {
                Ok(BACnetValue::Enumerated(*self.event_state.read() as u32))
            }
            PropertyId::StatusFlags => Ok(BACnetValue::BitString(self.status_flags().to_bits())),
            PropertyId::EventEnable => {
                Ok(BACnetValue::BitString(self.event_enable.read().to_bits()))
            }
            PropertyId::AckedTransitions => {
                Ok(BACnetValue::BitString(self.acked_transitions.read().to_bits()))
            }
            PropertyId::NotifyType => {
                Ok(BACnetValue::Enumerated(*self.notify_type.read() as u32))
            }
            PropertyId::NotificationClass => {
                Ok(BACnetValue::Unsigned(*self.notification_class.read()))
            }
            PropertyId::EventTimeStamps => {
                let ts = self.event_timestamps.read();
                Ok(BACnetValue::Array(vec![
                    BACnetValue::Unsigned64(ts[0]),
                    BACnetValue::Unsigned64(ts[1]),
                    BACnetValue::Unsigned64(ts[2]),
                ]))
            }
            PropertyId::TimeDelay => Ok(BACnetValue::Unsigned(*self.time_delay.read())),
            PropertyId::HighLimit => Ok(BACnetValue::Real(*self.high_limit.read())),
            PropertyId::LowLimit => Ok(BACnetValue::Real(*self.low_limit.read())),
            PropertyId::Deadband => Ok(BACnetValue::Real(*self.deadband.read())),
            PropertyId::LimitEnable => {
                let le = self.limit_enable.read();
                Ok(BACnetValue::BitString(vec![le.to_offnormal, le.to_normal]))
            }
            PropertyId::Reliability => self
                .properties
                .get(PropertyId::Reliability)
                .ok_or(PropertyError::NotFound(PropertyId::Reliability)),
            PropertyId::OutOfService => {
                Ok(BACnetValue::Boolean(self.out_of_service.load(Ordering::Acquire)))
            }
            _ => self
                .properties
                .get(property_id)
                .ok_or(PropertyError::NotFound(property_id)),
        }
    }

    fn write_property(
        &self,
        property_id: PropertyId,
        value: BACnetValue,
    ) -> Result<(), PropertyError> {
        match property_id {
            PropertyId::ObjectIdentifier
            | PropertyId::ObjectType
            | PropertyId::EventState
            | PropertyId::EventTimeStamps
            | PropertyId::AckedTransitions => Err(PropertyError::ReadOnly(property_id)),

            PropertyId::EventEnable => {
                if let BACnetValue::BitString(bits) = &value {
                    *self.event_enable.write() = EventTransitionBits::from_bits(bits);
                    Ok(())
                } else {
                    Err(PropertyError::InvalidDataType(property_id))
                }
            }
            PropertyId::NotifyType => {
                if let Some(v) = value.as_unsigned() {
                    NotifyType::from_u32(v)
                        .ok_or(PropertyError::ValueOutOfRange(property_id))?;
                    *self.notify_type.write() = NotifyType::from_u32(v).unwrap();
                    Ok(())
                } else {
                    Err(PropertyError::InvalidDataType(property_id))
                }
            }
            PropertyId::NotificationClass => {
                if let Some(v) = value.as_unsigned() {
                    *self.notification_class.write() = v;
                    Ok(())
                } else {
                    Err(PropertyError::InvalidDataType(property_id))
                }
            }
            PropertyId::TimeDelay => {
                if let Some(v) = value.as_unsigned() {
                    *self.time_delay.write() = v;
                    Ok(())
                } else {
                    Err(PropertyError::InvalidDataType(property_id))
                }
            }
            PropertyId::HighLimit => {
                if let Some(v) = value.as_real() {
                    *self.high_limit.write() = v;
                    Ok(())
                } else {
                    Err(PropertyError::InvalidDataType(property_id))
                }
            }
            PropertyId::LowLimit => {
                if let Some(v) = value.as_real() {
                    *self.low_limit.write() = v;
                    Ok(())
                } else {
                    Err(PropertyError::InvalidDataType(property_id))
                }
            }
            PropertyId::Deadband => {
                if let Some(v) = value.as_real() {
                    *self.deadband.write() = v;
                    Ok(())
                } else {
                    Err(PropertyError::InvalidDataType(property_id))
                }
            }
            PropertyId::OutOfService => {
                if let Some(v) = value.as_bool() {
                    self.out_of_service.store(v, Ordering::Release);
                    Ok(())
                } else {
                    Err(PropertyError::InvalidDataType(property_id))
                }
            }
            _ => {
                self.properties.set(property_id, value);
                Ok(())
            }
        }
    }

    fn list_properties(&self) -> Vec<PropertyId> {
        let mut props = vec![
            PropertyId::ObjectIdentifier,
            PropertyId::ObjectName,
            PropertyId::ObjectType,
            PropertyId::Description,
            PropertyId::EventType,
            PropertyId::EventState,
            PropertyId::StatusFlags,
            PropertyId::EventEnable,
            PropertyId::AckedTransitions,
            PropertyId::NotifyType,
            PropertyId::NotificationClass,
            PropertyId::EventTimeStamps,
            PropertyId::TimeDelay,
            PropertyId::Reliability,
            PropertyId::OutOfService,
        ];
        // Add limit-related properties for applicable event types
        if matches!(
            self.event_type,
            EventType::OutOfRange
                | EventType::FloatingLimit
                | EventType::DoubleOutOfRange
                | EventType::SignedOutOfRange
                | EventType::UnsignedOutOfRange
        ) {
            props.extend_from_slice(&[
                PropertyId::HighLimit,
                PropertyId::LowLimit,
                PropertyId::Deadband,
                PropertyId::LimitEnable,
            ]);
        }
        props
    }

    fn status_flags(&self) -> StatusFlags {
        let state = *self.event_state.read();
        StatusFlags {
            in_alarm: !matches!(state, EventState::Normal),
            fault: matches!(state, EventState::Fault),
            overridden: false,
            out_of_service: self.out_of_service.load(Ordering::Acquire),
        }
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

// ============================================================================
// NotificationClass object
// ============================================================================

/// Recipient entry for a NotificationClass.
#[derive(Debug, Clone)]
pub struct NotificationRecipient {
    /// Valid days (bit 0=Monday..bit 6=Sunday).
    pub valid_days: u8,
    /// Start time (seconds from midnight).
    pub from_time: u32,
    /// End time (seconds from midnight).
    pub to_time: u32,
    /// Destination address.
    pub destination: SocketAddr,
    /// Process identifier at the recipient.
    pub process_id: u32,
    /// Whether to confirm notifications.
    pub confirmed: bool,
    /// Which transitions to forward.
    pub transitions: EventTransitionBits,
}

/// BACnet NotificationClass object per ASHRAE 135, Clause 12.16.
///
/// Defines how event notifications are distributed to recipients,
/// with priority mapping and acknowledgment requirements.
pub struct NotificationClass {
    id: ObjectId,
    name: String,
    description: String,
    properties: PropertyStore,

    /// Notification class number (usually matches instance).
    notification_class: u32,
    /// Priority mapping: [to-offnormal, to-fault, to-normal] (0-255, lower = higher).
    priority: RwLock<[u8; 3]>,
    /// Ack required for transitions.
    ack_required: RwLock<EventTransitionBits>,
    /// Recipient list.
    recipient_list: RwLock<Vec<NotificationRecipient>>,
}

impl NotificationClass {
    /// Create a new NotificationClass object.
    pub fn new(instance: u32, name: impl Into<String>) -> Self {
        let id = ObjectId::new(ObjectType::NotificationClass, instance);
        let properties = PropertyStore::new();

        Self {
            id,
            name: name.into(),
            description: String::new(),
            properties,
            notification_class: instance,
            priority: RwLock::new([0, 0, 0]), // Highest priority by default
            ack_required: RwLock::new(EventTransitionBits::all()),
            recipient_list: RwLock::new(Vec::new()),
        }
    }

    /// Builder: set description.
    pub fn with_description(mut self, desc: impl Into<String>) -> Self {
        self.description = desc.into();
        self
    }

    /// Builder: set priority mapping.
    pub fn with_priority(self, to_offnormal: u8, to_fault: u8, to_normal: u8) -> Self {
        *self.priority.write() = [to_offnormal, to_fault, to_normal];
        self
    }

    /// Builder: set ack required bits.
    pub fn with_ack_required(self, ack: EventTransitionBits) -> Self {
        *self.ack_required.write() = ack;
        self
    }

    /// Add a recipient.
    pub fn add_recipient(&self, recipient: NotificationRecipient) {
        self.recipient_list.write().push(recipient);
    }

    /// Get all recipients.
    pub fn recipients(&self) -> Vec<NotificationRecipient> {
        self.recipient_list.read().clone()
    }

    /// Get priority for a given transition.
    pub fn priority_for_transition(&self, to_state: EventState) -> u8 {
        let p = self.priority.read();
        match to_state {
            EventState::Normal => p[2],
            EventState::Fault => p[1],
            _ => p[0], // offnormal, high/low limit
        }
    }

    /// Check if acknowledgment is required for a transition.
    pub fn ack_required_for_transition(&self, to_state: EventState) -> bool {
        let ack = self.ack_required.read();
        match to_state {
            EventState::Normal => ack.to_normal,
            EventState::Fault => ack.to_fault,
            _ => ack.to_offnormal,
        }
    }

    /// Get the notification class number.
    pub fn notification_class_number(&self) -> u32 {
        self.notification_class
    }
}

impl BACnetObject for NotificationClass {
    fn object_identifier(&self) -> ObjectId {
        self.id
    }

    fn object_name(&self) -> &str {
        &self.name
    }

    fn description(&self) -> Option<&str> {
        if self.description.is_empty() {
            None
        } else {
            Some(&self.description)
        }
    }

    fn read_property(&self, property_id: PropertyId) -> Result<BACnetValue, PropertyError> {
        match property_id {
            PropertyId::ObjectIdentifier => Ok(BACnetValue::ObjectIdentifier(self.id)),
            PropertyId::ObjectName => Ok(BACnetValue::CharacterString(self.name.clone())),
            PropertyId::ObjectType => {
                Ok(BACnetValue::Enumerated(ObjectType::NotificationClass as u32))
            }
            PropertyId::Description => Ok(BACnetValue::CharacterString(self.description.clone())),
            PropertyId::NotificationClass => {
                Ok(BACnetValue::Unsigned(self.notification_class))
            }
            PropertyId::Priority => {
                let p = self.priority.read();
                Ok(BACnetValue::Array(vec![
                    BACnetValue::Unsigned(p[0] as u32),
                    BACnetValue::Unsigned(p[1] as u32),
                    BACnetValue::Unsigned(p[2] as u32),
                ]))
            }
            PropertyId::AckRequired => {
                Ok(BACnetValue::BitString(self.ack_required.read().to_bits()))
            }
            _ => self
                .properties
                .get(property_id)
                .ok_or(PropertyError::NotFound(property_id)),
        }
    }

    fn write_property(
        &self,
        property_id: PropertyId,
        value: BACnetValue,
    ) -> Result<(), PropertyError> {
        match property_id {
            PropertyId::ObjectIdentifier | PropertyId::ObjectType | PropertyId::NotificationClass => {
                Err(PropertyError::ReadOnly(property_id))
            }
            PropertyId::Priority => {
                if let BACnetValue::Array(arr) = &value {
                    if arr.len() == 3 {
                        let p0 = arr[0].as_unsigned().ok_or(PropertyError::InvalidDataType(property_id))? as u8;
                        let p1 = arr[1].as_unsigned().ok_or(PropertyError::InvalidDataType(property_id))? as u8;
                        let p2 = arr[2].as_unsigned().ok_or(PropertyError::InvalidDataType(property_id))? as u8;
                        *self.priority.write() = [p0, p1, p2];
                        Ok(())
                    } else {
                        Err(PropertyError::ValueOutOfRange(property_id))
                    }
                } else {
                    Err(PropertyError::InvalidDataType(property_id))
                }
            }
            PropertyId::AckRequired => {
                if let BACnetValue::BitString(bits) = &value {
                    *self.ack_required.write() = EventTransitionBits::from_bits(bits);
                    Ok(())
                } else {
                    Err(PropertyError::InvalidDataType(property_id))
                }
            }
            _ => {
                self.properties.set(property_id, value);
                Ok(())
            }
        }
    }

    fn list_properties(&self) -> Vec<PropertyId> {
        vec![
            PropertyId::ObjectIdentifier,
            PropertyId::ObjectName,
            PropertyId::ObjectType,
            PropertyId::Description,
            PropertyId::NotificationClass,
            PropertyId::Priority,
            PropertyId::AckRequired,
        ]
    }

    fn status_flags(&self) -> StatusFlags {
        StatusFlags::default()
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_event_enrollment_creation() {
        let ee = EventEnrollment::new(1, "TestAlarm", EventType::OutOfRange)
            .with_description("Temperature alarm")
            .with_high_limit(80.0)
            .with_low_limit(20.0)
            .with_deadband(2.0);

        assert_eq!(ee.object_identifier(), ObjectId::new(ObjectType::EventEnrollment, 1));
        assert_eq!(ee.object_name(), "TestAlarm");
        assert_eq!(ee.event_type(), EventType::OutOfRange);
        assert_eq!(ee.event_state(), EventState::Normal);
    }

    #[test]
    fn test_out_of_range_high() {
        let ee = EventEnrollment::new(1, "TempAlarm", EventType::OutOfRange)
            .with_high_limit(80.0)
            .with_low_limit(20.0)
            .with_deadband(2.0);

        // Normal → HighLimit
        assert_eq!(ee.evaluate(85.0), Some(EventState::HighLimit));
        ee.transition_to(EventState::HighLimit, 1000);
        assert_eq!(ee.event_state(), EventState::HighLimit);

        // Still high, no transition
        assert_eq!(ee.evaluate(82.0), None);

        // Below high - deadband → Normal
        assert_eq!(ee.evaluate(77.0), Some(EventState::Normal));
        ee.transition_to(EventState::Normal, 2000);
        assert_eq!(ee.event_state(), EventState::Normal);
    }

    #[test]
    fn test_out_of_range_low() {
        let ee = EventEnrollment::new(1, "TempAlarm", EventType::OutOfRange)
            .with_high_limit(80.0)
            .with_low_limit(20.0)
            .with_deadband(2.0);

        // Normal → LowLimit
        assert_eq!(ee.evaluate(15.0), Some(EventState::LowLimit));
        ee.transition_to(EventState::LowLimit, 1000);

        // Above low + deadband → Normal
        assert_eq!(ee.evaluate(23.0), Some(EventState::Normal));
    }

    #[test]
    fn test_out_of_range_high_to_low() {
        let ee = EventEnrollment::new(1, "TempAlarm", EventType::OutOfRange)
            .with_high_limit(80.0)
            .with_low_limit(20.0)
            .with_deadband(2.0);

        ee.transition_to(EventState::HighLimit, 1000);
        // HighLimit → LowLimit (direct transition)
        assert_eq!(ee.evaluate(10.0), Some(EventState::LowLimit));
    }

    #[test]
    fn test_transition_generates_notification() {
        let ee = EventEnrollment::new(1, "Alarm", EventType::OutOfRange);

        // Enable all transitions
        *ee.event_enable.write() = EventTransitionBits::all();

        // Should generate notification
        assert!(ee.transition_to(EventState::HighLimit, 1000));
        // Acked transitions should be cleared for to-offnormal
        assert!(!ee.acked_transitions().to_offnormal);
        assert!(ee.acked_transitions().to_fault);
        assert!(ee.acked_transitions().to_normal);
    }

    #[test]
    fn test_transition_disabled_no_notification() {
        let ee = EventEnrollment::new(1, "Alarm", EventType::OutOfRange);

        // Disable all transitions
        *ee.event_enable.write() = EventTransitionBits::none();

        // Should NOT generate notification
        assert!(!ee.transition_to(EventState::HighLimit, 1000));
    }

    #[test]
    fn test_acknowledge() {
        let ee = EventEnrollment::new(1, "Alarm", EventType::OutOfRange);
        ee.transition_to(EventState::HighLimit, 1000);

        assert!(!ee.acked_transitions().to_offnormal);
        ee.acknowledge("to-offnormal").unwrap();
        assert!(ee.acked_transitions().to_offnormal);
    }

    #[test]
    fn test_event_timestamps() {
        let ee = EventEnrollment::new(1, "Alarm", EventType::OutOfRange);

        ee.transition_to(EventState::HighLimit, 1000);
        ee.transition_to(EventState::Normal, 2000);
        ee.transition_to(EventState::Fault, 3000);

        if let Ok(BACnetValue::Array(ts)) = ee.read_property(PropertyId::EventTimeStamps) {
            assert_eq!(ts[0], BACnetValue::Unsigned64(1000)); // to-offnormal
            assert_eq!(ts[1], BACnetValue::Unsigned64(3000)); // to-fault
            assert_eq!(ts[2], BACnetValue::Unsigned64(2000)); // to-normal
        } else {
            panic!("Expected Array for EventTimeStamps");
        }
    }

    #[test]
    fn test_event_enrollment_properties() {
        let ee = EventEnrollment::new(1, "Alarm", EventType::OutOfRange)
            .with_high_limit(90.0)
            .with_low_limit(10.0)
            .with_deadband(3.0)
            .with_notification_class(5)
            .with_time_delay(10);

        assert_eq!(
            ee.read_property(PropertyId::EventType).unwrap(),
            BACnetValue::Enumerated(EventType::OutOfRange as u32)
        );
        assert_eq!(
            ee.read_property(PropertyId::HighLimit).unwrap(),
            BACnetValue::Real(90.0)
        );
        assert_eq!(
            ee.read_property(PropertyId::LowLimit).unwrap(),
            BACnetValue::Real(10.0)
        );
        assert_eq!(
            ee.read_property(PropertyId::Deadband).unwrap(),
            BACnetValue::Real(3.0)
        );
        assert_eq!(
            ee.read_property(PropertyId::NotificationClass).unwrap(),
            BACnetValue::Unsigned(5)
        );
        assert_eq!(
            ee.read_property(PropertyId::TimeDelay).unwrap(),
            BACnetValue::Unsigned(10)
        );
    }

    #[test]
    fn test_event_enrollment_write_properties() {
        let ee = EventEnrollment::new(1, "Alarm", EventType::OutOfRange);

        ee.write_property(PropertyId::HighLimit, BACnetValue::Real(95.0)).unwrap();
        assert_eq!(
            ee.read_property(PropertyId::HighLimit).unwrap(),
            BACnetValue::Real(95.0)
        );

        ee.write_property(PropertyId::LowLimit, BACnetValue::Real(5.0)).unwrap();
        assert_eq!(
            ee.read_property(PropertyId::LowLimit).unwrap(),
            BACnetValue::Real(5.0)
        );

        // Read-only properties
        assert!(ee
            .write_property(PropertyId::EventState, BACnetValue::Enumerated(0))
            .is_err());
    }

    #[test]
    fn test_event_enrollment_status_flags() {
        let ee = EventEnrollment::new(1, "Alarm", EventType::OutOfRange);

        let flags = ee.status_flags();
        assert!(!flags.in_alarm);

        ee.transition_to(EventState::HighLimit, 1000);
        let flags = ee.status_flags();
        assert!(flags.in_alarm);
        assert!(!flags.fault);

        ee.transition_to(EventState::Fault, 2000);
        let flags = ee.status_flags();
        assert!(flags.in_alarm);
        assert!(flags.fault);
    }

    #[test]
    fn test_notification_class_creation() {
        let nc = NotificationClass::new(1, "DefaultClass")
            .with_description("Default notification class")
            .with_priority(3, 5, 10)
            .with_ack_required(EventTransitionBits::all());

        assert_eq!(nc.object_identifier(), ObjectId::new(ObjectType::NotificationClass, 1));
        assert_eq!(nc.notification_class_number(), 1);
        assert_eq!(nc.priority_for_transition(EventState::HighLimit), 3);
        assert_eq!(nc.priority_for_transition(EventState::Fault), 5);
        assert_eq!(nc.priority_for_transition(EventState::Normal), 10);
    }

    #[test]
    fn test_notification_class_recipients() {
        let nc = NotificationClass::new(1, "TestClass");
        nc.add_recipient(NotificationRecipient {
            valid_days: 0x7F, // all days
            from_time: 0,
            to_time: 86400,
            destination: "192.168.1.100:47808".parse().unwrap(),
            process_id: 1,
            confirmed: false,
            transitions: EventTransitionBits::all(),
        });

        assert_eq!(nc.recipients().len(), 1);
        assert_eq!(
            nc.recipients()[0].destination,
            "192.168.1.100:47808".parse::<SocketAddr>().unwrap()
        );
    }

    #[test]
    fn test_notification_class_properties() {
        let nc = NotificationClass::new(5, "NC5")
            .with_priority(1, 2, 3);

        assert_eq!(
            nc.read_property(PropertyId::NotificationClass).unwrap(),
            BACnetValue::Unsigned(5)
        );

        if let Ok(BACnetValue::Array(p)) = nc.read_property(PropertyId::Priority) {
            assert_eq!(p[0], BACnetValue::Unsigned(1));
            assert_eq!(p[1], BACnetValue::Unsigned(2));
            assert_eq!(p[2], BACnetValue::Unsigned(3));
        } else {
            panic!("Expected Array for Priority");
        }
    }

    #[test]
    fn test_notification_class_write_priority() {
        let nc = NotificationClass::new(1, "NC1");
        nc.write_property(
            PropertyId::Priority,
            BACnetValue::Array(vec![
                BACnetValue::Unsigned(10),
                BACnetValue::Unsigned(20),
                BACnetValue::Unsigned(30),
            ]),
        )
        .unwrap();

        assert_eq!(nc.priority_for_transition(EventState::HighLimit), 10);
        assert_eq!(nc.priority_for_transition(EventState::Fault), 20);
        assert_eq!(nc.priority_for_transition(EventState::Normal), 30);
    }

    #[test]
    fn test_event_transition_bits() {
        let bits = EventTransitionBits::all();
        assert_eq!(bits.to_bits(), vec![true, true, true]);

        let bits = EventTransitionBits::from_bits(&[true, false, true]);
        assert!(bits.to_offnormal);
        assert!(!bits.to_fault);
        assert!(bits.to_normal);
    }

    #[test]
    fn test_list_properties_includes_limits() {
        let ee = EventEnrollment::new(1, "RangeAlarm", EventType::OutOfRange);
        let props = ee.list_properties();
        assert!(props.contains(&PropertyId::HighLimit));
        assert!(props.contains(&PropertyId::LowLimit));
        assert!(props.contains(&PropertyId::Deadband));
    }

    #[test]
    fn test_list_properties_excludes_limits_for_non_range() {
        let ee = EventEnrollment::new(1, "StateAlarm", EventType::ChangeOfState);
        let props = ee.list_properties();
        assert!(!props.contains(&PropertyId::HighLimit));
        assert!(!props.contains(&PropertyId::LowLimit));
    }
}

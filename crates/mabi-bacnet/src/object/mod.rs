//! BACnet object model implementation.
//!
//! This module provides a trait-based BACnet object model supporting:
//! - Standard object types (Analog, Binary, MultiState)
//! - Property management with concurrent access
//! - COV (Change of Value) detection
//! - Extensible design for custom object types

pub mod device;
pub mod event_enrollment;
pub mod file;
pub mod property;
pub mod registry;
pub mod schedule;
pub mod standard;
pub mod traits;
pub mod trend_log;
pub mod types;

pub use device::{
    BACnetDateTime, CommunicationControlState, DeviceObject, DeviceObjectConfig, DeviceSystemStatus,
};
pub use event_enrollment::{
    EventEnrollment, EventNotification, EventTransitionBits, EventType, NotificationClass,
    NotificationRecipient, NotifyType,
};
pub use file::{FileAccessMethod, FileObject};
pub use property::{BACnetValue, PropertyError, PropertyId, PropertyStore, StatusFlags};
pub use registry::{default_object_descriptors, ObjectRegistry, ObjectTypeDescriptor};
pub use schedule::{
    Calendar, CalendarEntry, DateRange, ObjectPropertyReference, Schedule, SpecialEvent,
    SpecialEventPeriod, TimeValue,
};
pub use standard::{
    AnalogInput, AnalogOutput, AnalogValue, BinaryInput, BinaryOutput, BinaryValue,
    MultiStateInput, MultiStateOutput, MultiStateValue,
};
pub use traits::{BACnetObject, CovSupport, ObjectBuilder, WritableObject};
pub use trend_log::{
    DeviceObjectPropertyReference, LogDatum, LogRecord, LogStatus, LogTimestamp, TrendLog,
};
pub use types::{ObjectId, ObjectType};

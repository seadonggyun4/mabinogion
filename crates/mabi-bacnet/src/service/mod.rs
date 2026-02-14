//! BACnet service implementations.
//!
//! This module provides handlers for BACnet services:
//! - Property services (ReadProperty, WriteProperty, etc.)
//! - Property multiple services (ReadPropertyMultiple, WritePropertyMultiple)
//! - COV services (SubscribeCOV, COV notifications)
//! - Discovery services (Who-Is, I-Am)
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │                     Service Layer                           │
//! │                                                             │
//! │  ┌─────────────────┐  ┌─────────────────┐  ┌─────────────┐ │
//! │  │ Property        │  │ Discovery       │  │ COV         │ │
//! │  │ Services        │  │ Services        │  │ Services    │ │
//! │  │ (Read/Write)    │  │ (Who-Is/I-Am)   │  │ (Subscribe) │ │
//! │  └────────┬────────┘  └────────┬────────┘  └──────┬──────┘ │
//! │           │                    │                   │       │
//! │           └────────────────────┼───────────────────┘       │
//! │                                ▼                           │
//! │           ┌─────────────────────────────────────────┐      │
//! │           │          ServiceRegistry                │      │
//! │           │   (Confirmed & Unconfirmed Handlers)    │      │
//! │           └─────────────────────────────────────────┘      │
//! └─────────────────────────────────────────────────────────────┘
//! ```

pub mod alarm;
pub mod cov;
pub mod create_delete;
pub mod device_control;
pub mod discovery;
pub mod file_access;
pub mod handler;
pub mod property;
pub mod property_multiple;
pub mod read_range;
pub mod subscribe_cov;
pub mod tsm;

pub use cov::{CovManager, CovNotification, CovSubscription};
pub use discovery::{DiscoveryService, IAmResponse, WhoIsRequest};
pub use handler::{
    ConfirmedServiceHandler, ServiceContext, ServiceRegistry, ServiceResult,
    UnconfirmedServiceHandler,
};
pub use property::{PropertyService, ReadPropertyHandler, ReadPropertyRequest, WritePropertyHandler, WritePropertyRequest};
pub use property_multiple::{
    // Request/Response types
    ReadPropertyMultipleRequest, WritePropertyMultipleRequest,
    ReadAccessSpecification, WriteAccessSpecification,
    ReadAccessResult, WriteAccessResult,
    // Handlers
    ReadPropertyMultipleHandler, WritePropertyMultipleHandler,
    // Reusable abstractions
    PropertyReference, PropertyAccessResult, PropertyValueWrite,
};
pub use read_range::{ReadRangeHandler, ReadRangeRequest, RangeType};
pub use file_access::{AtomicReadFileHandler, AtomicWriteFileHandler, AtomicReadFileRequest, AtomicWriteFileRequest};
pub use alarm::{
    AcknowledgeAlarmHandler, ConfirmedEventNotificationHandler,
    GetAlarmSummaryHandler, GetEnrollmentSummaryHandler, GetEventInformationHandler,
};
pub use create_delete::{
    CreateObjectHandler, DeleteObjectHandler,
    CreateObjectRequest, DeleteObjectRequest,
    ObjectFactory, default_object_factory,
};
pub use device_control::{
    TimeSynchronizationHandler, UtcTimeSynchronizationHandler,
    DeviceCommunicationControlHandler, ReinitializeDeviceHandler,
    TimeSyncRequest, DeviceCommunicationControlRequest, ReinitializeDeviceRequest,
    EnableDisable, ReinitializedState,
};
pub use subscribe_cov::{SubscribeCovHandler, SubscribeCovPropertyHandler};
pub use tsm::{ServerTsm, TransactionKey, TsmConfig, TsmStatistics};

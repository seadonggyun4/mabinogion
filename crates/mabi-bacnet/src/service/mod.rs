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

pub mod cov;
pub mod discovery;
pub mod handler;
pub mod property;
pub mod property_multiple;
pub mod subscribe_cov;

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
pub use subscribe_cov::SubscribeCovHandler;

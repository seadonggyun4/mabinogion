//! BACnet object model implementation.
//!
//! This module provides a trait-based BACnet object model supporting:
//! - Standard object types (Analog, Binary, MultiState)
//! - Property management with concurrent access
//! - COV (Change of Value) detection
//! - Extensible design for custom object types

pub mod property;
pub mod registry;
pub mod standard;
pub mod traits;
pub mod types;

pub use property::{BACnetValue, PropertyError, PropertyId, PropertyStore, StatusFlags};
pub use registry::{default_object_descriptors, ObjectRegistry, ObjectTypeDescriptor};
pub use standard::{
    AnalogInput, AnalogOutput, AnalogValue, BinaryInput, BinaryOutput, BinaryValue,
    MultiStateInput, MultiStateOutput, MultiStateValue,
};
pub use traits::{BACnetObject, CovSupport, ObjectBuilder, WritableObject};
pub use types::{ObjectId, ObjectType};

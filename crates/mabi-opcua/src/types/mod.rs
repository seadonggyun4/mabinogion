//! OPC UA type system.
//!
//! This module provides OPC UA specific types that are compatible with the
//! OPC UA specification while maintaining interoperability with Mabinogion core types.

pub mod attribute;
pub mod data_value;
pub mod node_id;
pub mod status_code;
pub mod variant;

pub use attribute::{AccessLevel, AttributeId, WriteMask};
pub use data_value::{DataValue, DataValueBuilder};
pub use node_id::{ExpandedNodeId, NodeId, NodeIdParseError, NodeIdType};
pub use status_code::StatusCode;
pub use variant::{DataTypeId, Variant, VariantArrayValue, VariantScalarValue};

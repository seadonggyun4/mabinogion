//! OPC UA type system.
//!
//! This module provides OPC UA specific types that are compatible with the
//! OPC UA specification while maintaining interoperability with trap-sim-core.

pub mod attribute;
pub mod data_value;
pub mod node_id;
pub mod status_code;
pub mod variant;

pub use attribute::{AttributeId, AccessLevel, WriteMask};
pub use data_value::{DataValue, DataValueBuilder};
pub use node_id::{NodeId, NodeIdType, ExpandedNodeId, NodeIdParseError};
pub use status_code::StatusCode;
pub use variant::{Variant, VariantScalarValue, VariantArrayValue, DataTypeId};

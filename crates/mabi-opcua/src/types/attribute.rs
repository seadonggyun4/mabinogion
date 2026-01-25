//! OPC UA attribute definitions.
//!
//! Attributes define the metadata and characteristics of nodes.

use serde::{Deserialize, Serialize};
use std::fmt;

/// Attribute IDs matching OPC UA specification.
///
/// Each node class has a defined set of attributes that can be accessed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[repr(u32)]
pub enum AttributeId {
    /// Node ID of the node.
    NodeId = 1,
    /// Node class (Object, Variable, Method, etc.).
    NodeClass = 2,
    /// Browse name (qualified name for browsing).
    BrowseName = 3,
    /// Display name (localized text for display).
    DisplayName = 4,
    /// Description (localized text description).
    Description = 5,
    /// Write mask (indicates which attributes can be written).
    WriteMask = 6,
    /// User write mask (user-specific write permissions).
    UserWriteMask = 7,
    /// Is abstract (for type definitions).
    IsAbstract = 8,
    /// Symmetric (for reference types).
    Symmetric = 9,
    /// Inverse name (for reference types).
    InverseName = 10,
    /// Contains no loops (for views).
    ContainsNoLoops = 11,
    /// Event notifier (for objects and views).
    EventNotifier = 12,
    /// Value (for variables).
    Value = 13,
    /// Data type (for variables).
    DataType = 14,
    /// Value rank (for variables, indicates array dimensions).
    ValueRank = 15,
    /// Array dimensions (for variables).
    ArrayDimensions = 16,
    /// Access level (for variables).
    AccessLevel = 17,
    /// User access level (for variables).
    UserAccessLevel = 18,
    /// Minimum sampling interval (for variables).
    MinimumSamplingInterval = 19,
    /// Historizing (for variables).
    Historizing = 20,
    /// Executable (for methods).
    Executable = 21,
    /// User executable (for methods).
    UserExecutable = 22,
    /// Data type definition (for data types).
    DataTypeDefinition = 23,
    /// Role permissions.
    RolePermissions = 24,
    /// User role permissions.
    UserRolePermissions = 25,
    /// Access restrictions.
    AccessRestrictions = 26,
    /// Access level ex (extended access level).
    AccessLevelEx = 27,
}

impl AttributeId {
    /// Get the attribute ID from a numeric value.
    pub fn from_u32(value: u32) -> Option<Self> {
        match value {
            1 => Some(Self::NodeId),
            2 => Some(Self::NodeClass),
            3 => Some(Self::BrowseName),
            4 => Some(Self::DisplayName),
            5 => Some(Self::Description),
            6 => Some(Self::WriteMask),
            7 => Some(Self::UserWriteMask),
            8 => Some(Self::IsAbstract),
            9 => Some(Self::Symmetric),
            10 => Some(Self::InverseName),
            11 => Some(Self::ContainsNoLoops),
            12 => Some(Self::EventNotifier),
            13 => Some(Self::Value),
            14 => Some(Self::DataType),
            15 => Some(Self::ValueRank),
            16 => Some(Self::ArrayDimensions),
            17 => Some(Self::AccessLevel),
            18 => Some(Self::UserAccessLevel),
            19 => Some(Self::MinimumSamplingInterval),
            20 => Some(Self::Historizing),
            21 => Some(Self::Executable),
            22 => Some(Self::UserExecutable),
            23 => Some(Self::DataTypeDefinition),
            24 => Some(Self::RolePermissions),
            25 => Some(Self::UserRolePermissions),
            26 => Some(Self::AccessRestrictions),
            27 => Some(Self::AccessLevelEx),
            _ => None,
        }
    }

    /// Get the display name for this attribute.
    pub fn name(&self) -> &'static str {
        match self {
            Self::NodeId => "NodeId",
            Self::NodeClass => "NodeClass",
            Self::BrowseName => "BrowseName",
            Self::DisplayName => "DisplayName",
            Self::Description => "Description",
            Self::WriteMask => "WriteMask",
            Self::UserWriteMask => "UserWriteMask",
            Self::IsAbstract => "IsAbstract",
            Self::Symmetric => "Symmetric",
            Self::InverseName => "InverseName",
            Self::ContainsNoLoops => "ContainsNoLoops",
            Self::EventNotifier => "EventNotifier",
            Self::Value => "Value",
            Self::DataType => "DataType",
            Self::ValueRank => "ValueRank",
            Self::ArrayDimensions => "ArrayDimensions",
            Self::AccessLevel => "AccessLevel",
            Self::UserAccessLevel => "UserAccessLevel",
            Self::MinimumSamplingInterval => "MinimumSamplingInterval",
            Self::Historizing => "Historizing",
            Self::Executable => "Executable",
            Self::UserExecutable => "UserExecutable",
            Self::DataTypeDefinition => "DataTypeDefinition",
            Self::RolePermissions => "RolePermissions",
            Self::UserRolePermissions => "UserRolePermissions",
            Self::AccessRestrictions => "AccessRestrictions",
            Self::AccessLevelEx => "AccessLevelEx",
        }
    }
}

impl fmt::Display for AttributeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.name())
    }
}

/// Access level flags for variables.
///
/// These flags indicate what operations are permitted on a variable's value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub struct AccessLevel(u8);

impl AccessLevel {
    /// No access.
    pub const NONE: Self = Self(0);
    /// Value can be read.
    pub const CURRENT_READ: Self = Self(0x01);
    /// Value can be written.
    pub const CURRENT_WRITE: Self = Self(0x02);
    /// History can be read.
    pub const HISTORY_READ: Self = Self(0x04);
    /// History can be written.
    pub const HISTORY_WRITE: Self = Self(0x08);
    /// Semantic change notification.
    pub const SEMANTIC_CHANGE: Self = Self(0x10);
    /// Status write is supported.
    pub const STATUS_WRITE: Self = Self(0x20);
    /// Timestamp write is supported.
    pub const TIMESTAMP_WRITE: Self = Self(0x40);

    /// Read and write access.
    pub const READ_WRITE: Self = Self(0x03);

    /// Create from raw value.
    pub const fn from_raw(value: u8) -> Self {
        Self(value)
    }

    /// Get raw value.
    pub const fn raw(&self) -> u8 {
        self.0
    }

    /// Check if read is allowed.
    pub const fn can_read(&self) -> bool {
        self.0 & Self::CURRENT_READ.0 != 0
    }

    /// Check if write is allowed.
    pub const fn can_write(&self) -> bool {
        self.0 & Self::CURRENT_WRITE.0 != 0
    }

    /// Check if history read is allowed.
    pub const fn can_read_history(&self) -> bool {
        self.0 & Self::HISTORY_READ.0 != 0
    }

    /// Check if history write is allowed.
    pub const fn can_write_history(&self) -> bool {
        self.0 & Self::HISTORY_WRITE.0 != 0
    }

    /// Combine access levels using bitwise OR.
    pub const fn or(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }
}

impl std::ops::BitOr for AccessLevel {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self::Output {
        Self(self.0 | rhs.0)
    }
}

impl std::ops::BitOrAssign for AccessLevel {
    fn bitor_assign(&mut self, rhs: Self) {
        self.0 |= rhs.0;
    }
}

impl std::ops::BitAnd for AccessLevel {
    type Output = Self;

    fn bitand(self, rhs: Self) -> Self::Output {
        Self(self.0 & rhs.0)
    }
}

impl fmt::Display for AccessLevel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut flags = Vec::new();
        if self.can_read() {
            flags.push("Read");
        }
        if self.can_write() {
            flags.push("Write");
        }
        if self.can_read_history() {
            flags.push("HistoryRead");
        }
        if self.can_write_history() {
            flags.push("HistoryWrite");
        }
        if flags.is_empty() {
            write!(f, "None")
        } else {
            write!(f, "{}", flags.join("|"))
        }
    }
}

/// Write mask flags for nodes.
///
/// These flags indicate which attributes of a node can be modified.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub struct WriteMask(u32);

impl WriteMask {
    /// No attributes are writable.
    pub const NONE: Self = Self(0);
    /// AccessLevel attribute is writable.
    pub const ACCESS_LEVEL: Self = Self(1 << 0);
    /// ArrayDimensions attribute is writable.
    pub const ARRAY_DIMENSIONS: Self = Self(1 << 1);
    /// BrowseName attribute is writable.
    pub const BROWSE_NAME: Self = Self(1 << 2);
    /// ContainsNoLoops attribute is writable.
    pub const CONTAINS_NO_LOOPS: Self = Self(1 << 3);
    /// DataType attribute is writable.
    pub const DATA_TYPE: Self = Self(1 << 4);
    /// Description attribute is writable.
    pub const DESCRIPTION: Self = Self(1 << 5);
    /// DisplayName attribute is writable.
    pub const DISPLAY_NAME: Self = Self(1 << 6);
    /// EventNotifier attribute is writable.
    pub const EVENT_NOTIFIER: Self = Self(1 << 7);
    /// Executable attribute is writable.
    pub const EXECUTABLE: Self = Self(1 << 8);
    /// Historizing attribute is writable.
    pub const HISTORIZING: Self = Self(1 << 9);
    /// InverseName attribute is writable.
    pub const INVERSE_NAME: Self = Self(1 << 10);
    /// IsAbstract attribute is writable.
    pub const IS_ABSTRACT: Self = Self(1 << 11);
    /// MinimumSamplingInterval attribute is writable.
    pub const MINIMUM_SAMPLING_INTERVAL: Self = Self(1 << 12);
    /// NodeClass attribute is writable.
    pub const NODE_CLASS: Self = Self(1 << 13);
    /// NodeId attribute is writable.
    pub const NODE_ID: Self = Self(1 << 14);
    /// Symmetric attribute is writable.
    pub const SYMMETRIC: Self = Self(1 << 15);
    /// UserAccessLevel attribute is writable.
    pub const USER_ACCESS_LEVEL: Self = Self(1 << 16);
    /// UserExecutable attribute is writable.
    pub const USER_EXECUTABLE: Self = Self(1 << 17);
    /// UserWriteMask attribute is writable.
    pub const USER_WRITE_MASK: Self = Self(1 << 18);
    /// ValueRank attribute is writable.
    pub const VALUE_RANK: Self = Self(1 << 19);
    /// WriteMask attribute is writable.
    pub const WRITE_MASK: Self = Self(1 << 20);
    /// ValueForVariableType attribute is writable.
    pub const VALUE_FOR_VARIABLE_TYPE: Self = Self(1 << 21);

    /// Create from raw value.
    pub const fn from_raw(value: u32) -> Self {
        Self(value)
    }

    /// Get raw value.
    pub const fn raw(&self) -> u32 {
        self.0
    }

    /// Check if a specific bit is set.
    pub const fn contains(&self, other: Self) -> bool {
        self.0 & other.0 != 0
    }

    /// Combine write masks using bitwise OR.
    pub const fn or(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }
}

impl std::ops::BitOr for WriteMask {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self::Output {
        Self(self.0 | rhs.0)
    }
}

impl std::ops::BitOrAssign for WriteMask {
    fn bitor_assign(&mut self, rhs: Self) {
        self.0 |= rhs.0;
    }
}

/// Value rank constants for array dimension handling.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ValueRank(i32);

impl ValueRank {
    /// Value is not an array.
    pub const SCALAR: Self = Self(-1);
    /// Value can be scalar or any array.
    pub const SCALAR_OR_ANY_ARRAY: Self = Self(-3);
    /// Value can be any array (one or more dimensions).
    pub const ANY_ARRAY: Self = Self(0);
    /// Value is a one-dimensional array.
    pub const ONE_DIMENSIONAL: Self = Self(1);
    /// Value is a two-dimensional array.
    pub const TWO_DIMENSIONAL: Self = Self(2);

    /// Create from raw value.
    pub const fn from_raw(value: i32) -> Self {
        Self(value)
    }

    /// Get raw value.
    pub const fn raw(&self) -> i32 {
        self.0
    }

    /// Check if this is a scalar value rank.
    pub const fn is_scalar(&self) -> bool {
        self.0 == -1
    }

    /// Check if arrays are allowed.
    pub const fn allows_array(&self) -> bool {
        self.0 >= 0 || self.0 == -3
    }

    /// Check if scalars are allowed.
    pub const fn allows_scalar(&self) -> bool {
        self.0 == -1 || self.0 == -3
    }
}

impl Default for ValueRank {
    fn default() -> Self {
        Self::SCALAR
    }
}

impl fmt::Display for ValueRank {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0 {
            -1 => write!(f, "Scalar"),
            -3 => write!(f, "ScalarOrAnyArray"),
            0 => write!(f, "AnyArray"),
            1 => write!(f, "OneDimensional"),
            2 => write!(f, "TwoDimensional"),
            n if n > 0 => write!(f, "{}Dimensional", n),
            _ => write!(f, "Unknown({})", self.0),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_attribute_id() {
        assert_eq!(AttributeId::Value as u32, 13);
        assert_eq!(AttributeId::from_u32(13), Some(AttributeId::Value));
        assert_eq!(AttributeId::from_u32(999), None);
    }

    #[test]
    fn test_access_level() {
        let level = AccessLevel::CURRENT_READ | AccessLevel::CURRENT_WRITE;
        assert!(level.can_read());
        assert!(level.can_write());
        assert!(!level.can_read_history());

        assert_eq!(level, AccessLevel::READ_WRITE);
    }

    #[test]
    fn test_write_mask() {
        let mask = WriteMask::DESCRIPTION | WriteMask::DISPLAY_NAME;
        assert!(mask.contains(WriteMask::DESCRIPTION));
        assert!(mask.contains(WriteMask::DISPLAY_NAME));
        assert!(!mask.contains(WriteMask::VALUE_RANK));
    }

    #[test]
    fn test_value_rank() {
        assert!(ValueRank::SCALAR.is_scalar());
        assert!(!ValueRank::SCALAR.allows_array());
        assert!(ValueRank::SCALAR.allows_scalar());

        assert!(ValueRank::ONE_DIMENSIONAL.allows_array());
        assert!(!ValueRank::ONE_DIMENSIONAL.is_scalar());
    }
}

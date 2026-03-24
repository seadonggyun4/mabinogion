//! Register value types.

use serde::{Deserialize, Serialize};

/// Generic register value that can hold either bit or word data.
///
/// This enum provides a type-safe way to represent register values
/// while supporting both coil/discrete input (bool) and holding/input register (u16) types.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum RegisterValue {
    /// Boolean value for coils and discrete inputs.
    Bool(bool),
    /// 16-bit unsigned value for holding and input registers.
    Word(u16),
}

impl RegisterValue {
    /// Create a boolean register value.
    #[inline]
    pub const fn bool(value: bool) -> Self {
        Self::Bool(value)
    }

    /// Create a word register value.
    #[inline]
    pub const fn word(value: u16) -> Self {
        Self::Word(value)
    }

    /// Create a zero/false value for the given type.
    #[inline]
    pub const fn zero(is_bit_type: bool) -> Self {
        if is_bit_type {
            Self::Bool(false)
        } else {
            Self::Word(0)
        }
    }

    /// Get as boolean (returns false for Word(0), true otherwise).
    #[inline]
    pub const fn as_bool(&self) -> bool {
        match self {
            Self::Bool(v) => *v,
            Self::Word(v) => *v != 0,
        }
    }

    /// Get as u16 (returns 0 for false, 1 for true).
    #[inline]
    pub const fn as_word(&self) -> u16 {
        match self {
            Self::Bool(v) => {
                if *v {
                    1
                } else {
                    0
                }
            }
            Self::Word(v) => *v,
        }
    }

    /// Check if this is a boolean value.
    #[inline]
    pub const fn is_bool(&self) -> bool {
        matches!(self, Self::Bool(_))
    }

    /// Check if this is a word value.
    #[inline]
    pub const fn is_word(&self) -> bool {
        matches!(self, Self::Word(_))
    }

    /// Check if the value is zero/false.
    #[inline]
    pub const fn is_zero(&self) -> bool {
        match self {
            Self::Bool(v) => !*v,
            Self::Word(v) => *v == 0,
        }
    }

    /// Try to get the inner boolean value.
    #[inline]
    pub const fn try_as_bool(&self) -> Option<bool> {
        match self {
            Self::Bool(v) => Some(*v),
            Self::Word(_) => None,
        }
    }

    /// Try to get the inner word value.
    #[inline]
    pub const fn try_as_word(&self) -> Option<u16> {
        match self {
            Self::Word(v) => Some(*v),
            Self::Bool(_) => None,
        }
    }
}

impl Default for RegisterValue {
    fn default() -> Self {
        Self::Word(0)
    }
}

impl From<bool> for RegisterValue {
    #[inline]
    fn from(value: bool) -> Self {
        Self::Bool(value)
    }
}

impl From<u16> for RegisterValue {
    #[inline]
    fn from(value: u16) -> Self {
        Self::Word(value)
    }
}

impl From<i16> for RegisterValue {
    #[inline]
    fn from(value: i16) -> Self {
        Self::Word(value as u16)
    }
}

impl std::fmt::Display for RegisterValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Bool(v) => write!(f, "{}", if *v { "ON" } else { "OFF" }),
            Self::Word(v) => write!(f, "{}", v),
        }
    }
}

#[cfg(test)]
/// Represents a register change event with old and new values.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RegisterChange {
    /// The old value before the change.
    pub old_value: RegisterValue,
    /// The new value after the change.
    pub new_value: RegisterValue,
}

#[cfg(test)]
impl RegisterChange {
    /// Create a new register change.
    #[inline]
    pub const fn new(old_value: RegisterValue, new_value: RegisterValue) -> Self {
        Self {
            old_value,
            new_value,
        }
    }

    /// Check if the value actually changed.
    #[inline]
    pub fn has_changed(&self) -> bool {
        self.old_value != self.new_value
    }

    /// Get the delta for word values (new - old), returns 0 for bool values.
    #[inline]
    pub fn delta(&self) -> i32 {
        self.new_value.as_word() as i32 - self.old_value.as_word() as i32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_register_value_bool() {
        let v = RegisterValue::Bool(true);
        assert!(v.is_bool());
        assert!(!v.is_word());
        assert!(v.as_bool());
        assert_eq!(v.as_word(), 1);
        assert_eq!(v.try_as_bool(), Some(true));
        assert_eq!(v.try_as_word(), None);
    }

    #[test]
    fn test_register_value_word() {
        let v = RegisterValue::Word(12345);
        assert!(!v.is_bool());
        assert!(v.is_word());
        assert!(v.as_bool()); // non-zero is true
        assert_eq!(v.as_word(), 12345);
        assert_eq!(v.try_as_bool(), None);
        assert_eq!(v.try_as_word(), Some(12345));
    }

    #[test]
    fn test_register_value_zero() {
        let bool_zero = RegisterValue::zero(true);
        assert_eq!(bool_zero, RegisterValue::Bool(false));
        assert!(bool_zero.is_zero());

        let word_zero = RegisterValue::zero(false);
        assert_eq!(word_zero, RegisterValue::Word(0));
        assert!(word_zero.is_zero());
    }

    #[test]
    fn test_register_value_from() {
        assert_eq!(RegisterValue::from(true), RegisterValue::Bool(true));
        assert_eq!(RegisterValue::from(false), RegisterValue::Bool(false));
        assert_eq!(RegisterValue::from(100u16), RegisterValue::Word(100));
        assert_eq!(RegisterValue::from(-1i16), RegisterValue::Word(65535));
    }

    #[test]
    fn test_register_change() {
        let change = RegisterChange::new(RegisterValue::Word(100), RegisterValue::Word(150));
        assert!(change.has_changed());
        assert_eq!(change.delta(), 50);

        let no_change = RegisterChange::new(RegisterValue::Word(100), RegisterValue::Word(100));
        assert!(!no_change.has_changed());
        assert_eq!(no_change.delta(), 0);
    }

    #[test]
    fn test_display() {
        assert_eq!(format!("{}", RegisterValue::Bool(true)), "ON");
        assert_eq!(format!("{}", RegisterValue::Bool(false)), "OFF");
        assert_eq!(format!("{}", RegisterValue::Word(12345)), "12345");
    }
}

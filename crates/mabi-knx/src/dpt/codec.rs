//! DPT Codec trait and ID types.
//!
//! This module defines the core trait for DPT encoding/decoding,
//! allowing for extensible DPT implementations.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use super::DptValue;
use crate::error::{KnxError, KnxResult};

/// DPT identifier (e.g., DPT 9.001).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DptId {
    /// Main type number (1-255).
    pub main: u16,
    /// Subtype number (0-65535).
    pub sub: u16,
}

impl DptId {
    /// Create a new DPT ID.
    pub const fn new(main: u16, sub: u16) -> Self {
        Self { main, sub }
    }

    /// Create a DPT ID with subtype 0.
    pub const fn main_only(main: u16) -> Self {
        Self { main, sub: 0 }
    }

    /// Check if this is a main type only (sub = 0).
    pub fn is_main_only(&self) -> bool {
        self.sub == 0
    }

    /// Get just the main type.
    pub fn main_type(&self) -> DptId {
        Self::main_only(self.main)
    }
}

impl fmt::Display for DptId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.sub == 0 {
            write!(f, "DPT {}", self.main)
        } else {
            write!(f, "DPT {}.{:03}", self.main, self.sub)
        }
    }
}

impl FromStr for DptId {
    type Err = KnxError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        // Remove "DPT " or "dpt " prefix if present
        let s = s
            .trim()
            .strip_prefix("DPT ")
            .or_else(|| s.strip_prefix("dpt "))
            .or_else(|| s.strip_prefix("DPT"))
            .or_else(|| s.strip_prefix("dpt"))
            .unwrap_or(s)
            .trim();

        if let Some((main, sub)) = s.split_once('.') {
            let main: u16 = main
                .parse()
                .map_err(|_| KnxError::InvalidDpt(format!("Invalid main type: {}", main)))?;
            let sub: u16 = sub
                .parse()
                .map_err(|_| KnxError::InvalidDpt(format!("Invalid subtype: {}", sub)))?;
            Ok(Self::new(main, sub))
        } else {
            let main: u16 = s
                .parse()
                .map_err(|_| KnxError::InvalidDpt(format!("Invalid DPT: {}", s)))?;
            Ok(Self::main_only(main))
        }
    }
}

/// Trait for DPT encoding and decoding.
///
/// Implement this trait to add support for new DPT types.
///
/// # Example
///
/// ```rust,ignore
/// pub struct MyCustomDpt;
///
/// impl DptCodec for MyCustomDpt {
///     fn id(&self) -> DptId {
///         DptId::new(999, 1)
///     }
///
///     fn name(&self) -> &'static str {
///         "My Custom DPT"
///     }
///
///     fn size(&self) -> usize {
///         2
///     }
///
///     fn encode(&self, value: &DptValue) -> KnxResult<Vec<u8>> {
///         // Custom encoding logic
///         Ok(vec![0, 0])
///     }
///
///     fn decode(&self, data: &[u8]) -> KnxResult<DptValue> {
///         // Custom decoding logic
///         Ok(DptValue::U16(0))
///     }
/// }
/// ```
pub trait DptCodec: Send + Sync {
    /// Get the DPT identifier.
    fn id(&self) -> DptId;

    /// Get human-readable name.
    fn name(&self) -> &'static str;

    /// Get the encoded size in bytes.
    ///
    /// For variable-length DPTs (like strings), this returns the maximum size.
    fn size(&self) -> usize;

    /// Encode a value to bytes.
    fn encode(&self, value: &DptValue) -> KnxResult<Vec<u8>>;

    /// Decode bytes to a value.
    fn decode(&self, data: &[u8]) -> KnxResult<DptValue>;

    /// Get the default value for this DPT.
    fn default_value(&self) -> DptValue {
        DptValue::default()
    }

    /// Validate a value for this DPT.
    fn validate(&self, value: &DptValue) -> KnxResult<()> {
        // Default implementation: try encoding
        self.encode(value).map(|_| ())
    }

    /// Get the unit string (if applicable).
    fn unit(&self) -> Option<&'static str> {
        None
    }

    /// Get minimum value (for numeric types).
    fn min_value(&self) -> Option<f64> {
        None
    }

    /// Get maximum value (for numeric types).
    fn max_value(&self) -> Option<f64> {
        None
    }

    /// Check if this DPT uses small data (≤6 bits in APCI).
    fn is_small(&self) -> bool {
        self.size() == 0
    }

    /// Get description.
    fn description(&self) -> &'static str {
        self.name()
    }
}

/// Boxed DPT codec for dynamic dispatch.
pub type BoxedDptCodec = Box<dyn DptCodec>;

/// Helper function to validate data length.
pub fn validate_length(data: &[u8], expected: usize, dpt: &str) -> KnxResult<()> {
    if data.len() < expected {
        Err(KnxError::DptDecoding {
            dpt: dpt.to_string(),
            reason: format!(
                "Expected at least {} bytes, got {}",
                expected,
                data.len()
            ),
        })
    } else {
        Ok(())
    }
}

/// Helper macro for implementing simple DPT codecs.
#[macro_export]
macro_rules! impl_simple_dpt {
    ($name:ident, $main:expr, $sub:expr, $size:expr, $type_name:expr, $unit:expr) => {
        impl $crate::dpt::DptCodec for $name {
            fn id(&self) -> $crate::dpt::DptId {
                $crate::dpt::DptId::new($main, $sub)
            }

            fn name(&self) -> &'static str {
                $type_name
            }

            fn size(&self) -> usize {
                $size
            }

            fn unit(&self) -> Option<&'static str> {
                $unit
            }

            fn encode(&self, value: &$crate::dpt::DptValue) -> $crate::error::KnxResult<Vec<u8>> {
                self.encode_impl(value)
            }

            fn decode(&self, data: &[u8]) -> $crate::error::KnxResult<$crate::dpt::DptValue> {
                self.decode_impl(data)
            }
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dpt_id_display() {
        assert_eq!(DptId::new(9, 1).to_string(), "DPT 9.001");
        assert_eq!(DptId::main_only(1).to_string(), "DPT 1");
    }

    #[test]
    fn test_dpt_id_parse() {
        assert_eq!("9.001".parse::<DptId>().unwrap(), DptId::new(9, 1));
        assert_eq!("DPT 9.001".parse::<DptId>().unwrap(), DptId::new(9, 1));
        assert_eq!("dpt 1".parse::<DptId>().unwrap(), DptId::main_only(1));
        assert_eq!("1".parse::<DptId>().unwrap(), DptId::main_only(1));
    }

    #[test]
    fn test_dpt_id_main_type() {
        let id = DptId::new(9, 1);
        assert_eq!(id.main_type(), DptId::main_only(9));
    }
}

//! DPT Value types.
//!
//! This module defines the universal value type for DPT encoding/decoding.

use serde::{Deserialize, Serialize};
use std::fmt;

/// Universal DPT value type.
///
/// This enum represents all possible value types that can be encoded/decoded
/// by DPT codecs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum DptValue {
    /// Boolean value (DPT 1.x).
    Bool(bool),

    /// Unsigned 8-bit value (DPT 5.x).
    U8(u8),

    /// Signed 8-bit value (DPT 6.x).
    I8(i8),

    /// Unsigned 16-bit value (DPT 7.x).
    U16(u16),

    /// Signed 16-bit value (DPT 8.x).
    I16(i16),

    /// Unsigned 32-bit value (DPT 12.x).
    U32(u32),

    /// Signed 32-bit value (DPT 13.x).
    I32(i32),

    /// 16-bit float value (DPT 9.x).
    F16(f32),

    /// 32-bit float value (DPT 14.x).
    F32(f32),

    /// Time of day (DPT 10.x).
    Time {
        weekday: Option<u8>,
        hour: u8,
        minute: u8,
        second: u8,
    },

    /// Date (DPT 11.x).
    Date { year: u16, month: u8, day: u8 },

    /// DateTime (DPT 19.x).
    DateTime {
        year: u16,
        month: u8,
        day: u8,
        weekday: Option<u8>,
        hour: u8,
        minute: u8,
        second: u8,
    },

    /// String value (DPT 16.x).
    String(String),

    /// Scene number (DPT 17.x, DPT 18.x).
    Scene { number: u8, learn: bool },

    /// HVAC mode (DPT 20.x).
    HvacMode(HvacMode),

    /// Color RGB (DPT 232.x).
    ColorRgb { r: u8, g: u8, b: u8 },

    /// Color RGBW (DPT 251.x).
    ColorRgbw { r: u8, g: u8, b: u8, w: u8 },

    /// Dimming control (DPT 3.x).
    DimmingControl { direction: bool, step_code: u8 },

    /// Blinds control (DPT 3.x).
    BlindsControl { direction: bool, step_code: u8 },

    /// Priority control (DPT 2.x).
    PriorityControl { control: bool, value: bool },

    /// Raw bytes for unsupported types.
    Raw(Vec<u8>),
}

impl DptValue {
    // ========================================================================
    // Type checking methods
    // ========================================================================

    /// Check if value is boolean.
    pub fn is_bool(&self) -> bool {
        matches!(self, Self::Bool(_))
    }

    /// Check if value is numeric.
    pub fn is_numeric(&self) -> bool {
        matches!(
            self,
            Self::U8(_)
                | Self::I8(_)
                | Self::U16(_)
                | Self::I16(_)
                | Self::U32(_)
                | Self::I32(_)
                | Self::F16(_)
                | Self::F32(_)
        )
    }

    /// Check if value is a string.
    pub fn is_string(&self) -> bool {
        matches!(self, Self::String(_))
    }

    // ========================================================================
    // Conversion methods
    // ========================================================================

    /// Try to get as boolean.
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Self::Bool(v) => Some(*v),
            Self::U8(v) => Some(*v != 0),
            Self::I8(v) => Some(*v != 0),
            _ => None,
        }
    }

    /// Try to get as f64.
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Self::U8(v) => Some(*v as f64),
            Self::I8(v) => Some(*v as f64),
            Self::U16(v) => Some(*v as f64),
            Self::I16(v) => Some(*v as f64),
            Self::U32(v) => Some(*v as f64),
            Self::I32(v) => Some(*v as f64),
            Self::F16(v) => Some(*v as f64),
            Self::F32(v) => Some(*v as f64),
            _ => None,
        }
    }

    /// Try to get as string.
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::String(s) => Some(s),
            _ => None,
        }
    }

    /// Try to get raw bytes.
    pub fn as_raw(&self) -> Option<&[u8]> {
        match self {
            Self::Raw(bytes) => Some(bytes),
            _ => None,
        }
    }

    // ========================================================================
    // Factory methods
    // ========================================================================

    /// Create a time value.
    pub fn time(hour: u8, minute: u8, second: u8) -> Self {
        Self::Time {
            weekday: None,
            hour,
            minute,
            second,
        }
    }

    /// Create a time value with weekday.
    pub fn time_with_weekday(weekday: u8, hour: u8, minute: u8, second: u8) -> Self {
        Self::Time {
            weekday: Some(weekday),
            hour,
            minute,
            second,
        }
    }

    /// Create a date value.
    pub fn date(year: u16, month: u8, day: u8) -> Self {
        Self::Date { year, month, day }
    }

    /// Create an RGB color value.
    pub fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self::ColorRgb { r, g, b }
    }

    /// Create an RGBW color value.
    pub fn rgbw(r: u8, g: u8, b: u8, w: u8) -> Self {
        Self::ColorRgbw { r, g, b, w }
    }
}

impl Default for DptValue {
    fn default() -> Self {
        Self::Bool(false)
    }
}

impl fmt::Display for DptValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Bool(v) => write!(f, "{}", v),
            Self::U8(v) => write!(f, "{}", v),
            Self::I8(v) => write!(f, "{}", v),
            Self::U16(v) => write!(f, "{}", v),
            Self::I16(v) => write!(f, "{}", v),
            Self::U32(v) => write!(f, "{}", v),
            Self::I32(v) => write!(f, "{}", v),
            Self::F16(v) => write!(f, "{:.2}", v),
            Self::F32(v) => write!(f, "{:.4}", v),
            Self::Time {
                weekday,
                hour,
                minute,
                second,
            } => {
                if let Some(wd) = weekday {
                    write!(f, "{} {:02}:{:02}:{:02}", wd, hour, minute, second)
                } else {
                    write!(f, "{:02}:{:02}:{:02}", hour, minute, second)
                }
            }
            Self::Date { year, month, day } => write!(f, "{:04}-{:02}-{:02}", year, month, day),
            Self::DateTime {
                year,
                month,
                day,
                hour,
                minute,
                second,
                ..
            } => write!(
                f,
                "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
                year, month, day, hour, minute, second
            ),
            Self::String(s) => write!(f, "{}", s),
            Self::Scene { number, learn } => {
                if *learn {
                    write!(f, "Scene {} (learn)", number)
                } else {
                    write!(f, "Scene {}", number)
                }
            }
            Self::HvacMode(mode) => write!(f, "{:?}", mode),
            Self::ColorRgb { r, g, b } => write!(f, "RGB({},{},{})", r, g, b),
            Self::ColorRgbw { r, g, b, w } => write!(f, "RGBW({},{},{},{})", r, g, b, w),
            Self::DimmingControl {
                direction,
                step_code,
            } => {
                let dir = if *direction { "brighter" } else { "darker" };
                write!(f, "Dim {} step {}", dir, step_code)
            }
            Self::BlindsControl {
                direction,
                step_code,
            } => {
                let dir = if *direction { "down" } else { "up" };
                write!(f, "Blinds {} step {}", dir, step_code)
            }
            Self::PriorityControl { control, value } => {
                write!(f, "Priority ctrl={} val={}", control, value)
            }
            Self::Raw(bytes) => write!(f, "Raw({} bytes)", bytes.len()),
        }
    }
}

// ============================================================================
// Conversion implementations
// ============================================================================

impl From<bool> for DptValue {
    fn from(v: bool) -> Self {
        Self::Bool(v)
    }
}

impl From<u8> for DptValue {
    fn from(v: u8) -> Self {
        Self::U8(v)
    }
}

impl From<i8> for DptValue {
    fn from(v: i8) -> Self {
        Self::I8(v)
    }
}

impl From<u16> for DptValue {
    fn from(v: u16) -> Self {
        Self::U16(v)
    }
}

impl From<i16> for DptValue {
    fn from(v: i16) -> Self {
        Self::I16(v)
    }
}

impl From<u32> for DptValue {
    fn from(v: u32) -> Self {
        Self::U32(v)
    }
}

impl From<i32> for DptValue {
    fn from(v: i32) -> Self {
        Self::I32(v)
    }
}

impl From<f32> for DptValue {
    fn from(v: f32) -> Self {
        Self::F32(v)
    }
}

impl From<String> for DptValue {
    fn from(v: String) -> Self {
        Self::String(v)
    }
}

impl From<&str> for DptValue {
    fn from(v: &str) -> Self {
        Self::String(v.to_string())
    }
}

impl From<Vec<u8>> for DptValue {
    fn from(v: Vec<u8>) -> Self {
        Self::Raw(v)
    }
}

// ============================================================================
// HVAC Mode enum
// ============================================================================

/// HVAC operation mode (DPT 20.102).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HvacMode {
    Auto = 0,
    Comfort = 1,
    Standby = 2,
    Economy = 3,
    BuildingProtection = 4,
}

impl HvacMode {
    /// Create from raw value.
    pub fn from_raw(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::Auto),
            1 => Some(Self::Comfort),
            2 => Some(Self::Standby),
            3 => Some(Self::Economy),
            4 => Some(Self::BuildingProtection),
            _ => None,
        }
    }

    /// Convert to raw value.
    pub fn to_raw(self) -> u8 {
        self as u8
    }
}

impl Default for HvacMode {
    fn default() -> Self {
        Self::Auto
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dpt_value_display() {
        assert_eq!(DptValue::Bool(true).to_string(), "true");
        assert_eq!(DptValue::U8(42).to_string(), "42");
        assert_eq!(DptValue::F16(25.5).to_string(), "25.50");
    }

    #[test]
    fn test_dpt_value_conversions() {
        let v: DptValue = true.into();
        assert_eq!(v.as_bool(), Some(true));

        let v: DptValue = 42u8.into();
        assert_eq!(v.as_f64(), Some(42.0));

        let v: DptValue = "hello".into();
        assert_eq!(v.as_str(), Some("hello"));
    }

    #[test]
    fn test_dpt_value_time() {
        let v = DptValue::time(12, 30, 45);
        assert_eq!(v.to_string(), "12:30:45");

        let v = DptValue::time_with_weekday(1, 12, 30, 45);
        assert_eq!(v.to_string(), "1 12:30:45");
    }

    #[test]
    fn test_hvac_mode() {
        assert_eq!(HvacMode::from_raw(1), Some(HvacMode::Comfort));
        assert_eq!(HvacMode::Comfort.to_raw(), 1);
        assert_eq!(HvacMode::from_raw(99), None);
    }
}

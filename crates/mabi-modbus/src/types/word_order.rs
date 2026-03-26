//! Word order (byte order) definitions for multi-register values.
//!
//! Different Modbus devices use different byte orderings when storing
//! multi-byte values across registers. This module provides the `WordOrder`
//! enum to handle all common orderings.

use serde::{Deserialize, Serialize};
use std::fmt;

/// Byte ordering for multi-register values.
///
/// When a 32-bit or 64-bit value is stored across multiple 16-bit Modbus
/// registers, different devices use different byte orderings. This enum
/// represents the four most common orderings.
///
/// # Byte Arrangement for 32-bit value 0x12345678
///
/// | Order              | Register 0 | Register 1 | Bytes in memory |
/// |--------------------|------------|------------|-----------------|
/// | BigEndian          | 0x1234     | 0x5678     | 12 34 56 78     |
/// | LittleEndian       | 0x7856     | 0x3412     | 78 56 34 12     |
/// | BigEndianWordSwap  | 0x5678     | 0x1234     | 56 78 12 34     |
/// | LittleEndianWordSwap| 0x3412    | 0x7856     | 34 12 78 56     |
///
/// # Common Device Mappings
///
/// - **BigEndian (AB CD)**: Most Siemens PLCs, Modicon
/// - **LittleEndian (DC BA)**: Some Schneider devices
/// - **BigEndianWordSwap (CD AB)**: Many ABB devices, some Allen-Bradley
/// - **LittleEndianWordSwap (BA DC)**: Rare, but exists in some legacy systems
///
/// # Example
///
/// ```rust
/// use mabi_modbus::types::WordOrder;
///
/// // Default is BigEndian (most common in industrial protocols)
/// let order = WordOrder::default();
/// assert_eq!(order, WordOrder::BigEndian);
///
/// // Parse from string
/// let order: WordOrder = "big_endian_word_swap".parse().unwrap();
/// assert_eq!(order, WordOrder::BigEndianWordSwap);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WordOrder {
    /// Big Endian (AB CD) - Most significant byte first.
    ///
    /// For value 0x12345678:
    /// - Register 0: 0x1234 (bytes: 12 34)
    /// - Register 1: 0x5678 (bytes: 56 78)
    ///
    /// This is the most common format in industrial protocols.
    #[serde(alias = "be", alias = "big")]
    BigEndian,

    /// Little Endian (DC BA) - Least significant byte first.
    ///
    /// For value 0x12345678:
    /// - Register 0: 0x7856 (bytes: 78 56)
    /// - Register 1: 0x3412 (bytes: 34 12)
    #[serde(alias = "le", alias = "little")]
    LittleEndian,

    /// Big Endian with Word Swap (CD AB).
    ///
    /// For value 0x12345678:
    /// - Register 0: 0x5678 (bytes: 56 78)
    /// - Register 1: 0x1234 (bytes: 12 34)
    ///
    /// Common in ABB and some Allen-Bradley devices.
    #[serde(alias = "be_swap", alias = "mid_big")]
    BigEndianWordSwap,

    /// Little Endian with Word Swap (BA DC).
    ///
    /// For value 0x12345678:
    /// - Register 0: 0x3412 (bytes: 34 12)
    /// - Register 1: 0x7856 (bytes: 78 56)
    #[serde(alias = "le_swap", alias = "mid_little")]
    LittleEndianWordSwap,
}

impl Default for WordOrder {
    /// Returns `BigEndian` as the default word order.
    ///
    /// Big Endian is the most common format in industrial Modbus implementations.
    fn default() -> Self {
        Self::BigEndian
    }
}

impl WordOrder {
    /// Returns true if this order is big-endian based.
    #[inline]
    pub fn is_big_endian(&self) -> bool {
        matches!(self, Self::BigEndian | Self::BigEndianWordSwap)
    }

    /// Returns true if this order is little-endian based.
    #[inline]
    pub fn is_little_endian(&self) -> bool {
        matches!(self, Self::LittleEndian | Self::LittleEndianWordSwap)
    }

    /// Returns true if this order has word swap.
    #[inline]
    pub fn has_word_swap(&self) -> bool {
        matches!(self, Self::BigEndianWordSwap | Self::LittleEndianWordSwap)
    }

    /// Returns all available word orders.
    pub fn all() -> &'static [WordOrder] {
        &[
            Self::BigEndian,
            Self::LittleEndian,
            Self::BigEndianWordSwap,
            Self::LittleEndianWordSwap,
        ]
    }

    /// Returns a short code for this word order.
    ///
    /// Useful for display in compact formats.
    pub fn short_code(&self) -> &'static str {
        match self {
            Self::BigEndian => "AB_CD",
            Self::LittleEndian => "DC_BA",
            Self::BigEndianWordSwap => "CD_AB",
            Self::LittleEndianWordSwap => "BA_DC",
        }
    }

    /// Convert 4 bytes to registers according to this word order.
    ///
    /// Takes 4 bytes in big-endian order and converts them to 2 u16 registers
    /// according to this word order.
    pub fn bytes_to_registers_32(&self, bytes: [u8; 4]) -> [u16; 2] {
        match self {
            Self::BigEndian => {
                // AB CD -> [AB, CD]
                [
                    u16::from_be_bytes([bytes[0], bytes[1]]),
                    u16::from_be_bytes([bytes[2], bytes[3]]),
                ]
            }
            Self::LittleEndian => {
                // DC BA -> [DC, BA] (fully reversed)
                [
                    u16::from_le_bytes([bytes[3], bytes[2]]),
                    u16::from_le_bytes([bytes[1], bytes[0]]),
                ]
            }
            Self::BigEndianWordSwap => {
                // CD AB -> [CD, AB]
                [
                    u16::from_be_bytes([bytes[2], bytes[3]]),
                    u16::from_be_bytes([bytes[0], bytes[1]]),
                ]
            }
            Self::LittleEndianWordSwap => {
                // BA DC -> [BA, DC]
                [
                    u16::from_le_bytes([bytes[1], bytes[0]]),
                    u16::from_le_bytes([bytes[3], bytes[2]]),
                ]
            }
        }
    }

    /// Convert 2 registers to 4 bytes according to this word order.
    ///
    /// Returns bytes in big-endian order (standard byte representation).
    pub fn registers_to_bytes_32(&self, registers: [u16; 2]) -> [u8; 4] {
        match self {
            Self::BigEndian => {
                // [AB, CD] -> AB CD
                let hi = registers[0].to_be_bytes();
                let lo = registers[1].to_be_bytes();
                [hi[0], hi[1], lo[0], lo[1]]
            }
            Self::LittleEndian => {
                // [DC, BA] -> AB CD (reverse from LE)
                let hi = registers[1].to_le_bytes();
                let lo = registers[0].to_le_bytes();
                [hi[1], hi[0], lo[1], lo[0]]
            }
            Self::BigEndianWordSwap => {
                // [CD, AB] -> AB CD (swap words)
                let hi = registers[1].to_be_bytes();
                let lo = registers[0].to_be_bytes();
                [hi[0], hi[1], lo[0], lo[1]]
            }
            Self::LittleEndianWordSwap => {
                // [BA, DC] -> AB CD
                let hi = registers[0].to_le_bytes();
                let lo = registers[1].to_le_bytes();
                [hi[1], hi[0], lo[1], lo[0]]
            }
        }
    }

    /// Convert 8 bytes to 4 registers according to this word order.
    pub fn bytes_to_registers_64(&self, bytes: [u8; 8]) -> [u16; 4] {
        let hi = self.bytes_to_registers_32([bytes[0], bytes[1], bytes[2], bytes[3]]);
        let lo = self.bytes_to_registers_32([bytes[4], bytes[5], bytes[6], bytes[7]]);

        match self {
            Self::BigEndian | Self::LittleEndianWordSwap => [hi[0], hi[1], lo[0], lo[1]],
            Self::LittleEndian | Self::BigEndianWordSwap => [lo[0], lo[1], hi[0], hi[1]],
        }
    }

    /// Convert 4 registers to 8 bytes according to this word order.
    pub fn registers_to_bytes_64(&self, registers: [u16; 4]) -> [u8; 8] {
        match self {
            Self::BigEndian | Self::LittleEndianWordSwap => {
                let hi = self.registers_to_bytes_32([registers[0], registers[1]]);
                let lo = self.registers_to_bytes_32([registers[2], registers[3]]);
                [hi[0], hi[1], hi[2], hi[3], lo[0], lo[1], lo[2], lo[3]]
            }
            Self::LittleEndian | Self::BigEndianWordSwap => {
                let hi = self.registers_to_bytes_32([registers[2], registers[3]]);
                let lo = self.registers_to_bytes_32([registers[0], registers[1]]);
                [hi[0], hi[1], hi[2], hi[3], lo[0], lo[1], lo[2], lo[3]]
            }
        }
    }
}

impl fmt::Display for WordOrder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BigEndian => write!(f, "Big Endian (AB CD)"),
            Self::LittleEndian => write!(f, "Little Endian (DC BA)"),
            Self::BigEndianWordSwap => write!(f, "Big Endian Word Swap (CD AB)"),
            Self::LittleEndianWordSwap => write!(f, "Little Endian Word Swap (BA DC)"),
        }
    }
}

impl std::str::FromStr for WordOrder {
    type Err = WordOrderParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let normalized = s.to_lowercase().replace('-', "_").replace(' ', "_");

        match normalized.as_str() {
            "big_endian" | "be" | "big" | "ab_cd" | "abcd" => Ok(Self::BigEndian),
            "little_endian" | "le" | "little" | "dc_ba" | "dcba" => Ok(Self::LittleEndian),
            "big_endian_word_swap" | "be_swap" | "mid_big" | "cd_ab" | "cdab" => {
                Ok(Self::BigEndianWordSwap)
            }
            "little_endian_word_swap" | "le_swap" | "mid_little" | "ba_dc" | "badc" => {
                Ok(Self::LittleEndianWordSwap)
            }
            _ => Err(WordOrderParseError(s.to_string())),
        }
    }
}

/// Error when parsing a word order from string.
#[derive(Debug, Clone)]
pub struct WordOrderParseError(String);

impl fmt::Display for WordOrderParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Invalid word order '{}'. Valid options: big_endian, little_endian, big_endian_word_swap, little_endian_word_swap",
            self.0
        )
    }
}

impl std::error::Error for WordOrderParseError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_is_big_endian() {
        assert_eq!(WordOrder::default(), WordOrder::BigEndian);
    }

    #[test]
    fn test_is_big_endian() {
        assert!(WordOrder::BigEndian.is_big_endian());
        assert!(WordOrder::BigEndianWordSwap.is_big_endian());
        assert!(!WordOrder::LittleEndian.is_big_endian());
        assert!(!WordOrder::LittleEndianWordSwap.is_big_endian());
    }

    #[test]
    fn test_has_word_swap() {
        assert!(!WordOrder::BigEndian.has_word_swap());
        assert!(!WordOrder::LittleEndian.has_word_swap());
        assert!(WordOrder::BigEndianWordSwap.has_word_swap());
        assert!(WordOrder::LittleEndianWordSwap.has_word_swap());
    }

    #[test]
    fn test_bytes_to_registers_32_big_endian() {
        let bytes = [0x12, 0x34, 0x56, 0x78];
        let regs = WordOrder::BigEndian.bytes_to_registers_32(bytes);
        assert_eq!(regs, [0x1234, 0x5678]);
    }

    #[test]
    fn test_bytes_to_registers_32_big_endian_word_swap() {
        let bytes = [0x12, 0x34, 0x56, 0x78];
        let regs = WordOrder::BigEndianWordSwap.bytes_to_registers_32(bytes);
        assert_eq!(regs, [0x5678, 0x1234]);
    }

    #[test]
    fn test_round_trip_32() {
        let original_bytes = [0x12, 0x34, 0x56, 0x78];

        for order in WordOrder::all() {
            let regs = order.bytes_to_registers_32(original_bytes);
            let result = order.registers_to_bytes_32(regs);
            assert_eq!(result, original_bytes, "Round-trip failed for {:?}", order);
        }
    }

    #[test]
    fn test_f32_round_trip() {
        let value: f32 = 123.456;
        let bytes = value.to_be_bytes();

        for order in WordOrder::all() {
            let regs = order.bytes_to_registers_32(bytes);
            let result_bytes = order.registers_to_bytes_32(regs);
            let result = f32::from_be_bytes(result_bytes);
            assert!(
                (result - value).abs() < 0.0001,
                "f32 round-trip failed for {:?}: expected {}, got {}",
                order,
                value,
                result
            );
        }
    }

    #[test]
    fn test_parse_word_order() {
        assert_eq!(
            "big_endian".parse::<WordOrder>().unwrap(),
            WordOrder::BigEndian
        );
        assert_eq!("BE".parse::<WordOrder>().unwrap(), WordOrder::BigEndian);
        assert_eq!("AB_CD".parse::<WordOrder>().unwrap(), WordOrder::BigEndian);
        assert_eq!(
            "cd_ab".parse::<WordOrder>().unwrap(),
            WordOrder::BigEndianWordSwap
        );
        assert_eq!(
            "little_endian".parse::<WordOrder>().unwrap(),
            WordOrder::LittleEndian
        );
        assert!("invalid".parse::<WordOrder>().is_err());
    }

    #[test]
    fn test_serde() {
        let order = WordOrder::BigEndianWordSwap;
        let json = serde_json::to_string(&order).unwrap();
        assert_eq!(json, "\"big_endian_word_swap\"");

        let parsed: WordOrder = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, order);
    }

    #[test]
    fn test_short_code() {
        assert_eq!(WordOrder::BigEndian.short_code(), "AB_CD");
        assert_eq!(WordOrder::LittleEndian.short_code(), "DC_BA");
        assert_eq!(WordOrder::BigEndianWordSwap.short_code(), "CD_AB");
        assert_eq!(WordOrder::LittleEndianWordSwap.short_code(), "BA_DC");
    }
}

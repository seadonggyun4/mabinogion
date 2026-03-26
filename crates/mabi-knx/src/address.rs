//! KNX address types.
//!
//! This module provides implementations for KNX individual addresses (physical addresses)
//! and group addresses used in KNX communication.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::error::KnxError;

// ============================================================================
// Individual Address (Physical Address)
// ============================================================================

/// KNX Individual Address (physical address).
///
/// Format: Area.Line.Device (e.g., "1.2.3")
/// - Area: 0-15 (4 bits)
/// - Line: 0-15 (4 bits)
/// - Device: 0-255 (8 bits)
///
/// Total: 16 bits
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct IndividualAddress {
    /// Area (0-15).
    area: u8,
    /// Line (0-15).
    line: u8,
    /// Device (0-255).
    device: u8,
}

impl IndividualAddress {
    /// Maximum area value.
    pub const MAX_AREA: u8 = 15;
    /// Maximum line value.
    pub const MAX_LINE: u8 = 15;
    /// Maximum device value.
    pub const MAX_DEVICE: u8 = 255;

    /// Create a new individual address.
    ///
    /// # Panics
    /// Panics if area > 15 or line > 15.
    pub fn new(area: u8, line: u8, device: u8) -> Self {
        assert!(area <= Self::MAX_AREA, "Area must be 0-15");
        assert!(line <= Self::MAX_LINE, "Line must be 0-15");
        Self { area, line, device }
    }

    /// Try to create a new individual address with validation.
    pub fn try_new(area: u8, line: u8, device: u8) -> Result<Self, KnxError> {
        if area > Self::MAX_AREA {
            return Err(KnxError::AddressOutOfRange {
                address: format!("area={}", area),
                valid_range: "0-15".to_string(),
            });
        }
        if line > Self::MAX_LINE {
            return Err(KnxError::AddressOutOfRange {
                address: format!("line={}", line),
                valid_range: "0-15".to_string(),
            });
        }
        Ok(Self { area, line, device })
    }

    /// Get the area component.
    #[inline]
    pub fn area(&self) -> u8 {
        self.area
    }

    /// Get the line component.
    #[inline]
    pub fn line(&self) -> u8 {
        self.line
    }

    /// Get the device component.
    #[inline]
    pub fn device(&self) -> u8 {
        self.device
    }

    /// Encode to 16-bit value.
    #[inline]
    pub fn encode(&self) -> u16 {
        ((self.area as u16) << 12) | ((self.line as u16) << 8) | (self.device as u16)
    }

    /// Decode from 16-bit value.
    #[inline]
    pub fn decode(value: u16) -> Self {
        Self {
            area: ((value >> 12) & 0x0F) as u8,
            line: ((value >> 8) & 0x0F) as u8,
            device: (value & 0xFF) as u8,
        }
    }

    /// Encode to byte array (big-endian).
    pub fn to_bytes(&self) -> [u8; 2] {
        self.encode().to_be_bytes()
    }

    /// Decode from byte array (big-endian).
    pub fn from_bytes(bytes: [u8; 2]) -> Self {
        Self::decode(u16::from_be_bytes(bytes))
    }

    /// Check if this is a broadcast address (0.0.0).
    #[inline]
    pub fn is_broadcast(&self) -> bool {
        self.area == 0 && self.line == 0 && self.device == 0
    }

    /// Check if this is a valid device address (not 0.0.0).
    #[inline]
    pub fn is_valid_device(&self) -> bool {
        !self.is_broadcast()
    }
}

impl Default for IndividualAddress {
    fn default() -> Self {
        Self::new(1, 1, 1)
    }
}

impl fmt::Display for IndividualAddress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.area, self.line, self.device)
    }
}

impl FromStr for IndividualAddress {
    type Err = KnxError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let parts: Vec<&str> = s.split('.').collect();
        if parts.len() != 3 {
            return Err(KnxError::InvalidIndividualAddress(format!(
                "Expected format 'area.line.device', got '{}'",
                s
            )));
        }

        let area: u8 = parts[0].parse().map_err(|_| {
            KnxError::InvalidIndividualAddress(format!("Invalid area: {}", parts[0]))
        })?;
        let line: u8 = parts[1].parse().map_err(|_| {
            KnxError::InvalidIndividualAddress(format!("Invalid line: {}", parts[1]))
        })?;
        let device: u8 = parts[2].parse().map_err(|_| {
            KnxError::InvalidIndividualAddress(format!("Invalid device: {}", parts[2]))
        })?;

        Self::try_new(area, line, device)
            .map_err(|_| KnxError::InvalidIndividualAddress(s.to_string()))
    }
}

impl From<u16> for IndividualAddress {
    fn from(value: u16) -> Self {
        Self::decode(value)
    }
}

impl From<IndividualAddress> for u16 {
    fn from(addr: IndividualAddress) -> Self {
        addr.encode()
    }
}

// ============================================================================
// Group Address
// ============================================================================

/// KNX Group Address.
///
/// Supports multiple formats:
/// - 3-level: Main/Middle/Sub (e.g., "1/2/3")
/// - 2-level: Main/Sub (e.g., "1/2048")
/// - Free: Raw 16-bit value (e.g., "12345")
///
/// 3-level format (most common):
/// - Main: 0-31 (5 bits)
/// - Middle: 0-7 (3 bits)
/// - Sub: 0-255 (8 bits)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct GroupAddress {
    raw: u16,
}

impl GroupAddress {
    /// Maximum main group value (3-level).
    pub const MAX_MAIN: u8 = 31;
    /// Maximum middle group value (3-level).
    pub const MAX_MIDDLE: u8 = 7;
    /// Maximum sub group value (3-level).
    pub const MAX_SUB: u8 = 255;

    /// Create from 3-level format (main/middle/sub).
    ///
    /// # Panics
    /// Panics if values are out of range.
    pub fn three_level(main: u8, middle: u8, sub: u8) -> Self {
        assert!(main <= Self::MAX_MAIN, "Main must be 0-31");
        assert!(middle <= Self::MAX_MIDDLE, "Middle must be 0-7");

        let raw =
            ((main as u16 & 0x1F) << 11) | ((middle as u16 & 0x07) << 8) | (sub as u16 & 0xFF);
        Self { raw }
    }

    /// Try to create from 3-level format with validation.
    pub fn try_three_level(main: u8, middle: u8, sub: u8) -> Result<Self, KnxError> {
        if main > Self::MAX_MAIN {
            return Err(KnxError::AddressOutOfRange {
                address: format!("main={}", main),
                valid_range: "0-31".to_string(),
            });
        }
        if middle > Self::MAX_MIDDLE {
            return Err(KnxError::AddressOutOfRange {
                address: format!("middle={}", middle),
                valid_range: "0-7".to_string(),
            });
        }
        Ok(Self::three_level(main, middle, sub))
    }

    /// Create from 2-level format (main/sub).
    pub fn two_level(main: u8, sub: u16) -> Self {
        assert!(main <= Self::MAX_MAIN, "Main must be 0-31");
        assert!(sub <= 0x07FF, "Sub must be 0-2047");

        let raw = ((main as u16 & 0x1F) << 11) | (sub & 0x07FF);
        Self { raw }
    }

    /// Create from raw 16-bit value.
    #[inline]
    pub fn from_raw(raw: u16) -> Self {
        Self { raw }
    }

    /// Get the raw 16-bit value.
    #[inline]
    pub fn raw(&self) -> u16 {
        self.raw
    }

    /// Parse as 3-level format.
    pub fn as_three_level(&self) -> (u8, u8, u8) {
        let main = ((self.raw >> 11) & 0x1F) as u8;
        let middle = ((self.raw >> 8) & 0x07) as u8;
        let sub = (self.raw & 0xFF) as u8;
        (main, middle, sub)
    }

    /// Parse as 2-level format.
    pub fn as_two_level(&self) -> (u8, u16) {
        let main = ((self.raw >> 11) & 0x1F) as u8;
        let sub = self.raw & 0x07FF;
        (main, sub)
    }

    /// Get main group (3-level).
    #[inline]
    pub fn main(&self) -> u8 {
        ((self.raw >> 11) & 0x1F) as u8
    }

    /// Get middle group (3-level).
    #[inline]
    pub fn middle(&self) -> u8 {
        ((self.raw >> 8) & 0x07) as u8
    }

    /// Get sub group (3-level).
    #[inline]
    pub fn sub(&self) -> u8 {
        (self.raw & 0xFF) as u8
    }

    /// Encode to byte array (big-endian).
    pub fn to_bytes(&self) -> [u8; 2] {
        self.raw.to_be_bytes()
    }

    /// Decode from byte array (big-endian).
    pub fn from_bytes(bytes: [u8; 2]) -> Self {
        Self {
            raw: u16::from_be_bytes(bytes),
        }
    }

    /// Check if this is group address 0/0/0.
    #[inline]
    pub fn is_zero(&self) -> bool {
        self.raw == 0
    }

    /// Format as 3-level string.
    pub fn format_three_level(&self) -> String {
        let (main, middle, sub) = self.as_three_level();
        format!("{}/{}/{}", main, middle, sub)
    }

    /// Format as 2-level string.
    pub fn format_two_level(&self) -> String {
        let (main, sub) = self.as_two_level();
        format!("{}/{}", main, sub)
    }
}

impl Default for GroupAddress {
    fn default() -> Self {
        Self::three_level(0, 0, 1)
    }
}

impl fmt::Display for GroupAddress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (main, middle, sub) = self.as_three_level();
        write!(f, "{}/{}/{}", main, middle, sub)
    }
}

impl FromStr for GroupAddress {
    type Err = KnxError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let parts: Vec<&str> = s.split('/').collect();

        match parts.len() {
            // 3-level format: main/middle/sub
            3 => {
                let main: u8 = parts[0].parse().map_err(|_| {
                    KnxError::InvalidGroupAddress(format!("Invalid main group: {}", parts[0]))
                })?;
                let middle: u8 = parts[1].parse().map_err(|_| {
                    KnxError::InvalidGroupAddress(format!("Invalid middle group: {}", parts[1]))
                })?;
                let sub: u8 = parts[2].parse().map_err(|_| {
                    KnxError::InvalidGroupAddress(format!("Invalid sub group: {}", parts[2]))
                })?;
                Self::try_three_level(main, middle, sub)
            }
            // 2-level format: main/sub
            2 => {
                let main: u8 = parts[0].parse().map_err(|_| {
                    KnxError::InvalidGroupAddress(format!("Invalid main group: {}", parts[0]))
                })?;
                let sub: u16 = parts[1].parse().map_err(|_| {
                    KnxError::InvalidGroupAddress(format!("Invalid sub group: {}", parts[1]))
                })?;
                if main > Self::MAX_MAIN {
                    return Err(KnxError::AddressOutOfRange {
                        address: format!("main={}", main),
                        valid_range: "0-31".to_string(),
                    });
                }
                if sub > 0x07FF {
                    return Err(KnxError::AddressOutOfRange {
                        address: format!("sub={}", sub),
                        valid_range: "0-2047".to_string(),
                    });
                }
                Ok(Self::two_level(main, sub))
            }
            // Free format: raw value
            1 => {
                let raw: u16 = parts[0]
                    .parse()
                    .map_err(|_| KnxError::InvalidGroupAddress(s.to_string()))?;
                Ok(Self::from_raw(raw))
            }
            _ => Err(KnxError::InvalidGroupAddress(format!(
                "Invalid format: expected 'main/middle/sub', 'main/sub', or raw value, got '{}'",
                s
            ))),
        }
    }
}

impl From<u16> for GroupAddress {
    fn from(value: u16) -> Self {
        Self::from_raw(value)
    }
}

impl From<GroupAddress> for u16 {
    fn from(addr: GroupAddress) -> Self {
        addr.raw()
    }
}

// ============================================================================
// Address Type Enum
// ============================================================================

/// KNX address type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AddressType {
    /// Individual (physical) address.
    Individual,
    /// Group address.
    Group,
}

impl AddressType {
    /// Create from the address type bit in control byte.
    #[inline]
    pub fn from_bit(bit: bool) -> Self {
        if bit {
            Self::Group
        } else {
            Self::Individual
        }
    }

    /// Convert to address type bit.
    #[inline]
    pub fn to_bit(&self) -> bool {
        matches!(self, Self::Group)
    }
}

// ============================================================================
// Address Range
// ============================================================================

/// A range of group addresses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupAddressRange {
    start: GroupAddress,
    end: GroupAddress,
}

impl GroupAddressRange {
    /// Create a new address range.
    pub fn new(start: GroupAddress, end: GroupAddress) -> Self {
        Self { start, end }
    }

    /// Check if address is in range.
    pub fn contains(&self, addr: &GroupAddress) -> bool {
        addr.raw() >= self.start.raw() && addr.raw() <= self.end.raw()
    }

    /// Iterate over all addresses in range.
    pub fn iter(&self) -> impl Iterator<Item = GroupAddress> {
        (self.start.raw()..=self.end.raw()).map(GroupAddress::from_raw)
    }

    /// Get the number of addresses in range.
    pub fn len(&self) -> usize {
        (self.end.raw() - self.start.raw() + 1) as usize
    }

    /// Check if range is empty.
    pub fn is_empty(&self) -> bool {
        self.start.raw() > self.end.raw()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ========================================================================
    // Individual Address Tests
    // ========================================================================

    #[test]
    fn test_individual_address_new() {
        let addr = IndividualAddress::new(1, 2, 3);
        assert_eq!(addr.area(), 1);
        assert_eq!(addr.line(), 2);
        assert_eq!(addr.device(), 3);
    }

    #[test]
    fn test_individual_address_encode_decode() {
        let addr = IndividualAddress::new(15, 15, 255);
        let encoded = addr.encode();
        let decoded = IndividualAddress::decode(encoded);
        assert_eq!(addr, decoded);
    }

    #[test]
    fn test_individual_address_display() {
        let addr = IndividualAddress::new(1, 2, 3);
        assert_eq!(addr.to_string(), "1.2.3");
    }

    #[test]
    fn test_individual_address_parse() {
        let addr: IndividualAddress = "1.2.3".parse().unwrap();
        assert_eq!(addr.area(), 1);
        assert_eq!(addr.line(), 2);
        assert_eq!(addr.device(), 3);
    }

    #[test]
    fn test_individual_address_parse_invalid() {
        assert!("1.2".parse::<IndividualAddress>().is_err());
        assert!("1.2.3.4".parse::<IndividualAddress>().is_err());
        assert!("a.b.c".parse::<IndividualAddress>().is_err());
        assert!("16.0.0".parse::<IndividualAddress>().is_err()); // area > 15
    }

    #[test]
    fn test_individual_address_bytes() {
        let addr = IndividualAddress::new(1, 2, 3);
        let bytes = addr.to_bytes();
        let decoded = IndividualAddress::from_bytes(bytes);
        assert_eq!(addr, decoded);
    }

    // ========================================================================
    // Group Address Tests
    // ========================================================================

    #[test]
    fn test_group_address_three_level() {
        let addr = GroupAddress::three_level(1, 2, 3);
        let (main, middle, sub) = addr.as_three_level();
        assert_eq!(main, 1);
        assert_eq!(middle, 2);
        assert_eq!(sub, 3);
    }

    #[test]
    fn test_group_address_two_level() {
        let addr = GroupAddress::two_level(1, 515);
        let (main, sub) = addr.as_two_level();
        assert_eq!(main, 1);
        assert_eq!(sub, 515);
    }

    #[test]
    fn test_group_address_display() {
        let addr = GroupAddress::three_level(1, 2, 3);
        assert_eq!(addr.to_string(), "1/2/3");
    }

    #[test]
    fn test_group_address_parse_three_level() {
        let addr: GroupAddress = "1/2/3".parse().unwrap();
        assert_eq!(addr.main(), 1);
        assert_eq!(addr.middle(), 2);
        assert_eq!(addr.sub(), 3);
    }

    #[test]
    fn test_group_address_parse_two_level() {
        let addr: GroupAddress = "1/515".parse().unwrap();
        let (main, sub) = addr.as_two_level();
        assert_eq!(main, 1);
        assert_eq!(sub, 515);
    }

    #[test]
    fn test_group_address_parse_raw() {
        let addr: GroupAddress = "12345".parse().unwrap();
        assert_eq!(addr.raw(), 12345);
    }

    #[test]
    fn test_group_address_bytes() {
        let addr = GroupAddress::three_level(1, 2, 3);
        let bytes = addr.to_bytes();
        let decoded = GroupAddress::from_bytes(bytes);
        assert_eq!(addr, decoded);
    }

    // ========================================================================
    // Address Range Tests
    // ========================================================================

    #[test]
    fn test_address_range_contains() {
        let range = GroupAddressRange::new(
            GroupAddress::three_level(1, 0, 0),
            GroupAddress::three_level(1, 0, 255),
        );

        assert!(range.contains(&GroupAddress::three_level(1, 0, 100)));
        assert!(!range.contains(&GroupAddress::three_level(2, 0, 0)));
    }

    #[test]
    fn test_address_range_len() {
        let range = GroupAddressRange::new(
            GroupAddress::three_level(1, 0, 0),
            GroupAddress::three_level(1, 0, 9),
        );
        assert_eq!(range.len(), 10);
    }
}

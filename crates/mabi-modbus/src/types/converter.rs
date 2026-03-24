//! Type-safe register value converter.
//!
//! This module provides the `RegisterConverter` struct for converting between
//! Rust types and Modbus registers with configurable byte ordering.

use std::borrow::Cow;

use super::data_type::RegisterDataType;
use super::word_order::WordOrder;

/// Type-safe converter between Rust values and Modbus registers.
///
/// Handles byte ordering (word order) and provides convenient methods for
/// converting various data types to/from register arrays.
///
/// # Thread Safety
///
/// `RegisterConverter` is `Send + Sync` and can be shared across threads.
///
/// # Example
///
/// ```rust
/// use mabi_modbus::types::{RegisterConverter, WordOrder};
///
/// // Create converter with specific word order
/// let converter = RegisterConverter::new(WordOrder::BigEndianWordSwap);
///
/// // Convert f32 to registers
/// let value: f32 = 123.456;
/// let registers = converter.f32_to_registers(value);
///
/// // Convert back
/// let result = converter.registers_to_f32(&registers);
/// assert!((result - value).abs() < 0.001);
///
/// // Convert i32 with different endianness
/// let i_value: i32 = -12345;
/// let i_regs = converter.i32_to_registers(i_value);
/// let i_result = converter.registers_to_i32(&i_regs);
/// assert_eq!(i_result, i_value);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RegisterConverter {
    word_order: WordOrder,
}

impl Default for RegisterConverter {
    fn default() -> Self {
        Self::new(WordOrder::default())
    }
}

impl RegisterConverter {
    /// Create a new converter with the specified word order.
    pub fn new(word_order: WordOrder) -> Self {
        Self { word_order }
    }

    /// Create a converter with Big Endian word order.
    pub fn big_endian() -> Self {
        Self::new(WordOrder::BigEndian)
    }

    /// Create a converter with Little Endian word order.
    pub fn little_endian() -> Self {
        Self::new(WordOrder::LittleEndian)
    }

    /// Create a converter with Big Endian Word Swap order.
    pub fn big_endian_word_swap() -> Self {
        Self::new(WordOrder::BigEndianWordSwap)
    }

    /// Create a converter with Little Endian Word Swap order.
    pub fn little_endian_word_swap() -> Self {
        Self::new(WordOrder::LittleEndianWordSwap)
    }

    /// Get the current word order.
    pub fn word_order(&self) -> WordOrder {
        self.word_order
    }

    /// Create a new converter with a different word order.
    pub fn with_word_order(&self, word_order: WordOrder) -> Self {
        Self { word_order }
    }

    // ==========================================================================
    // Bool conversions (1 register)
    // ==========================================================================

    /// Convert a boolean to a single register.
    ///
    /// - `true` -> 0xFF00 (Modbus standard)
    /// - `false` -> 0x0000
    #[inline]
    pub fn bool_to_register(&self, value: bool) -> u16 {
        if value {
            0xFF00
        } else {
            0x0000
        }
    }

    /// Convert a register to a boolean.
    ///
    /// Any non-zero value is considered `true`.
    #[inline]
    pub fn register_to_bool(&self, register: u16) -> bool {
        register != 0
    }

    // ==========================================================================
    // Int16/UInt16 conversions (1 register)
    // ==========================================================================

    /// Convert an i16 to a single register.
    #[inline]
    pub fn i16_to_register(&self, value: i16) -> u16 {
        value as u16
    }

    /// Convert a register to i16.
    #[inline]
    pub fn register_to_i16(&self, register: u16) -> i16 {
        register as i16
    }

    /// Convert a u16 to a single register (identity).
    #[inline]
    pub fn u16_to_register(&self, value: u16) -> u16 {
        value
    }

    /// Convert a register to u16 (identity).
    #[inline]
    pub fn register_to_u16(&self, register: u16) -> u16 {
        register
    }

    // ==========================================================================
    // Int32/UInt32 conversions (2 registers)
    // ==========================================================================

    /// Convert an i32 to two registers.
    pub fn i32_to_registers(&self, value: i32) -> [u16; 2] {
        self.word_order.bytes_to_registers_32(value.to_be_bytes())
    }

    /// Convert two registers to i32.
    pub fn registers_to_i32(&self, registers: &[u16]) -> i32 {
        assert!(registers.len() >= 2, "Need at least 2 registers for i32");
        let bytes = self
            .word_order
            .registers_to_bytes_32([registers[0], registers[1]]);
        i32::from_be_bytes(bytes)
    }

    /// Convert a u32 to two registers.
    pub fn u32_to_registers(&self, value: u32) -> [u16; 2] {
        self.word_order.bytes_to_registers_32(value.to_be_bytes())
    }

    /// Convert two registers to u32.
    pub fn registers_to_u32(&self, registers: &[u16]) -> u32 {
        assert!(registers.len() >= 2, "Need at least 2 registers for u32");
        let bytes = self
            .word_order
            .registers_to_bytes_32([registers[0], registers[1]]);
        u32::from_be_bytes(bytes)
    }

    // ==========================================================================
    // Float32 conversions (2 registers)
    // ==========================================================================

    /// Convert an f32 to two registers.
    pub fn f32_to_registers(&self, value: f32) -> [u16; 2] {
        self.word_order.bytes_to_registers_32(value.to_be_bytes())
    }

    /// Convert two registers to f32.
    pub fn registers_to_f32(&self, registers: &[u16]) -> f32 {
        assert!(registers.len() >= 2, "Need at least 2 registers for f32");
        let bytes = self
            .word_order
            .registers_to_bytes_32([registers[0], registers[1]]);
        f32::from_be_bytes(bytes)
    }

    // ==========================================================================
    // Int64/UInt64 conversions (4 registers)
    // ==========================================================================

    /// Convert an i64 to four registers.
    pub fn i64_to_registers(&self, value: i64) -> [u16; 4] {
        self.word_order.bytes_to_registers_64(value.to_be_bytes())
    }

    /// Convert four registers to i64.
    pub fn registers_to_i64(&self, registers: &[u16]) -> i64 {
        assert!(registers.len() >= 4, "Need at least 4 registers for i64");
        let bytes = self.word_order.registers_to_bytes_64([
            registers[0],
            registers[1],
            registers[2],
            registers[3],
        ]);
        i64::from_be_bytes(bytes)
    }

    /// Convert a u64 to four registers.
    pub fn u64_to_registers(&self, value: u64) -> [u16; 4] {
        self.word_order.bytes_to_registers_64(value.to_be_bytes())
    }

    /// Convert four registers to u64.
    pub fn registers_to_u64(&self, registers: &[u16]) -> u64 {
        assert!(registers.len() >= 4, "Need at least 4 registers for u64");
        let bytes = self.word_order.registers_to_bytes_64([
            registers[0],
            registers[1],
            registers[2],
            registers[3],
        ]);
        u64::from_be_bytes(bytes)
    }

    // ==========================================================================
    // Float64 conversions (4 registers)
    // ==========================================================================

    /// Convert an f64 to four registers.
    pub fn f64_to_registers(&self, value: f64) -> [u16; 4] {
        self.word_order.bytes_to_registers_64(value.to_be_bytes())
    }

    /// Convert four registers to f64.
    pub fn registers_to_f64(&self, registers: &[u16]) -> f64 {
        assert!(registers.len() >= 4, "Need at least 4 registers for f64");
        let bytes = self.word_order.registers_to_bytes_64([
            registers[0],
            registers[1],
            registers[2],
            registers[3],
        ]);
        f64::from_be_bytes(bytes)
    }

    // ==========================================================================
    // String conversions (N registers)
    // ==========================================================================

    /// Convert a string to registers.
    ///
    /// Each register holds 2 bytes (characters). If the string is shorter
    /// than the register capacity, it will be null-padded.
    ///
    /// # Arguments
    ///
    /// * `value` - The string to convert
    /// * `register_count` - Number of registers to use (each holds 2 chars)
    ///
    /// # Returns
    ///
    /// A vector of registers containing the string bytes.
    pub fn string_to_registers(&self, value: &str, register_count: usize) -> Vec<u16> {
        let bytes = value.as_bytes();
        let mut result = Vec::with_capacity(register_count);

        for i in 0..register_count {
            let offset = i * 2;
            let hi = bytes.get(offset).copied().unwrap_or(0);
            let lo = bytes.get(offset + 1).copied().unwrap_or(0);
            result.push(u16::from_be_bytes([hi, lo]));
        }

        result
    }

    /// Convert registers to a string.
    ///
    /// Stops at the first null byte or end of registers.
    pub fn registers_to_string(&self, registers: &[u16]) -> String {
        let mut bytes = Vec::with_capacity(registers.len() * 2);

        for &reg in registers {
            let [hi, lo] = reg.to_be_bytes();
            if hi == 0 {
                break;
            }
            bytes.push(hi);
            if lo == 0 {
                break;
            }
            bytes.push(lo);
        }

        String::from_utf8_lossy(&bytes).into_owned()
    }

    /// Convert registers to a string, trimming trailing nulls and whitespace.
    pub fn registers_to_string_trimmed(&self, registers: &[u16]) -> String {
        self.registers_to_string(registers)
            .trim_end_matches(|c: char| c == '\0' || c.is_whitespace())
            .to_string()
    }

    // ==========================================================================
    // Bytes conversions (N registers)
    // ==========================================================================

    /// Convert bytes to registers.
    pub fn bytes_to_registers(&self, bytes: &[u8]) -> Vec<u16> {
        let mut result = Vec::with_capacity((bytes.len() + 1) / 2);

        for chunk in bytes.chunks(2) {
            let hi = chunk[0];
            let lo = chunk.get(1).copied().unwrap_or(0);
            result.push(u16::from_be_bytes([hi, lo]));
        }

        result
    }

    /// Convert registers to bytes.
    pub fn registers_to_bytes(&self, registers: &[u16]) -> Vec<u8> {
        let mut result = Vec::with_capacity(registers.len() * 2);

        for &reg in registers {
            let [hi, lo] = reg.to_be_bytes();
            result.push(hi);
            result.push(lo);
        }

        result
    }

    // ==========================================================================
    // Generic conversions using RegisterDataType
    // ==========================================================================

    /// Convert a typed value to registers.
    ///
    /// # Arguments
    ///
    /// * `data_type` - The data type of the value
    /// * `value` - The value as a byte slice (in native byte order)
    ///
    /// # Returns
    ///
    /// A vector of registers representing the value.
    pub fn value_to_registers(&self, data_type: RegisterDataType, value: &TypedValue) -> Vec<u16> {
        match value {
            TypedValue::Bool(v) => vec![self.bool_to_register(*v)],
            TypedValue::Int16(v) => vec![self.i16_to_register(*v)],
            TypedValue::UInt16(v) => vec![self.u16_to_register(*v)],
            TypedValue::Int32(v) => self.i32_to_registers(*v).to_vec(),
            TypedValue::UInt32(v) => self.u32_to_registers(*v).to_vec(),
            TypedValue::Float32(v) => self.f32_to_registers(*v).to_vec(),
            TypedValue::Int64(v) => self.i64_to_registers(*v).to_vec(),
            TypedValue::UInt64(v) => self.u64_to_registers(*v).to_vec(),
            TypedValue::Float64(v) => self.f64_to_registers(*v).to_vec(),
            TypedValue::String(v) => {
                let reg_count = data_type.register_count() as usize;
                self.string_to_registers(v, reg_count)
            }
            TypedValue::Bytes(v) => self.bytes_to_registers(v),
        }
    }

    /// Convert registers to a typed value.
    ///
    /// # Arguments
    ///
    /// * `data_type` - The expected data type
    /// * `registers` - The register values
    ///
    /// # Returns
    ///
    /// The typed value extracted from the registers.
    pub fn registers_to_value(
        &self,
        data_type: RegisterDataType,
        registers: &[u16],
    ) -> TypedValue<'_> {
        match data_type {
            RegisterDataType::Bool => TypedValue::Bool(self.register_to_bool(registers[0])),
            RegisterDataType::Int16 => TypedValue::Int16(self.register_to_i16(registers[0])),
            RegisterDataType::UInt16 => TypedValue::UInt16(self.register_to_u16(registers[0])),
            RegisterDataType::Int32 => TypedValue::Int32(self.registers_to_i32(registers)),
            RegisterDataType::UInt32 => TypedValue::UInt32(self.registers_to_u32(registers)),
            RegisterDataType::Float32 => TypedValue::Float32(self.registers_to_f32(registers)),
            RegisterDataType::Int64 => TypedValue::Int64(self.registers_to_i64(registers)),
            RegisterDataType::UInt64 => TypedValue::UInt64(self.registers_to_u64(registers)),
            RegisterDataType::Float64 => TypedValue::Float64(self.registers_to_f64(registers)),
            RegisterDataType::String(_) => {
                TypedValue::String(Cow::Owned(self.registers_to_string_trimmed(registers)))
            }
            RegisterDataType::Bytes(_) => {
                TypedValue::Bytes(Cow::Owned(self.registers_to_bytes(registers)))
            }
        }
    }
}

/// A typed value that can be converted to/from registers.
#[derive(Debug, Clone, PartialEq)]
pub enum TypedValue<'a> {
    Bool(bool),
    Int16(i16),
    UInt16(u16),
    Int32(i32),
    UInt32(u32),
    Float32(f32),
    Int64(i64),
    UInt64(u64),
    Float64(f64),
    String(Cow<'a, str>),
    Bytes(Cow<'a, [u8]>),
}

impl<'a> TypedValue<'a> {
    /// Get the data type of this value.
    pub fn data_type(&self) -> RegisterDataType {
        match self {
            Self::Bool(_) => RegisterDataType::Bool,
            Self::Int16(_) => RegisterDataType::Int16,
            Self::UInt16(_) => RegisterDataType::UInt16,
            Self::Int32(_) => RegisterDataType::Int32,
            Self::UInt32(_) => RegisterDataType::UInt32,
            Self::Float32(_) => RegisterDataType::Float32,
            Self::Int64(_) => RegisterDataType::Int64,
            Self::UInt64(_) => RegisterDataType::UInt64,
            Self::Float64(_) => RegisterDataType::Float64,
            Self::String(s) => RegisterDataType::String(((s.len() + 1) / 2) as u16),
            Self::Bytes(b) => RegisterDataType::Bytes(((b.len() + 1) / 2) as u16),
        }
    }

    /// Convert to an owned value (no lifetime bound).
    pub fn into_owned(self) -> TypedValue<'static> {
        match self {
            Self::Bool(v) => TypedValue::Bool(v),
            Self::Int16(v) => TypedValue::Int16(v),
            Self::UInt16(v) => TypedValue::UInt16(v),
            Self::Int32(v) => TypedValue::Int32(v),
            Self::UInt32(v) => TypedValue::UInt32(v),
            Self::Float32(v) => TypedValue::Float32(v),
            Self::Int64(v) => TypedValue::Int64(v),
            Self::UInt64(v) => TypedValue::UInt64(v),
            Self::Float64(v) => TypedValue::Float64(v),
            Self::String(s) => TypedValue::String(Cow::Owned(s.into_owned())),
            Self::Bytes(b) => TypedValue::Bytes(Cow::Owned(b.into_owned())),
        }
    }

    /// Try to get as f64 (for numeric types).
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Self::Bool(v) => Some(if *v { 1.0 } else { 0.0 }),
            Self::Int16(v) => Some(*v as f64),
            Self::UInt16(v) => Some(*v as f64),
            Self::Int32(v) => Some(*v as f64),
            Self::UInt32(v) => Some(*v as f64),
            Self::Float32(v) => Some(*v as f64),
            Self::Int64(v) => Some(*v as f64),
            Self::UInt64(v) => Some(*v as f64),
            Self::Float64(v) => Some(*v),
            Self::String(_) | Self::Bytes(_) => None,
        }
    }
}

impl From<bool> for TypedValue<'static> {
    fn from(v: bool) -> Self {
        Self::Bool(v)
    }
}

impl From<i16> for TypedValue<'static> {
    fn from(v: i16) -> Self {
        Self::Int16(v)
    }
}

impl From<u16> for TypedValue<'static> {
    fn from(v: u16) -> Self {
        Self::UInt16(v)
    }
}

impl From<i32> for TypedValue<'static> {
    fn from(v: i32) -> Self {
        Self::Int32(v)
    }
}

impl From<u32> for TypedValue<'static> {
    fn from(v: u32) -> Self {
        Self::UInt32(v)
    }
}

impl From<f32> for TypedValue<'static> {
    fn from(v: f32) -> Self {
        Self::Float32(v)
    }
}

impl From<i64> for TypedValue<'static> {
    fn from(v: i64) -> Self {
        Self::Int64(v)
    }
}

impl From<u64> for TypedValue<'static> {
    fn from(v: u64) -> Self {
        Self::UInt64(v)
    }
}

impl From<f64> for TypedValue<'static> {
    fn from(v: f64) -> Self {
        Self::Float64(v)
    }
}

impl From<String> for TypedValue<'static> {
    fn from(v: String) -> Self {
        Self::String(Cow::Owned(v))
    }
}

impl<'a> From<&'a str> for TypedValue<'a> {
    fn from(v: &'a str) -> Self {
        Self::String(Cow::Borrowed(v))
    }
}

impl From<Vec<u8>> for TypedValue<'static> {
    fn from(v: Vec<u8>) -> Self {
        Self::Bytes(Cow::Owned(v))
    }
}

impl<'a> From<&'a [u8]> for TypedValue<'a> {
    fn from(v: &'a [u8]) -> Self {
        Self::Bytes(Cow::Borrowed(v))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bool_conversion() {
        let conv = RegisterConverter::default();

        assert_eq!(conv.bool_to_register(true), 0xFF00);
        assert_eq!(conv.bool_to_register(false), 0x0000);

        assert!(conv.register_to_bool(0xFF00));
        assert!(conv.register_to_bool(0x0001));
        assert!(!conv.register_to_bool(0x0000));
    }

    #[test]
    fn test_i16_conversion() {
        let conv = RegisterConverter::default();

        assert_eq!(conv.i16_to_register(-1), 0xFFFF);
        assert_eq!(conv.i16_to_register(32767), 0x7FFF);

        assert_eq!(conv.register_to_i16(0xFFFF), -1);
        assert_eq!(conv.register_to_i16(0x7FFF), 32767);
    }

    #[test]
    fn test_i32_big_endian() {
        let conv = RegisterConverter::big_endian();
        let value: i32 = 0x12345678;

        let regs = conv.i32_to_registers(value);
        assert_eq!(regs, [0x1234, 0x5678]);

        let result = conv.registers_to_i32(&regs);
        assert_eq!(result, value);
    }

    #[test]
    fn test_i32_big_endian_word_swap() {
        let conv = RegisterConverter::big_endian_word_swap();
        let value: i32 = 0x12345678;

        let regs = conv.i32_to_registers(value);
        assert_eq!(regs, [0x5678, 0x1234]); // Words swapped

        let result = conv.registers_to_i32(&regs);
        assert_eq!(result, value);
    }

    #[test]
    fn test_f32_round_trip() {
        for order in WordOrder::all() {
            let conv = RegisterConverter::new(*order);
            let value: f32 = 123.456;

            let regs = conv.f32_to_registers(value);
            let result = conv.registers_to_f32(&regs);

            assert!(
                (result - value).abs() < 0.0001,
                "f32 round-trip failed for {:?}",
                order
            );
        }
    }

    #[test]
    fn test_f64_round_trip() {
        for order in WordOrder::all() {
            let conv = RegisterConverter::new(*order);
            let value: f64 = 123456.789012345;

            let regs = conv.f64_to_registers(value);
            let result = conv.registers_to_f64(&regs);

            assert!(
                (result - value).abs() < 0.000001,
                "f64 round-trip failed for {:?}",
                order
            );
        }
    }

    #[test]
    fn test_negative_values() {
        let conv = RegisterConverter::big_endian();

        // Negative i32
        let value: i32 = -12345;
        let regs = conv.i32_to_registers(value);
        let result = conv.registers_to_i32(&regs);
        assert_eq!(result, value);

        // Negative i64
        let value64: i64 = -9876543210;
        let regs64 = conv.i64_to_registers(value64);
        let result64 = conv.registers_to_i64(&regs64);
        assert_eq!(result64, value64);
    }

    #[test]
    fn test_string_conversion() {
        let conv = RegisterConverter::default();

        let value = "Hello, World!";
        let regs = conv.string_to_registers(value, 8); // 16 chars max
        assert_eq!(regs.len(), 8);

        let result = conv.registers_to_string_trimmed(&regs);
        assert_eq!(result, value);
    }

    #[test]
    fn test_bytes_conversion() {
        let conv = RegisterConverter::default();

        let value = vec![0x01, 0x02, 0x03, 0x04, 0x05];
        let regs = conv.bytes_to_registers(&value);
        assert_eq!(regs.len(), 3); // 5 bytes = 3 registers

        let result = conv.registers_to_bytes(&regs);
        // Result will be padded with 0 to even length
        assert_eq!(&result[..5], &value[..]);
    }

    #[test]
    fn test_typed_value_conversion() {
        let conv = RegisterConverter::big_endian();

        // Float32
        let value = TypedValue::Float32(123.456);
        let regs = conv.value_to_registers(RegisterDataType::Float32, &value);
        let result = conv.registers_to_value(RegisterDataType::Float32, &regs);

        if let TypedValue::Float32(v) = result {
            assert!((v - 123.456).abs() < 0.001);
        } else {
            panic!("Expected Float32");
        }
    }

    #[test]
    fn test_typed_value_data_type() {
        assert_eq!(TypedValue::Bool(true).data_type(), RegisterDataType::Bool);
        assert_eq!(
            TypedValue::Float32(1.0).data_type(),
            RegisterDataType::Float32
        );
        assert_eq!(
            TypedValue::String(Cow::Borrowed("test")).data_type(),
            RegisterDataType::String(2)
        );
    }
}

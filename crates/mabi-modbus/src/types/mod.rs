//! Data types and byte order support for Modbus register conversions.
//!
//! This module provides:
//! - `RegisterDataType`: Supported data types for Modbus registers
//! - `WordOrder`: Byte ordering configurations for multi-register values
//! - `RegisterConverter`: Type-safe conversions between values and registers
//!
//! # Architecture
//!
//! ```text
//! ┌────────────────────────────────────────────────────────────────────────┐
//! │                        Data Type System                                 │
//! │  ┌──────────────────────────────────────────────────────────────────┐  │
//! │  │                    RegisterDataType                               │  │
//! │  │  Int16 │ UInt16 │ Int32 │ UInt32 │ Float32 │ Float64 │ String    │  │
//! │  └──────────────────────────────────────────────────────────────────┘  │
//! │                                                                         │
//! │  ┌──────────────────────────────────────────────────────────────────┐  │
//! │  │                      WordOrder                                    │  │
//! │  │  BigEndian    │ LittleEndian │ BigEndianSwap │ LittleEndianSwap  │  │
//! │  │  (AB CD)      │ (DC BA)      │ (CD AB)       │ (BA DC)           │  │
//! │  └──────────────────────────────────────────────────────────────────┘  │
//! │                                                                         │
//! │  ┌──────────────────────────────────────────────────────────────────┐  │
//! │  │                   RegisterConverter                               │  │
//! │  │  registers_to_f32() │ f32_to_registers() │ registers_to_i32()   │  │
//! │  │  i32_to_registers() │ registers_to_f64() │ registers_to_string() │  │
//! │  └──────────────────────────────────────────────────────────────────┘  │
//! └────────────────────────────────────────────────────────────────────────┘
//! ```
//!
//! # Example
//!
//! ```rust
//! use mabi_modbus::types::{RegisterConverter, WordOrder, RegisterDataType};
//!
//! // Create converter with Big Endian byte order
//! let converter = RegisterConverter::new(WordOrder::BigEndian);
//!
//! // Convert f32 to registers
//! let value: f32 = 123.456;
//! let registers = converter.f32_to_registers(value);
//! assert_eq!(registers.len(), 2);
//!
//! // Convert back to f32
//! let result = converter.registers_to_f32(&registers);
//! assert!((result - value).abs() < 0.001);
//!
//! // Use different word order (common in some PLC systems)
//! let swapped = RegisterConverter::new(WordOrder::BigEndianWordSwap);
//! let swapped_regs = swapped.f32_to_registers(value);
//! // Note: swapped_regs will have words in different order
//! ```

mod converter;
mod data_type;
mod word_order;

pub use converter::{RegisterConverter, TypedValue};
pub use data_type::RegisterDataType;
pub use word_order::WordOrder;

#[cfg(test)]
mod tests;

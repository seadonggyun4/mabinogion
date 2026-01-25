//! KNX Datapoint Types (DPT).
//!
//! This module provides a trait-based, extensible system for KNX Datapoint Types.
//! New DPT types can be easily added by implementing the [`DptCodec`] trait.
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │                     DptCodec Trait                          │
//! │   encode(&DptValue) -> Vec<u8>                              │
//! │   decode(&[u8]) -> DptValue                                 │
//! │   size() -> usize                                           │
//! │   id() -> DptId                                             │
//! └─────────────────────────────────────────────────────────────┘
//!                              │
//!          ┌──────────────────┼──────────────────┐
//!          │                  │                  │
//!          ▼                  ▼                  ▼
//!   ┌────────────┐    ┌────────────┐    ┌────────────┐
//!   │  Dpt1Bool  │    │  Dpt9Float │    │  Custom    │
//!   │  (1-bit)   │    │  (2-byte)  │    │  DPT       │
//!   └────────────┘    └────────────┘    └────────────┘
//! ```
//!
//! ## Example
//!
//! ```rust,ignore
//! use mabi_knx::dpt::{DptCodec, Dpt1Switch, DptValue};
//!
//! let codec = Dpt1Switch;
//! let value = DptValue::Bool(true);
//! let encoded = codec.encode(&value).unwrap();
//! let decoded = codec.decode(&encoded).unwrap();
//! ```

mod codec;
mod registry;
mod types;
mod value;

pub use codec::{DptCodec, DptId, BoxedDptCodec};
pub use registry::DptRegistry;
pub use types::*;
pub use value::DptValue;

/// Standard KNX 16-bit float encoding.
pub fn encode_dpt9(value: f32) -> u16 {
    if value == 0.0 {
        return 0;
    }

    let mut exp: i32 = 0;
    let mut mantissa = (value * 100.0) as i32;

    // Normalize mantissa to fit in 11 bits (signed: -2048 to 2047)
    while mantissa.abs() > 2047 && exp < 15 {
        mantissa /= 2;
        exp += 1;
    }

    let sign = if mantissa < 0 { 1u16 } else { 0u16 };
    let mantissa = if sign == 1 {
        (mantissa + 2048) as u16
    } else {
        mantissa as u16
    };

    (sign << 15) | ((exp as u16 & 0x0F) << 11) | (mantissa & 0x07FF)
}

/// Standard KNX 16-bit float decoding.
pub fn decode_dpt9(raw: u16) -> f32 {
    let sign = (raw >> 15) & 0x01;
    let exp = ((raw >> 11) & 0x0F) as i32;
    let mantissa = (raw & 0x07FF) as i32;

    let mantissa = if sign == 1 {
        mantissa - 2048
    } else {
        mantissa
    };

    0.01 * (mantissa as f32) * 2.0_f32.powi(exp)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dpt9_encode_decode() {
        let values = [0.0, 1.0, -1.0, 25.5, -40.0, 100.0, 670760.0];

        for &original in &values {
            let encoded = encode_dpt9(original);
            let decoded = decode_dpt9(encoded);
            // DPT9 has limited precision
            let tolerance = original.abs() * 0.02 + 0.01;
            assert!(
                (decoded - original).abs() < tolerance,
                "Failed for {}: got {}",
                original,
                decoded
            );
        }
    }

    #[test]
    fn test_dpt9_zero() {
        assert_eq!(encode_dpt9(0.0), 0);
        assert_eq!(decode_dpt9(0), 0.0);
    }
}

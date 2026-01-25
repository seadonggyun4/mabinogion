//! Standard DPT type implementations.
//!
//! This module provides implementations for common KNX DPT types.

use super::codec::{validate_length, DptCodec, DptId};
use super::value::DptValue;
use super::{decode_dpt9, encode_dpt9};
use crate::error::{KnxError, KnxResult};

// ============================================================================
// DPT 1: Boolean (1 bit)
// ============================================================================

/// DPT 1.001 - Switch (Off/On).
#[derive(Debug, Clone, Copy, Default)]
pub struct Dpt1Switch;

impl Dpt1Switch {
    fn encode_impl(&self, value: &DptValue) -> KnxResult<Vec<u8>> {
        match value {
            DptValue::Bool(v) => Ok(vec![if *v { 1 } else { 0 }]),
            DptValue::U8(v) => Ok(vec![if *v != 0 { 1 } else { 0 }]),
            _ => Err(KnxError::dpt_encoding("DPT 1.001", "Expected boolean value")),
        }
    }

    fn decode_impl(&self, data: &[u8]) -> KnxResult<DptValue> {
        // DPT 1 uses only the LSB, even if data comes from APCI small data
        let byte = data.first().copied().unwrap_or(0);
        Ok(DptValue::Bool(byte & 0x01 != 0))
    }
}

impl DptCodec for Dpt1Switch {
    fn id(&self) -> DptId {
        DptId::new(1, 1)
    }

    fn name(&self) -> &'static str {
        "Switch"
    }

    fn size(&self) -> usize {
        0 // Uses small data (≤6 bits in APCI)
    }

    fn encode(&self, value: &DptValue) -> KnxResult<Vec<u8>> {
        self.encode_impl(value)
    }

    fn decode(&self, data: &[u8]) -> KnxResult<DptValue> {
        self.decode_impl(data)
    }

    fn is_small(&self) -> bool {
        true
    }

    fn default_value(&self) -> DptValue {
        DptValue::Bool(false)
    }
}

/// DPT 1.002 - Bool (False/True).
#[derive(Debug, Clone, Copy, Default)]
pub struct Dpt1Bool;

impl DptCodec for Dpt1Bool {
    fn id(&self) -> DptId {
        DptId::new(1, 2)
    }

    fn name(&self) -> &'static str {
        "Bool"
    }

    fn size(&self) -> usize {
        0
    }

    fn encode(&self, value: &DptValue) -> KnxResult<Vec<u8>> {
        Dpt1Switch.encode(value)
    }

    fn decode(&self, data: &[u8]) -> KnxResult<DptValue> {
        Dpt1Switch.decode(data)
    }

    fn is_small(&self) -> bool {
        true
    }

    fn default_value(&self) -> DptValue {
        DptValue::Bool(false)
    }
}

/// DPT 1.008 - Up/Down.
#[derive(Debug, Clone, Copy, Default)]
pub struct Dpt1UpDown;

impl DptCodec for Dpt1UpDown {
    fn id(&self) -> DptId {
        DptId::new(1, 8)
    }

    fn name(&self) -> &'static str {
        "Up/Down"
    }

    fn size(&self) -> usize {
        0
    }

    fn encode(&self, value: &DptValue) -> KnxResult<Vec<u8>> {
        Dpt1Switch.encode(value)
    }

    fn decode(&self, data: &[u8]) -> KnxResult<DptValue> {
        Dpt1Switch.decode(data)
    }

    fn is_small(&self) -> bool {
        true
    }

    fn description(&self) -> &'static str {
        "0=Up, 1=Down"
    }
}

// ============================================================================
// DPT 2: Priority Control (2 bits)
// ============================================================================

/// DPT 2.001 - Switch Control.
#[derive(Debug, Clone, Copy, Default)]
pub struct Dpt2SwitchControl;

impl DptCodec for Dpt2SwitchControl {
    fn id(&self) -> DptId {
        DptId::new(2, 1)
    }

    fn name(&self) -> &'static str {
        "Switch Control"
    }

    fn size(&self) -> usize {
        0
    }

    fn encode(&self, value: &DptValue) -> KnxResult<Vec<u8>> {
        match value {
            DptValue::PriorityControl { control, value } => {
                let byte = (if *control { 0x02 } else { 0 }) | (if *value { 0x01 } else { 0 });
                Ok(vec![byte])
            }
            _ => Err(KnxError::dpt_encoding(
                "DPT 2.001",
                "Expected PriorityControl value",
            )),
        }
    }

    fn decode(&self, data: &[u8]) -> KnxResult<DptValue> {
        let byte = data.first().copied().unwrap_or(0);
        Ok(DptValue::PriorityControl {
            control: byte & 0x02 != 0,
            value: byte & 0x01 != 0,
        })
    }

    fn is_small(&self) -> bool {
        true
    }
}

// ============================================================================
// DPT 3: Dimming Control (4 bits)
// ============================================================================

/// DPT 3.007 - Dimming Control.
#[derive(Debug, Clone, Copy, Default)]
pub struct Dpt3DimmingControl;

impl DptCodec for Dpt3DimmingControl {
    fn id(&self) -> DptId {
        DptId::new(3, 7)
    }

    fn name(&self) -> &'static str {
        "Dimming Control"
    }

    fn size(&self) -> usize {
        0
    }

    fn encode(&self, value: &DptValue) -> KnxResult<Vec<u8>> {
        match value {
            DptValue::DimmingControl {
                direction,
                step_code,
            } => {
                let byte = (if *direction { 0x08 } else { 0 }) | (*step_code & 0x07);
                Ok(vec![byte])
            }
            _ => Err(KnxError::dpt_encoding(
                "DPT 3.007",
                "Expected DimmingControl value",
            )),
        }
    }

    fn decode(&self, data: &[u8]) -> KnxResult<DptValue> {
        let byte = data.first().copied().unwrap_or(0);
        Ok(DptValue::DimmingControl {
            direction: byte & 0x08 != 0,
            step_code: byte & 0x07,
        })
    }

    fn is_small(&self) -> bool {
        true
    }

    fn description(&self) -> &'static str {
        "Direction: 0=decrease, 1=increase; StepCode: 0=break, 1-7=steps"
    }
}

/// DPT 3.008 - Blinds Control.
#[derive(Debug, Clone, Copy, Default)]
pub struct Dpt3BlindsControl;

impl DptCodec for Dpt3BlindsControl {
    fn id(&self) -> DptId {
        DptId::new(3, 8)
    }

    fn name(&self) -> &'static str {
        "Blinds Control"
    }

    fn size(&self) -> usize {
        0
    }

    fn encode(&self, value: &DptValue) -> KnxResult<Vec<u8>> {
        match value {
            DptValue::BlindsControl {
                direction,
                step_code,
            } => {
                let byte = (if *direction { 0x08 } else { 0 }) | (*step_code & 0x07);
                Ok(vec![byte])
            }
            _ => Err(KnxError::dpt_encoding(
                "DPT 3.008",
                "Expected BlindsControl value",
            )),
        }
    }

    fn decode(&self, data: &[u8]) -> KnxResult<DptValue> {
        let byte = data.first().copied().unwrap_or(0);
        Ok(DptValue::BlindsControl {
            direction: byte & 0x08 != 0,
            step_code: byte & 0x07,
        })
    }

    fn is_small(&self) -> bool {
        true
    }
}

// ============================================================================
// DPT 5: Unsigned 8-bit (1 byte)
// ============================================================================

/// DPT 5.001 - Percentage (0-100%).
#[derive(Debug, Clone, Copy, Default)]
pub struct Dpt5Scaling;

impl DptCodec for Dpt5Scaling {
    fn id(&self) -> DptId {
        DptId::new(5, 1)
    }

    fn name(&self) -> &'static str {
        "Scaling"
    }

    fn size(&self) -> usize {
        1
    }

    fn encode(&self, value: &DptValue) -> KnxResult<Vec<u8>> {
        match value {
            DptValue::U8(v) => Ok(vec![*v]),
            DptValue::F16(v) | DptValue::F32(v) => {
                // Convert 0-100% to 0-255
                let scaled = ((*v).clamp(0.0, 100.0) * 2.55) as u8;
                Ok(vec![scaled])
            }
            _ => Err(KnxError::dpt_encoding("DPT 5.001", "Expected numeric value")),
        }
    }

    fn decode(&self, data: &[u8]) -> KnxResult<DptValue> {
        validate_length(data, 1, "DPT 5.001")?;
        // Return as percentage (0-100)
        let percent = (data[0] as f32) / 2.55;
        Ok(DptValue::F16(percent))
    }

    fn unit(&self) -> Option<&'static str> {
        Some("%")
    }

    fn min_value(&self) -> Option<f64> {
        Some(0.0)
    }

    fn max_value(&self) -> Option<f64> {
        Some(100.0)
    }

    fn default_value(&self) -> DptValue {
        DptValue::F16(0.0)
    }
}

/// DPT 5.003 - Angle (0-360°).
#[derive(Debug, Clone, Copy, Default)]
pub struct Dpt5Angle;

impl DptCodec for Dpt5Angle {
    fn id(&self) -> DptId {
        DptId::new(5, 3)
    }

    fn name(&self) -> &'static str {
        "Angle"
    }

    fn size(&self) -> usize {
        1
    }

    fn encode(&self, value: &DptValue) -> KnxResult<Vec<u8>> {
        match value {
            DptValue::U8(v) => Ok(vec![*v]),
            DptValue::F16(v) | DptValue::F32(v) => {
                // Convert 0-360° to 0-255
                let scaled = ((*v).clamp(0.0, 360.0) * (255.0 / 360.0)) as u8;
                Ok(vec![scaled])
            }
            _ => Err(KnxError::dpt_encoding("DPT 5.003", "Expected numeric value")),
        }
    }

    fn decode(&self, data: &[u8]) -> KnxResult<DptValue> {
        validate_length(data, 1, "DPT 5.003")?;
        let angle = (data[0] as f32) * (360.0 / 255.0);
        Ok(DptValue::F16(angle))
    }

    fn unit(&self) -> Option<&'static str> {
        Some("°")
    }

    fn min_value(&self) -> Option<f64> {
        Some(0.0)
    }

    fn max_value(&self) -> Option<f64> {
        Some(360.0)
    }
}

/// DPT 5.010 - Counter (0-255).
#[derive(Debug, Clone, Copy, Default)]
pub struct Dpt5Counter;

impl DptCodec for Dpt5Counter {
    fn id(&self) -> DptId {
        DptId::new(5, 10)
    }

    fn name(&self) -> &'static str {
        "Counter Pulses"
    }

    fn size(&self) -> usize {
        1
    }

    fn encode(&self, value: &DptValue) -> KnxResult<Vec<u8>> {
        match value {
            DptValue::U8(v) => Ok(vec![*v]),
            _ => Err(KnxError::dpt_encoding("DPT 5.010", "Expected U8 value")),
        }
    }

    fn decode(&self, data: &[u8]) -> KnxResult<DptValue> {
        validate_length(data, 1, "DPT 5.010")?;
        Ok(DptValue::U8(data[0]))
    }

    fn min_value(&self) -> Option<f64> {
        Some(0.0)
    }

    fn max_value(&self) -> Option<f64> {
        Some(255.0)
    }
}

// ============================================================================
// DPT 6: Signed 8-bit (1 byte)
// ============================================================================

/// DPT 6.001 - Percentage (-128..127%).
#[derive(Debug, Clone, Copy, Default)]
pub struct Dpt6Percent;

impl DptCodec for Dpt6Percent {
    fn id(&self) -> DptId {
        DptId::new(6, 1)
    }

    fn name(&self) -> &'static str {
        "Percent (Signed)"
    }

    fn size(&self) -> usize {
        1
    }

    fn encode(&self, value: &DptValue) -> KnxResult<Vec<u8>> {
        match value {
            DptValue::I8(v) => Ok(vec![*v as u8]),
            _ => Err(KnxError::dpt_encoding("DPT 6.001", "Expected I8 value")),
        }
    }

    fn decode(&self, data: &[u8]) -> KnxResult<DptValue> {
        validate_length(data, 1, "DPT 6.001")?;
        Ok(DptValue::I8(data[0] as i8))
    }

    fn unit(&self) -> Option<&'static str> {
        Some("%")
    }

    fn min_value(&self) -> Option<f64> {
        Some(-128.0)
    }

    fn max_value(&self) -> Option<f64> {
        Some(127.0)
    }
}

// ============================================================================
// DPT 7: Unsigned 16-bit (2 bytes)
// ============================================================================

/// DPT 7.001 - Pulses.
#[derive(Debug, Clone, Copy, Default)]
pub struct Dpt7Pulses;

impl DptCodec for Dpt7Pulses {
    fn id(&self) -> DptId {
        DptId::new(7, 1)
    }

    fn name(&self) -> &'static str {
        "Pulses"
    }

    fn size(&self) -> usize {
        2
    }

    fn encode(&self, value: &DptValue) -> KnxResult<Vec<u8>> {
        match value {
            DptValue::U16(v) => Ok(v.to_be_bytes().to_vec()),
            _ => Err(KnxError::dpt_encoding("DPT 7.001", "Expected U16 value")),
        }
    }

    fn decode(&self, data: &[u8]) -> KnxResult<DptValue> {
        validate_length(data, 2, "DPT 7.001")?;
        let value = u16::from_be_bytes([data[0], data[1]]);
        Ok(DptValue::U16(value))
    }

    fn min_value(&self) -> Option<f64> {
        Some(0.0)
    }

    fn max_value(&self) -> Option<f64> {
        Some(65535.0)
    }
}

// ============================================================================
// DPT 8: Signed 16-bit (2 bytes)
// ============================================================================

/// DPT 8.001 - Pulses Difference.
#[derive(Debug, Clone, Copy, Default)]
pub struct Dpt8PulsesDiff;

impl DptCodec for Dpt8PulsesDiff {
    fn id(&self) -> DptId {
        DptId::new(8, 1)
    }

    fn name(&self) -> &'static str {
        "Pulses Difference"
    }

    fn size(&self) -> usize {
        2
    }

    fn encode(&self, value: &DptValue) -> KnxResult<Vec<u8>> {
        match value {
            DptValue::I16(v) => Ok(v.to_be_bytes().to_vec()),
            _ => Err(KnxError::dpt_encoding("DPT 8.001", "Expected I16 value")),
        }
    }

    fn decode(&self, data: &[u8]) -> KnxResult<DptValue> {
        validate_length(data, 2, "DPT 8.001")?;
        let value = i16::from_be_bytes([data[0], data[1]]);
        Ok(DptValue::I16(value))
    }

    fn min_value(&self) -> Option<f64> {
        Some(-32768.0)
    }

    fn max_value(&self) -> Option<f64> {
        Some(32767.0)
    }
}

// ============================================================================
// DPT 9: 16-bit Float (2 bytes)
// ============================================================================

/// DPT 9.001 - Temperature (°C).
#[derive(Debug, Clone, Copy, Default)]
pub struct Dpt9Temperature;

impl DptCodec for Dpt9Temperature {
    fn id(&self) -> DptId {
        DptId::new(9, 1)
    }

    fn name(&self) -> &'static str {
        "Temperature"
    }

    fn size(&self) -> usize {
        2
    }

    fn encode(&self, value: &DptValue) -> KnxResult<Vec<u8>> {
        let float_val = match value {
            DptValue::F16(v) | DptValue::F32(v) => *v,
            DptValue::I16(v) => *v as f32,
            DptValue::U16(v) => *v as f32,
            _ => {
                return Err(KnxError::dpt_encoding(
                    "DPT 9.001",
                    "Expected numeric value",
                ))
            }
        };
        Ok(encode_dpt9(float_val).to_be_bytes().to_vec())
    }

    fn decode(&self, data: &[u8]) -> KnxResult<DptValue> {
        validate_length(data, 2, "DPT 9.001")?;
        let raw = u16::from_be_bytes([data[0], data[1]]);
        Ok(DptValue::F16(decode_dpt9(raw)))
    }

    fn unit(&self) -> Option<&'static str> {
        Some("°C")
    }

    fn min_value(&self) -> Option<f64> {
        Some(-273.0)
    }

    fn max_value(&self) -> Option<f64> {
        Some(670760.0)
    }

    fn default_value(&self) -> DptValue {
        DptValue::F16(20.0)
    }
}

/// DPT 9.004 - Lux.
#[derive(Debug, Clone, Copy, Default)]
pub struct Dpt9Lux;

impl DptCodec for Dpt9Lux {
    fn id(&self) -> DptId {
        DptId::new(9, 4)
    }

    fn name(&self) -> &'static str {
        "Lux"
    }

    fn size(&self) -> usize {
        2
    }

    fn encode(&self, value: &DptValue) -> KnxResult<Vec<u8>> {
        Dpt9Temperature.encode(value)
    }

    fn decode(&self, data: &[u8]) -> KnxResult<DptValue> {
        Dpt9Temperature.decode(data)
    }

    fn unit(&self) -> Option<&'static str> {
        Some("lux")
    }

    fn min_value(&self) -> Option<f64> {
        Some(0.0)
    }

    fn max_value(&self) -> Option<f64> {
        Some(670760.0)
    }
}

/// DPT 9.007 - Humidity (%).
#[derive(Debug, Clone, Copy, Default)]
pub struct Dpt9Humidity;

impl DptCodec for Dpt9Humidity {
    fn id(&self) -> DptId {
        DptId::new(9, 7)
    }

    fn name(&self) -> &'static str {
        "Humidity"
    }

    fn size(&self) -> usize {
        2
    }

    fn encode(&self, value: &DptValue) -> KnxResult<Vec<u8>> {
        Dpt9Temperature.encode(value)
    }

    fn decode(&self, data: &[u8]) -> KnxResult<DptValue> {
        Dpt9Temperature.decode(data)
    }

    fn unit(&self) -> Option<&'static str> {
        Some("%")
    }

    fn min_value(&self) -> Option<f64> {
        Some(0.0)
    }

    fn max_value(&self) -> Option<f64> {
        Some(100.0)
    }
}

// ============================================================================
// DPT 12: Unsigned 32-bit (4 bytes)
// ============================================================================

/// DPT 12.001 - Counter Value.
#[derive(Debug, Clone, Copy, Default)]
pub struct Dpt12Counter;

impl DptCodec for Dpt12Counter {
    fn id(&self) -> DptId {
        DptId::new(12, 1)
    }

    fn name(&self) -> &'static str {
        "Counter Value"
    }

    fn size(&self) -> usize {
        4
    }

    fn encode(&self, value: &DptValue) -> KnxResult<Vec<u8>> {
        match value {
            DptValue::U32(v) => Ok(v.to_be_bytes().to_vec()),
            _ => Err(KnxError::dpt_encoding("DPT 12.001", "Expected U32 value")),
        }
    }

    fn decode(&self, data: &[u8]) -> KnxResult<DptValue> {
        validate_length(data, 4, "DPT 12.001")?;
        let value = u32::from_be_bytes([data[0], data[1], data[2], data[3]]);
        Ok(DptValue::U32(value))
    }
}

// ============================================================================
// DPT 13: Signed 32-bit (4 bytes)
// ============================================================================

/// DPT 13.001 - Counter Value Signed.
#[derive(Debug, Clone, Copy, Default)]
pub struct Dpt13Counter;

impl DptCodec for Dpt13Counter {
    fn id(&self) -> DptId {
        DptId::new(13, 1)
    }

    fn name(&self) -> &'static str {
        "Counter Value (Signed)"
    }

    fn size(&self) -> usize {
        4
    }

    fn encode(&self, value: &DptValue) -> KnxResult<Vec<u8>> {
        match value {
            DptValue::I32(v) => Ok(v.to_be_bytes().to_vec()),
            _ => Err(KnxError::dpt_encoding("DPT 13.001", "Expected I32 value")),
        }
    }

    fn decode(&self, data: &[u8]) -> KnxResult<DptValue> {
        validate_length(data, 4, "DPT 13.001")?;
        let value = i32::from_be_bytes([data[0], data[1], data[2], data[3]]);
        Ok(DptValue::I32(value))
    }
}

// ============================================================================
// DPT 14: 32-bit Float (4 bytes)
// ============================================================================

/// DPT 14.* - IEEE 754 Float.
#[derive(Debug, Clone, Copy, Default)]
pub struct Dpt14Float;

impl DptCodec for Dpt14Float {
    fn id(&self) -> DptId {
        DptId::main_only(14)
    }

    fn name(&self) -> &'static str {
        "Float (32-bit)"
    }

    fn size(&self) -> usize {
        4
    }

    fn encode(&self, value: &DptValue) -> KnxResult<Vec<u8>> {
        match value {
            DptValue::F32(v) => Ok(v.to_be_bytes().to_vec()),
            DptValue::F16(v) => Ok((*v).to_be_bytes().to_vec()),
            _ => Err(KnxError::dpt_encoding("DPT 14", "Expected float value")),
        }
    }

    fn decode(&self, data: &[u8]) -> KnxResult<DptValue> {
        validate_length(data, 4, "DPT 14")?;
        let value = f32::from_be_bytes([data[0], data[1], data[2], data[3]]);
        Ok(DptValue::F32(value))
    }
}

// ============================================================================
// DPT 16: String (14 bytes)
// ============================================================================

/// DPT 16.001 - ASCII String (14 characters).
#[derive(Debug, Clone, Copy, Default)]
pub struct Dpt16String;

impl DptCodec for Dpt16String {
    fn id(&self) -> DptId {
        DptId::new(16, 1)
    }

    fn name(&self) -> &'static str {
        "String (ASCII)"
    }

    fn size(&self) -> usize {
        14
    }

    fn encode(&self, value: &DptValue) -> KnxResult<Vec<u8>> {
        match value {
            DptValue::String(s) => {
                let mut bytes = vec![0u8; 14];
                let s_bytes = s.as_bytes();
                let len = s_bytes.len().min(14);
                bytes[..len].copy_from_slice(&s_bytes[..len]);
                Ok(bytes)
            }
            _ => Err(KnxError::dpt_encoding("DPT 16.001", "Expected string value")),
        }
    }

    fn decode(&self, data: &[u8]) -> KnxResult<DptValue> {
        validate_length(data, 14, "DPT 16.001")?;
        // Find null terminator or use all 14 bytes
        let end = data.iter().position(|&b| b == 0).unwrap_or(14);
        let s = String::from_utf8_lossy(&data[..end]).to_string();
        Ok(DptValue::String(s))
    }

    fn default_value(&self) -> DptValue {
        DptValue::String(String::new())
    }
}

// ============================================================================
// DPT 17/18: Scene Control
// ============================================================================

/// DPT 17.001 - Scene Number.
#[derive(Debug, Clone, Copy, Default)]
pub struct Dpt17Scene;

impl DptCodec for Dpt17Scene {
    fn id(&self) -> DptId {
        DptId::new(17, 1)
    }

    fn name(&self) -> &'static str {
        "Scene Number"
    }

    fn size(&self) -> usize {
        1
    }

    fn encode(&self, value: &DptValue) -> KnxResult<Vec<u8>> {
        match value {
            DptValue::U8(v) => {
                if *v > 63 {
                    return Err(KnxError::DptValueOutOfRange {
                        value: v.to_string(),
                        valid_range: "0-63".to_string(),
                    });
                }
                Ok(vec![*v])
            }
            DptValue::Scene { number, .. } => {
                if *number > 63 {
                    return Err(KnxError::DptValueOutOfRange {
                        value: number.to_string(),
                        valid_range: "0-63".to_string(),
                    });
                }
                Ok(vec![*number])
            }
            _ => Err(KnxError::dpt_encoding(
                "DPT 17.001",
                "Expected scene number",
            )),
        }
    }

    fn decode(&self, data: &[u8]) -> KnxResult<DptValue> {
        validate_length(data, 1, "DPT 17.001")?;
        Ok(DptValue::Scene {
            number: data[0] & 0x3F,
            learn: false,
        })
    }

    fn min_value(&self) -> Option<f64> {
        Some(0.0)
    }

    fn max_value(&self) -> Option<f64> {
        Some(63.0)
    }
}

/// DPT 18.001 - Scene Control.
#[derive(Debug, Clone, Copy, Default)]
pub struct Dpt18SceneControl;

impl DptCodec for Dpt18SceneControl {
    fn id(&self) -> DptId {
        DptId::new(18, 1)
    }

    fn name(&self) -> &'static str {
        "Scene Control"
    }

    fn size(&self) -> usize {
        1
    }

    fn encode(&self, value: &DptValue) -> KnxResult<Vec<u8>> {
        match value {
            DptValue::Scene { number, learn } => {
                if *number > 63 {
                    return Err(KnxError::DptValueOutOfRange {
                        value: number.to_string(),
                        valid_range: "0-63".to_string(),
                    });
                }
                let byte = (if *learn { 0x80 } else { 0 }) | (*number & 0x3F);
                Ok(vec![byte])
            }
            _ => Err(KnxError::dpt_encoding(
                "DPT 18.001",
                "Expected Scene value",
            )),
        }
    }

    fn decode(&self, data: &[u8]) -> KnxResult<DptValue> {
        validate_length(data, 1, "DPT 18.001")?;
        Ok(DptValue::Scene {
            number: data[0] & 0x3F,
            learn: data[0] & 0x80 != 0,
        })
    }
}

// ============================================================================
// DPT 20: HVAC Mode
// ============================================================================

/// DPT 20.102 - HVAC Mode.
#[derive(Debug, Clone, Copy, Default)]
pub struct Dpt20HvacMode;

impl DptCodec for Dpt20HvacMode {
    fn id(&self) -> DptId {
        DptId::new(20, 102)
    }

    fn name(&self) -> &'static str {
        "HVAC Mode"
    }

    fn size(&self) -> usize {
        1
    }

    fn encode(&self, value: &DptValue) -> KnxResult<Vec<u8>> {
        match value {
            DptValue::HvacMode(mode) => Ok(vec![mode.to_raw()]),
            DptValue::U8(v) => Ok(vec![*v]),
            _ => Err(KnxError::dpt_encoding(
                "DPT 20.102",
                "Expected HvacMode value",
            )),
        }
    }

    fn decode(&self, data: &[u8]) -> KnxResult<DptValue> {
        use super::value::HvacMode;
        validate_length(data, 1, "DPT 20.102")?;
        match HvacMode::from_raw(data[0]) {
            Some(mode) => Ok(DptValue::HvacMode(mode)),
            None => Ok(DptValue::U8(data[0])),
        }
    }
}

// ============================================================================
// DPT 232: RGB Color (3 bytes)
// ============================================================================

/// DPT 232.600 - Color RGB.
#[derive(Debug, Clone, Copy, Default)]
pub struct Dpt232Rgb;

impl DptCodec for Dpt232Rgb {
    fn id(&self) -> DptId {
        DptId::new(232, 600)
    }

    fn name(&self) -> &'static str {
        "Color RGB"
    }

    fn size(&self) -> usize {
        3
    }

    fn encode(&self, value: &DptValue) -> KnxResult<Vec<u8>> {
        match value {
            DptValue::ColorRgb { r, g, b } => Ok(vec![*r, *g, *b]),
            _ => Err(KnxError::dpt_encoding(
                "DPT 232.600",
                "Expected ColorRgb value",
            )),
        }
    }

    fn decode(&self, data: &[u8]) -> KnxResult<DptValue> {
        validate_length(data, 3, "DPT 232.600")?;
        Ok(DptValue::ColorRgb {
            r: data[0],
            g: data[1],
            b: data[2],
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dpt1_switch() {
        let codec = Dpt1Switch;

        let encoded = codec.encode(&DptValue::Bool(true)).unwrap();
        assert_eq!(encoded, vec![1]);

        let decoded = codec.decode(&[1]).unwrap();
        assert_eq!(decoded, DptValue::Bool(true));

        let decoded = codec.decode(&[0]).unwrap();
        assert_eq!(decoded, DptValue::Bool(false));
    }

    #[test]
    fn test_dpt5_scaling() {
        let codec = Dpt5Scaling;

        // Test encode
        let encoded = codec.encode(&DptValue::F16(100.0)).unwrap();
        assert_eq!(encoded, vec![255]);

        let encoded = codec.encode(&DptValue::F16(50.0)).unwrap();
        assert!(encoded[0] >= 127 && encoded[0] <= 128);

        // Test decode
        let decoded = codec.decode(&[255]).unwrap();
        if let DptValue::F16(v) = decoded {
            assert!((v - 100.0).abs() < 0.5);
        } else {
            panic!("Expected F16");
        }
    }

    #[test]
    fn test_dpt9_temperature() {
        let codec = Dpt9Temperature;

        // Test encode/decode round-trip
        let original = DptValue::F16(25.5);
        let encoded = codec.encode(&original).unwrap();
        let decoded = codec.decode(&encoded).unwrap();

        if let (DptValue::F16(o), DptValue::F16(d)) = (&original, &decoded) {
            assert!((o - d).abs() < 0.1, "Expected ~{}, got {}", o, d);
        }
    }

    #[test]
    fn test_dpt16_string() {
        let codec = Dpt16String;

        let value = DptValue::String("Hello".to_string());
        let encoded = codec.encode(&value).unwrap();
        assert_eq!(encoded.len(), 14);
        assert_eq!(&encoded[..5], b"Hello");

        let decoded = codec.decode(&encoded).unwrap();
        assert_eq!(decoded, DptValue::String("Hello".to_string()));
    }

    #[test]
    fn test_dpt232_rgb() {
        let codec = Dpt232Rgb;

        let value = DptValue::rgb(255, 128, 64);
        let encoded = codec.encode(&value).unwrap();
        assert_eq!(encoded, vec![255, 128, 64]);

        let decoded = codec.decode(&[255, 128, 64]).unwrap();
        assert_eq!(
            decoded,
            DptValue::ColorRgb {
                r: 255,
                g: 128,
                b: 64
            }
        );
    }
}

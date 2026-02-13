//! Wrong unit ID fault injection.
//!
//! Corrupts the unit ID (slave address) in Modbus responses, triggering
//! `ResponseValidator` unit_id verification failures in trap-modbus.

use rand::Rng;

use super::config::{FaultTypeConfig, UnitIdCorruptionMode};
use super::stats::{FaultStats, FaultStatsSnapshot};
use super::targeting::FaultTarget;
use super::{FaultAction, ModbusFault, ModbusFaultContext, TransportKind};

/// Corrupts the unit ID in Modbus responses.
///
/// For RTU: modifies the unit ID byte at position 0 of the frame.
/// For TCP: modifies the unit ID in the MBAP header.
///
/// This fault returns `SendResponse` with the PDU intact — the server
/// integration layer is responsible for using the corrupted unit_id
/// when building the frame/MBAP header.
///
/// # Modes
///
/// - `Fixed`: Always use a specific unit ID
/// - `Increment`: Add 1 to the original (wrapping)
/// - `Random`: Use a random unit ID (different from original)
/// - `Swap`: Use `255 - original`
pub struct WrongUnitIdFault {
    mode: UnitIdCorruptionMode,
    fixed_id: u8,
    target: FaultTarget,
    stats: FaultStats,
}

impl WrongUnitIdFault {
    /// Create a new wrong unit ID fault.
    pub fn new(mode: UnitIdCorruptionMode, target: FaultTarget) -> Self {
        Self {
            mode,
            fixed_id: 0xFF,
            target,
            stats: FaultStats::new(),
        }
    }

    /// Set the fixed unit ID (for Fixed mode).
    pub fn with_fixed_id(mut self, id: u8) -> Self {
        self.fixed_id = id;
        self
    }

    /// Create from config.
    pub fn from_config(config: &FaultTypeConfig, target: FaultTarget) -> Self {
        let mut fault = Self {
            mode: config.unit_id_mode.unwrap_or(UnitIdCorruptionMode::Increment),
            fixed_id: config.fixed_unit_id.unwrap_or(0xFF),
            target,
            stats: FaultStats::new(),
        };
        if let Some(id) = config.fixed_unit_id {
            fault.fixed_id = id;
        }
        fault
    }

    /// Compute the corrupted unit ID.
    fn corrupt_unit_id(&self, original: u8) -> u8 {
        match self.mode {
            UnitIdCorruptionMode::Fixed => self.fixed_id,
            UnitIdCorruptionMode::Increment => original.wrapping_add(1),
            UnitIdCorruptionMode::Random => {
                let mut rng = rand::thread_rng();
                loop {
                    let candidate: u8 = rng.gen();
                    if candidate != original {
                        return candidate;
                    }
                }
            }
            UnitIdCorruptionMode::Swap => 255u8.wrapping_sub(original),
        }
    }
}

impl ModbusFault for WrongUnitIdFault {
    fn fault_type(&self) -> &'static str {
        "wrong_unit_id"
    }

    fn is_enabled(&self) -> bool {
        self.stats.is_enabled()
    }

    fn set_enabled(&self, enabled: bool) {
        self.stats.set_enabled(enabled);
    }

    fn should_activate(&self, ctx: &ModbusFaultContext) -> bool {
        self.stats.record_check();
        self.target.should_activate(ctx.unit_id, ctx.function_code)
    }

    fn apply(&self, ctx: &ModbusFaultContext) -> FaultAction {
        self.stats.record_activation();
        self.stats.record_affected();

        let bad_id = self.corrupt_unit_id(ctx.unit_id);

        // For RTU: build complete raw frame with wrong unit_id
        // For TCP: return response PDU and let the server override the unit_id in MBAP
        match ctx.transport {
            TransportKind::Rtu => {
                // Build raw frame: [bad_unit_id][response_pdu][CRC]
                let mut frame = Vec::with_capacity(1 + ctx.response_pdu.len() + 2);
                frame.push(bad_id);
                frame.extend_from_slice(&ctx.response_pdu);

                // Compute CRC over the corrupted frame (to make it look like a valid frame
                // from a different device, not a CRC error)
                let crc = compute_crc(&frame);
                frame.push((crc & 0xFF) as u8);
                frame.push((crc >> 8) as u8);

                FaultAction::SendRawBytes(frame)
            }
            TransportKind::Tcp => {
                // For TCP, we encode the bad unit_id in the first byte of response
                // The server integration layer reads this and puts it in the MBAP header.
                // We prepend a marker byte that the server layer understands.
                // Actually, we use a different approach: return the response as-is
                // and let the server integration layer handle the unit_id override.
                // We'll encode the bad unit_id in a dedicated FaultAction variant...
                // But we don't have one. Instead, for TCP, we modify the response
                // to include metadata the server can read.
                //
                // Simplest approach: return SendResponse and have the pipeline
                // pass along the override_unit_id via the context. But since
                // we can't modify the pipeline from here, we'll embed the bad
                // unit_id as metadata in the response bytes using a convention:
                // the first byte of the response is the FC, so we can't change that.
                //
                // For TCP, the unit_id is in the MBAP header, not in the PDU.
                // The cleanest solution is to build raw MBAP bytes:
                let mut mbap = Vec::with_capacity(7 + ctx.response_pdu.len());
                // Transaction ID (2 bytes, big-endian)
                mbap.push((ctx.transaction_id >> 8) as u8);
                mbap.push((ctx.transaction_id & 0xFF) as u8);
                // Protocol ID (2 bytes, always 0x0000 for Modbus)
                mbap.push(0x00);
                mbap.push(0x00);
                // Length (2 bytes, big-endian) = unit_id(1) + PDU
                let length = 1 + ctx.response_pdu.len() as u16;
                mbap.push((length >> 8) as u8);
                mbap.push((length & 0xFF) as u8);
                // Unit ID (corrupted)
                mbap.push(bad_id);
                // PDU
                mbap.extend_from_slice(&ctx.response_pdu);

                FaultAction::SendRawBytes(mbap)
            }
        }
    }

    fn stats(&self) -> FaultStatsSnapshot {
        self.stats.snapshot()
    }

    fn reset_stats(&self) {
        self.stats.reset();
    }
}

/// Compute Modbus CRC-16.
fn compute_crc(data: &[u8]) -> u16 {
    let mut crc: u16 = 0xFFFF;
    for &byte in data {
        crc ^= byte as u16;
        for _ in 0..8 {
            if crc & 0x0001 != 0 {
                crc = (crc >> 1) ^ 0xA001;
            } else {
                crc >>= 1;
            }
        }
    }
    crc
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rtu_ctx() -> ModbusFaultContext {
        ModbusFaultContext::rtu(1, 0x03, &[0x03, 0x00, 0x00, 0x00, 0x01], &[0x03, 0x02, 0x00, 0x64], 1)
    }

    fn tcp_ctx() -> ModbusFaultContext {
        ModbusFaultContext::tcp(1, 0x03, &[0x03, 0x00, 0x00, 0x00, 0x01], &[0x03, 0x02, 0x00, 0x64], 42, 1)
    }

    #[test]
    fn test_fixed_mode_rtu() {
        let fault = WrongUnitIdFault::new(UnitIdCorruptionMode::Fixed, FaultTarget::new())
            .with_fixed_id(99);
        let action = fault.apply(&rtu_ctx());

        match action {
            FaultAction::SendRawBytes(bytes) => {
                assert_eq!(bytes[0], 99); // corrupted unit_id
                assert_eq!(&bytes[1..5], &[0x03, 0x02, 0x00, 0x64]); // PDU intact
                // CRC should be valid for the corrupted frame
                let crc_data = &bytes[..bytes.len() - 2];
                let expected_crc = compute_crc(crc_data);
                let actual_crc = (bytes[bytes.len() - 2] as u16) | ((bytes[bytes.len() - 1] as u16) << 8);
                assert_eq!(actual_crc, expected_crc);
            }
            _ => panic!("Expected SendRawBytes for RTU"),
        }
    }

    #[test]
    fn test_increment_mode() {
        let fault = WrongUnitIdFault::new(UnitIdCorruptionMode::Increment, FaultTarget::new());
        let ctx = rtu_ctx(); // unit_id = 1
        let action = fault.apply(&ctx);

        match action {
            FaultAction::SendRawBytes(bytes) => {
                assert_eq!(bytes[0], 2); // 1 + 1
            }
            _ => panic!("Expected SendRawBytes"),
        }
    }

    #[test]
    fn test_swap_mode() {
        let fault = WrongUnitIdFault::new(UnitIdCorruptionMode::Swap, FaultTarget::new());
        let ctx = rtu_ctx(); // unit_id = 1
        let action = fault.apply(&ctx);

        match action {
            FaultAction::SendRawBytes(bytes) => {
                assert_eq!(bytes[0], 254); // 255 - 1
            }
            _ => panic!("Expected SendRawBytes"),
        }
    }

    #[test]
    fn test_random_mode() {
        let fault = WrongUnitIdFault::new(UnitIdCorruptionMode::Random, FaultTarget::new());
        let ctx = rtu_ctx(); // unit_id = 1
        let action = fault.apply(&ctx);

        match action {
            FaultAction::SendRawBytes(bytes) => {
                assert_ne!(bytes[0], 1); // must differ
            }
            _ => panic!("Expected SendRawBytes"),
        }
    }

    #[test]
    fn test_tcp_mode() {
        let fault = WrongUnitIdFault::new(UnitIdCorruptionMode::Fixed, FaultTarget::new())
            .with_fixed_id(99);
        let action = fault.apply(&tcp_ctx());

        match action {
            FaultAction::SendRawBytes(bytes) => {
                // MBAP header: TID(2) + Protocol(2) + Length(2) + UnitID(1) + PDU
                assert_eq!(bytes[0], 0x00); // TID high
                assert_eq!(bytes[1], 42);   // TID low
                assert_eq!(bytes[2], 0x00); // Protocol high
                assert_eq!(bytes[3], 0x00); // Protocol low
                assert_eq!(bytes[6], 99);   // corrupted unit_id
                assert_eq!(&bytes[7..], &[0x03, 0x02, 0x00, 0x64]); // PDU intact
            }
            _ => panic!("Expected SendRawBytes for TCP"),
        }
    }

    #[test]
    fn test_increment_wrapping() {
        let fault = WrongUnitIdFault::new(UnitIdCorruptionMode::Increment, FaultTarget::new());
        let ctx = ModbusFaultContext::rtu(255, 0x03, &[0x03], &[0x03, 0x02], 1);
        let action = fault.apply(&ctx);

        match action {
            FaultAction::SendRawBytes(bytes) => {
                assert_eq!(bytes[0], 0); // 255 + 1 wraps to 0
            }
            _ => panic!("Expected SendRawBytes"),
        }
    }

    #[test]
    fn test_from_config() {
        let config = FaultTypeConfig {
            unit_id_mode: Some(UnitIdCorruptionMode::Fixed),
            fixed_unit_id: Some(42),
            ..Default::default()
        };
        let fault = WrongUnitIdFault::from_config(&config, FaultTarget::new());
        assert_eq!(fault.mode, UnitIdCorruptionMode::Fixed);
        assert_eq!(fault.fixed_id, 42);
    }
}

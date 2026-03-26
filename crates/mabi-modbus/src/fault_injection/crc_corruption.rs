//! CRC corruption fault for RTU frames.
//!
//! Corrupts the CRC-16 checksum in RTU response frames, triggering
//! `ErrorClassifier → ProtocolRecovery` in trap-modbus.

use rand::Rng;

use super::config::{CrcCorruptionMode, FaultTypeConfig};
use super::stats::{FaultStats, FaultStatsSnapshot};
use super::targeting::FaultTarget;
use super::{FaultAction, ModbusFault, ModbusFaultContext, TransportKind};

/// Corrupts the CRC-16 in RTU response frames.
///
/// This fault operates at the wire level: it takes the response PDU,
/// computes the correct CRC, corrupts it according to the mode, and
/// returns `SendRawBytes` so the server bypasses CRC recalculation.
///
/// # CRC Corruption Modes
///
/// - `Zero`: Set CRC to `0x0000`
/// - `Invert`: Bitwise-NOT the correct CRC
/// - `RandomXor`: XOR with a random non-zero value
/// - `SetValue`: Use a caller-specified CRC value
/// - `SwapBytes`: Swap high and low bytes of the CRC
pub struct CrcCorruptionFault {
    mode: CrcCorruptionMode,
    /// Fixed CRC value for SetValue mode.
    fixed_crc: u16,
    target: FaultTarget,
    stats: FaultStats,
}

impl CrcCorruptionFault {
    /// Create a new CRC corruption fault.
    pub fn new(mode: CrcCorruptionMode, target: FaultTarget) -> Self {
        Self {
            mode,
            fixed_crc: 0xDEAD,
            target,
            stats: FaultStats::new(),
        }
    }

    /// Set a fixed CRC value (for SetValue mode).
    pub fn with_fixed_crc(mut self, crc: u16) -> Self {
        self.fixed_crc = crc;
        self
    }

    /// Create from config.
    pub fn from_config(config: &FaultTypeConfig, target: FaultTarget) -> Self {
        Self {
            mode: config.crc_mode.unwrap_or(CrcCorruptionMode::Invert),
            fixed_crc: 0xDEAD,
            target,
            stats: FaultStats::new(),
        }
    }

    /// Compute Modbus CRC-16 for a byte slice.
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

    /// Corrupt a CRC value according to the configured mode.
    fn corrupt_crc(&self, correct_crc: u16) -> u16 {
        match self.mode {
            CrcCorruptionMode::Zero => 0x0000,
            CrcCorruptionMode::Invert => !correct_crc,
            CrcCorruptionMode::RandomXor => {
                let mut rng = rand::thread_rng();
                let xor_val: u16 = rng.gen_range(1..=0xFFFF);
                correct_crc ^ xor_val
            }
            CrcCorruptionMode::SetValue => self.fixed_crc,
            CrcCorruptionMode::SwapBytes => {
                let hi = (correct_crc >> 8) & 0xFF;
                let lo = correct_crc & 0xFF;
                (lo << 8) | hi
            }
        }
    }
}

impl ModbusFault for CrcCorruptionFault {
    fn fault_type(&self) -> &'static str {
        "crc_corruption"
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

        // Build the complete RTU frame: [unit_id][PDU][corrupted_CRC]
        let mut frame = Vec::with_capacity(1 + ctx.response_pdu.len() + 2);
        frame.push(ctx.unit_id);
        frame.extend_from_slice(&ctx.response_pdu);

        let correct_crc = Self::compute_crc(&frame);
        let bad_crc = self.corrupt_crc(correct_crc);

        frame.push((bad_crc & 0xFF) as u8); // CRC low byte first (Modbus RTU)
        frame.push((bad_crc >> 8) as u8); // CRC high byte

        FaultAction::SendRawBytes(frame)
    }

    fn stats(&self) -> FaultStatsSnapshot {
        self.stats.snapshot()
    }

    fn reset_stats(&self) {
        self.stats.reset();
    }

    fn compatible_transport(&self) -> Option<TransportKind> {
        Some(TransportKind::Rtu)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rtu_ctx() -> ModbusFaultContext {
        ModbusFaultContext::rtu(
            1,
            0x03,
            &[0x03, 0x00, 0x00, 0x00, 0x01],
            &[0x03, 0x02, 0x00, 0x64],
            1,
        )
    }

    #[test]
    fn test_crc_zero() {
        let fault = CrcCorruptionFault::new(CrcCorruptionMode::Zero, FaultTarget::new());
        let ctx = rtu_ctx();
        let action = fault.apply(&ctx);

        match action {
            FaultAction::SendRawBytes(bytes) => {
                let len = bytes.len();
                // Last 2 bytes should be 0x00, 0x00
                assert_eq!(bytes[len - 2], 0x00);
                assert_eq!(bytes[len - 1], 0x00);
                // First byte should be unit_id
                assert_eq!(bytes[0], 1);
                // PDU should be intact
                assert_eq!(&bytes[1..len - 2], &[0x03, 0x02, 0x00, 0x64]);
            }
            _ => panic!("Expected SendRawBytes"),
        }
    }

    #[test]
    fn test_crc_invert() {
        let fault = CrcCorruptionFault::new(CrcCorruptionMode::Invert, FaultTarget::new());
        let ctx = rtu_ctx();
        let action = fault.apply(&ctx);

        match action {
            FaultAction::SendRawBytes(bytes) => {
                let len = bytes.len();
                // Compute correct CRC
                let frame_data = &bytes[..len - 2];
                let correct_crc = CrcCorruptionFault::compute_crc(frame_data);
                let actual_crc = (bytes[len - 2] as u16) | ((bytes[len - 1] as u16) << 8);
                // Should be inverted
                assert_eq!(actual_crc, !correct_crc);
            }
            _ => panic!("Expected SendRawBytes"),
        }
    }

    #[test]
    fn test_crc_swap_bytes() {
        let fault = CrcCorruptionFault::new(CrcCorruptionMode::SwapBytes, FaultTarget::new());
        let ctx = rtu_ctx();
        let action = fault.apply(&ctx);

        match action {
            FaultAction::SendRawBytes(bytes) => {
                let len = bytes.len();
                let frame_data = &bytes[..len - 2];
                let correct_crc = CrcCorruptionFault::compute_crc(frame_data);
                let expected = ((correct_crc & 0xFF) << 8) | ((correct_crc >> 8) & 0xFF);
                let actual_crc = (bytes[len - 2] as u16) | ((bytes[len - 1] as u16) << 8);
                assert_eq!(actual_crc, expected);
            }
            _ => panic!("Expected SendRawBytes"),
        }
    }

    #[test]
    fn test_crc_set_value() {
        let fault = CrcCorruptionFault::new(CrcCorruptionMode::SetValue, FaultTarget::new())
            .with_fixed_crc(0xBEEF);
        let ctx = rtu_ctx();
        let action = fault.apply(&ctx);

        match action {
            FaultAction::SendRawBytes(bytes) => {
                let len = bytes.len();
                let actual_crc = (bytes[len - 2] as u16) | ((bytes[len - 1] as u16) << 8);
                assert_eq!(actual_crc, 0xBEEF);
            }
            _ => panic!("Expected SendRawBytes"),
        }
    }

    #[test]
    fn test_crc_random_xor() {
        let fault = CrcCorruptionFault::new(CrcCorruptionMode::RandomXor, FaultTarget::new());
        let ctx = rtu_ctx();
        let action = fault.apply(&ctx);

        match action {
            FaultAction::SendRawBytes(bytes) => {
                let len = bytes.len();
                let frame_data = &bytes[..len - 2];
                let correct_crc = CrcCorruptionFault::compute_crc(frame_data);
                let actual_crc = (bytes[len - 2] as u16) | ((bytes[len - 1] as u16) << 8);
                // Should differ from correct CRC (XOR with non-zero)
                assert_ne!(actual_crc, correct_crc);
            }
            _ => panic!("Expected SendRawBytes"),
        }
    }

    #[test]
    fn test_rtu_only_transport() {
        let fault = CrcCorruptionFault::new(CrcCorruptionMode::Zero, FaultTarget::new());
        assert_eq!(fault.compatible_transport(), Some(TransportKind::Rtu));
    }

    #[test]
    fn test_stats_tracking() {
        let fault = CrcCorruptionFault::new(CrcCorruptionMode::Zero, FaultTarget::new());
        let ctx = rtu_ctx();

        assert!(fault.should_activate(&ctx));
        fault.apply(&ctx);

        let stats = fault.stats();
        assert_eq!(stats.checks, 1);
        assert_eq!(stats.activations, 1);
        assert_eq!(stats.affected_requests, 1);
    }

    #[test]
    fn test_crc_computation() {
        // Known CRC test vector: [0x01, 0x03, 0x00, 0x00, 0x00, 0x01] -> CRC = 0x0A84
        // Wire format: low byte 0x84 first, high byte 0x0A second
        let data = [0x01, 0x03, 0x00, 0x00, 0x00, 0x01];
        let crc = CrcCorruptionFault::compute_crc(&data);
        assert_eq!(crc, 0x0A84);
    }
}

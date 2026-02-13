//! Partial frame fault injection.
//!
//! Sends incomplete RTU frames, testing `byte_timeout` and frame
//! detection logic in trap-modbus.

use super::config::{FaultTypeConfig, PartialFrameMode};
use super::stats::{FaultStats, FaultStatsSnapshot};
use super::targeting::FaultTarget;
use super::{FaultAction, ModbusFault, ModbusFaultContext, TransportKind};

/// Sends partial RTU frames.
///
/// Instead of sending a complete response frame, this fault sends only
/// a portion of the frame data. The client should detect the incomplete
/// frame via byte_timeout (inter-character timeout) and treat it as
/// a protocol error.
///
/// # Modes
///
/// - `FixedCount`: Send exactly N bytes
/// - `Percentage`: Send a percentage of the complete frame
/// - `UpToFc`: Send only unit_id + function_code (2 bytes)
/// - `UpToData`: Send everything except the CRC (data without checksum)
pub struct PartialFrameFault {
    mode: PartialFrameMode,
    /// Number of bytes for FixedCount mode.
    byte_count: usize,
    /// Percentage for Percentage mode (0.0 to 1.0).
    percentage: f64,
    target: FaultTarget,
    stats: FaultStats,
}

impl PartialFrameFault {
    /// Create a new partial frame fault.
    pub fn new(mode: PartialFrameMode, target: FaultTarget) -> Self {
        Self {
            mode,
            byte_count: 2,
            percentage: 0.5,
            target,
            stats: FaultStats::new(),
        }
    }

    /// Set the byte count for FixedCount mode.
    pub fn with_byte_count(mut self, n: usize) -> Self {
        self.byte_count = n;
        self
    }

    /// Set the percentage for Percentage mode.
    pub fn with_percentage(mut self, p: f64) -> Self {
        self.percentage = p.clamp(0.0, 1.0);
        self
    }

    /// Create from config.
    pub fn from_config(config: &FaultTypeConfig, target: FaultTarget) -> Self {
        Self {
            mode: config.partial_mode.unwrap_or(PartialFrameMode::UpToFc),
            byte_count: config.partial_bytes.unwrap_or(2),
            percentage: config.partial_percentage.unwrap_or(0.5),
            target,
            stats: FaultStats::new(),
        }
    }

    /// Build the partial frame bytes.
    fn build_partial(&self, unit_id: u8, pdu: &[u8]) -> Vec<u8> {
        // Complete RTU frame would be: [unit_id][PDU][CRC_lo][CRC_hi]
        // Build the complete frame first (without CRC), then truncate
        let mut full_frame = Vec::with_capacity(1 + pdu.len() + 2);
        full_frame.push(unit_id);
        full_frame.extend_from_slice(pdu);

        // Add correct CRC so we're sending a partial valid frame
        let crc = compute_crc(&full_frame);
        full_frame.push((crc & 0xFF) as u8);
        full_frame.push((crc >> 8) as u8);

        // Now truncate according to mode
        let total_len = full_frame.len();

        let keep = match self.mode {
            PartialFrameMode::FixedCount => self.byte_count.min(total_len).max(1),
            PartialFrameMode::Percentage => {
                ((total_len as f64 * self.percentage).ceil() as usize)
                    .max(1)
                    .min(total_len)
            }
            PartialFrameMode::UpToFc => {
                // unit_id + function_code = 2 bytes
                2.min(total_len)
            }
            PartialFrameMode::UpToData => {
                // Everything except CRC (last 2 bytes)
                if total_len > 2 {
                    total_len - 2
                } else {
                    total_len
                }
            }
        };

        full_frame.truncate(keep);
        full_frame
    }
}

impl ModbusFault for PartialFrameFault {
    fn fault_type(&self) -> &'static str {
        "partial_frame"
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

        let partial = self.build_partial(ctx.unit_id, &ctx.response_pdu);
        FaultAction::SendPartial { bytes: partial }
    }

    fn stats(&self) -> FaultStatsSnapshot {
        self.stats.snapshot()
    }

    fn reset_stats(&self) {
        self.stats.reset();
    }

    fn is_short_circuit(&self) -> bool {
        true
    }

    fn compatible_transport(&self) -> Option<TransportKind> {
        Some(TransportKind::Rtu)
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
        // Response: FC(0x03) + ByteCount(0x02) + Data(0x00, 0x64)
        ModbusFaultContext::rtu(
            1, 0x03,
            &[0x03, 0x00, 0x00, 0x00, 0x01],
            &[0x03, 0x02, 0x00, 0x64],
            1,
        )
    }

    #[test]
    fn test_up_to_fc() {
        let fault = PartialFrameFault::new(PartialFrameMode::UpToFc, FaultTarget::new());
        let action = fault.apply(&rtu_ctx());

        match action {
            FaultAction::SendPartial { bytes } => {
                assert_eq!(bytes.len(), 2);
                assert_eq!(bytes[0], 1);    // unit_id
                assert_eq!(bytes[1], 0x03); // FC
            }
            _ => panic!("Expected SendPartial"),
        }
    }

    #[test]
    fn test_up_to_data() {
        let fault = PartialFrameFault::new(PartialFrameMode::UpToData, FaultTarget::new());
        let action = fault.apply(&rtu_ctx());

        match action {
            FaultAction::SendPartial { bytes } => {
                // Full frame = [1, 0x03, 0x02, 0x00, 0x64, CRC_lo, CRC_hi] = 7 bytes
                // UpToData = 7 - 2 = 5 bytes
                assert_eq!(bytes.len(), 5);
                assert_eq!(bytes[0], 1);    // unit_id
                assert_eq!(bytes[1], 0x03); // FC
                assert_eq!(bytes[2], 0x02); // byte count
                assert_eq!(bytes[3], 0x00); // data hi
                assert_eq!(bytes[4], 0x64); // data lo
            }
            _ => panic!("Expected SendPartial"),
        }
    }

    #[test]
    fn test_fixed_count() {
        let fault = PartialFrameFault::new(PartialFrameMode::FixedCount, FaultTarget::new())
            .with_byte_count(3);
        let action = fault.apply(&rtu_ctx());

        match action {
            FaultAction::SendPartial { bytes } => {
                assert_eq!(bytes.len(), 3);
                assert_eq!(bytes[0], 1);    // unit_id
                assert_eq!(bytes[1], 0x03); // FC
                assert_eq!(bytes[2], 0x02); // byte count
            }
            _ => panic!("Expected SendPartial"),
        }
    }

    #[test]
    fn test_fixed_count_minimum() {
        let fault = PartialFrameFault::new(PartialFrameMode::FixedCount, FaultTarget::new())
            .with_byte_count(0);
        let action = fault.apply(&rtu_ctx());

        match action {
            FaultAction::SendPartial { bytes } => {
                assert_eq!(bytes.len(), 1); // minimum 1 byte
            }
            _ => panic!("Expected SendPartial"),
        }
    }

    #[test]
    fn test_percentage() {
        let fault = PartialFrameFault::new(PartialFrameMode::Percentage, FaultTarget::new())
            .with_percentage(0.5);
        let action = fault.apply(&rtu_ctx());

        match action {
            FaultAction::SendPartial { bytes } => {
                // Full frame = 7 bytes, 50% = ceil(3.5) = 4 bytes
                assert_eq!(bytes.len(), 4);
            }
            _ => panic!("Expected SendPartial"),
        }
    }

    #[test]
    fn test_is_short_circuit() {
        let fault = PartialFrameFault::new(PartialFrameMode::UpToFc, FaultTarget::new());
        assert!(fault.is_short_circuit());
    }

    #[test]
    fn test_rtu_only() {
        let fault = PartialFrameFault::new(PartialFrameMode::UpToFc, FaultTarget::new());
        assert_eq!(fault.compatible_transport(), Some(TransportKind::Rtu));
    }

    #[test]
    fn test_stats() {
        let fault = PartialFrameFault::new(PartialFrameMode::UpToFc, FaultTarget::new());
        let ctx = rtu_ctx();

        assert!(fault.should_activate(&ctx));
        fault.apply(&ctx);

        let stats = fault.stats();
        assert_eq!(stats.checks, 1);
        assert_eq!(stats.activations, 1);
        assert_eq!(stats.affected_requests, 1);
    }

    #[test]
    fn test_from_config() {
        let config = FaultTypeConfig {
            partial_mode: Some(PartialFrameMode::FixedCount),
            partial_bytes: Some(4),
            ..Default::default()
        };
        let fault = PartialFrameFault::from_config(&config, FaultTarget::new());
        let action = fault.apply(&rtu_ctx());
        match action {
            FaultAction::SendPartial { bytes } => {
                assert_eq!(bytes.len(), 4);
            }
            _ => panic!("Expected SendPartial"),
        }
    }
}

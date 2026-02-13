//! Truncated response fault injection.
//!
//! Truncates Modbus response PDUs, triggering `ResponseValidator`
//! data length verification failures in trap-modbus.

use super::config::{FaultTypeConfig, TruncationMode};
use super::stats::{FaultStats, FaultStatsSnapshot};
use super::targeting::FaultTarget;
use super::{FaultAction, ModbusFault, ModbusFaultContext};

/// Truncates Modbus response PDUs.
///
/// This fault removes bytes from the response, producing PDUs that
/// are shorter than expected. The trap-modbus `ResponseValidator`
/// checks data length against the byte count field in the PDU.
///
/// # Modes
///
/// - `FixedBytes`: Keep only the first N bytes
/// - `Percentage`: Keep a percentage of the response
/// - `RemoveLastN`: Remove the last N bytes
/// - `HeaderOnly`: Keep only the function code byte (1 byte)
pub struct TruncatedResponseFault {
    mode: TruncationMode,
    /// Number of bytes for FixedBytes/RemoveLastN modes.
    byte_count: usize,
    /// Percentage for Percentage mode (0.0 to 1.0).
    percentage: f64,
    target: FaultTarget,
    stats: FaultStats,
}

impl TruncatedResponseFault {
    /// Create a new truncated response fault.
    pub fn new(mode: TruncationMode, target: FaultTarget) -> Self {
        Self {
            mode,
            byte_count: 1,
            percentage: 0.5,
            target,
            stats: FaultStats::new(),
        }
    }

    /// Set the byte count for FixedBytes/RemoveLastN modes.
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
            mode: config.truncation_mode.unwrap_or(TruncationMode::HeaderOnly),
            byte_count: config.truncation_bytes.unwrap_or(1),
            percentage: config.truncation_percentage.unwrap_or(0.5),
            target,
            stats: FaultStats::new(),
        }
    }

    /// Apply truncation to the response PDU.
    fn truncate(&self, pdu: &[u8]) -> Vec<u8> {
        if pdu.is_empty() {
            return Vec::new();
        }

        match self.mode {
            TruncationMode::FixedBytes => {
                let keep = self.byte_count.min(pdu.len());
                pdu[..keep].to_vec()
            }
            TruncationMode::Percentage => {
                let keep = ((pdu.len() as f64 * self.percentage).ceil() as usize)
                    .max(1)
                    .min(pdu.len());
                pdu[..keep].to_vec()
            }
            TruncationMode::RemoveLastN => {
                let remove = self.byte_count.min(pdu.len());
                let keep = pdu.len() - remove;
                if keep == 0 {
                    // Keep at least 1 byte (the FC)
                    pdu[..1].to_vec()
                } else {
                    pdu[..keep].to_vec()
                }
            }
            TruncationMode::HeaderOnly => {
                // Keep only the function code byte
                pdu[..1].to_vec()
            }
        }
    }
}

impl ModbusFault for TruncatedResponseFault {
    fn fault_type(&self) -> &'static str {
        "truncated_response"
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

        let truncated = self.truncate(&ctx.response_pdu);
        FaultAction::SendResponse(truncated)
    }

    fn stats(&self) -> FaultStatsSnapshot {
        self.stats.snapshot()
    }

    fn reset_stats(&self) {
        self.stats.reset();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_ctx() -> ModbusFaultContext {
        // FC 0x03 Read Holding Registers response: FC(1) + ByteCount(1) + Data(6)
        ModbusFaultContext::tcp(
            1, 0x03,
            &[0x03, 0x00, 0x00, 0x00, 0x03],
            &[0x03, 0x06, 0x00, 0x64, 0x00, 0xC8, 0x01, 0x2C],
            1, 1,
        )
    }

    #[test]
    fn test_header_only() {
        let fault = TruncatedResponseFault::new(TruncationMode::HeaderOnly, FaultTarget::new());
        let action = fault.apply(&test_ctx());

        match action {
            FaultAction::SendResponse(pdu) => {
                assert_eq!(pdu.len(), 1);
                assert_eq!(pdu[0], 0x03);
            }
            _ => panic!("Expected SendResponse"),
        }
    }

    #[test]
    fn test_fixed_bytes() {
        let fault = TruncatedResponseFault::new(TruncationMode::FixedBytes, FaultTarget::new())
            .with_byte_count(3);
        let action = fault.apply(&test_ctx());

        match action {
            FaultAction::SendResponse(pdu) => {
                assert_eq!(pdu.len(), 3);
                assert_eq!(pdu, vec![0x03, 0x06, 0x00]);
            }
            _ => panic!("Expected SendResponse"),
        }
    }

    #[test]
    fn test_fixed_bytes_exceeds_length() {
        let fault = TruncatedResponseFault::new(TruncationMode::FixedBytes, FaultTarget::new())
            .with_byte_count(100);
        let ctx = test_ctx();
        let action = fault.apply(&ctx);

        match action {
            FaultAction::SendResponse(pdu) => {
                assert_eq!(pdu.len(), ctx.response_pdu.len());
            }
            _ => panic!("Expected SendResponse"),
        }
    }

    #[test]
    fn test_percentage_half() {
        let fault = TruncatedResponseFault::new(TruncationMode::Percentage, FaultTarget::new())
            .with_percentage(0.5);
        let ctx = test_ctx(); // 8 bytes
        let action = fault.apply(&ctx);

        match action {
            FaultAction::SendResponse(pdu) => {
                assert_eq!(pdu.len(), 4); // ceil(8 * 0.5)
            }
            _ => panic!("Expected SendResponse"),
        }
    }

    #[test]
    fn test_percentage_minimal() {
        let fault = TruncatedResponseFault::new(TruncationMode::Percentage, FaultTarget::new())
            .with_percentage(0.01);
        let action = fault.apply(&test_ctx());

        match action {
            FaultAction::SendResponse(pdu) => {
                // Should keep at least 1 byte
                assert!(!pdu.is_empty());
                assert_eq!(pdu[0], 0x03);
            }
            _ => panic!("Expected SendResponse"),
        }
    }

    #[test]
    fn test_remove_last_n() {
        let fault = TruncatedResponseFault::new(TruncationMode::RemoveLastN, FaultTarget::new())
            .with_byte_count(3);
        let ctx = test_ctx(); // [0x03, 0x06, 0x00, 0x64, 0x00, 0xC8, 0x01, 0x2C]
        let action = fault.apply(&ctx);

        match action {
            FaultAction::SendResponse(pdu) => {
                assert_eq!(pdu.len(), 5); // 8 - 3
                assert_eq!(pdu, vec![0x03, 0x06, 0x00, 0x64, 0x00]);
            }
            _ => panic!("Expected SendResponse"),
        }
    }

    #[test]
    fn test_remove_last_n_exceeds_length() {
        let fault = TruncatedResponseFault::new(TruncationMode::RemoveLastN, FaultTarget::new())
            .with_byte_count(100);
        let action = fault.apply(&test_ctx());

        match action {
            FaultAction::SendResponse(pdu) => {
                // Should keep at least the FC byte
                assert_eq!(pdu.len(), 1);
                assert_eq!(pdu[0], 0x03);
            }
            _ => panic!("Expected SendResponse"),
        }
    }

    #[test]
    fn test_empty_response() {
        let fault = TruncatedResponseFault::new(TruncationMode::HeaderOnly, FaultTarget::new());
        let ctx = ModbusFaultContext::tcp(1, 0x03, &[0x03], &[], 1, 1);
        let action = fault.apply(&ctx);

        match action {
            FaultAction::SendResponse(pdu) => {
                assert!(pdu.is_empty());
            }
            _ => panic!("Expected SendResponse"),
        }
    }

    #[test]
    fn test_stats() {
        let fault = TruncatedResponseFault::new(TruncationMode::HeaderOnly, FaultTarget::new());
        let ctx = test_ctx();

        fault.should_activate(&ctx);
        fault.apply(&ctx);

        let stats = fault.stats();
        assert_eq!(stats.checks, 1);
        assert_eq!(stats.activations, 1);
        assert_eq!(stats.affected_requests, 1);
    }

    #[test]
    fn test_from_config() {
        let config = FaultTypeConfig {
            truncation_mode: Some(TruncationMode::FixedBytes),
            truncation_bytes: Some(2),
            ..Default::default()
        };
        let fault = TruncatedResponseFault::from_config(&config, FaultTarget::new());
        let action = fault.apply(&test_ctx());
        match action {
            FaultAction::SendResponse(pdu) => assert_eq!(pdu.len(), 2),
            _ => panic!("Expected SendResponse"),
        }
    }
}

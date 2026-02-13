//! Extra data fault injection.
//!
//! Appends extra bytes to Modbus response PDUs, triggering
//! `ResponseValidator` strict mode length checking in trap-modbus.

use rand::Rng;

use super::config::{ExtraDataMode, FaultTypeConfig};
use super::stats::{FaultStats, FaultStatsSnapshot};
use super::targeting::FaultTarget;
use super::{FaultAction, ModbusFault, ModbusFaultContext};

/// Appends extra bytes to Modbus response PDUs.
///
/// Many Modbus implementations ignore trailing bytes, but strict
/// validators should reject responses that are longer than expected.
/// This fault tests that behavior.
///
/// # Modes
///
/// - `AppendBytes`: Append specific byte values
/// - `AppendRandom`: Append N random bytes
/// - `DuplicateLastN`: Duplicate the last N bytes of the response
pub struct ExtraDataFault {
    mode: ExtraDataMode,
    /// Specific bytes to append (for AppendBytes mode).
    bytes: Vec<u8>,
    /// Number of bytes to append/duplicate.
    count: usize,
    target: FaultTarget,
    stats: FaultStats,
}

impl ExtraDataFault {
    /// Create a new extra data fault.
    pub fn new(mode: ExtraDataMode, count: usize, target: FaultTarget) -> Self {
        Self {
            mode,
            bytes: Vec::new(),
            count,
            target,
            stats: FaultStats::new(),
        }
    }

    /// Set specific bytes to append (for AppendBytes mode).
    pub fn with_bytes(mut self, bytes: Vec<u8>) -> Self {
        self.bytes = bytes;
        self
    }

    /// Create from config.
    pub fn from_config(config: &FaultTypeConfig, target: FaultTarget) -> Self {
        Self {
            mode: config.extra_data_mode.unwrap_or(ExtraDataMode::AppendRandom),
            bytes: config.extra_bytes.clone().unwrap_or_default(),
            count: config.extra_count.unwrap_or(4),
            target,
            stats: FaultStats::new(),
        }
    }

    /// Generate extra bytes according to the mode.
    fn generate_extra(&self, pdu: &[u8]) -> Vec<u8> {
        match self.mode {
            ExtraDataMode::AppendBytes => {
                if self.bytes.is_empty() {
                    // Default: append zero bytes
                    vec![0x00; self.count]
                } else {
                    self.bytes.clone()
                }
            }
            ExtraDataMode::AppendRandom => {
                let mut rng = rand::thread_rng();
                (0..self.count).map(|_| rng.gen::<u8>()).collect()
            }
            ExtraDataMode::DuplicateLastN => {
                if pdu.is_empty() {
                    return Vec::new();
                }
                let n = self.count.min(pdu.len());
                let start = pdu.len() - n;
                pdu[start..].to_vec()
            }
        }
    }
}

impl ModbusFault for ExtraDataFault {
    fn fault_type(&self) -> &'static str {
        "extra_data"
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

        let extra = self.generate_extra(&ctx.response_pdu);
        let mut response = ctx.response_pdu.clone();
        response.extend_from_slice(&extra);

        FaultAction::SendResponse(response)
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
        ModbusFaultContext::tcp(
            1, 0x03,
            &[0x03, 0x00, 0x00, 0x00, 0x01],
            &[0x03, 0x02, 0x00, 0x64],
            1, 1,
        )
    }

    #[test]
    fn test_append_specific_bytes() {
        let fault = ExtraDataFault::new(ExtraDataMode::AppendBytes, 3, FaultTarget::new())
            .with_bytes(vec![0xDE, 0xAD, 0xBE]);
        let action = fault.apply(&test_ctx());

        match action {
            FaultAction::SendResponse(pdu) => {
                assert_eq!(pdu.len(), 7); // 4 original + 3 extra
                assert_eq!(&pdu[..4], &[0x03, 0x02, 0x00, 0x64]);
                assert_eq!(&pdu[4..], &[0xDE, 0xAD, 0xBE]);
            }
            _ => panic!("Expected SendResponse"),
        }
    }

    #[test]
    fn test_append_random() {
        let fault = ExtraDataFault::new(ExtraDataMode::AppendRandom, 5, FaultTarget::new());
        let ctx = test_ctx();
        let action = fault.apply(&ctx);

        match action {
            FaultAction::SendResponse(pdu) => {
                assert_eq!(pdu.len(), 9); // 4 original + 5 random
                assert_eq!(&pdu[..4], &ctx.response_pdu);
            }
            _ => panic!("Expected SendResponse"),
        }
    }

    #[test]
    fn test_duplicate_last_n() {
        let fault = ExtraDataFault::new(ExtraDataMode::DuplicateLastN, 2, FaultTarget::new());
        let action = fault.apply(&test_ctx()); // response: [0x03, 0x02, 0x00, 0x64]

        match action {
            FaultAction::SendResponse(pdu) => {
                assert_eq!(pdu.len(), 6); // 4 original + 2 duplicated
                assert_eq!(&pdu[..4], &[0x03, 0x02, 0x00, 0x64]);
                assert_eq!(&pdu[4..], &[0x00, 0x64]); // last 2 bytes duplicated
            }
            _ => panic!("Expected SendResponse"),
        }
    }

    #[test]
    fn test_duplicate_exceeds_length() {
        let fault = ExtraDataFault::new(ExtraDataMode::DuplicateLastN, 100, FaultTarget::new());
        let ctx = test_ctx(); // 4 bytes
        let action = fault.apply(&ctx);

        match action {
            FaultAction::SendResponse(pdu) => {
                assert_eq!(pdu.len(), 8); // 4 original + 4 (all bytes duplicated)
                assert_eq!(&pdu[4..], &ctx.response_pdu);
            }
            _ => panic!("Expected SendResponse"),
        }
    }

    #[test]
    fn test_append_bytes_empty_default() {
        let fault = ExtraDataFault::new(ExtraDataMode::AppendBytes, 3, FaultTarget::new());
        // No specific bytes set, should append zeros
        let action = fault.apply(&test_ctx());

        match action {
            FaultAction::SendResponse(pdu) => {
                assert_eq!(pdu.len(), 7);
                assert_eq!(&pdu[4..], &[0x00, 0x00, 0x00]);
            }
            _ => panic!("Expected SendResponse"),
        }
    }

    #[test]
    fn test_empty_response() {
        let fault = ExtraDataFault::new(ExtraDataMode::DuplicateLastN, 3, FaultTarget::new());
        let ctx = ModbusFaultContext::tcp(1, 0x03, &[0x03], &[], 1, 1);
        let action = fault.apply(&ctx);

        match action {
            FaultAction::SendResponse(pdu) => {
                assert!(pdu.is_empty()); // nothing to duplicate
            }
            _ => panic!("Expected SendResponse"),
        }
    }

    #[test]
    fn test_from_config() {
        let config = FaultTypeConfig {
            extra_data_mode: Some(ExtraDataMode::AppendBytes),
            extra_bytes: Some(vec![0xAA, 0xBB]),
            extra_count: Some(2),
            ..Default::default()
        };
        let fault = ExtraDataFault::from_config(&config, FaultTarget::new());
        let action = fault.apply(&test_ctx());
        match action {
            FaultAction::SendResponse(pdu) => {
                assert_eq!(&pdu[4..], &[0xAA, 0xBB]);
            }
            _ => panic!("Expected SendResponse"),
        }
    }
}

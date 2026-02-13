//! Wrong function code fault injection.
//!
//! Corrupts the function code byte in Modbus responses, triggering
//! `ResponseValidator` FC matching failures in trap-modbus.

use rand::Rng;

use super::config::{FaultTypeConfig, FcCorruptionMode};
use super::stats::{FaultStats, FaultStatsSnapshot};
use super::targeting::FaultTarget;
use super::{FaultAction, ModbusFault, ModbusFaultContext};

/// Corrupts the function code in Modbus response PDUs.
///
/// The function code is always at byte 0 of the PDU. This fault modifies
/// that byte according to the configured mode.
///
/// # Modes
///
/// - `Fixed`: Always use a specific function code
/// - `Increment`: Add 1 to the original FC
/// - `Random`: Use a random FC (different from original)
/// - `SwapRW`: Swap between read FCs (01-04) and write FCs (05, 06, 0F, 10)
pub struct WrongFunctionCodeFault {
    mode: FcCorruptionMode,
    fixed_fc: u8,
    target: FaultTarget,
    stats: FaultStats,
}

impl WrongFunctionCodeFault {
    /// Create a new wrong function code fault.
    pub fn new(mode: FcCorruptionMode, target: FaultTarget) -> Self {
        Self {
            mode,
            fixed_fc: 0xFF,
            target,
            stats: FaultStats::new(),
        }
    }

    /// Set a fixed function code (for Fixed mode).
    pub fn with_fixed_fc(mut self, fc: u8) -> Self {
        self.fixed_fc = fc;
        self
    }

    /// Create from config.
    pub fn from_config(config: &FaultTypeConfig, target: FaultTarget) -> Self {
        Self {
            mode: config.fc_mode.unwrap_or(FcCorruptionMode::Increment),
            fixed_fc: config.fixed_fc.unwrap_or(0xFF),
            target,
            stats: FaultStats::new(),
        }
    }

    /// Compute the corrupted function code.
    fn corrupt_fc(&self, original: u8) -> u8 {
        match self.mode {
            FcCorruptionMode::Fixed => self.fixed_fc,
            FcCorruptionMode::Increment => original.wrapping_add(1),
            FcCorruptionMode::Random => {
                let mut rng = rand::thread_rng();
                loop {
                    let candidate: u8 = rng.gen_range(1..=0x7F);
                    if candidate != original {
                        return candidate;
                    }
                }
            }
            FcCorruptionMode::SwapRW => {
                // Read FCs: 0x01, 0x02, 0x03, 0x04
                // Write FCs: 0x05, 0x06, 0x0F, 0x10
                match original {
                    0x01 => 0x05,
                    0x02 => 0x0F,
                    0x03 => 0x10,
                    0x04 => 0x06,
                    0x05 => 0x01,
                    0x06 => 0x04,
                    0x0F => 0x02,
                    0x10 => 0x03,
                    0x16 => 0x03,
                    0x17 => 0x03,
                    _ => original.wrapping_add(1),
                }
            }
        }
    }
}

impl ModbusFault for WrongFunctionCodeFault {
    fn fault_type(&self) -> &'static str {
        "wrong_function_code"
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

        let mut response = ctx.response_pdu.clone();
        if !response.is_empty() {
            response[0] = self.corrupt_fc(ctx.function_code);
        }

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
    fn test_fixed_mode() {
        let fault = WrongFunctionCodeFault::new(FcCorruptionMode::Fixed, FaultTarget::new())
            .with_fixed_fc(0x10);
        let action = fault.apply(&test_ctx());

        match action {
            FaultAction::SendResponse(pdu) => {
                assert_eq!(pdu[0], 0x10);
                assert_eq!(&pdu[1..], &[0x02, 0x00, 0x64]); // rest intact
            }
            _ => panic!("Expected SendResponse"),
        }
    }

    #[test]
    fn test_increment_mode() {
        let fault = WrongFunctionCodeFault::new(FcCorruptionMode::Increment, FaultTarget::new());
        let action = fault.apply(&test_ctx());

        match action {
            FaultAction::SendResponse(pdu) => {
                assert_eq!(pdu[0], 0x04); // 0x03 + 1
            }
            _ => panic!("Expected SendResponse"),
        }
    }

    #[test]
    fn test_random_mode() {
        let fault = WrongFunctionCodeFault::new(FcCorruptionMode::Random, FaultTarget::new());
        let ctx = test_ctx();
        let action = fault.apply(&ctx);

        match action {
            FaultAction::SendResponse(pdu) => {
                assert_ne!(pdu[0], 0x03);
            }
            _ => panic!("Expected SendResponse"),
        }
    }

    #[test]
    fn test_swap_rw_read_to_write() {
        let fault = WrongFunctionCodeFault::new(FcCorruptionMode::SwapRW, FaultTarget::new());

        // FC 0x03 (Read Holding) -> 0x10 (Write Multiple)
        let ctx = test_ctx();
        match fault.apply(&ctx) {
            FaultAction::SendResponse(pdu) => assert_eq!(pdu[0], 0x10),
            _ => panic!("Expected SendResponse"),
        }
    }

    #[test]
    fn test_swap_rw_write_to_read() {
        let fault = WrongFunctionCodeFault::new(FcCorruptionMode::SwapRW, FaultTarget::new());

        // FC 0x10 (Write Multiple) -> 0x03 (Read Holding)
        let ctx = ModbusFaultContext::tcp(1, 0x10, &[0x10], &[0x10, 0x00], 1, 1);
        match fault.apply(&ctx) {
            FaultAction::SendResponse(pdu) => assert_eq!(pdu[0], 0x03),
            _ => panic!("Expected SendResponse"),
        }
    }

    #[test]
    fn test_swap_rw_all_mappings() {
        let fault = WrongFunctionCodeFault::new(FcCorruptionMode::SwapRW, FaultTarget::new());

        let mappings = vec![
            (0x01, 0x05), (0x02, 0x0F), (0x03, 0x10), (0x04, 0x06),
            (0x05, 0x01), (0x06, 0x04), (0x0F, 0x02), (0x10, 0x03),
        ];

        for (from, to) in mappings {
            let ctx = ModbusFaultContext::tcp(1, from, &[from], &[from, 0x00], 1, 1);
            match fault.apply(&ctx) {
                FaultAction::SendResponse(pdu) => assert_eq!(pdu[0], to, "FC 0x{:02X} should map to 0x{:02X}", from, to),
                _ => panic!("Expected SendResponse"),
            }
        }
    }

    #[test]
    fn test_empty_response() {
        let fault = WrongFunctionCodeFault::new(FcCorruptionMode::Fixed, FaultTarget::new())
            .with_fixed_fc(0x10);
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
    fn test_from_config() {
        let config = FaultTypeConfig {
            fc_mode: Some(FcCorruptionMode::Fixed),
            fixed_fc: Some(0x42),
            ..Default::default()
        };
        let fault = WrongFunctionCodeFault::from_config(&config, FaultTarget::new());
        let ctx = test_ctx();
        match fault.apply(&ctx) {
            FaultAction::SendResponse(pdu) => assert_eq!(pdu[0], 0x42),
            _ => panic!("Expected SendResponse"),
        }
    }
}

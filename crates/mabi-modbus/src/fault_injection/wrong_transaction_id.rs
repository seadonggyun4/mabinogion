//! Wrong transaction ID fault injection.
//!
//! Corrupts the transaction ID in TCP MBAP headers, triggering
//! `TransactionIdTracker` response matching failures in trap-modbus.

use rand::Rng;

use super::config::{FaultTypeConfig, TidCorruptionMode};
use super::stats::{FaultStats, FaultStatsSnapshot};
use super::targeting::FaultTarget;
use super::{FaultAction, ModbusFault, ModbusFaultContext, TransportKind};

/// Corrupts the transaction ID in TCP MBAP headers.
///
/// The transaction ID is a 2-byte value in the MBAP header that clients
/// use to match responses with requests. Corrupting it tests the
/// `TransactionIdTracker` in trap-modbus.
///
/// # Modes
///
/// - `Fixed`: Always use a specific TID
/// - `Increment`: Add 1 to the original TID
/// - `Random`: Use a random TID (different from original)
/// - `SwapBytes`: Swap high and low bytes
pub struct WrongTransactionIdFault {
    mode: TidCorruptionMode,
    fixed_tid: u16,
    target: FaultTarget,
    stats: FaultStats,
}

impl WrongTransactionIdFault {
    /// Create a new wrong transaction ID fault.
    pub fn new(mode: TidCorruptionMode, target: FaultTarget) -> Self {
        Self {
            mode,
            fixed_tid: 0xFFFF,
            target,
            stats: FaultStats::new(),
        }
    }

    /// Set a fixed transaction ID (for Fixed mode).
    pub fn with_fixed_tid(mut self, tid: u16) -> Self {
        self.fixed_tid = tid;
        self
    }

    /// Create from config.
    pub fn from_config(config: &FaultTypeConfig, target: FaultTarget) -> Self {
        Self {
            mode: config.tid_mode.unwrap_or(TidCorruptionMode::Increment),
            fixed_tid: config.fixed_tid.unwrap_or(0xFFFF),
            target,
            stats: FaultStats::new(),
        }
    }

    /// Compute the corrupted transaction ID.
    fn corrupt_tid(&self, original: u16) -> u16 {
        match self.mode {
            TidCorruptionMode::Fixed => self.fixed_tid,
            TidCorruptionMode::Increment => original.wrapping_add(1),
            TidCorruptionMode::Random => {
                let mut rng = rand::thread_rng();
                loop {
                    let candidate: u16 = rng.gen();
                    if candidate != original {
                        return candidate;
                    }
                }
            }
            TidCorruptionMode::SwapBytes => {
                let hi = (original >> 8) & 0xFF;
                let lo = original & 0xFF;
                (lo << 8) | hi
            }
        }
    }
}

impl ModbusFault for WrongTransactionIdFault {
    fn fault_type(&self) -> &'static str {
        "wrong_transaction_id"
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

        let bad_tid = self.corrupt_tid(ctx.transaction_id);

        FaultAction::OverrideTransactionId {
            transaction_id: bad_tid,
            response: ctx.response_pdu.clone(),
        }
    }

    fn stats(&self) -> FaultStatsSnapshot {
        self.stats.snapshot()
    }

    fn reset_stats(&self) {
        self.stats.reset();
    }

    fn compatible_transport(&self) -> Option<TransportKind> {
        Some(TransportKind::Tcp)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tcp_ctx() -> ModbusFaultContext {
        ModbusFaultContext::tcp(
            1, 0x03,
            &[0x03, 0x00, 0x00, 0x00, 0x01],
            &[0x03, 0x02, 0x00, 0x64],
            100, 1,
        )
    }

    #[test]
    fn test_fixed_mode() {
        let fault = WrongTransactionIdFault::new(TidCorruptionMode::Fixed, FaultTarget::new())
            .with_fixed_tid(999);
        let action = fault.apply(&tcp_ctx());

        match action {
            FaultAction::OverrideTransactionId { transaction_id, response } => {
                assert_eq!(transaction_id, 999);
                assert_eq!(response, vec![0x03, 0x02, 0x00, 0x64]);
            }
            _ => panic!("Expected OverrideTransactionId"),
        }
    }

    #[test]
    fn test_increment_mode() {
        let fault = WrongTransactionIdFault::new(TidCorruptionMode::Increment, FaultTarget::new());
        let action = fault.apply(&tcp_ctx()); // TID = 100

        match action {
            FaultAction::OverrideTransactionId { transaction_id, .. } => {
                assert_eq!(transaction_id, 101);
            }
            _ => panic!("Expected OverrideTransactionId"),
        }
    }

    #[test]
    fn test_random_mode() {
        let fault = WrongTransactionIdFault::new(TidCorruptionMode::Random, FaultTarget::new());
        let action = fault.apply(&tcp_ctx()); // TID = 100

        match action {
            FaultAction::OverrideTransactionId { transaction_id, .. } => {
                assert_ne!(transaction_id, 100);
            }
            _ => panic!("Expected OverrideTransactionId"),
        }
    }

    #[test]
    fn test_swap_bytes_mode() {
        let fault = WrongTransactionIdFault::new(TidCorruptionMode::SwapBytes, FaultTarget::new());
        // TID = 0x0064 (100), swapped = 0x6400
        let action = fault.apply(&tcp_ctx());

        match action {
            FaultAction::OverrideTransactionId { transaction_id, .. } => {
                assert_eq!(transaction_id, 0x6400);
            }
            _ => panic!("Expected OverrideTransactionId"),
        }
    }

    #[test]
    fn test_increment_wrapping() {
        let fault = WrongTransactionIdFault::new(TidCorruptionMode::Increment, FaultTarget::new());
        let ctx = ModbusFaultContext::tcp(1, 0x03, &[0x03], &[0x03], 0xFFFF, 1);
        let action = fault.apply(&ctx);

        match action {
            FaultAction::OverrideTransactionId { transaction_id, .. } => {
                assert_eq!(transaction_id, 0);
            }
            _ => panic!("Expected OverrideTransactionId"),
        }
    }

    #[test]
    fn test_tcp_only_transport() {
        let fault = WrongTransactionIdFault::new(TidCorruptionMode::Fixed, FaultTarget::new());
        assert_eq!(fault.compatible_transport(), Some(TransportKind::Tcp));
    }

    #[test]
    fn test_from_config() {
        let config = FaultTypeConfig {
            tid_mode: Some(TidCorruptionMode::Fixed),
            fixed_tid: Some(42),
            ..Default::default()
        };
        let fault = WrongTransactionIdFault::from_config(&config, FaultTarget::new());
        let action = fault.apply(&tcp_ctx());
        match action {
            FaultAction::OverrideTransactionId { transaction_id, .. } => {
                assert_eq!(transaction_id, 42);
            }
            _ => panic!("Expected OverrideTransactionId"),
        }
    }
}

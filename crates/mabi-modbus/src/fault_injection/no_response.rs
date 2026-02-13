//! No response (silent drop) fault injection.
//!
//! Drops Modbus responses entirely, triggering `ErrorClassifier → RetryImmediate`
//! (timeout-based) in trap-modbus.

use std::sync::atomic::{AtomicU64, Ordering};

use super::stats::{FaultStats, FaultStatsSnapshot};
use super::targeting::FaultTarget;
use super::{FaultAction, ModbusFault, ModbusFaultContext};

/// Drops Modbus responses silently.
///
/// This fault causes the server to not send any response, simulating
/// a device that has become unresponsive or a network that dropped
/// the response packet. The client will eventually time out.
///
/// # Consecutive Drop Tracking
///
/// The fault tracks consecutive drops, which is useful for testing
/// progressive retry/backoff behavior:
///
/// ```rust,ignore
/// let fault = NoResponseFault::new(FaultTarget::new().with_probability(1.0));
/// // After 3 drops:
/// assert_eq!(fault.consecutive_drops(), 3);
/// ```
pub struct NoResponseFault {
    target: FaultTarget,
    stats: FaultStats,
    /// Track consecutive drops for monitoring.
    consecutive_drops: AtomicU64,
}

impl NoResponseFault {
    /// Create a new no-response fault.
    pub fn new(target: FaultTarget) -> Self {
        Self {
            target,
            stats: FaultStats::new(),
            consecutive_drops: AtomicU64::new(0),
        }
    }

    /// Get the current consecutive drop count.
    pub fn consecutive_drops(&self) -> u64 {
        self.consecutive_drops.load(Ordering::Acquire)
    }

    /// Reset the consecutive drop counter.
    /// Called externally when a response IS sent (breaking the streak).
    pub fn reset_consecutive(&self) {
        self.consecutive_drops.store(0, Ordering::Release);
    }
}

impl ModbusFault for NoResponseFault {
    fn fault_type(&self) -> &'static str {
        "no_response"
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

    fn apply(&self, _ctx: &ModbusFaultContext) -> FaultAction {
        self.stats.record_activation();
        self.stats.record_affected();
        self.consecutive_drops.fetch_add(1, Ordering::Relaxed);

        FaultAction::DropResponse
    }

    fn stats(&self) -> FaultStatsSnapshot {
        self.stats.snapshot()
    }

    fn reset_stats(&self) {
        self.stats.reset();
        self.consecutive_drops.store(0, Ordering::Release);
    }

    fn is_short_circuit(&self) -> bool {
        true
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
    fn test_always_drops() {
        let fault = NoResponseFault::new(FaultTarget::new());
        let action = fault.apply(&test_ctx());
        assert!(matches!(action, FaultAction::DropResponse));
    }

    #[test]
    fn test_consecutive_drops() {
        let fault = NoResponseFault::new(FaultTarget::new());
        let ctx = test_ctx();

        assert_eq!(fault.consecutive_drops(), 0);

        fault.apply(&ctx);
        assert_eq!(fault.consecutive_drops(), 1);

        fault.apply(&ctx);
        assert_eq!(fault.consecutive_drops(), 2);

        fault.apply(&ctx);
        assert_eq!(fault.consecutive_drops(), 3);
    }

    #[test]
    fn test_reset_consecutive() {
        let fault = NoResponseFault::new(FaultTarget::new());
        let ctx = test_ctx();

        fault.apply(&ctx);
        fault.apply(&ctx);
        assert_eq!(fault.consecutive_drops(), 2);

        fault.reset_consecutive();
        assert_eq!(fault.consecutive_drops(), 0);
    }

    #[test]
    fn test_is_short_circuit() {
        let fault = NoResponseFault::new(FaultTarget::new());
        assert!(fault.is_short_circuit());
    }

    #[test]
    fn test_stats() {
        let fault = NoResponseFault::new(FaultTarget::new());
        let ctx = test_ctx();

        assert!(fault.should_activate(&ctx));
        fault.apply(&ctx);
        fault.should_activate(&ctx);
        fault.apply(&ctx);

        let stats = fault.stats();
        assert_eq!(stats.checks, 2);
        assert_eq!(stats.activations, 2);
        assert_eq!(stats.affected_requests, 2);
    }

    #[test]
    fn test_reset_stats() {
        let fault = NoResponseFault::new(FaultTarget::new());
        let ctx = test_ctx();

        fault.should_activate(&ctx);
        fault.apply(&ctx);

        fault.reset_stats();
        let stats = fault.stats();
        assert_eq!(stats.checks, 0);
        assert_eq!(stats.activations, 0);
        assert_eq!(fault.consecutive_drops(), 0);
    }

    #[test]
    fn test_probability_targeting() {
        let fault = NoResponseFault::new(FaultTarget::new().with_probability(0.0));
        let ctx = test_ctx();

        // Should never activate with 0% probability
        for _ in 0..100 {
            assert!(!fault.should_activate(&ctx));
        }
    }
}

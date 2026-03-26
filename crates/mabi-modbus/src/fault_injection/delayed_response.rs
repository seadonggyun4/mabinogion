//! Delayed response fault injection.
//!
//! Introduces configurable delays before sending Modbus responses,
//! triggering `TimeoutConfig` response_timeout handling in trap-modbus.

use std::time::Duration;

use rand::Rng;

use super::config::FaultTypeConfig;
use super::stats::{FaultStats, FaultStatsSnapshot};
use super::targeting::FaultTarget;
use super::{FaultAction, ModbusFault, ModbusFaultContext};

/// Delays Modbus responses by a configurable duration.
///
/// This fault introduces a delay before the response is sent, testing
/// the client's timeout handling. The delay can include random jitter
/// to simulate realistic network conditions.
///
/// # Delay Calculation
///
/// `total_delay = base_delay + random(0..jitter)`
///
/// To consistently exceed the default 500ms response timeout:
/// - `base_delay = 600ms, jitter = 0ms` (always exceeds)
/// - `base_delay = 400ms, jitter = 200ms` (sometimes exceeds)
pub struct DelayedResponseFault {
    /// Base delay before sending response.
    base_delay: Duration,
    /// Maximum random jitter added to the base delay.
    jitter: Duration,
    target: FaultTarget,
    stats: FaultStats,
}

impl DelayedResponseFault {
    /// Create a new delayed response fault.
    pub fn new(base_delay: Duration, jitter: Duration, target: FaultTarget) -> Self {
        Self {
            base_delay,
            jitter,
            target,
            stats: FaultStats::new(),
        }
    }

    /// Create from config.
    pub fn from_config(config: &FaultTypeConfig, target: FaultTarget) -> Self {
        Self {
            base_delay: Duration::from_millis(config.delay_ms.unwrap_or(1000)),
            jitter: Duration::from_millis(config.jitter_ms.unwrap_or(0)),
            target,
            stats: FaultStats::new(),
        }
    }

    /// Compute the actual delay (base + random jitter).
    fn compute_delay(&self) -> Duration {
        if self.jitter.is_zero() {
            return self.base_delay;
        }

        let mut rng = rand::thread_rng();
        let jitter_ms = rng.gen_range(0..=self.jitter.as_millis() as u64);
        self.base_delay + Duration::from_millis(jitter_ms)
    }
}

impl ModbusFault for DelayedResponseFault {
    fn fault_type(&self) -> &'static str {
        "delayed_response"
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

        let delay = self.compute_delay();

        FaultAction::DelayThenSend {
            delay,
            response: ctx.response_pdu.clone(),
        }
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
            1,
            0x03,
            &[0x03, 0x00, 0x00, 0x00, 0x01],
            &[0x03, 0x02, 0x00, 0x64],
            1,
            1,
        )
    }

    #[test]
    fn test_fixed_delay() {
        let fault = DelayedResponseFault::new(
            Duration::from_millis(500),
            Duration::ZERO,
            FaultTarget::new(),
        );
        let action = fault.apply(&test_ctx());

        match action {
            FaultAction::DelayThenSend { delay, response } => {
                assert_eq!(delay, Duration::from_millis(500));
                assert_eq!(response, vec![0x03, 0x02, 0x00, 0x64]);
            }
            _ => panic!("Expected DelayThenSend"),
        }
    }

    #[test]
    fn test_delay_with_jitter() {
        let fault = DelayedResponseFault::new(
            Duration::from_millis(100),
            Duration::from_millis(200),
            FaultTarget::new(),
        );

        // Run multiple times to verify jitter range
        for _ in 0..20 {
            let action = fault.apply(&test_ctx());
            match action {
                FaultAction::DelayThenSend { delay, .. } => {
                    assert!(delay >= Duration::from_millis(100));
                    assert!(delay <= Duration::from_millis(300));
                }
                _ => panic!("Expected DelayThenSend"),
            }
        }
    }

    #[test]
    fn test_response_intact() {
        let fault =
            DelayedResponseFault::new(Duration::from_secs(1), Duration::ZERO, FaultTarget::new());
        let ctx = test_ctx();
        let action = fault.apply(&ctx);

        match action {
            FaultAction::DelayThenSend { response, .. } => {
                assert_eq!(response, ctx.response_pdu);
            }
            _ => panic!("Expected DelayThenSend"),
        }
    }

    #[test]
    fn test_from_config() {
        let config = FaultTypeConfig {
            delay_ms: Some(2000),
            jitter_ms: Some(500),
            ..Default::default()
        };
        let fault = DelayedResponseFault::from_config(&config, FaultTarget::new());
        let action = fault.apply(&test_ctx());

        match action {
            FaultAction::DelayThenSend { delay, .. } => {
                assert!(delay >= Duration::from_millis(2000));
                assert!(delay <= Duration::from_millis(2500));
            }
            _ => panic!("Expected DelayThenSend"),
        }
    }

    #[test]
    fn test_from_config_defaults() {
        let config = FaultTypeConfig::default();
        let fault = DelayedResponseFault::from_config(&config, FaultTarget::new());
        let action = fault.apply(&test_ctx());

        match action {
            FaultAction::DelayThenSend { delay, .. } => {
                assert_eq!(delay, Duration::from_millis(1000));
            }
            _ => panic!("Expected DelayThenSend"),
        }
    }

    #[test]
    fn test_stats() {
        let fault = DelayedResponseFault::new(
            Duration::from_millis(100),
            Duration::ZERO,
            FaultTarget::new(),
        );
        let ctx = test_ctx();

        assert!(fault.should_activate(&ctx));
        fault.apply(&ctx);

        let stats = fault.stats();
        assert_eq!(stats.checks, 1);
        assert_eq!(stats.activations, 1);
        assert_eq!(stats.affected_requests, 1);
    }
}

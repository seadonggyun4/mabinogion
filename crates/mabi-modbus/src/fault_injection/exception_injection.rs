//! Exception code injection fault.
//!
//! Forces specific Modbus exception codes in responses, testing
//! the full `ErrorClassifier` 4-way classification in trap-modbus.

use super::config::FaultTypeConfig;
use super::stats::{FaultStats, FaultStatsSnapshot};
use super::targeting::FaultTarget;
use super::{FaultAction, ModbusFault, ModbusFaultContext};

/// Injects forced Modbus exception responses.
///
/// Replaces the normal response with an exception response PDU,
/// regardless of whether the actual operation succeeded. This tests
/// how the client handles various exception codes.
///
/// # Exception PDU Format
///
/// ```text
/// [FC | 0x80] [ExceptionCode]
/// ```
///
/// # Common Exception Codes for Testing
///
/// - `0x01` IllegalFunction → `ErrorClassifier::Unrecoverable`
/// - `0x02` IllegalDataAddress → `ErrorClassifier::Unrecoverable`
/// - `0x04` SlaveDeviceFailure → `ErrorClassifier::RetryImmediate`
/// - `0x06` SlaveDeviceBusy → `ErrorClassifier::RetryImmediate`
/// - `0x0B` GatewayTargetFailed → `ErrorClassifier::LinkRecovery`
pub struct ExceptionInjectionFault {
    /// The exception code to inject.
    exception_code: u8,
    target: FaultTarget,
    stats: FaultStats,
}

impl ExceptionInjectionFault {
    /// Create a new exception injection fault.
    pub fn new(exception_code: u8, target: FaultTarget) -> Self {
        Self {
            exception_code,
            target,
            stats: FaultStats::new(),
        }
    }

    /// Create from config.
    pub fn from_config(config: &FaultTypeConfig, target: FaultTarget) -> Self {
        Self {
            exception_code: config.exception_code.unwrap_or(0x04),
            target,
            stats: FaultStats::new(),
        }
    }

    /// Get the configured exception code.
    pub fn exception_code(&self) -> u8 {
        self.exception_code
    }
}

impl ModbusFault for ExceptionInjectionFault {
    fn fault_type(&self) -> &'static str {
        "exception_injection"
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

        // Build exception response PDU: [FC | 0x80, exception_code]
        let exception_pdu = vec![ctx.function_code | 0x80, self.exception_code];

        FaultAction::SendResponse(exception_pdu)
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
    fn test_illegal_function() {
        let fault = ExceptionInjectionFault::new(0x01, FaultTarget::new());
        let action = fault.apply(&test_ctx());

        match action {
            FaultAction::SendResponse(pdu) => {
                assert_eq!(pdu.len(), 2);
                assert_eq!(pdu[0], 0x83); // 0x03 | 0x80
                assert_eq!(pdu[1], 0x01);
            }
            _ => panic!("Expected SendResponse"),
        }
    }

    #[test]
    fn test_illegal_data_address() {
        let fault = ExceptionInjectionFault::new(0x02, FaultTarget::new());
        let action = fault.apply(&test_ctx());

        match action {
            FaultAction::SendResponse(pdu) => {
                assert_eq!(pdu[0], 0x83);
                assert_eq!(pdu[1], 0x02);
            }
            _ => panic!("Expected SendResponse"),
        }
    }

    #[test]
    fn test_slave_device_failure() {
        let fault = ExceptionInjectionFault::new(0x04, FaultTarget::new());
        let action = fault.apply(&test_ctx());

        match action {
            FaultAction::SendResponse(pdu) => {
                assert_eq!(pdu[0], 0x83);
                assert_eq!(pdu[1], 0x04);
            }
            _ => panic!("Expected SendResponse"),
        }
    }

    #[test]
    fn test_slave_device_busy() {
        let fault = ExceptionInjectionFault::new(0x06, FaultTarget::new());
        let action = fault.apply(&test_ctx());

        match action {
            FaultAction::SendResponse(pdu) => {
                assert_eq!(pdu[0], 0x83);
                assert_eq!(pdu[1], 0x06);
            }
            _ => panic!("Expected SendResponse"),
        }
    }

    #[test]
    fn test_gateway_target_failed() {
        let fault = ExceptionInjectionFault::new(0x0B, FaultTarget::new());
        let action = fault.apply(&test_ctx());

        match action {
            FaultAction::SendResponse(pdu) => {
                assert_eq!(pdu[0], 0x83);
                assert_eq!(pdu[1], 0x0B);
            }
            _ => panic!("Expected SendResponse"),
        }
    }

    #[test]
    fn test_different_function_codes() {
        let fault = ExceptionInjectionFault::new(0x01, FaultTarget::new());

        // FC 0x10 Write Multiple Registers
        let ctx = ModbusFaultContext::tcp(1, 0x10, &[0x10], &[0x10, 0x00], 1, 1);
        match fault.apply(&ctx) {
            FaultAction::SendResponse(pdu) => {
                assert_eq!(pdu[0], 0x90); // 0x10 | 0x80
                assert_eq!(pdu[1], 0x01);
            }
            _ => panic!("Expected SendResponse"),
        }

        // FC 0x01 Read Coils
        let ctx = ModbusFaultContext::tcp(1, 0x01, &[0x01], &[0x01, 0x01, 0xFF], 1, 1);
        match fault.apply(&ctx) {
            FaultAction::SendResponse(pdu) => {
                assert_eq!(pdu[0], 0x81); // 0x01 | 0x80
                assert_eq!(pdu[1], 0x01);
            }
            _ => panic!("Expected SendResponse"),
        }
    }

    #[test]
    fn test_all_standard_exception_codes() {
        let codes = [0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x08, 0x0A, 0x0B];
        for &code in &codes {
            let fault = ExceptionInjectionFault::new(code, FaultTarget::new());
            let action = fault.apply(&test_ctx());
            match action {
                FaultAction::SendResponse(pdu) => {
                    assert_eq!(pdu[1], code, "Exception code 0x{:02X} mismatch", code);
                }
                _ => panic!("Expected SendResponse for code 0x{:02X}", code),
            }
        }
    }

    #[test]
    fn test_from_config() {
        let config = FaultTypeConfig {
            exception_code: Some(0x06),
            ..Default::default()
        };
        let fault = ExceptionInjectionFault::from_config(&config, FaultTarget::new());
        assert_eq!(fault.exception_code(), 0x06);
    }

    #[test]
    fn test_from_config_default() {
        let config = FaultTypeConfig::default();
        let fault = ExceptionInjectionFault::from_config(&config, FaultTarget::new());
        assert_eq!(fault.exception_code(), 0x04); // default: SlaveDeviceFailure
    }

    #[test]
    fn test_stats() {
        let fault = ExceptionInjectionFault::new(0x01, FaultTarget::new());
        let ctx = test_ctx();

        assert!(fault.should_activate(&ctx));
        fault.apply(&ctx);

        let stats = fault.stats();
        assert_eq!(stats.checks, 1);
        assert_eq!(stats.activations, 1);
        assert_eq!(stats.affected_requests, 1);
    }
}

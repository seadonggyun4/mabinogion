//! Protocol timeout fault injection.

use std::any::Any;
use std::time::Duration;

use async_trait::async_trait;
use rand::prelude::*;
use serde::{Deserialize, Serialize};

use crate::context::FaultContext;
use crate::error::ChaosResult;
use crate::fault::{
    BaseFault, Fault, FaultBehavior, FaultCategory, FaultMetadata, FaultSeverity, FaultStatistics,
};

// =============================================================================
// Timeout Pattern
// =============================================================================

/// Pattern for timeout behavior.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum TimeoutPattern {
    /// No response at all.
    #[default]
    NoResponse,

    /// Partial response then timeout.
    PartialResponse,

    /// Response after timeout period.
    DelayedResponse,

    /// Intermittent (sometimes works, sometimes times out).
    Intermittent { success_rate: f64 },
}

// =============================================================================
// Timeout Configuration
// =============================================================================

/// Configuration for timeout fault.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimeoutConfig {
    /// Timeout duration in milliseconds.
    pub timeout_ms: u64,

    /// Timeout pattern.
    pub pattern: TimeoutPattern,

    /// Error message for timeout.
    #[serde(default = "default_timeout_message")]
    pub error_message: String,

    /// Whether to actually wait for timeout duration.
    #[serde(default = "default_true")]
    pub wait_duration: bool,
}

fn default_timeout_message() -> String {
    "Request timed out".to_string()
}

fn default_true() -> bool {
    true
}

impl Default for TimeoutConfig {
    fn default() -> Self {
        Self {
            timeout_ms: 5000,
            pattern: TimeoutPattern::NoResponse,
            error_message: default_timeout_message(),
            wait_duration: true,
        }
    }
}

impl TimeoutConfig {
    /// Create a simple timeout config.
    pub fn simple(timeout_ms: u64) -> Self {
        Self {
            timeout_ms,
            ..Default::default()
        }
    }

    /// Create an intermittent timeout config.
    pub fn intermittent(timeout_ms: u64, success_rate: f64) -> Self {
        Self {
            timeout_ms,
            pattern: TimeoutPattern::Intermittent {
                success_rate: success_rate.clamp(0.0, 1.0),
            },
            ..Default::default()
        }
    }

    /// Create a delayed response config.
    pub fn delayed(timeout_ms: u64) -> Self {
        Self {
            timeout_ms,
            pattern: TimeoutPattern::DelayedResponse,
            ..Default::default()
        }
    }

    /// Don't actually wait for timeout.
    pub fn no_wait(mut self) -> Self {
        self.wait_duration = false;
        self
    }

    /// Set error message.
    pub fn with_message(mut self, message: impl Into<String>) -> Self {
        self.error_message = message.into();
        self
    }
}

// =============================================================================
// Timeout Fault
// =============================================================================

/// Protocol timeout fault implementation.
///
/// Simulates request timeouts at the protocol level.
///
/// # Example
///
/// ```rust,ignore
/// use mabi_chaos::protocol::TimeoutFault;
///
/// // Simple 5 second timeout
/// let fault = TimeoutFault::builder()
///     .id("timeout-001")
///     .timeout_ms(5000)
///     .build();
///
/// // Intermittent timeouts (70% success rate)
/// let fault = TimeoutFault::builder()
///     .id("timeout-002")
///     .intermittent(3000, 0.7)
///     .build();
/// ```
#[derive(Debug, Clone)]
pub struct TimeoutFault {
    base: BaseFault,
    config: TimeoutConfig,
    rng: StdRng,
}

impl TimeoutFault {
    /// Create a new timeout fault.
    pub fn new(id: impl Into<String>, config: TimeoutConfig) -> Self {
        let id = id.into();
        let metadata = FaultMetadata::new(&id, "Protocol Timeout", FaultCategory::Protocol)
            .with_description("Simulates request timeouts")
            .with_severity(FaultSeverity::High);

        Self {
            base: BaseFault::new(metadata),
            config,
            rng: StdRng::from_entropy(),
        }
    }

    /// Create a builder.
    pub fn builder() -> TimeoutFaultBuilder {
        TimeoutFaultBuilder::default()
    }

    /// Check if should timeout based on pattern.
    fn should_timeout(&mut self) -> bool {
        match &self.config.pattern {
            TimeoutPattern::NoResponse | TimeoutPattern::PartialResponse => true,
            TimeoutPattern::DelayedResponse => true,
            TimeoutPattern::Intermittent { success_rate } => {
                self.rng.gen::<f64>() > *success_rate
            }
        }
    }

    /// Get configuration.
    pub fn config(&self) -> &TimeoutConfig {
        &self.config
    }
}

#[async_trait]
impl Fault for TimeoutFault {
    fn metadata(&self) -> &FaultMetadata {
        &self.base.metadata
    }

    fn enable(&mut self) {
        self.base.enable();
    }

    fn disable(&mut self) {
        self.base.disable();
    }

    async fn should_activate(&self, ctx: &FaultContext) -> ChaosResult<bool> {
        if !self.base.matches_target(ctx.target.identifier()) {
            return Ok(false);
        }
        Ok(true)
    }

    async fn apply(&self, ctx: &mut FaultContext) -> ChaosResult<FaultBehavior> {
        let should_timeout = {
            let mut this = self.clone();
            if !this.base.check_probability() {
                return Ok(FaultBehavior::Continue);
            }
            this.should_timeout()
        };

        if !should_timeout {
            return Ok(FaultBehavior::Continue);
        }

        ctx.record_applied_fault(self.id(), "timeout");

        tracing::debug!(
            fault_id = %self.id(),
            target = %ctx.target.device_id,
            timeout_ms = %self.config.timeout_ms,
            pattern = ?self.config.pattern,
            "Simulating timeout"
        );

        // Wait if configured
        if self.config.wait_duration {
            let wait_time = match &self.config.pattern {
                TimeoutPattern::PartialResponse => self.config.timeout_ms / 2,
                TimeoutPattern::DelayedResponse => self.config.timeout_ms,
                _ => self.config.timeout_ms,
            };
            ctx.add_delay(wait_time);
            tokio::time::sleep(Duration::from_millis(wait_time)).await;
        }

        match &self.config.pattern {
            TimeoutPattern::DelayedResponse => {
                // Allow the response after delay
                Ok(FaultBehavior::Delay {
                    duration_ms: self.config.timeout_ms,
                })
            }
            _ => {
                Ok(FaultBehavior::Abort {
                    error: self.config.error_message.clone(),
                })
            }
        }
    }

    fn reset(&mut self) {
        self.base.reset();
    }

    fn statistics(&self) -> FaultStatistics {
        self.base.statistics.clone()
    }

    fn clone_box(&self) -> Box<dyn Fault> {
        Box::new(self.clone())
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

// =============================================================================
// Builder
// =============================================================================

/// Builder for timeout fault.
#[derive(Debug, Default)]
pub struct TimeoutFaultBuilder {
    id: Option<String>,
    name: Option<String>,
    config: TimeoutConfig,
    probability: f64,
    targets: Vec<String>,
    severity: FaultSeverity,
}

impl TimeoutFaultBuilder {
    /// Set fault ID.
    pub fn id(mut self, id: impl Into<String>) -> Self {
        self.id = Some(id.into());
        self
    }

    /// Set fault name.
    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    /// Set timeout duration.
    pub fn timeout_ms(mut self, ms: u64) -> Self {
        self.config.timeout_ms = ms;
        self
    }

    /// Set pattern.
    pub fn pattern(mut self, pattern: TimeoutPattern) -> Self {
        self.config.pattern = pattern;
        self
    }

    /// Set intermittent pattern.
    pub fn intermittent(mut self, timeout_ms: u64, success_rate: f64) -> Self {
        self.config.timeout_ms = timeout_ms;
        self.config.pattern = TimeoutPattern::Intermittent {
            success_rate: success_rate.clamp(0.0, 1.0),
        };
        self
    }

    /// Set delayed response pattern.
    pub fn delayed(mut self) -> Self {
        self.config.pattern = TimeoutPattern::DelayedResponse;
        self
    }

    /// Don't wait for timeout.
    pub fn no_wait(mut self) -> Self {
        self.config.wait_duration = false;
        self
    }

    /// Set error message.
    pub fn message(mut self, message: impl Into<String>) -> Self {
        self.config.error_message = message.into();
        self
    }

    /// Set probability.
    pub fn probability(mut self, p: f64) -> Self {
        self.probability = p.clamp(0.0, 1.0);
        self
    }

    /// Add target pattern.
    pub fn target(mut self, target: impl Into<String>) -> Self {
        self.targets.push(target.into());
        self
    }

    /// Set severity.
    pub fn severity(mut self, severity: FaultSeverity) -> Self {
        self.severity = severity;
        self
    }

    /// Build the fault.
    pub fn build(self) -> TimeoutFault {
        let id = self.id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        let mut fault = TimeoutFault::new(&id, self.config);

        if let Some(name) = self.name {
            fault.base.metadata.name = name;
        }

        fault.base.metadata.probability = if self.probability == 0.0 {
            1.0
        } else {
            self.probability
        };
        fault.base.metadata.targets = self.targets;
        fault.base.metadata.severity = self.severity;

        fault
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_always_timeout() {
        let config = TimeoutConfig::simple(100);
        let mut fault = TimeoutFault::new("test", config);

        assert!(fault.should_timeout());
    }

    #[test]
    fn test_intermittent_timeout() {
        let config = TimeoutConfig::intermittent(100, 0.5);
        let mut fault = TimeoutFault::new("test", config);

        let mut timeouts = 0;
        let iterations = 1000;

        for _ in 0..iterations {
            if fault.should_timeout() {
                timeouts += 1;
            }
        }

        // Should be approximately 50% timeouts
        let ratio = timeouts as f64 / iterations as f64;
        assert!(ratio > 0.4 && ratio < 0.6, "ratio was {}", ratio);
    }

    #[test]
    fn test_builder() {
        let fault = TimeoutFault::builder()
            .id("timeout-001")
            .timeout_ms(3000)
            .intermittent(3000, 0.8)
            .no_wait()
            .message("Custom timeout message")
            .target("device-*")
            .build();

        assert_eq!(fault.id(), "timeout-001");
        assert_eq!(fault.config.timeout_ms, 3000);
        assert!(!fault.config.wait_duration);
    }
}

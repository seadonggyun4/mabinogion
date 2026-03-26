//! State transition fault injection.

use std::any::Any;

use async_trait::async_trait;
use rand::prelude::*;
use serde::{Deserialize, Serialize};

use crate::context::{FaultContext, OperationType};
use crate::error::ChaosResult;
use crate::fault::{
    BaseFault, Fault, FaultBehavior, FaultCategory, FaultMetadata, FaultSeverity, FaultStatistics,
};

// =============================================================================
// Transition Configuration
// =============================================================================

/// Configuration for state transition faults.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransitionConfig {
    /// Probability of failing initialization.
    #[serde(default)]
    pub fail_init_probability: f64,

    /// Probability of failing start.
    #[serde(default)]
    pub fail_start_probability: f64,

    /// Probability of failing stop.
    #[serde(default)]
    pub fail_stop_probability: f64,

    /// Probability of failing tick.
    #[serde(default)]
    pub fail_tick_probability: f64,

    /// Whether failures are permanent until reset.
    #[serde(default)]
    pub permanent_failure: bool,

    /// Number of retries before permanent failure.
    #[serde(default = "default_retries")]
    pub retry_limit: u32,

    /// Error message for failed transitions.
    #[serde(default = "default_error")]
    pub error_message: String,

    /// Error code for failed transitions.
    #[serde(default)]
    pub error_code: i32,
}

fn default_retries() -> u32 {
    3
}

fn default_error() -> String {
    "State transition failed".to_string()
}

impl Default for TransitionConfig {
    fn default() -> Self {
        Self {
            fail_init_probability: 0.0,
            fail_start_probability: 0.0,
            fail_stop_probability: 0.0,
            fail_tick_probability: 0.0,
            permanent_failure: false,
            retry_limit: 3,
            error_message: default_error(),
            error_code: -1,
        }
    }
}

impl TransitionConfig {
    /// Create config for failing initialization.
    pub fn fail_init(probability: f64) -> Self {
        Self {
            fail_init_probability: probability.clamp(0.0, 1.0),
            ..Default::default()
        }
    }

    /// Create config for failing start.
    pub fn fail_start(probability: f64) -> Self {
        Self {
            fail_start_probability: probability.clamp(0.0, 1.0),
            ..Default::default()
        }
    }

    /// Create config for failing all transitions.
    pub fn fail_all(probability: f64) -> Self {
        let p = probability.clamp(0.0, 1.0);
        Self {
            fail_init_probability: p,
            fail_start_probability: p,
            fail_stop_probability: p,
            fail_tick_probability: p,
            ..Default::default()
        }
    }

    /// Set permanent failure.
    pub fn with_permanent(mut self) -> Self {
        self.permanent_failure = true;
        self
    }

    /// Set retry limit.
    pub fn with_retries(mut self, limit: u32) -> Self {
        self.retry_limit = limit;
        self
    }

    /// Set error message.
    pub fn with_error(mut self, code: i32, message: impl Into<String>) -> Self {
        self.error_code = code;
        self.error_message = message.into();
        self
    }
}

// =============================================================================
// State Transition Fault
// =============================================================================

/// State transition fault implementation.
///
/// Fails device state transitions like init, start, stop, or tick.
///
/// # Example
///
/// ```rust,ignore
/// use mabi_chaos::device::StateTransitionFault;
///
/// // Fail 10% of initializations
/// let fault = StateTransitionFault::builder()
///     .id("state-001")
///     .fail_init(0.1)
///     .build();
///
/// // Fail all transitions with 5% probability
/// let fault = StateTransitionFault::builder()
///     .id("state-002")
///     .fail_all(0.05)
///     .permanent()
///     .build();
/// ```
#[derive(Debug, Clone)]
pub struct StateTransitionFault {
    base: BaseFault,
    config: TransitionConfig,
    rng: StdRng,

    // State tracking
    failed_permanently: bool,
    failure_counts: std::collections::HashMap<String, u32>,
}

impl StateTransitionFault {
    /// Create a new state transition fault.
    pub fn new(id: impl Into<String>, config: TransitionConfig) -> Self {
        let id = id.into();
        let metadata = FaultMetadata::new(&id, "State Transition Failure", FaultCategory::Device)
            .with_description("Fails device state transitions")
            .with_severity(FaultSeverity::High);

        Self {
            base: BaseFault::new(metadata),
            config,
            rng: StdRng::from_entropy(),
            failed_permanently: false,
            failure_counts: std::collections::HashMap::new(),
        }
    }

    /// Create a builder.
    pub fn builder() -> TransitionFaultBuilder {
        TransitionFaultBuilder::default()
    }

    /// Check if should fail the given operation.
    fn should_fail(&mut self, operation: &str) -> bool {
        if self.failed_permanently {
            return true;
        }

        let probability = match operation {
            "init" | "initialize" => self.config.fail_init_probability,
            "start" => self.config.fail_start_probability,
            "stop" => self.config.fail_stop_probability,
            "tick" => self.config.fail_tick_probability,
            _ => 0.0,
        };

        if probability <= 0.0 {
            return false;
        }

        if self.rng.gen::<f64>() < probability {
            let count = self
                .failure_counts
                .entry(operation.to_string())
                .or_insert(0);
            *count += 1;

            if self.config.permanent_failure && *count >= self.config.retry_limit {
                self.failed_permanently = true;
            }

            true
        } else {
            false
        }
    }

    /// Get configuration.
    pub fn config(&self) -> &TransitionConfig {
        &self.config
    }

    /// Check if permanently failed.
    pub fn is_permanently_failed(&self) -> bool {
        self.failed_permanently
    }

    /// Get failure count for operation.
    pub fn failure_count(&self, operation: &str) -> u32 {
        self.failure_counts.get(operation).copied().unwrap_or(0)
    }

    /// Clear permanent failure.
    pub fn clear_failure(&mut self) {
        self.failed_permanently = false;
        self.failure_counts.clear();
    }
}

#[async_trait]
impl Fault for StateTransitionFault {
    fn metadata(&self) -> &FaultMetadata {
        &self.base.metadata
    }

    fn enable(&mut self) {
        self.base.enable();
    }

    fn disable(&mut self) {
        self.base.disable();
        self.clear_failure();
    }

    async fn should_activate(&self, ctx: &FaultContext) -> ChaosResult<bool> {
        if !self.base.matches_target(ctx.target.identifier()) {
            return Ok(false);
        }

        // Only apply to lifecycle operations
        match &ctx.operation {
            OperationType::Initialize
            | OperationType::Start
            | OperationType::Stop
            | OperationType::Tick => Ok(true),
            _ => Ok(false),
        }
    }

    async fn apply(&self, ctx: &mut FaultContext) -> ChaosResult<FaultBehavior> {
        let operation = match &ctx.operation {
            OperationType::Initialize => "initialize",
            OperationType::Start => "start",
            OperationType::Stop => "stop",
            OperationType::Tick => "tick",
            _ => return Ok(FaultBehavior::Continue),
        };

        let should_fail = {
            let mut this = self.clone();
            if !this.base.check_probability() {
                return Ok(FaultBehavior::Continue);
            }
            this.should_fail(operation)
        };

        if should_fail {
            ctx.record_applied_fault(self.id(), &format!("fail_{}", operation));

            tracing::debug!(
                fault_id = %self.id(),
                target = %ctx.target.device_id,
                operation = %operation,
                permanent = %self.failed_permanently,
                "Failing state transition"
            );

            return Ok(FaultBehavior::ReturnError {
                error_code: self.config.error_code,
                message: format!("{}: {}", self.config.error_message, operation),
            });
        }

        Ok(FaultBehavior::Continue)
    }

    fn reset(&mut self) {
        self.base.reset();
        self.clear_failure();
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

/// Builder for state transition fault.
#[derive(Debug, Default)]
pub struct TransitionFaultBuilder {
    id: Option<String>,
    name: Option<String>,
    config: TransitionConfig,
    probability: f64,
    targets: Vec<String>,
    severity: FaultSeverity,
}

impl TransitionFaultBuilder {
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

    /// Set fail init probability.
    pub fn fail_init(mut self, probability: f64) -> Self {
        self.config.fail_init_probability = probability.clamp(0.0, 1.0);
        self
    }

    /// Set fail start probability.
    pub fn fail_start(mut self, probability: f64) -> Self {
        self.config.fail_start_probability = probability.clamp(0.0, 1.0);
        self
    }

    /// Set fail stop probability.
    pub fn fail_stop(mut self, probability: f64) -> Self {
        self.config.fail_stop_probability = probability.clamp(0.0, 1.0);
        self
    }

    /// Set fail tick probability.
    pub fn fail_tick(mut self, probability: f64) -> Self {
        self.config.fail_tick_probability = probability.clamp(0.0, 1.0);
        self
    }

    /// Fail all transitions with same probability.
    pub fn fail_all(mut self, probability: f64) -> Self {
        let p = probability.clamp(0.0, 1.0);
        self.config.fail_init_probability = p;
        self.config.fail_start_probability = p;
        self.config.fail_stop_probability = p;
        self.config.fail_tick_probability = p;
        self
    }

    /// Set permanent failure mode.
    pub fn permanent(mut self) -> Self {
        self.config.permanent_failure = true;
        self
    }

    /// Set retry limit.
    pub fn retries(mut self, limit: u32) -> Self {
        self.config.retry_limit = limit;
        self
    }

    /// Set error response.
    pub fn error(mut self, code: i32, message: impl Into<String>) -> Self {
        self.config.error_code = code;
        self.config.error_message = message.into();
        self
    }

    /// Set activation probability.
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
    pub fn build(self) -> StateTransitionFault {
        let id = self.id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        let mut fault = StateTransitionFault::new(&id, self.config);

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
    fn test_fail_init() {
        let config = TransitionConfig::fail_init(1.0);
        let mut fault = StateTransitionFault::new("test", config);

        assert!(fault.should_fail("init"));
        assert!(fault.should_fail("initialize"));
        assert!(!fault.should_fail("start"));
    }

    #[test]
    fn test_fail_all() {
        let config = TransitionConfig::fail_all(1.0);
        let mut fault = StateTransitionFault::new("test", config);

        assert!(fault.should_fail("init"));
        assert!(fault.should_fail("start"));
        assert!(fault.should_fail("stop"));
        assert!(fault.should_fail("tick"));
    }

    #[test]
    fn test_permanent_failure() {
        let config = TransitionConfig::fail_init(1.0)
            .with_permanent()
            .with_retries(2);
        let mut fault = StateTransitionFault::new("test", config);

        // First two failures
        assert!(fault.should_fail("init"));
        assert!(!fault.is_permanently_failed());
        assert!(fault.should_fail("init"));
        assert!(fault.is_permanently_failed());

        // Now everything should fail
        assert!(fault.should_fail("start"));
        assert!(fault.should_fail("tick"));
    }

    #[test]
    fn test_clear_failure() {
        let config = TransitionConfig::fail_init(1.0)
            .with_permanent()
            .with_retries(1);
        let mut fault = StateTransitionFault::new("test", config);

        fault.should_fail("init");
        assert!(fault.is_permanently_failed());

        fault.clear_failure();
        assert!(!fault.is_permanently_failed());
    }

    #[test]
    fn test_builder() {
        let fault = StateTransitionFault::builder()
            .id("state-001")
            .fail_init(0.1)
            .fail_start(0.2)
            .permanent()
            .retries(5)
            .error(-100, "Device initialization failed")
            .target("device-*")
            .build();

        assert_eq!(fault.id(), "state-001");
        assert_eq!(fault.config.fail_init_probability, 0.1);
        assert_eq!(fault.config.fail_start_probability, 0.2);
        assert!(fault.config.permanent_failure);
    }
}

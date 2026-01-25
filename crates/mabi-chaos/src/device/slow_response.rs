//! Slow response fault injection.

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
// Slow Response Configuration
// =============================================================================

/// Configuration for slow response fault.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlowResponseConfig {
    /// Base delay in milliseconds.
    pub base_delay_ms: u64,

    /// Maximum additional random delay.
    #[serde(default)]
    pub max_additional_delay_ms: u64,

    /// Delay multiplier (scales with request size or complexity).
    #[serde(default = "default_multiplier")]
    pub multiplier: f64,

    /// Whether delay scales with number of points requested.
    #[serde(default)]
    pub scale_with_points: bool,

    /// Per-point delay when scaling (ms).
    #[serde(default)]
    pub per_point_delay_ms: u64,

    /// Progressive slowdown (each request gets slower).
    #[serde(default)]
    pub progressive: bool,

    /// Progressive increase per request (ms).
    #[serde(default)]
    pub progressive_increase_ms: u64,

    /// Maximum progressive delay (ms).
    #[serde(default = "default_max_progressive")]
    pub max_progressive_delay_ms: u64,
}

fn default_multiplier() -> f64 {
    1.0
}

fn default_max_progressive() -> u64 {
    30000
}

impl Default for SlowResponseConfig {
    fn default() -> Self {
        Self {
            base_delay_ms: 1000,
            max_additional_delay_ms: 0,
            multiplier: 1.0,
            scale_with_points: false,
            per_point_delay_ms: 0,
            progressive: false,
            progressive_increase_ms: 0,
            max_progressive_delay_ms: 30000,
        }
    }
}

impl SlowResponseConfig {
    /// Create a simple delay config.
    pub fn fixed(delay_ms: u64) -> Self {
        Self {
            base_delay_ms: delay_ms,
            ..Default::default()
        }
    }

    /// Create a random delay config.
    pub fn random(min_ms: u64, max_ms: u64) -> Self {
        Self {
            base_delay_ms: min_ms,
            max_additional_delay_ms: max_ms.saturating_sub(min_ms),
            ..Default::default()
        }
    }

    /// Create a scaling delay config.
    pub fn scaling(base_ms: u64, per_point_ms: u64) -> Self {
        Self {
            base_delay_ms: base_ms,
            scale_with_points: true,
            per_point_delay_ms: per_point_ms,
            ..Default::default()
        }
    }

    /// Create a progressive delay config.
    pub fn progressive(start_ms: u64, increase_ms: u64, max_ms: u64) -> Self {
        Self {
            base_delay_ms: start_ms,
            progressive: true,
            progressive_increase_ms: increase_ms,
            max_progressive_delay_ms: max_ms,
            ..Default::default()
        }
    }

    /// Set multiplier.
    pub fn with_multiplier(mut self, multiplier: f64) -> Self {
        self.multiplier = multiplier;
        self
    }
}

// =============================================================================
// Slow Response Fault
// =============================================================================

/// Slow response fault implementation.
///
/// Simulates slow device responses with configurable patterns.
///
/// # Example
///
/// ```rust,ignore
/// use mabi_chaos::device::SlowResponseFault;
///
/// // Fixed 500ms delay
/// let fault = SlowResponseFault::builder()
///     .id("slow-001")
///     .delay_ms(500)
///     .build();
///
/// // Progressive slowdown
/// let fault = SlowResponseFault::builder()
///     .id("slow-002")
///     .progressive(100, 50, 5000)
///     .build();
/// ```
#[derive(Debug, Clone)]
pub struct SlowResponseFault {
    base: BaseFault,
    config: SlowResponseConfig,
    rng: StdRng,

    // State for progressive delay
    current_progressive_delay_ms: u64,
    request_count: u64,
}

impl SlowResponseFault {
    /// Create a new slow response fault.
    pub fn new(id: impl Into<String>, config: SlowResponseConfig) -> Self {
        let id = id.into();
        let metadata = FaultMetadata::new(&id, "Slow Response", FaultCategory::Device)
            .with_description("Simulates slow device responses")
            .with_severity(FaultSeverity::Medium);

        Self {
            base: BaseFault::new(metadata),
            config,
            rng: StdRng::from_entropy(),
            current_progressive_delay_ms: 0,
            request_count: 0,
        }
    }

    /// Create a builder.
    pub fn builder() -> SlowResponseFaultBuilder {
        SlowResponseFaultBuilder::default()
    }

    /// Calculate delay for a request.
    pub fn calculate_delay(&mut self, point_count: usize) -> Duration {
        self.request_count += 1;

        let mut delay_ms = self.config.base_delay_ms;

        // Add random component
        if self.config.max_additional_delay_ms > 0 {
            delay_ms += self.rng.gen_range(0..=self.config.max_additional_delay_ms);
        }

        // Scale with points
        if self.config.scale_with_points && point_count > 0 {
            delay_ms += self.config.per_point_delay_ms * point_count as u64;
        }

        // Progressive increase
        if self.config.progressive {
            self.current_progressive_delay_ms = (self.current_progressive_delay_ms
                + self.config.progressive_increase_ms)
                .min(self.config.max_progressive_delay_ms);
            delay_ms += self.current_progressive_delay_ms;
        }

        // Apply multiplier
        delay_ms = (delay_ms as f64 * self.config.multiplier) as u64;

        Duration::from_millis(delay_ms)
    }

    /// Get configuration.
    pub fn config(&self) -> &SlowResponseConfig {
        &self.config
    }

    /// Get request count.
    pub fn request_count(&self) -> u64 {
        self.request_count
    }

    /// Reset progressive state.
    pub fn reset_progressive(&mut self) {
        self.current_progressive_delay_ms = 0;
        self.request_count = 0;
    }
}

#[async_trait]
impl Fault for SlowResponseFault {
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
        let delay = {
            let mut this = self.clone();
            if !this.base.check_probability() {
                return Ok(FaultBehavior::Continue);
            }
            let point_count = ctx.point_ids().len();
            this.calculate_delay(point_count)
        };

        let delay_ms = delay.as_millis() as u64;

        if delay_ms > 0 {
            ctx.add_delay(delay_ms);
            ctx.record_applied_fault(self.id(), "slow");

            tracing::debug!(
                fault_id = %self.id(),
                target = %ctx.target.device_id,
                delay_ms = %delay_ms,
                "Injecting slow response"
            );

            tokio::time::sleep(delay).await;

            return Ok(FaultBehavior::Delay { duration_ms: delay_ms });
        }

        Ok(FaultBehavior::Continue)
    }

    fn reset(&mut self) {
        self.base.reset();
        self.reset_progressive();
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

/// Builder for slow response fault.
#[derive(Debug, Default)]
pub struct SlowResponseFaultBuilder {
    id: Option<String>,
    name: Option<String>,
    config: SlowResponseConfig,
    probability: f64,
    targets: Vec<String>,
    severity: FaultSeverity,
}

impl SlowResponseFaultBuilder {
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

    /// Set fixed delay.
    pub fn delay_ms(mut self, ms: u64) -> Self {
        self.config.base_delay_ms = ms;
        self
    }

    /// Set random delay range.
    pub fn random_delay(mut self, min_ms: u64, max_ms: u64) -> Self {
        self.config.base_delay_ms = min_ms;
        self.config.max_additional_delay_ms = max_ms.saturating_sub(min_ms);
        self
    }

    /// Set scaling with points.
    pub fn scale_with_points(mut self, per_point_ms: u64) -> Self {
        self.config.scale_with_points = true;
        self.config.per_point_delay_ms = per_point_ms;
        self
    }

    /// Set progressive delay.
    pub fn progressive(mut self, start_ms: u64, increase_ms: u64, max_ms: u64) -> Self {
        self.config.base_delay_ms = start_ms;
        self.config.progressive = true;
        self.config.progressive_increase_ms = increase_ms;
        self.config.max_progressive_delay_ms = max_ms;
        self
    }

    /// Set multiplier.
    pub fn multiplier(mut self, m: f64) -> Self {
        self.config.multiplier = m;
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
    pub fn build(self) -> SlowResponseFault {
        let id = self.id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        let mut fault = SlowResponseFault::new(&id, self.config);

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
    fn test_fixed_delay() {
        let config = SlowResponseConfig::fixed(100);
        let mut fault = SlowResponseFault::new("test", config);

        let delay = fault.calculate_delay(0);
        assert_eq!(delay, Duration::from_millis(100));
    }

    #[test]
    fn test_random_delay() {
        let config = SlowResponseConfig::random(50, 150);
        let mut fault = SlowResponseFault::new("test", config);

        for _ in 0..100 {
            let delay = fault.calculate_delay(0);
            assert!(delay >= Duration::from_millis(50));
            assert!(delay <= Duration::from_millis(150));
        }
    }

    #[test]
    fn test_scaling_delay() {
        let config = SlowResponseConfig::scaling(100, 10);
        let mut fault = SlowResponseFault::new("test", config);

        let delay1 = fault.calculate_delay(1);
        let delay5 = fault.calculate_delay(5);
        let delay10 = fault.calculate_delay(10);

        assert_eq!(delay1, Duration::from_millis(110));
        assert_eq!(delay5, Duration::from_millis(150));
        assert_eq!(delay10, Duration::from_millis(200));
    }

    #[test]
    fn test_progressive_delay() {
        let config = SlowResponseConfig::progressive(100, 50, 300);
        let mut fault = SlowResponseFault::new("test", config);

        let d1 = fault.calculate_delay(0);
        let d2 = fault.calculate_delay(0);
        let d3 = fault.calculate_delay(0);
        let d4 = fault.calculate_delay(0);
        let d5 = fault.calculate_delay(0);
        let d6 = fault.calculate_delay(0);
        let d7 = fault.calculate_delay(0);

        // Progressive delay: base + current_progressive (capped at max_progressive)
        // current_progressive increases by 50 each call, capped at 300
        assert_eq!(d1, Duration::from_millis(150)); // 100 + 50
        assert_eq!(d2, Duration::from_millis(200)); // 100 + 100
        assert_eq!(d3, Duration::from_millis(250)); // 100 + 150
        assert_eq!(d4, Duration::from_millis(300)); // 100 + 200
        assert_eq!(d5, Duration::from_millis(350)); // 100 + 250
        assert_eq!(d6, Duration::from_millis(400)); // 100 + 300 (capped)
        assert_eq!(d7, Duration::from_millis(400)); // 100 + 300 (still capped)
    }

    #[test]
    fn test_builder() {
        let fault = SlowResponseFault::builder()
            .id("slow-001")
            .delay_ms(500)
            .probability(0.5)
            .target("device-*")
            .build();

        assert_eq!(fault.id(), "slow-001");
        assert_eq!(fault.config.base_delay_ms, 500);
    }
}

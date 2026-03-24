//! Network latency fault injection.

use std::any::Any;
use std::time::Duration;

use async_trait::async_trait;
use rand::prelude::*;
use rand_distr::{LogNormal, Normal};
use serde::{Deserialize, Serialize};

use crate::context::FaultContext;
use crate::error::ChaosResult;
use crate::fault::{
    BaseFault, Fault, FaultBehavior, FaultCategory, FaultMetadata, FaultSeverity, FaultStatistics,
};

// =============================================================================
// Latency Distribution
// =============================================================================

/// Latency distribution types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum LatencyDistribution {
    /// Constant latency (no variation).
    Constant,

    /// Uniform distribution between base ± jitter.
    #[default]
    Uniform,

    /// Normal (Gaussian) distribution.
    Normal,

    /// Log-normal distribution (realistic network latency).
    LogNormal,

    /// Exponential distribution.
    Exponential,

    /// Bimodal distribution (simulates good/bad network states).
    Bimodal,
}

// =============================================================================
// Latency Configuration
// =============================================================================

/// Configuration for latency injection.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LatencyConfig {
    /// Base latency in milliseconds.
    pub base_ms: u64,

    /// Jitter (variation) in milliseconds.
    #[serde(default)]
    pub jitter_ms: u64,

    /// Distribution type.
    #[serde(default)]
    pub distribution: LatencyDistribution,

    /// Minimum latency (clamp).
    #[serde(default)]
    pub min_ms: Option<u64>,

    /// Maximum latency (clamp).
    #[serde(default)]
    pub max_ms: Option<u64>,

    /// Spike probability (for occasional large spikes).
    #[serde(default)]
    pub spike_probability: f64,

    /// Spike multiplier (how much to multiply base latency).
    #[serde(default = "default_spike_multiplier")]
    pub spike_multiplier: f64,
}

fn default_spike_multiplier() -> f64 {
    3.0
}

impl Default for LatencyConfig {
    fn default() -> Self {
        Self {
            base_ms: 0,
            jitter_ms: 0,
            distribution: LatencyDistribution::Constant,
            min_ms: None,
            max_ms: None,
            spike_probability: 0.0,
            spike_multiplier: 3.0,
        }
    }
}

impl LatencyConfig {
    /// Create a constant latency config.
    pub fn constant(ms: u64) -> Self {
        Self {
            base_ms: ms,
            distribution: LatencyDistribution::Constant,
            ..Default::default()
        }
    }

    /// Create a uniform distribution config.
    pub fn uniform(base_ms: u64, jitter_ms: u64) -> Self {
        Self {
            base_ms,
            jitter_ms,
            distribution: LatencyDistribution::Uniform,
            ..Default::default()
        }
    }

    /// Create a normal distribution config.
    pub fn normal(base_ms: u64, jitter_ms: u64) -> Self {
        Self {
            base_ms,
            jitter_ms,
            distribution: LatencyDistribution::Normal,
            ..Default::default()
        }
    }

    /// Create a log-normal distribution config (realistic network latency).
    pub fn log_normal(base_ms: u64) -> Self {
        Self {
            base_ms,
            distribution: LatencyDistribution::LogNormal,
            ..Default::default()
        }
    }

    /// Set minimum latency.
    pub fn with_min(mut self, min_ms: u64) -> Self {
        self.min_ms = Some(min_ms);
        self
    }

    /// Set maximum latency.
    pub fn with_max(mut self, max_ms: u64) -> Self {
        self.max_ms = Some(max_ms);
        self
    }

    /// Set spike configuration.
    pub fn with_spikes(mut self, probability: f64, multiplier: f64) -> Self {
        self.spike_probability = probability.clamp(0.0, 1.0);
        self.spike_multiplier = multiplier;
        self
    }
}

// =============================================================================
// Network Latency Fault
// =============================================================================

/// Network latency fault implementation.
///
/// Injects configurable latency into operations to simulate network delays.
///
/// # Example
///
/// ```rust,ignore
/// use mabi_chaos::network::NetworkLatencyFault;
///
/// let fault = NetworkLatencyFault::builder()
///     .id("latency-001")
///     .base_ms(100)
///     .jitter_ms(50)
///     .distribution(LatencyDistribution::Normal)
///     .probability(0.8)
///     .build();
/// ```
#[derive(Debug, Clone)]
pub struct NetworkLatencyFault {
    base: BaseFault,
    config: LatencyConfig,
    rng: StdRng,
}

impl NetworkLatencyFault {
    /// Create a new latency fault.
    pub fn new(id: impl Into<String>, config: LatencyConfig) -> Self {
        let id = id.into();
        let metadata = FaultMetadata::new(&id, "Network Latency", FaultCategory::Network)
            .with_description("Injects configurable latency into operations")
            .with_severity(FaultSeverity::Low);

        Self {
            base: BaseFault::new(metadata),
            config,
            rng: StdRng::from_entropy(),
        }
    }

    /// Create a builder.
    pub fn builder() -> LatencyFaultBuilder {
        LatencyFaultBuilder::default()
    }

    /// Calculate the latency to apply.
    pub fn calculate_latency(&mut self) -> Duration {
        let base = self.config.base_ms as f64;
        let jitter = self.config.jitter_ms as f64;

        // Check for spike
        let (base, jitter) = if self.config.spike_probability > 0.0
            && self.rng.gen::<f64>() < self.config.spike_probability
        {
            (
                base * self.config.spike_multiplier,
                jitter * self.config.spike_multiplier,
            )
        } else {
            (base, jitter)
        };

        let delay_ms = match self.config.distribution {
            LatencyDistribution::Constant => base,

            LatencyDistribution::Uniform => {
                let min = (base - jitter).max(0.0);
                let max = base + jitter;
                if min >= max {
                    base
                } else {
                    self.rng.gen_range(min..=max)
                }
            }

            LatencyDistribution::Normal => {
                let std_dev = (jitter / 3.0).max(0.1);
                let dist = Normal::new(base, std_dev).unwrap();
                dist.sample(&mut self.rng).max(0.0)
            }

            LatencyDistribution::LogNormal => {
                if base > 0.0 {
                    let mu = base.ln();
                    let sigma = 0.5;
                    let dist = LogNormal::new(mu, sigma).unwrap();
                    dist.sample(&mut self.rng)
                } else {
                    0.0
                }
            }

            LatencyDistribution::Exponential => {
                // Exponential with mean = base
                if base > 0.0 {
                    -base * self.rng.gen::<f64>().ln()
                } else {
                    0.0
                }
            }

            LatencyDistribution::Bimodal => {
                // 70% low latency, 30% high latency
                if self.rng.gen::<f64>() < 0.7 {
                    base * 0.5 + self.rng.gen::<f64>() * jitter * 0.5
                } else {
                    base * 2.0 + self.rng.gen::<f64>() * jitter * 2.0
                }
            }
        };

        // Apply clamps
        let delay_ms = if let Some(min) = self.config.min_ms {
            delay_ms.max(min as f64)
        } else {
            delay_ms
        };

        let delay_ms = if let Some(max) = self.config.max_ms {
            delay_ms.min(max as f64)
        } else {
            delay_ms
        };

        Duration::from_millis(delay_ms as u64)
    }

    /// Get the configuration.
    pub fn config(&self) -> &LatencyConfig {
        &self.config
    }

    /// Update the configuration.
    pub fn set_config(&mut self, config: LatencyConfig) {
        self.config = config;
    }
}

#[async_trait]
impl Fault for NetworkLatencyFault {
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
        // Check if target matches
        if !self.base.matches_target(ctx.target.identifier()) {
            return Ok(false);
        }
        Ok(true)
    }

    async fn apply(&self, ctx: &mut FaultContext) -> ChaosResult<FaultBehavior> {
        // Clone self to get mutable access for RNG
        let delay = {
            let mut this = self.clone();
            if !this.base.check_probability() {
                return Ok(FaultBehavior::Continue);
            }
            this.calculate_latency()
        };

        let delay_ms = delay.as_millis() as u64;

        if delay_ms > 0 {
            ctx.add_delay(delay_ms);
            ctx.record_applied_fault(self.id(), "delay");

            tracing::debug!(
                fault_id = %self.id(),
                delay_ms = %delay_ms,
                target = %ctx.target.device_id,
                "Injecting network latency"
            );

            // Actually sleep
            tokio::time::sleep(delay).await;

            return Ok(FaultBehavior::Delay {
                duration_ms: delay_ms,
            });
        }

        Ok(FaultBehavior::Continue)
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

/// Builder for network latency fault.
#[derive(Debug, Default)]
pub struct LatencyFaultBuilder {
    id: Option<String>,
    name: Option<String>,
    config: LatencyConfig,
    probability: f64,
    targets: Vec<String>,
    severity: FaultSeverity,
}

impl LatencyFaultBuilder {
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

    /// Set base latency.
    pub fn base_ms(mut self, ms: u64) -> Self {
        self.config.base_ms = ms;
        self
    }

    /// Set jitter.
    pub fn jitter_ms(mut self, ms: u64) -> Self {
        self.config.jitter_ms = ms;
        self
    }

    /// Set distribution.
    pub fn distribution(mut self, distribution: LatencyDistribution) -> Self {
        self.config.distribution = distribution;
        self
    }

    /// Set minimum latency.
    pub fn min_ms(mut self, ms: u64) -> Self {
        self.config.min_ms = Some(ms);
        self
    }

    /// Set maximum latency.
    pub fn max_ms(mut self, ms: u64) -> Self {
        self.config.max_ms = Some(ms);
        self
    }

    /// Set spike configuration.
    pub fn with_spikes(mut self, probability: f64, multiplier: f64) -> Self {
        self.config.spike_probability = probability;
        self.config.spike_multiplier = multiplier;
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
    pub fn build(self) -> NetworkLatencyFault {
        let id = self.id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        let mut fault = NetworkLatencyFault::new(&id, self.config);

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
    use crate::context::TargetInfo;
    use mabi_core::Protocol;

    #[test]
    fn test_constant_latency() {
        let config = LatencyConfig::constant(100);
        let mut fault = NetworkLatencyFault::new("test", config);

        let delay = fault.calculate_latency();
        assert_eq!(delay, Duration::from_millis(100));
    }

    #[test]
    fn test_uniform_latency() {
        let config = LatencyConfig::uniform(100, 50);
        let mut fault = NetworkLatencyFault::new("test", config);

        for _ in 0..100 {
            let delay = fault.calculate_latency();
            assert!(delay >= Duration::from_millis(50));
            assert!(delay <= Duration::from_millis(150));
        }
    }

    #[test]
    fn test_latency_clamping() {
        let config = LatencyConfig::uniform(100, 200).with_min(50).with_max(120);
        let mut fault = NetworkLatencyFault::new("test", config);

        for _ in 0..100 {
            let delay = fault.calculate_latency();
            assert!(delay >= Duration::from_millis(50));
            assert!(delay <= Duration::from_millis(120));
        }
    }

    #[test]
    fn test_builder() {
        let fault = NetworkLatencyFault::builder()
            .id("latency-001")
            .name("Test Latency")
            .base_ms(100)
            .jitter_ms(50)
            .distribution(LatencyDistribution::Normal)
            .probability(0.5)
            .target("modbus-*")
            .severity(FaultSeverity::Medium)
            .build();

        assert_eq!(fault.id(), "latency-001");
        assert_eq!(fault.base.metadata.probability, 0.5);
        assert!(fault.base.metadata.matches_target("modbus-001"));
    }

    #[tokio::test]
    async fn test_fault_application() {
        let fault = NetworkLatencyFault::builder()
            .id("test")
            .base_ms(10) // Short delay for test
            .build();

        let mut ctx = FaultContext::new(
            TargetInfo::device("device-001"),
            crate::context::OperationType::Read {
                point_id: "temp".to_string(),
            },
            Protocol::ModbusTcp,
        );

        let _result = fault.apply(&mut ctx).await.unwrap();
        assert!(ctx.was_affected());
        assert!(ctx.accumulated_delay_ms >= 10);
    }
}

//! Packet loss fault injection.

use std::any::Any;

use async_trait::async_trait;
use rand::prelude::*;
use serde::{Deserialize, Serialize};

use crate::context::FaultContext;
use crate::error::ChaosResult;
use crate::fault::{
    BaseFault, Fault, FaultBehavior, FaultCategory, FaultMetadata, FaultSeverity, FaultStatistics,
};

// =============================================================================
// Burst Configuration
// =============================================================================

/// Configuration for burst packet loss.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BurstConfig {
    /// Probability of starting a burst.
    pub probability: f64,

    /// Minimum burst length.
    pub min_length: usize,

    /// Maximum burst length.
    pub max_length: usize,
}

impl Default for BurstConfig {
    fn default() -> Self {
        Self {
            probability: 0.0,
            min_length: 1,
            max_length: 5,
        }
    }
}

impl BurstConfig {
    /// Create a burst config.
    pub fn new(probability: f64, min_length: usize, max_length: usize) -> Self {
        Self {
            probability: probability.clamp(0.0, 1.0),
            min_length,
            max_length: max_length.max(min_length),
        }
    }
}

// =============================================================================
// Packet Loss Configuration
// =============================================================================

/// Configuration for packet loss.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PacketLossConfig {
    /// Base loss rate (0.0 - 1.0).
    pub loss_rate: f64,

    /// Burst configuration.
    #[serde(default)]
    pub burst: Option<BurstConfig>,

    /// Correlated loss (Gilbert-Elliott model).
    /// Probability of staying in loss state.
    #[serde(default)]
    pub correlation: f64,

    /// Recovery probability (for correlated loss).
    #[serde(default = "default_recovery")]
    pub recovery_probability: f64,
}

fn default_recovery() -> f64 {
    0.8
}

impl Default for PacketLossConfig {
    fn default() -> Self {
        Self {
            loss_rate: 0.0,
            burst: None,
            correlation: 0.0,
            recovery_probability: 0.8,
        }
    }
}

impl PacketLossConfig {
    /// Create a simple loss config.
    pub fn simple(loss_rate: f64) -> Self {
        Self {
            loss_rate: loss_rate.clamp(0.0, 1.0),
            ..Default::default()
        }
    }

    /// Create a burst loss config.
    pub fn with_burst(loss_rate: f64, burst: BurstConfig) -> Self {
        Self {
            loss_rate: loss_rate.clamp(0.0, 1.0),
            burst: Some(burst),
            ..Default::default()
        }
    }

    /// Create a correlated loss config (Gilbert-Elliott model).
    pub fn correlated(loss_rate: f64, correlation: f64) -> Self {
        Self {
            loss_rate: loss_rate.clamp(0.0, 1.0),
            correlation: correlation.clamp(0.0, 1.0),
            ..Default::default()
        }
    }
}

// =============================================================================
// Packet Loss Fault
// =============================================================================

/// Packet loss fault implementation.
///
/// Simulates packet loss with configurable patterns:
/// - Simple random loss
/// - Burst loss (consecutive packet drops)
/// - Correlated loss (Gilbert-Elliott model)
///
/// # Example
///
/// ```rust,ignore
/// use mabi_chaos::network::PacketLossFault;
///
/// // Simple 10% packet loss
/// let fault = PacketLossFault::builder()
///     .id("loss-001")
///     .loss_rate(0.1)
///     .build();
///
/// // Burst loss
/// let fault = PacketLossFault::builder()
///     .id("loss-002")
///     .loss_rate(0.05)
///     .burst(BurstConfig::new(0.3, 2, 5))
///     .build();
/// ```
#[derive(Debug, Clone)]
pub struct PacketLossFault {
    base: BaseFault,
    config: PacketLossConfig,
    rng: StdRng,

    // State for burst loss
    current_burst: usize,

    // State for correlated loss (Gilbert-Elliott)
    in_loss_state: bool,
}

impl PacketLossFault {
    /// Create a new packet loss fault.
    pub fn new(id: impl Into<String>, config: PacketLossConfig) -> Self {
        let id = id.into();
        let metadata = FaultMetadata::new(&id, "Packet Loss", FaultCategory::Network)
            .with_description("Simulates packet loss with configurable patterns")
            .with_severity(FaultSeverity::Medium);

        Self {
            base: BaseFault::new(metadata),
            config,
            rng: StdRng::from_entropy(),
            current_burst: 0,
            in_loss_state: false,
        }
    }

    /// Create a builder.
    pub fn builder() -> PacketLossFaultBuilder {
        PacketLossFaultBuilder::default()
    }

    /// Check if packet should be dropped.
    pub fn should_drop(&mut self) -> bool {
        // Continue burst if active
        if self.current_burst > 0 {
            self.current_burst -= 1;
            return true;
        }

        // Correlated loss (Gilbert-Elliott model)
        if self.config.correlation > 0.0 {
            if self.in_loss_state {
                // Currently in loss state
                if self.rng.gen::<f64>() < self.config.recovery_probability {
                    self.in_loss_state = false;
                    return false;
                }
                return true;
            } else {
                // Currently in good state
                if self.rng.gen::<f64>() < self.config.loss_rate {
                    if self.rng.gen::<f64>() < self.config.correlation {
                        self.in_loss_state = true;
                    }
                    return true;
                }
                return false;
            }
        }

        // Simple random loss
        if self.rng.gen::<f64>() < self.config.loss_rate {
            // Check if burst should start
            if let Some(ref burst) = self.config.burst {
                if self.rng.gen::<f64>() < burst.probability {
                    self.current_burst = self.rng.gen_range(burst.min_length..=burst.max_length);
                    if self.current_burst > 0 {
                        self.current_burst -= 1; // Count this packet
                    }
                }
            }
            return true;
        }

        false
    }

    /// Get configuration.
    pub fn config(&self) -> &PacketLossConfig {
        &self.config
    }

    /// Update configuration.
    pub fn set_config(&mut self, config: PacketLossConfig) {
        self.config = config;
    }

    /// Reset state.
    pub fn reset_state(&mut self) {
        self.current_burst = 0;
        self.in_loss_state = false;
    }
}

#[async_trait]
impl Fault for PacketLossFault {
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
        let should_drop = {
            let mut this = self.clone();
            if !this.base.check_probability() {
                return Ok(FaultBehavior::Continue);
            }
            this.should_drop()
        };

        if should_drop {
            ctx.record_applied_fault(self.id(), "drop");

            tracing::debug!(
                fault_id = %self.id(),
                target = %ctx.target.device_id,
                "Dropping packet (packet loss)"
            );

            return Ok(FaultBehavior::Skip);
        }

        Ok(FaultBehavior::Continue)
    }

    fn reset(&mut self) {
        self.base.reset();
        self.reset_state();
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

/// Builder for packet loss fault.
#[derive(Debug, Default)]
pub struct PacketLossFaultBuilder {
    id: Option<String>,
    name: Option<String>,
    config: PacketLossConfig,
    probability: f64,
    targets: Vec<String>,
    severity: FaultSeverity,
}

impl PacketLossFaultBuilder {
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

    /// Set loss rate.
    pub fn loss_rate(mut self, rate: f64) -> Self {
        self.config.loss_rate = rate.clamp(0.0, 1.0);
        self
    }

    /// Set burst configuration.
    pub fn burst(mut self, burst: BurstConfig) -> Self {
        self.config.burst = Some(burst);
        self
    }

    /// Set correlation (Gilbert-Elliott model).
    pub fn correlation(mut self, correlation: f64) -> Self {
        self.config.correlation = correlation.clamp(0.0, 1.0);
        self
    }

    /// Set recovery probability.
    pub fn recovery_probability(mut self, p: f64) -> Self {
        self.config.recovery_probability = p.clamp(0.0, 1.0);
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
    pub fn build(self) -> PacketLossFault {
        let id = self.id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        let mut fault = PacketLossFault::new(&id, self.config);

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
    fn test_simple_loss() {
        let config = PacketLossConfig::simple(0.5);
        let mut fault = PacketLossFault::new("test", config);

        let mut dropped = 0;
        let iterations = 10000;

        for _ in 0..iterations {
            if fault.should_drop() {
                dropped += 1;
            }
        }

        let ratio = dropped as f64 / iterations as f64;
        assert!(ratio > 0.4 && ratio < 0.6, "ratio was {}", ratio);
    }

    #[test]
    fn test_zero_loss() {
        let config = PacketLossConfig::simple(0.0);
        let mut fault = PacketLossFault::new("test", config);

        for _ in 0..100 {
            assert!(!fault.should_drop());
        }
    }

    #[test]
    fn test_full_loss() {
        let config = PacketLossConfig::simple(1.0);
        let mut fault = PacketLossFault::new("test", config);

        for _ in 0..100 {
            assert!(fault.should_drop());
        }
    }

    #[test]
    fn test_burst_loss() {
        let config = PacketLossConfig::with_burst(
            0.3,
            BurstConfig::new(1.0, 3, 3), // Always burst of exactly 3
        );
        let mut fault = PacketLossFault::new("test", config);

        // Find a burst
        let mut consecutive = 0;
        let mut max_consecutive = 0;

        for _ in 0..1000 {
            if fault.should_drop() {
                consecutive += 1;
                max_consecutive = max_consecutive.max(consecutive);
            } else {
                consecutive = 0;
            }
        }

        // Should have at least one burst of 3+
        assert!(max_consecutive >= 3, "max consecutive was {}", max_consecutive);
    }

    #[test]
    fn test_builder() {
        let fault = PacketLossFault::builder()
            .id("loss-001")
            .name("Test Loss")
            .loss_rate(0.2)
            .target("device-*")
            .severity(FaultSeverity::High)
            .build();

        assert_eq!(fault.id(), "loss-001");
        assert_eq!(fault.config.loss_rate, 0.2);
    }
}

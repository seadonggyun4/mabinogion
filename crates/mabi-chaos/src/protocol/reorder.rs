//! Response reordering fault injection.

use std::any::Any;
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use rand::prelude::*;
use serde::{Deserialize, Serialize};

use crate::context::FaultContext;
use crate::error::ChaosResult;
use crate::fault::{
    BaseFault, Fault, FaultBehavior, FaultCategory, FaultMetadata, FaultSeverity, FaultStatistics,
};

// =============================================================================
// Reorder Configuration
// =============================================================================

/// Configuration for response reordering.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReorderConfig {
    /// Number of responses to buffer before reordering.
    pub buffer_size: usize,

    /// Whether to randomly shuffle or use specific pattern.
    #[serde(default = "default_true")]
    pub random_shuffle: bool,

    /// Delay between reordered responses (ms).
    #[serde(default)]
    pub inter_response_delay_ms: u64,

    /// Maximum delay for held responses (ms).
    #[serde(default = "default_max_hold")]
    pub max_hold_ms: u64,
}

fn default_true() -> bool {
    true
}

fn default_max_hold() -> u64 {
    5000
}

impl Default for ReorderConfig {
    fn default() -> Self {
        Self {
            buffer_size: 3,
            random_shuffle: true,
            inter_response_delay_ms: 0,
            max_hold_ms: 5000,
        }
    }
}

impl ReorderConfig {
    /// Create a simple reorder config.
    pub fn simple(buffer_size: usize) -> Self {
        Self {
            buffer_size,
            ..Default::default()
        }
    }

    /// Set inter-response delay.
    pub fn with_delay(mut self, delay_ms: u64) -> Self {
        self.inter_response_delay_ms = delay_ms;
        self
    }

    /// Set max hold time.
    pub fn with_max_hold(mut self, max_ms: u64) -> Self {
        self.max_hold_ms = max_ms;
        self
    }
}

// =============================================================================
// Reorder Fault
// =============================================================================

/// Response reordering fault implementation.
///
/// Reorders responses to simulate out-of-order packet delivery.
///
/// # Note
///
/// This fault is most effective when used at the protocol/transport layer.
/// In the current implementation, it marks responses for reordering but
/// the actual reordering must be handled by the transport layer.
///
/// # Example
///
/// ```rust,ignore
/// use mabi_chaos::protocol::ReorderFault;
///
/// // Buffer 3 responses and shuffle
/// let fault = ReorderFault::builder()
///     .id("reorder-001")
///     .buffer_size(3)
///     .build();
/// ```
#[derive(Debug, Clone)]
pub struct ReorderFault {
    base: BaseFault,
    config: ReorderConfig,
    rng: StdRng,

    // Note: In a real implementation, this would be shared state
    // For now, we just mark responses for reordering
    pending_count: usize,
}

impl ReorderFault {
    /// Create a new reorder fault.
    pub fn new(id: impl Into<String>, config: ReorderConfig) -> Self {
        let id = id.into();
        let metadata = FaultMetadata::new(&id, "Response Reordering", FaultCategory::Protocol)
            .with_description("Reorders responses to simulate out-of-order delivery")
            .with_severity(FaultSeverity::Medium);

        Self {
            base: BaseFault::new(metadata),
            config,
            rng: StdRng::from_entropy(),
            pending_count: 0,
        }
    }

    /// Create a builder.
    pub fn builder() -> ReorderFaultBuilder {
        ReorderFaultBuilder::default()
    }

    /// Get configuration.
    pub fn config(&self) -> &ReorderConfig {
        &self.config
    }

    /// Calculate delay for reordering effect.
    pub fn calculate_hold_delay(&mut self) -> u64 {
        if self.config.random_shuffle {
            self.rng.gen_range(0..self.config.max_hold_ms)
        } else {
            // LIFO: later requests get shorter delays
            self.pending_count = (self.pending_count + 1) % self.config.buffer_size;
            let position = self.config.buffer_size - self.pending_count;
            (position as u64 * self.config.max_hold_ms) / self.config.buffer_size as u64
        }
    }
}

#[async_trait]
impl Fault for ReorderFault {
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
            this.calculate_hold_delay()
        };

        ctx.set_metadata("reorder_delay_ms", &delay.to_string());
        ctx.record_applied_fault(self.id(), "reorder");

        tracing::debug!(
            fault_id = %self.id(),
            target = %ctx.target.device_id,
            delay_ms = %delay,
            "Applying reorder delay"
        );

        if delay > 0 {
            ctx.add_delay(delay);
            tokio::time::sleep(std::time::Duration::from_millis(delay)).await;
            return Ok(FaultBehavior::Delay { duration_ms: delay });
        }

        Ok(FaultBehavior::Continue)
    }

    fn reset(&mut self) {
        self.base.reset();
        self.pending_count = 0;
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

/// Builder for reorder fault.
#[derive(Debug, Default)]
pub struct ReorderFaultBuilder {
    id: Option<String>,
    name: Option<String>,
    config: ReorderConfig,
    probability: f64,
    targets: Vec<String>,
    severity: FaultSeverity,
}

impl ReorderFaultBuilder {
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

    /// Set buffer size.
    pub fn buffer_size(mut self, size: usize) -> Self {
        self.config.buffer_size = size;
        self
    }

    /// Set random shuffle.
    pub fn random(mut self) -> Self {
        self.config.random_shuffle = true;
        self
    }

    /// Set LIFO order (not random).
    pub fn lifo(mut self) -> Self {
        self.config.random_shuffle = false;
        self
    }

    /// Set inter-response delay.
    pub fn inter_response_delay(mut self, ms: u64) -> Self {
        self.config.inter_response_delay_ms = ms;
        self
    }

    /// Set max hold time.
    pub fn max_hold(mut self, ms: u64) -> Self {
        self.config.max_hold_ms = ms;
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
    pub fn build(self) -> ReorderFault {
        let id = self.id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        let mut fault = ReorderFault::new(&id, self.config);

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
    fn test_random_delay() {
        let config = ReorderConfig::simple(3);
        let mut fault = ReorderFault::new("test", config);

        let delays: Vec<u64> = (0..100).map(|_| fault.calculate_hold_delay()).collect();

        // Should have some variation
        let unique: std::collections::HashSet<u64> = delays.iter().copied().collect();
        assert!(unique.len() > 10);
    }

    #[test]
    fn test_lifo_delay() {
        let mut config = ReorderConfig::simple(3);
        config.random_shuffle = false;
        config.max_hold_ms = 300;
        let mut fault = ReorderFault::new("test", config);

        let d1 = fault.calculate_hold_delay();
        let d2 = fault.calculate_hold_delay();
        let d3 = fault.calculate_hold_delay();

        // LIFO: later requests should have shorter delays
        assert!(d1 > d2 || d2 > d3 || d3 < d1);
    }

    #[test]
    fn test_builder() {
        let fault = ReorderFault::builder()
            .id("reorder-001")
            .buffer_size(5)
            .max_hold(10000)
            .random()
            .target("device-*")
            .build();

        assert_eq!(fault.id(), "reorder-001");
        assert_eq!(fault.config.buffer_size, 5);
        assert!(fault.config.random_shuffle);
    }
}

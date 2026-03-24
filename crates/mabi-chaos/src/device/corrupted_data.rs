//! Corrupted data fault injection.

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
// Corruption Strategy
// =============================================================================

/// Strategy for data corruption.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum CorruptionStrategy {
    /// Random bit flips.
    #[default]
    BitFlip,

    /// Set values to zero.
    ZeroOut,

    /// Set values to maximum.
    MaxOut,

    /// Random values within valid range.
    RandomInRange,

    /// Random values outside valid range.
    OutOfRange,

    /// Swap values between points.
    Swap,

    /// Duplicate previous value.
    StaleData,

    /// Return NaN for floats.
    NaN,

    /// Truncate data.
    Truncate,

    /// Add noise to values.
    Noise { std_dev: f64 },

    /// Offset values by amount.
    Offset { amount: f64 },

    /// Scale values by factor.
    Scale { factor: f64 },
}

// =============================================================================
// Corruption Configuration
// =============================================================================

/// Configuration for data corruption.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CorruptionConfig {
    /// Corruption strategy.
    pub strategy: CorruptionStrategy,

    /// Probability of corrupting each value.
    pub value_corruption_rate: f64,

    /// Whether to mark corrupted data as bad quality.
    #[serde(default)]
    pub mark_bad_quality: bool,

    /// Custom error message for bad quality.
    #[serde(default)]
    pub quality_message: Option<String>,

    /// Specific points to corrupt (empty = all points).
    #[serde(default)]
    pub target_points: Vec<String>,
}

impl Default for CorruptionConfig {
    fn default() -> Self {
        Self {
            strategy: CorruptionStrategy::BitFlip,
            value_corruption_rate: 0.1,
            mark_bad_quality: true,
            quality_message: None,
            target_points: Vec::new(),
        }
    }
}

impl CorruptionConfig {
    /// Create a bit flip corruption config.
    pub fn bit_flip(corruption_rate: f64) -> Self {
        Self {
            strategy: CorruptionStrategy::BitFlip,
            value_corruption_rate: corruption_rate.clamp(0.0, 1.0),
            ..Default::default()
        }
    }

    /// Create a noise corruption config.
    pub fn noise(std_dev: f64, corruption_rate: f64) -> Self {
        Self {
            strategy: CorruptionStrategy::Noise { std_dev },
            value_corruption_rate: corruption_rate.clamp(0.0, 1.0),
            ..Default::default()
        }
    }

    /// Create a stale data corruption config.
    pub fn stale_data(corruption_rate: f64) -> Self {
        Self {
            strategy: CorruptionStrategy::StaleData,
            value_corruption_rate: corruption_rate.clamp(0.0, 1.0),
            ..Default::default()
        }
    }

    /// Create an offset corruption config.
    pub fn offset(amount: f64, corruption_rate: f64) -> Self {
        Self {
            strategy: CorruptionStrategy::Offset { amount },
            value_corruption_rate: corruption_rate.clamp(0.0, 1.0),
            ..Default::default()
        }
    }

    /// Set to mark bad quality.
    pub fn with_bad_quality(mut self) -> Self {
        self.mark_bad_quality = true;
        self
    }

    /// Set specific target points.
    pub fn with_targets(mut self, targets: Vec<String>) -> Self {
        self.target_points = targets;
        self
    }
}

// =============================================================================
// Corrupted Data Fault
// =============================================================================

/// Corrupted data fault implementation.
///
/// Returns corrupted or invalid data from device reads.
///
/// # Example
///
/// ```rust,ignore
/// use mabi_chaos::device::CorruptedDataFault;
///
/// // Random bit flips
/// let fault = CorruptedDataFault::builder()
///     .id("corrupt-001")
///     .strategy(CorruptionStrategy::BitFlip)
///     .corruption_rate(0.1)
///     .build();
///
/// // Add noise to readings
/// let fault = CorruptedDataFault::builder()
///     .id("corrupt-002")
///     .noise(5.0)
///     .corruption_rate(0.5)
///     .build();
/// ```
#[derive(Debug, Clone)]
pub struct CorruptedDataFault {
    base: BaseFault,
    config: CorruptionConfig,
    rng: StdRng,

    // State for stale data
    last_values: std::collections::HashMap<String, mabi_core::Value>,
}

impl CorruptedDataFault {
    /// Create a new corrupted data fault.
    pub fn new(id: impl Into<String>, config: CorruptionConfig) -> Self {
        let id = id.into();
        let metadata = FaultMetadata::new(&id, "Corrupted Data", FaultCategory::Device)
            .with_description("Returns corrupted or invalid data")
            .with_severity(FaultSeverity::Medium);

        Self {
            base: BaseFault::new(metadata),
            config,
            rng: StdRng::from_entropy(),
            last_values: std::collections::HashMap::new(),
        }
    }

    /// Create a builder.
    pub fn builder() -> CorruptionFaultBuilder {
        CorruptionFaultBuilder::default()
    }

    /// Check if a point should be corrupted.
    fn should_corrupt_point(&mut self, point_id: &str) -> bool {
        // Check if specific points are targeted
        if !self.config.target_points.is_empty()
            && !self.config.target_points.iter().any(|p| p == point_id)
        {
            return false;
        }

        self.rng.gen::<f64>() < self.config.value_corruption_rate
    }

    /// Corrupt a value.
    fn corrupt_value(&mut self, value: &mabi_core::Value, point_id: &str) -> mabi_core::Value {
        use mabi_core::Value;

        match &self.config.strategy {
            CorruptionStrategy::BitFlip => match value {
                Value::I32(v) => Value::I32(v ^ (1 << self.rng.gen_range(0..32))),
                Value::U32(v) => Value::U32(v ^ (1 << self.rng.gen_range(0..32))),
                Value::I64(v) => Value::I64(v ^ (1 << self.rng.gen_range(0..64))),
                Value::U64(v) => Value::U64(v ^ (1 << self.rng.gen_range(0..64))),
                Value::F32(v) => {
                    let bits = v.to_bits() ^ (1 << self.rng.gen_range(0..32));
                    Value::F32(f32::from_bits(bits))
                }
                Value::F64(v) => {
                    let bits = v.to_bits() ^ (1 << self.rng.gen_range(0..64));
                    Value::F64(f64::from_bits(bits))
                }
                Value::Bool(v) => Value::Bool(!v),
                _ => value.clone(),
            },

            CorruptionStrategy::ZeroOut => match value {
                Value::I32(_) => Value::I32(0),
                Value::U32(_) => Value::U32(0),
                Value::I64(_) => Value::I64(0),
                Value::U64(_) => Value::U64(0),
                Value::F32(_) => Value::F32(0.0),
                Value::F64(_) => Value::F64(0.0),
                Value::Bool(_) => Value::Bool(false),
                _ => value.clone(),
            },

            CorruptionStrategy::MaxOut => match value {
                Value::I32(_) => Value::I32(i32::MAX),
                Value::U32(_) => Value::U32(u32::MAX),
                Value::I64(_) => Value::I64(i64::MAX),
                Value::U64(_) => Value::U64(u64::MAX),
                Value::F32(_) => Value::F32(f32::MAX),
                Value::F64(_) => Value::F64(f64::MAX),
                Value::Bool(_) => Value::Bool(true),
                _ => value.clone(),
            },

            CorruptionStrategy::RandomInRange => match value {
                Value::I32(_) => Value::I32(self.rng.gen_range(-1000..1000)),
                Value::U32(_) => Value::U32(self.rng.gen_range(0..1000)),
                Value::F32(_) => Value::F32(self.rng.gen_range(-100.0..100.0)),
                Value::F64(_) => Value::F64(self.rng.gen_range(-100.0..100.0)),
                Value::Bool(_) => Value::Bool(self.rng.gen()),
                _ => value.clone(),
            },

            CorruptionStrategy::OutOfRange => match value {
                Value::I32(v) => Value::I32(if *v >= 0 { i32::MIN } else { i32::MAX }),
                Value::U32(v) => Value::U32(if *v < 1000 { u32::MAX } else { 0 }),
                Value::F32(_) => Value::F32(if self.rng.gen() {
                    f32::INFINITY
                } else {
                    f32::NEG_INFINITY
                }),
                Value::F64(_) => Value::F64(if self.rng.gen() {
                    f64::INFINITY
                } else {
                    f64::NEG_INFINITY
                }),
                _ => value.clone(),
            },

            CorruptionStrategy::StaleData => {
                if let Some(last) = self.last_values.get(point_id) {
                    last.clone()
                } else {
                    value.clone()
                }
            }

            CorruptionStrategy::NaN => match value {
                Value::F32(_) => Value::F32(f32::NAN),
                Value::F64(_) => Value::F64(f64::NAN),
                _ => value.clone(),
            },

            CorruptionStrategy::Truncate => match value {
                Value::I32(v) => Value::I32(v & 0xFF),
                Value::U32(v) => Value::U32(v & 0xFF),
                Value::I64(v) => Value::I64(v & 0xFFFF),
                Value::U64(v) => Value::U64(v & 0xFFFF),
                Value::String(s) => Value::String(s.chars().take(1).collect()),
                _ => value.clone(),
            },

            CorruptionStrategy::Noise { std_dev } => {
                let noise: f64 = rand_distr::Normal::new(0.0, *std_dev)
                    .map(|d| d.sample(&mut self.rng))
                    .unwrap_or(0.0);

                match value {
                    Value::I32(v) => Value::I32((*v as f64 + noise) as i32),
                    Value::U32(v) => Value::U32(((*v as f64 + noise).max(0.0)) as u32),
                    Value::F32(v) => Value::F32(*v + noise as f32),
                    Value::F64(v) => Value::F64(*v + noise),
                    _ => value.clone(),
                }
            }

            CorruptionStrategy::Offset { amount } => match value {
                Value::I32(v) => Value::I32((*v as f64 + amount) as i32),
                Value::U32(v) => Value::U32(((*v as f64 + amount).max(0.0)) as u32),
                Value::F32(v) => Value::F32(*v + *amount as f32),
                Value::F64(v) => Value::F64(*v + amount),
                _ => value.clone(),
            },

            CorruptionStrategy::Scale { factor } => match value {
                Value::I32(v) => Value::I32((*v as f64 * factor) as i32),
                Value::U32(v) => Value::U32(((*v as f64 * factor).max(0.0)) as u32),
                Value::F32(v) => Value::F32(*v * *factor as f32),
                Value::F64(v) => Value::F64(*v * factor),
                _ => value.clone(),
            },

            CorruptionStrategy::Swap => {
                // Swap is handled at the response level
                value.clone()
            }
        }
    }

    /// Get configuration.
    pub fn config(&self) -> &CorruptionConfig {
        &self.config
    }
}

#[async_trait]
impl Fault for CorruptedDataFault {
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
        // Only apply to read operations
        if !ctx.is_read() {
            return Ok(false);
        }
        Ok(true)
    }

    async fn apply(&self, ctx: &mut FaultContext) -> ChaosResult<FaultBehavior> {
        if !self.clone().base.check_probability() {
            return Ok(FaultBehavior::Continue);
        }

        ctx.record_applied_fault(self.id(), "corrupt");
        ctx.set_metadata(
            "corruption_strategy",
            &format!("{:?}", self.config.strategy),
        );

        if self.config.mark_bad_quality {
            ctx.set_metadata("mark_bad_quality", "true");
        }

        tracing::debug!(
            fault_id = %self.id(),
            target = %ctx.target.device_id,
            strategy = ?self.config.strategy,
            "Corrupting response data"
        );

        Ok(FaultBehavior::Modify)
    }

    async fn after_operation(&self, ctx: &mut FaultContext) -> ChaosResult<FaultBehavior> {
        // Apply corruption to response data if we're modifying
        if ctx.get_metadata("corruption_strategy").is_none() {
            return Ok(FaultBehavior::Continue);
        }

        if let Some(ref mut response) = ctx.response_data {
            let mut this = self.clone();

            for point in &mut response.data_points {
                let point_id_str = point.id.to_string();
                if this.should_corrupt_point(&point_id_str) {
                    // Store current value for stale data strategy
                    this.last_values
                        .insert(point_id_str.clone(), point.value.clone());

                    // Corrupt the value
                    point.value = this.corrupt_value(&point.value, &point_id_str);

                    // Mark bad quality if configured
                    if this.config.mark_bad_quality {
                        point.quality = mabi_core::Quality::BAD;
                    }
                }
            }

            // Handle swap strategy
            if matches!(this.config.strategy, CorruptionStrategy::Swap)
                && response.data_points.len() >= 2
            {
                let len = response.data_points.len();
                let i = this.rng.gen_range(0..len);
                let j = this.rng.gen_range(0..len);
                if i != j {
                    let temp = response.data_points[i].value.clone();
                    response.data_points[i].value = response.data_points[j].value.clone();
                    response.data_points[j].value = temp;
                }
            }
        }

        Ok(FaultBehavior::Continue)
    }

    fn reset(&mut self) {
        self.base.reset();
        self.last_values.clear();
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

/// Builder for corrupted data fault.
#[derive(Debug, Default)]
pub struct CorruptionFaultBuilder {
    id: Option<String>,
    name: Option<String>,
    config: CorruptionConfig,
    probability: f64,
    targets: Vec<String>,
    severity: FaultSeverity,
}

impl CorruptionFaultBuilder {
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

    /// Set corruption strategy.
    pub fn strategy(mut self, strategy: CorruptionStrategy) -> Self {
        self.config.strategy = strategy;
        self
    }

    /// Set corruption rate.
    pub fn corruption_rate(mut self, rate: f64) -> Self {
        self.config.value_corruption_rate = rate.clamp(0.0, 1.0);
        self
    }

    /// Set noise strategy.
    pub fn noise(mut self, std_dev: f64) -> Self {
        self.config.strategy = CorruptionStrategy::Noise { std_dev };
        self
    }

    /// Set offset strategy.
    pub fn offset(mut self, amount: f64) -> Self {
        self.config.strategy = CorruptionStrategy::Offset { amount };
        self
    }

    /// Set scale strategy.
    pub fn scale(mut self, factor: f64) -> Self {
        self.config.strategy = CorruptionStrategy::Scale { factor };
        self
    }

    /// Mark corrupted data as bad quality.
    pub fn mark_bad_quality(mut self) -> Self {
        self.config.mark_bad_quality = true;
        self
    }

    /// Set specific points to corrupt.
    pub fn target_points(mut self, points: Vec<String>) -> Self {
        self.config.target_points = points;
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
    pub fn build(self) -> CorruptedDataFault {
        let id = self.id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        let mut fault = CorruptedDataFault::new(&id, self.config);

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
    use mabi_core::Value;

    #[test]
    fn test_bit_flip() {
        let config = CorruptionConfig::bit_flip(1.0);
        let mut fault = CorruptedDataFault::new("test", config);

        let original = Value::I32(100);
        let corrupted = fault.corrupt_value(&original, "test");

        // Should be different due to bit flip
        assert_ne!(original, corrupted);
    }

    #[test]
    fn test_zero_out() {
        let config = CorruptionConfig {
            strategy: CorruptionStrategy::ZeroOut,
            value_corruption_rate: 1.0,
            ..Default::default()
        };
        let mut fault = CorruptedDataFault::new("test", config);

        assert_eq!(fault.corrupt_value(&Value::I32(100), "test"), Value::I32(0));
        assert_eq!(
            fault.corrupt_value(&Value::F64(123.456), "test"),
            Value::F64(0.0)
        );
    }

    #[test]
    fn test_noise() {
        let config = CorruptionConfig::noise(10.0, 1.0);
        let mut fault = CorruptedDataFault::new("test", config);

        let original = 100.0;
        let mut different_count = 0;

        for _ in 0..100 {
            if let Value::F64(v) = fault.corrupt_value(&Value::F64(original), "test") {
                if (v - original).abs() > 0.001 {
                    different_count += 1;
                }
            }
        }

        // Most values should be different due to noise
        assert!(different_count > 90);
    }

    #[test]
    fn test_builder() {
        let fault = CorruptedDataFault::builder()
            .id("corrupt-001")
            .strategy(CorruptionStrategy::BitFlip)
            .corruption_rate(0.5)
            .mark_bad_quality()
            .target("device-*")
            .build();

        assert_eq!(fault.id(), "corrupt-001");
        assert!(fault.config.mark_bad_quality);
    }
}

//! Checksum fault injection.

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
// Checksum Configuration
// =============================================================================

/// Configuration for checksum fault.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChecksumConfig {
    /// Probability of corrupting checksum.
    pub corruption_probability: f64,

    /// Whether to corrupt CRC-16 (Modbus RTU).
    #[serde(default = "default_true")]
    pub corrupt_crc16: bool,

    /// Whether to corrupt CRC-32.
    #[serde(default = "default_true")]
    pub corrupt_crc32: bool,

    /// Whether to corrupt LRC (ASCII).
    #[serde(default = "default_true")]
    pub corrupt_lrc: bool,

    /// Whether to zero out checksum.
    #[serde(default)]
    pub zero_checksum: bool,

    /// Whether to invert checksum.
    #[serde(default)]
    pub invert_checksum: bool,

    /// Custom checksum offset from end of packet.
    #[serde(default = "default_checksum_offset")]
    pub checksum_offset: usize,

    /// Custom checksum length.
    #[serde(default = "default_checksum_length")]
    pub checksum_length: usize,
}

fn default_true() -> bool {
    true
}

fn default_checksum_offset() -> usize {
    2
}

fn default_checksum_length() -> usize {
    2
}

impl Default for ChecksumConfig {
    fn default() -> Self {
        Self {
            corruption_probability: 0.1,
            corrupt_crc16: true,
            corrupt_crc32: true,
            corrupt_lrc: true,
            zero_checksum: false,
            invert_checksum: false,
            checksum_offset: 2,
            checksum_length: 2,
        }
    }
}

impl ChecksumConfig {
    /// Create a simple checksum corruption config.
    pub fn simple(probability: f64) -> Self {
        Self {
            corruption_probability: probability.clamp(0.0, 1.0),
            ..Default::default()
        }
    }

    /// Create a zero checksum config.
    pub fn zero() -> Self {
        Self {
            zero_checksum: true,
            ..Default::default()
        }
    }

    /// Create an inverted checksum config.
    pub fn inverted() -> Self {
        Self {
            invert_checksum: true,
            ..Default::default()
        }
    }

    /// Set custom checksum location.
    pub fn with_location(mut self, offset: usize, length: usize) -> Self {
        self.checksum_offset = offset;
        self.checksum_length = length;
        self
    }
}

// =============================================================================
// Checksum Fault
// =============================================================================

/// Checksum fault implementation.
///
/// Corrupts packet checksums to simulate transmission errors.
///
/// # Example
///
/// ```rust,ignore
/// use mabi_chaos::protocol::ChecksumFault;
///
/// // Corrupt 10% of checksums
/// let fault = ChecksumFault::builder()
///     .id("crc-001")
///     .probability(0.1)
///     .build();
///
/// // Zero out all checksums
/// let fault = ChecksumFault::builder()
///     .id("crc-002")
///     .zero()
///     .build();
/// ```
#[derive(Debug, Clone)]
pub struct ChecksumFault {
    base: BaseFault,
    config: ChecksumConfig,
    rng: StdRng,
}

impl ChecksumFault {
    /// Create a new checksum fault.
    pub fn new(id: impl Into<String>, config: ChecksumConfig) -> Self {
        let id = id.into();
        let metadata = FaultMetadata::new(&id, "Invalid Checksum", FaultCategory::Protocol)
            .with_description("Corrupts packet checksums")
            .with_severity(FaultSeverity::Medium);

        Self {
            base: BaseFault::new(metadata),
            config,
            rng: StdRng::from_entropy(),
        }
    }

    /// Create a builder.
    pub fn builder() -> ChecksumFaultBuilder {
        ChecksumFaultBuilder::default()
    }

    /// Corrupt checksum in data.
    pub fn corrupt_checksum(&mut self, data: &mut Vec<u8>) {
        if data.len() < self.config.checksum_offset + self.config.checksum_length {
            return;
        }

        let checksum_start = data.len() - self.config.checksum_offset;
        let checksum_end = checksum_start + self.config.checksum_length;

        if self.config.zero_checksum {
            for i in checksum_start..checksum_end.min(data.len()) {
                data[i] = 0;
            }
        } else if self.config.invert_checksum {
            for i in checksum_start..checksum_end.min(data.len()) {
                data[i] = !data[i];
            }
        } else {
            // Random corruption
            for i in checksum_start..checksum_end.min(data.len()) {
                data[i] ^= self.rng.gen::<u8>() | 1; // Ensure at least one bit flips
            }
        }
    }

    /// Get configuration.
    pub fn config(&self) -> &ChecksumConfig {
        &self.config
    }
}

#[async_trait]
impl Fault for ChecksumFault {
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
        if !self.clone().base.check_probability() {
            return Ok(FaultBehavior::Continue);
        }

        // Check corruption probability
        if self.clone().rng.gen::<f64>() > self.config.corruption_probability {
            return Ok(FaultBehavior::Continue);
        }

        ctx.set_metadata("corrupt_checksum", "true");
        ctx.record_applied_fault(self.id(), "checksum");

        tracing::debug!(
            fault_id = %self.id(),
            target = %ctx.target.device_id,
            "Marking checksum for corruption"
        );

        Ok(FaultBehavior::Modify)
    }

    async fn after_operation(&self, ctx: &mut FaultContext) -> ChaosResult<FaultBehavior> {
        if ctx.get_metadata("corrupt_checksum").is_none() {
            return Ok(FaultBehavior::Continue);
        }

        if let Some(ref mut response) = ctx.response_data {
            if let Some(ref mut raw) = response.raw {
                let mut this = self.clone();
                this.corrupt_checksum(raw);
            }
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

/// Builder for checksum fault.
#[derive(Debug, Default)]
pub struct ChecksumFaultBuilder {
    id: Option<String>,
    name: Option<String>,
    config: ChecksumConfig,
    probability: f64,
    targets: Vec<String>,
    severity: FaultSeverity,
}

impl ChecksumFaultBuilder {
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

    /// Set corruption probability.
    pub fn corruption_probability(mut self, p: f64) -> Self {
        self.config.corruption_probability = p.clamp(0.0, 1.0);
        self
    }

    /// Zero out checksums.
    pub fn zero(mut self) -> Self {
        self.config.zero_checksum = true;
        self
    }

    /// Invert checksums.
    pub fn invert(mut self) -> Self {
        self.config.invert_checksum = true;
        self
    }

    /// Set checksum location.
    pub fn location(mut self, offset: usize, length: usize) -> Self {
        self.config.checksum_offset = offset;
        self.config.checksum_length = length;
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
    pub fn build(self) -> ChecksumFault {
        let id = self.id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        let mut fault = ChecksumFault::new(&id, self.config);

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
    fn test_random_corruption() {
        let config = ChecksumConfig::simple(1.0);
        let mut fault = ChecksumFault::new("test", config);

        let original = vec![0x01, 0x02, 0x03, 0x04, 0xAB, 0xCD];
        let mut data = original.clone();
        fault.corrupt_checksum(&mut data);

        // Last 2 bytes should be different
        assert_ne!(&original[4..], &data[4..]);
        // First bytes unchanged
        assert_eq!(&original[..4], &data[..4]);
    }

    #[test]
    fn test_zero_checksum() {
        let config = ChecksumConfig::zero();
        let mut fault = ChecksumFault::new("test", config);

        let mut data = vec![0x01, 0x02, 0x03, 0x04, 0xAB, 0xCD];
        fault.corrupt_checksum(&mut data);

        assert_eq!(data[4], 0);
        assert_eq!(data[5], 0);
    }

    #[test]
    fn test_invert_checksum() {
        let config = ChecksumConfig::inverted();
        let mut fault = ChecksumFault::new("test", config);

        let mut data = vec![0x01, 0x02, 0x03, 0x04, 0xAB, 0xCD];
        fault.corrupt_checksum(&mut data);

        assert_eq!(data[4], !0xAB);
        assert_eq!(data[5], !0xCD);
    }

    #[test]
    fn test_builder() {
        let fault = ChecksumFault::builder()
            .id("crc-001")
            .corruption_probability(0.5)
            .zero()
            .target("modbus-*")
            .build();

        assert_eq!(fault.id(), "crc-001");
        assert!(fault.config.zero_checksum);
    }
}

//! Malformed packet fault injection.

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
// Malformation Type
// =============================================================================

/// Type of packet malformation.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum MalformationType {
    /// Random byte corruption.
    #[default]
    RandomCorruption,

    /// Truncate packet.
    Truncation { min_bytes: usize, max_bytes: usize },

    /// Invalid header fields.
    InvalidHeader,

    /// Invalid length field.
    InvalidLength,

    /// Invalid function/service code.
    InvalidFunction,

    /// Extra bytes appended.
    ExtraData { min_bytes: usize, max_bytes: usize },

    /// Null bytes injection.
    NullInjection,

    /// Invalid encoding.
    InvalidEncoding,

    /// Fragment packet incorrectly.
    BadFragmentation,

    /// Invalid protocol version.
    InvalidVersion,
}

// =============================================================================
// Malformed Configuration
// =============================================================================

/// Configuration for malformed packet fault.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MalformedConfig {
    /// Type of malformation.
    pub malformation_type: MalformationType,

    /// Number of bytes to corrupt (for RandomCorruption).
    #[serde(default = "default_corrupt_bytes")]
    pub corrupt_bytes: usize,

    /// Whether malformation affects request or response.
    #[serde(default)]
    pub affect_request: bool,

    /// Whether malformation affects response.
    #[serde(default = "default_true")]
    pub affect_response: bool,
}

fn default_corrupt_bytes() -> usize {
    1
}

fn default_true() -> bool {
    true
}

impl Default for MalformedConfig {
    fn default() -> Self {
        Self {
            malformation_type: MalformationType::RandomCorruption,
            corrupt_bytes: 1,
            affect_request: false,
            affect_response: true,
        }
    }
}

impl MalformedConfig {
    /// Create random corruption config.
    pub fn random_corruption(corrupt_bytes: usize) -> Self {
        Self {
            malformation_type: MalformationType::RandomCorruption,
            corrupt_bytes,
            ..Default::default()
        }
    }

    /// Create truncation config.
    pub fn truncation(min_bytes: usize, max_bytes: usize) -> Self {
        Self {
            malformation_type: MalformationType::Truncation { min_bytes, max_bytes },
            ..Default::default()
        }
    }

    /// Create invalid header config.
    pub fn invalid_header() -> Self {
        Self {
            malformation_type: MalformationType::InvalidHeader,
            ..Default::default()
        }
    }

    /// Affect requests instead of responses.
    pub fn request_side(mut self) -> Self {
        self.affect_request = true;
        self.affect_response = false;
        self
    }

    /// Affect both requests and responses.
    pub fn both_sides(mut self) -> Self {
        self.affect_request = true;
        self.affect_response = true;
        self
    }
}

// =============================================================================
// Malformed Packet Fault
// =============================================================================

/// Malformed packet fault implementation.
///
/// Generates protocol packets with various malformations.
///
/// # Example
///
/// ```rust,ignore
/// use mabi_chaos::protocol::MalformedPacketFault;
///
/// // Random byte corruption
/// let fault = MalformedPacketFault::builder()
///     .id("malformed-001")
///     .random_corruption(2)
///     .probability(0.1)
///     .build();
///
/// // Truncate packets
/// let fault = MalformedPacketFault::builder()
///     .id("malformed-002")
///     .truncation(5, 20)
///     .build();
/// ```
#[derive(Debug, Clone)]
pub struct MalformedPacketFault {
    base: BaseFault,
    config: MalformedConfig,
    rng: StdRng,
}

impl MalformedPacketFault {
    /// Create a new malformed packet fault.
    pub fn new(id: impl Into<String>, config: MalformedConfig) -> Self {
        let id = id.into();
        let metadata = FaultMetadata::new(&id, "Malformed Packet", FaultCategory::Protocol)
            .with_description("Generates malformed protocol packets")
            .with_severity(FaultSeverity::Medium);

        Self {
            base: BaseFault::new(metadata),
            config,
            rng: StdRng::from_entropy(),
        }
    }

    /// Create a builder.
    pub fn builder() -> MalformedFaultBuilder {
        MalformedFaultBuilder::default()
    }

    /// Apply malformation to data.
    pub fn malform(&mut self, data: &mut Vec<u8>) {
        if data.is_empty() {
            return;
        }

        match &self.config.malformation_type {
            MalformationType::RandomCorruption => {
                for _ in 0..self.config.corrupt_bytes {
                    if !data.is_empty() {
                        let idx = self.rng.gen_range(0..data.len());
                        data[idx] ^= self.rng.gen::<u8>();
                    }
                }
            }

            MalformationType::Truncation { min_bytes, max_bytes } => {
                let truncate_by = self.rng.gen_range(*min_bytes..=*max_bytes);
                let new_len = data.len().saturating_sub(truncate_by);
                data.truncate(new_len.max(1));
            }

            MalformationType::InvalidHeader => {
                // Corrupt first few bytes (header)
                let header_len = (data.len() / 4).max(1).min(8);
                for i in 0..header_len {
                    data[i] = self.rng.gen();
                }
            }

            MalformationType::InvalidLength => {
                // Set length field to invalid value (assuming byte 4-5 is length for many protocols)
                if data.len() >= 6 {
                    let invalid_len = self.rng.gen_range(1000..65535u16);
                    data[4] = (invalid_len >> 8) as u8;
                    data[5] = invalid_len as u8;
                }
            }

            MalformationType::InvalidFunction => {
                // Corrupt function code (often byte 7 for Modbus-like)
                if data.len() >= 8 {
                    data[7] = self.rng.gen_range(128..255);
                }
            }

            MalformationType::ExtraData { min_bytes, max_bytes } => {
                let extra = self.rng.gen_range(*min_bytes..=*max_bytes);
                for _ in 0..extra {
                    data.push(self.rng.gen());
                }
            }

            MalformationType::NullInjection => {
                // Insert null bytes at random positions
                let count = self.rng.gen_range(1..=3);
                for _ in 0..count {
                    if !data.is_empty() {
                        let idx = self.rng.gen_range(0..=data.len());
                        data.insert(idx, 0);
                    }
                }
            }

            MalformationType::InvalidEncoding => {
                // Replace valid characters with invalid UTF-8 sequences
                for byte in data.iter_mut() {
                    if self.rng.gen_bool(0.3) {
                        *byte = self.rng.gen_range(0x80..0xFF);
                    }
                }
            }

            MalformationType::BadFragmentation => {
                // Simulate bad fragmentation by duplicating a portion
                if data.len() >= 4 {
                    let start = self.rng.gen_range(0..data.len() / 2);
                    let end = self.rng.gen_range(start + 1..data.len());
                    let fragment: Vec<u8> = data[start..end].to_vec();
                    data.extend(fragment);
                }
            }

            MalformationType::InvalidVersion => {
                // Set version field to invalid (often first byte or two)
                if !data.is_empty() {
                    data[0] = self.rng.gen_range(100..255);
                }
                if data.len() >= 2 {
                    data[1] = self.rng.gen_range(100..255);
                }
            }
        }
    }

    /// Get configuration.
    pub fn config(&self) -> &MalformedConfig {
        &self.config
    }
}

#[async_trait]
impl Fault for MalformedPacketFault {
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

        // Mark for modification in after_operation
        ctx.set_metadata("malform_type", &format!("{:?}", self.config.malformation_type));
        ctx.record_applied_fault(self.id(), "malform");

        tracing::debug!(
            fault_id = %self.id(),
            target = %ctx.target.device_id,
            malformation = ?self.config.malformation_type,
            "Marking packet for malformation"
        );

        Ok(FaultBehavior::Modify)
    }

    async fn after_operation(&self, ctx: &mut FaultContext) -> ChaosResult<FaultBehavior> {
        if ctx.get_metadata("malform_type").is_none() {
            return Ok(FaultBehavior::Continue);
        }

        // Apply malformation to response data
        if self.config.affect_response {
            if let Some(ref mut response) = ctx.response_data {
                if let Some(ref mut raw) = response.raw {
                    let mut this = self.clone();
                    this.malform(raw);
                }
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

/// Builder for malformed packet fault.
#[derive(Debug, Default)]
pub struct MalformedFaultBuilder {
    id: Option<String>,
    name: Option<String>,
    config: MalformedConfig,
    probability: f64,
    targets: Vec<String>,
    severity: FaultSeverity,
}

impl MalformedFaultBuilder {
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

    /// Set malformation type.
    pub fn malformation_type(mut self, t: MalformationType) -> Self {
        self.config.malformation_type = t;
        self
    }

    /// Set random corruption.
    pub fn random_corruption(mut self, bytes: usize) -> Self {
        self.config.malformation_type = MalformationType::RandomCorruption;
        self.config.corrupt_bytes = bytes;
        self
    }

    /// Set truncation.
    pub fn truncation(mut self, min_bytes: usize, max_bytes: usize) -> Self {
        self.config.malformation_type = MalformationType::Truncation { min_bytes, max_bytes };
        self
    }

    /// Set invalid header.
    pub fn invalid_header(mut self) -> Self {
        self.config.malformation_type = MalformationType::InvalidHeader;
        self
    }

    /// Set invalid length.
    pub fn invalid_length(mut self) -> Self {
        self.config.malformation_type = MalformationType::InvalidLength;
        self
    }

    /// Set extra data.
    pub fn extra_data(mut self, min_bytes: usize, max_bytes: usize) -> Self {
        self.config.malformation_type = MalformationType::ExtraData { min_bytes, max_bytes };
        self
    }

    /// Affect request side.
    pub fn request_side(mut self) -> Self {
        self.config.affect_request = true;
        self.config.affect_response = false;
        self
    }

    /// Affect response side.
    pub fn response_side(mut self) -> Self {
        self.config.affect_request = false;
        self.config.affect_response = true;
        self
    }

    /// Affect both sides.
    pub fn both_sides(mut self) -> Self {
        self.config.affect_request = true;
        self.config.affect_response = true;
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
    pub fn build(self) -> MalformedPacketFault {
        let id = self.id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        let mut fault = MalformedPacketFault::new(&id, self.config);

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
        let config = MalformedConfig::random_corruption(3);
        let mut fault = MalformedPacketFault::new("test", config);

        let original = vec![0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08];
        let mut data = original.clone();
        fault.malform(&mut data);

        // Should be different after corruption
        assert_ne!(original, data);
        assert_eq!(original.len(), data.len());
    }

    #[test]
    fn test_truncation() {
        let config = MalformedConfig::truncation(3, 5);
        let mut fault = MalformedPacketFault::new("test", config);

        let mut data = vec![0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08];
        fault.malform(&mut data);

        assert!(data.len() >= 3);
        assert!(data.len() <= 5);
    }

    #[test]
    fn test_extra_data() {
        let config = MalformedConfig {
            malformation_type: MalformationType::ExtraData { min_bytes: 5, max_bytes: 10 },
            ..Default::default()
        };
        let mut fault = MalformedPacketFault::new("test", config);

        let mut data = vec![0x01, 0x02, 0x03];
        let original_len = data.len();
        fault.malform(&mut data);

        assert!(data.len() >= original_len + 5);
        assert!(data.len() <= original_len + 10);
    }

    #[test]
    fn test_builder() {
        let fault = MalformedPacketFault::builder()
            .id("malformed-001")
            .random_corruption(2)
            .probability(0.5)
            .target("device-*")
            .build();

        assert_eq!(fault.id(), "malformed-001");
        assert_eq!(fault.config.corrupt_bytes, 2);
    }
}

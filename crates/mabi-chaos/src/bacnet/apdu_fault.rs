//! BACnet APDU-level fault injection.
//!
//! Simulates APDU (Application Protocol Data Unit) corruption at the wire level:
//! - Invalid APDU type bytes
//! - Corrupted invoke IDs (reuse, out-of-range)
//! - Wrong service choice codes
//! - Truncated APDU payload
//! - Invalid max-APDU-length-accepted values
//! - Duplicate invoke IDs (triggers TSM duplicate detection)

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
// APDU Fault Type
// =============================================================================

/// Type of APDU-level fault to inject.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ApduFaultType {
    /// Corrupt the APDU type nibble (byte 0, bits 4-7).
    #[default]
    InvalidApduType,

    /// Set the invoke ID to a specific or random invalid value.
    CorruptInvokeId {
        /// If Some, force this invoke ID. If None, pick randomly.
        forced_id: Option<u8>,
    },

    /// Replace the confirmed service choice with an unsupported value.
    InvalidServiceChoice {
        /// If Some, force this service code. If None, pick from reserved range.
        forced_code: Option<u8>,
    },

    /// Truncate the APDU payload after the header, leaving an incomplete message.
    TruncatePayload {
        /// Maximum bytes to keep (including header). 0 = header only.
        max_bytes: usize,
    },

    /// Set max-APDU-length-accepted to an impossibly small value.
    InvalidMaxApduLength {
        /// The small value to advertise.
        advertised_length: u16,
    },

    /// Reuse a recently-seen invoke ID to trigger TSM duplicate detection.
    DuplicateInvokeId,

    /// Inject a completely random APDU (garbage bytes).
    GarbageApdu {
        /// Length of garbage data.
        length: usize,
    },

    /// Set the segmentation flag incorrectly (claim segmented when not, or vice versa).
    WrongSegmentationFlags,
}

// =============================================================================
// APDU Fault Configuration
// =============================================================================

/// Configuration for APDU-level fault injection.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApduFaultConfig {
    /// Type of APDU fault to inject.
    pub fault_type: ApduFaultType,

    /// Whether to affect requests (client → server direction).
    #[serde(default)]
    pub affect_request: bool,

    /// Whether to affect responses (server → client direction).
    #[serde(default = "default_true")]
    pub affect_response: bool,
}

fn default_true() -> bool {
    true
}

impl Default for ApduFaultConfig {
    fn default() -> Self {
        Self {
            fault_type: ApduFaultType::InvalidApduType,
            affect_request: false,
            affect_response: true,
        }
    }
}

impl ApduFaultConfig {
    pub fn invalid_apdu_type() -> Self {
        Self::default()
    }

    pub fn corrupt_invoke_id(forced_id: Option<u8>) -> Self {
        Self {
            fault_type: ApduFaultType::CorruptInvokeId { forced_id },
            ..Default::default()
        }
    }

    pub fn invalid_service_choice(forced_code: Option<u8>) -> Self {
        Self {
            fault_type: ApduFaultType::InvalidServiceChoice { forced_code },
            ..Default::default()
        }
    }

    pub fn truncate_payload(max_bytes: usize) -> Self {
        Self {
            fault_type: ApduFaultType::TruncatePayload { max_bytes },
            ..Default::default()
        }
    }

    pub fn garbage_apdu(length: usize) -> Self {
        Self {
            fault_type: ApduFaultType::GarbageApdu { length },
            ..Default::default()
        }
    }

    pub fn duplicate_invoke_id() -> Self {
        Self {
            fault_type: ApduFaultType::DuplicateInvokeId,
            ..Default::default()
        }
    }

    pub fn request_side(mut self) -> Self {
        self.affect_request = true;
        self.affect_response = false;
        self
    }

    pub fn both_sides(mut self) -> Self {
        self.affect_request = true;
        self.affect_response = true;
        self
    }
}

// =============================================================================
// APDU Fault Implementation
// =============================================================================

/// BACnet APDU-level fault.
///
/// Corrupts APDU data at the wire level to test client/server resilience
/// to malformed BACnet/IP messages.
///
/// # Example
///
/// ```rust,ignore
/// use mabi_chaos::bacnet::ApduFault;
///
/// let fault = ApduFault::builder()
///     .id("apdu-corrupt-001")
///     .invalid_apdu_type()
///     .probability(0.05)
///     .target("bacnet-*")
///     .build();
/// ```
#[derive(Debug, Clone)]
pub struct ApduFault {
    base: BaseFault,
    config: ApduFaultConfig,
    rng: StdRng,
    /// Last seen invoke ID for duplicate injection.
    last_invoke_id: Option<u8>,
}

impl ApduFault {
    pub fn new(id: impl Into<String>, config: ApduFaultConfig) -> Self {
        let id = id.into();
        let metadata =
            FaultMetadata::new(&id, "BACnet APDU Fault", FaultCategory::Protocol)
                .with_description("Corrupts BACnet APDU at the wire level")
                .with_severity(FaultSeverity::High)
                .with_tag("bacnet")
                .with_tag("apdu");

        Self {
            base: BaseFault::new(metadata),
            config,
            rng: StdRng::from_entropy(),
            last_invoke_id: None,
        }
    }

    pub fn builder() -> ApduFaultBuilder {
        ApduFaultBuilder::default()
    }

    /// Apply the APDU-level fault to raw packet data.
    pub fn corrupt_apdu(&mut self, data: &mut Vec<u8>) {
        if data.is_empty() {
            return;
        }

        match &self.config.fault_type {
            ApduFaultType::InvalidApduType => {
                // Byte 0 bits 4-7 hold the APDU type.
                // Valid BACnet types: 0-7. Set to invalid.
                let invalid = self.rng.gen_range(8u8..16) << 4;
                data[0] = (data[0] & 0x0F) | invalid;
            }

            ApduFaultType::CorruptInvokeId { forced_id } => {
                // ConfirmedRequest APDU: invoke ID is at offset 2
                // ComplexAck APDU: invoke ID is at offset 1
                let apdu_type = (data[0] >> 4) & 0x0F;
                let invoke_offset = match apdu_type {
                    0 => Some(2), // ConfirmedRequest
                    3 => Some(1), // ComplexAck
                    5 => Some(1), // Error
                    6 => Some(1), // Reject
                    7 => Some(1), // Abort
                    _ => None,
                };
                if let Some(offset) = invoke_offset {
                    if data.len() > offset {
                        data[offset] = forced_id.unwrap_or_else(|| self.rng.gen());
                    }
                }
            }

            ApduFaultType::InvalidServiceChoice { forced_code } => {
                // ConfirmedRequest: service choice at offset 3
                let apdu_type = (data[0] >> 4) & 0x0F;
                let svc_offset = match apdu_type {
                    0 => Some(3), // ConfirmedRequest
                    3 => Some(2), // ComplexAck
                    _ => None,
                };
                if let Some(offset) = svc_offset {
                    if data.len() > offset {
                        // Use a code in the reserved range (128-255)
                        data[offset] = forced_code.unwrap_or_else(|| self.rng.gen_range(128..255));
                    }
                }
            }

            ApduFaultType::TruncatePayload { max_bytes } => {
                let keep = (*max_bytes).min(data.len());
                data.truncate(keep.max(1));
            }

            ApduFaultType::InvalidMaxApduLength { advertised_length } => {
                // ConfirmedRequest has max-APDU-length-accepted at offset 1 (encoded in nibble)
                if !data.is_empty() {
                    // Encode small length in metadata field
                    let encoded = match *advertised_length {
                        0..=50 => 0x00,
                        51..=128 => 0x01,
                        129..=206 => 0x02,
                        _ => 0x03,
                    };
                    data[0] = (data[0] & 0xF0) | (encoded & 0x0F);
                }
            }

            ApduFaultType::DuplicateInvokeId => {
                if let Some(last_id) = self.last_invoke_id {
                    let apdu_type = (data[0] >> 4) & 0x0F;
                    let invoke_offset = match apdu_type {
                        0 => Some(2),
                        3 => Some(1),
                        _ => None,
                    };
                    if let Some(offset) = invoke_offset {
                        if data.len() > offset {
                            data[offset] = last_id;
                        }
                    }
                }
                // Track current invoke ID
                let apdu_type = (data[0] >> 4) & 0x0F;
                let invoke_offset = match apdu_type {
                    0 => Some(2),
                    3 => Some(1),
                    _ => None,
                };
                if let Some(offset) = invoke_offset {
                    if data.len() > offset {
                        self.last_invoke_id = Some(data[offset]);
                    }
                }
            }

            ApduFaultType::GarbageApdu { length } => {
                data.clear();
                for _ in 0..*length {
                    data.push(self.rng.gen());
                }
            }

            ApduFaultType::WrongSegmentationFlags => {
                // Flip the segmentation bit (bit 3 of byte 0 for ConfirmedRequest)
                if !data.is_empty() {
                    data[0] ^= 0x08;
                }
            }
        }
    }

    pub fn config(&self) -> &ApduFaultConfig {
        &self.config
    }
}

#[async_trait]
impl Fault for ApduFault {
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

        ctx.set_metadata("bacnet_apdu_fault", &format!("{:?}", self.config.fault_type));
        ctx.record_applied_fault(self.id(), "apdu_corrupt");

        tracing::debug!(
            fault_id = %self.id(),
            target = %ctx.target.device_id,
            fault_type = ?self.config.fault_type,
            "Applying BACnet APDU fault"
        );

        Ok(FaultBehavior::Modify)
    }

    async fn after_operation(&self, ctx: &mut FaultContext) -> ChaosResult<FaultBehavior> {
        if ctx.get_metadata("bacnet_apdu_fault").is_none() {
            return Ok(FaultBehavior::Continue);
        }

        if self.config.affect_response {
            if let Some(ref mut response) = ctx.response_data {
                if let Some(ref mut raw) = response.raw {
                    let mut this = self.clone();
                    this.corrupt_apdu(raw);
                }
            }
        }

        Ok(FaultBehavior::Continue)
    }

    fn reset(&mut self) {
        self.base.reset();
        self.last_invoke_id = None;
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

#[derive(Debug, Default)]
pub struct ApduFaultBuilder {
    id: Option<String>,
    name: Option<String>,
    config: ApduFaultConfig,
    probability: f64,
    targets: Vec<String>,
    severity: FaultSeverity,
}

impl ApduFaultBuilder {
    pub fn id(mut self, id: impl Into<String>) -> Self {
        self.id = Some(id.into());
        self
    }

    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    pub fn fault_type(mut self, t: ApduFaultType) -> Self {
        self.config.fault_type = t;
        self
    }

    pub fn invalid_apdu_type(mut self) -> Self {
        self.config.fault_type = ApduFaultType::InvalidApduType;
        self
    }

    pub fn corrupt_invoke_id(mut self, forced_id: Option<u8>) -> Self {
        self.config.fault_type = ApduFaultType::CorruptInvokeId { forced_id };
        self
    }

    pub fn invalid_service_choice(mut self, forced_code: Option<u8>) -> Self {
        self.config.fault_type = ApduFaultType::InvalidServiceChoice { forced_code };
        self
    }

    pub fn truncate_payload(mut self, max_bytes: usize) -> Self {
        self.config.fault_type = ApduFaultType::TruncatePayload { max_bytes };
        self
    }

    pub fn garbage_apdu(mut self, length: usize) -> Self {
        self.config.fault_type = ApduFaultType::GarbageApdu { length };
        self
    }

    pub fn duplicate_invoke_id(mut self) -> Self {
        self.config.fault_type = ApduFaultType::DuplicateInvokeId;
        self
    }

    pub fn request_side(mut self) -> Self {
        self.config.affect_request = true;
        self.config.affect_response = false;
        self
    }

    pub fn response_side(mut self) -> Self {
        self.config.affect_request = false;
        self.config.affect_response = true;
        self
    }

    pub fn both_sides(mut self) -> Self {
        self.config.affect_request = true;
        self.config.affect_response = true;
        self
    }

    pub fn probability(mut self, p: f64) -> Self {
        self.probability = p.clamp(0.0, 1.0);
        self
    }

    pub fn target(mut self, target: impl Into<String>) -> Self {
        self.targets.push(target.into());
        self
    }

    pub fn severity(mut self, severity: FaultSeverity) -> Self {
        self.severity = severity;
        self
    }

    pub fn build(self) -> ApduFault {
        let id = self.id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        let mut fault = ApduFault::new(&id, self.config);

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

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_invalid_apdu_type() {
        let config = ApduFaultConfig::invalid_apdu_type();
        let mut fault = ApduFault::new("test", config);

        // Simulate a ConfirmedRequest APDU (type 0)
        let mut data = vec![0x00, 0x04, 0x01, 0x0C, 0x0C, 0x00];
        fault.corrupt_apdu(&mut data);
        let apdu_type = (data[0] >> 4) & 0x0F;
        assert!(apdu_type >= 8, "APDU type should be invalid (>=8), got {}", apdu_type);
    }

    #[test]
    fn test_corrupt_invoke_id() {
        let config = ApduFaultConfig::corrupt_invoke_id(Some(42));
        let mut fault = ApduFault::new("test", config);

        // ConfirmedRequest: invoke ID at offset 2
        let mut data = vec![0x00, 0x04, 0x01, 0x0C, 0x0C, 0x00];
        fault.corrupt_apdu(&mut data);
        assert_eq!(data[2], 42);
    }

    #[test]
    fn test_invalid_service_choice() {
        let config = ApduFaultConfig::invalid_service_choice(Some(200));
        let mut fault = ApduFault::new("test", config);

        let mut data = vec![0x00, 0x04, 0x01, 0x0C, 0x0C, 0x00];
        fault.corrupt_apdu(&mut data);
        assert_eq!(data[3], 200);
    }

    #[test]
    fn test_truncate_payload() {
        let config = ApduFaultConfig::truncate_payload(3);
        let mut fault = ApduFault::new("test", config);

        let mut data = vec![0x00, 0x04, 0x01, 0x0C, 0x0C, 0x00, 0x01, 0x02];
        fault.corrupt_apdu(&mut data);
        assert_eq!(data.len(), 3);
    }

    #[test]
    fn test_garbage_apdu() {
        let config = ApduFaultConfig::garbage_apdu(20);
        let mut fault = ApduFault::new("test", config);

        let mut data = vec![0x00, 0x04, 0x01, 0x0C];
        fault.corrupt_apdu(&mut data);
        assert_eq!(data.len(), 20);
    }

    #[test]
    fn test_duplicate_invoke_id() {
        let config = ApduFaultConfig::duplicate_invoke_id();
        let mut fault = ApduFault::new("test", config);

        // First call: learn invoke ID 5
        let mut data1 = vec![0x00, 0x04, 0x05, 0x0C, 0x0C];
        fault.corrupt_apdu(&mut data1);
        assert_eq!(fault.last_invoke_id, Some(5));

        // Second call: should reuse invoke ID 5
        let mut data2 = vec![0x00, 0x04, 0x09, 0x0C, 0x0C];
        fault.corrupt_apdu(&mut data2);
        assert_eq!(data2[2], 5);
    }

    #[test]
    fn test_wrong_segmentation_flags() {
        let config = ApduFaultConfig {
            fault_type: ApduFaultType::WrongSegmentationFlags,
            ..Default::default()
        };
        let mut fault = ApduFault::new("test", config);

        let mut data = vec![0x00, 0x04, 0x01, 0x0C];
        let original_byte0 = data[0];
        fault.corrupt_apdu(&mut data);
        assert_eq!(data[0] ^ original_byte0, 0x08);
    }

    #[test]
    fn test_builder() {
        let fault = ApduFault::builder()
            .id("apdu-001")
            .invalid_apdu_type()
            .probability(0.1)
            .target("bacnet-*")
            .severity(FaultSeverity::High)
            .build();

        assert_eq!(fault.id(), "apdu-001");
        assert_eq!(fault.base.metadata.probability, 0.1);
        assert!(fault.base.metadata.tags.contains(&"bacnet".to_string()));
    }
}

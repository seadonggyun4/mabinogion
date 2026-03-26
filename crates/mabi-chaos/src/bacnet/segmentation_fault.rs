//! BACnet APDU segmentation fault injection.
//!
//! Simulates failures in BACnet segmented message handling:
//! - Drop individual segments (causing reassembly timeout)
//! - Reorder segments (out-of-sequence delivery)
//! - Duplicate segments (test deduplication)
//! - Wrong sequence number in segment header
//! - Wrong proposed/actual window size
//! - Premature last-segment flag
//! - Segment size exceeding max-APDU-length

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
// Segmentation Fault Type
// =============================================================================

/// Type of segmentation fault to inject.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SegmentationFaultType {
    /// Drop a specific segment from the sequence.
    DropSegment {
        /// Which segment to drop (0-based). If None, pick randomly.
        segment_index: Option<usize>,
    },

    /// Deliver segments in wrong order.
    ReorderSegments,

    /// Send the same segment twice.
    DuplicateSegment {
        /// Which segment to duplicate. If None, pick randomly.
        segment_index: Option<usize>,
    },

    /// Set an incorrect sequence number in the segment header.
    WrongSequenceNumber {
        /// Offset to apply to the sequence number.
        offset: i8,
    },

    /// Set an invalid proposed/actual window size.
    InvalidWindowSize {
        /// Window size to inject (valid BACnet range is 1-127).
        window_size: u8,
    },

    /// Set the "more segments" flag incorrectly:
    /// mark a middle segment as the last, or the last as not-last.
    WrongMoreSegmentsFlag,

    /// Send a segment that exceeds the negotiated max-APDU-length.
    OversizedSegment {
        /// Extra bytes to append beyond the APDU limit.
        extra_bytes: usize,
    },

    /// Send a Segment-Ack with a wrong invoke ID.
    WrongSegmentAckInvokeId {
        /// Forced wrong invoke ID.
        wrong_invoke_id: u8,
    },

    /// Inject a long delay between segments (fragmentation timeout test).
    InterSegmentDelay {
        /// Delay in milliseconds between segments.
        delay_ms: u64,
    },
}

impl Default for SegmentationFaultType {
    fn default() -> Self {
        Self::DropSegment {
            segment_index: None,
        }
    }
}

// =============================================================================
// Segmentation Fault Configuration
// =============================================================================

/// Configuration for segmentation fault injection.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SegmentationFaultConfig {
    /// Type of segmentation fault.
    pub fault_type: SegmentationFaultType,

    /// Only affect responses with segment count >= this threshold.
    /// Use 0 to affect all segmented responses.
    #[serde(default)]
    pub min_segment_count: usize,
}

impl Default for SegmentationFaultConfig {
    fn default() -> Self {
        Self {
            fault_type: SegmentationFaultType::DropSegment {
                segment_index: None,
            },
            min_segment_count: 0,
        }
    }
}

impl SegmentationFaultConfig {
    pub fn drop_segment(segment_index: Option<usize>) -> Self {
        Self {
            fault_type: SegmentationFaultType::DropSegment { segment_index },
            ..Default::default()
        }
    }

    pub fn reorder_segments() -> Self {
        Self {
            fault_type: SegmentationFaultType::ReorderSegments,
            ..Default::default()
        }
    }

    pub fn duplicate_segment(segment_index: Option<usize>) -> Self {
        Self {
            fault_type: SegmentationFaultType::DuplicateSegment { segment_index },
            ..Default::default()
        }
    }

    pub fn wrong_sequence_number(offset: i8) -> Self {
        Self {
            fault_type: SegmentationFaultType::WrongSequenceNumber { offset },
            ..Default::default()
        }
    }

    pub fn invalid_window_size(window_size: u8) -> Self {
        Self {
            fault_type: SegmentationFaultType::InvalidWindowSize { window_size },
            ..Default::default()
        }
    }

    pub fn wrong_more_segments_flag() -> Self {
        Self {
            fault_type: SegmentationFaultType::WrongMoreSegmentsFlag,
            ..Default::default()
        }
    }

    pub fn oversized_segment(extra_bytes: usize) -> Self {
        Self {
            fault_type: SegmentationFaultType::OversizedSegment { extra_bytes },
            ..Default::default()
        }
    }

    pub fn inter_segment_delay(delay_ms: u64) -> Self {
        Self {
            fault_type: SegmentationFaultType::InterSegmentDelay { delay_ms },
            ..Default::default()
        }
    }

    pub fn min_segments(mut self, count: usize) -> Self {
        self.min_segment_count = count;
        self
    }
}

// =============================================================================
// Segmentation Fault Implementation
// =============================================================================

/// BACnet APDU segmentation fault.
///
/// Corrupts segmented BACnet message delivery to test reassembly
/// resilience, timeout handling, and duplicate detection.
///
/// # Example
///
/// ```rust,ignore
/// use mabi_chaos::bacnet::SegmentationFault;
///
/// // Drop the second segment from segmented responses
/// let fault = SegmentationFault::builder()
///     .id("seg-drop-001")
///     .drop_segment(Some(1))
///     .min_segments(3)
///     .probability(0.2)
///     .target("bacnet-*")
///     .build();
///
/// // Reorder segments
/// let fault = SegmentationFault::builder()
///     .id("seg-reorder-001")
///     .reorder_segments()
///     .build();
/// ```
#[derive(Debug, Clone)]
pub struct SegmentationFault {
    base: BaseFault,
    config: SegmentationFaultConfig,
    rng: StdRng,
}

impl SegmentationFault {
    pub fn new(id: impl Into<String>, config: SegmentationFaultConfig) -> Self {
        let id = id.into();
        let metadata =
            FaultMetadata::new(&id, "BACnet Segmentation Fault", FaultCategory::Protocol)
                .with_description("Corrupts BACnet APDU segmented message handling")
                .with_severity(FaultSeverity::High)
                .with_tag("bacnet")
                .with_tag("segmentation");

        Self {
            base: BaseFault::new(metadata),
            config,
            rng: StdRng::from_entropy(),
        }
    }

    pub fn builder() -> SegmentationFaultBuilder {
        SegmentationFaultBuilder::default()
    }

    /// Apply segmentation-level corruption to raw segment data.
    ///
    /// `data` represents a single segment's APDU bytes. The header format is:
    /// - byte 0: APDU type (bits 7-4), segmented flag (bit 3), more-segments (bit 2)
    /// - byte 1: sequence number (for segmented messages)
    /// - byte 2: proposed window size
    pub fn corrupt_segment(&mut self, data: &mut Vec<u8>) {
        if data.is_empty() {
            return;
        }

        match &self.config.fault_type {
            SegmentationFaultType::DropSegment { .. } => {
                // Signal drop by clearing the data
                data.clear();
            }

            SegmentationFaultType::ReorderSegments => {
                // Swap the sequence number with a random offset
                if data.len() > 1 {
                    let original_seq = data[1];
                    let new_seq = original_seq.wrapping_add(self.rng.gen_range(1..5));
                    data[1] = new_seq;
                }
            }

            SegmentationFaultType::DuplicateSegment { .. } => {
                // Mark for duplication (caller handles actual duplication)
                // For raw-level, append a copy indicator
                let original = data.clone();
                data.extend_from_slice(&original);
            }

            SegmentationFaultType::WrongSequenceNumber { offset } => {
                if data.len() > 1 {
                    data[1] = (data[1] as i16 + *offset as i16) as u8;
                }
            }

            SegmentationFaultType::InvalidWindowSize { window_size } => {
                if data.len() > 2 {
                    data[2] = *window_size;
                }
            }

            SegmentationFaultType::WrongMoreSegmentsFlag => {
                if !data.is_empty() {
                    // Toggle bit 2 (more-segments flag)
                    data[0] ^= 0x04;
                }
            }

            SegmentationFaultType::OversizedSegment { extra_bytes } => {
                for _ in 0..*extra_bytes {
                    data.push(self.rng.gen());
                }
            }

            SegmentationFaultType::WrongSegmentAckInvokeId { wrong_invoke_id } => {
                // Segment-Ack format: byte 0 = type, byte 1 = invoke ID
                if data.len() > 1 {
                    data[1] = *wrong_invoke_id;
                }
            }

            SegmentationFaultType::InterSegmentDelay { .. } => {
                // Delay is handled at the behavior level
            }
        }
    }

    pub fn config(&self) -> &SegmentationFaultConfig {
        &self.config
    }
}

#[async_trait]
impl Fault for SegmentationFault {
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

        // Check minimum segment count via metadata
        if self.config.min_segment_count > 0 {
            if let Some(count_str) = ctx.get_metadata("bacnet_segment_count") {
                if let Ok(count) = count_str.parse::<usize>() {
                    if count < self.config.min_segment_count {
                        return Ok(false);
                    }
                }
            }
        }

        Ok(true)
    }

    async fn apply(&self, ctx: &mut FaultContext) -> ChaosResult<FaultBehavior> {
        if !self.clone().base.check_probability() {
            return Ok(FaultBehavior::Continue);
        }

        ctx.set_metadata(
            "bacnet_segmentation_fault",
            &format!("{:?}", self.config.fault_type),
        );
        ctx.record_applied_fault(self.id(), "segmentation_fault");

        tracing::debug!(
            fault_id = %self.id(),
            target = %ctx.target.device_id,
            fault_type = ?self.config.fault_type,
            "Applying BACnet segmentation fault"
        );

        match &self.config.fault_type {
            SegmentationFaultType::DropSegment { .. } => Ok(FaultBehavior::Skip),

            SegmentationFaultType::InterSegmentDelay { delay_ms } => {
                ctx.add_delay(*delay_ms);
                tokio::time::sleep(std::time::Duration::from_millis(*delay_ms)).await;
                Ok(FaultBehavior::Delay {
                    duration_ms: *delay_ms,
                })
            }

            _ => Ok(FaultBehavior::Modify),
        }
    }

    async fn after_operation(&self, ctx: &mut FaultContext) -> ChaosResult<FaultBehavior> {
        if ctx.get_metadata("bacnet_segmentation_fault").is_none() {
            return Ok(FaultBehavior::Continue);
        }

        if let Some(ref mut response) = ctx.response_data {
            if let Some(ref mut raw) = response.raw {
                let mut this = self.clone();
                this.corrupt_segment(raw);
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

#[derive(Debug, Default)]
pub struct SegmentationFaultBuilder {
    id: Option<String>,
    name: Option<String>,
    config: SegmentationFaultConfig,
    probability: f64,
    targets: Vec<String>,
    severity: FaultSeverity,
}

impl SegmentationFaultBuilder {
    pub fn id(mut self, id: impl Into<String>) -> Self {
        self.id = Some(id.into());
        self
    }

    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    pub fn fault_type(mut self, t: SegmentationFaultType) -> Self {
        self.config.fault_type = t;
        self
    }

    pub fn drop_segment(mut self, index: Option<usize>) -> Self {
        self.config.fault_type = SegmentationFaultType::DropSegment {
            segment_index: index,
        };
        self
    }

    pub fn reorder_segments(mut self) -> Self {
        self.config.fault_type = SegmentationFaultType::ReorderSegments;
        self
    }

    pub fn duplicate_segment(mut self, index: Option<usize>) -> Self {
        self.config.fault_type = SegmentationFaultType::DuplicateSegment {
            segment_index: index,
        };
        self
    }

    pub fn wrong_sequence_number(mut self, offset: i8) -> Self {
        self.config.fault_type = SegmentationFaultType::WrongSequenceNumber { offset };
        self
    }

    pub fn invalid_window_size(mut self, window_size: u8) -> Self {
        self.config.fault_type = SegmentationFaultType::InvalidWindowSize { window_size };
        self
    }

    pub fn wrong_more_segments_flag(mut self) -> Self {
        self.config.fault_type = SegmentationFaultType::WrongMoreSegmentsFlag;
        self
    }

    pub fn oversized_segment(mut self, extra_bytes: usize) -> Self {
        self.config.fault_type = SegmentationFaultType::OversizedSegment { extra_bytes };
        self
    }

    pub fn inter_segment_delay(mut self, delay_ms: u64) -> Self {
        self.config.fault_type = SegmentationFaultType::InterSegmentDelay { delay_ms };
        self
    }

    pub fn min_segments(mut self, count: usize) -> Self {
        self.config.min_segment_count = count;
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

    pub fn build(self) -> SegmentationFault {
        let id = self.id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        let mut fault = SegmentationFault::new(&id, self.config);

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
    fn test_drop_segment() {
        let config = SegmentationFaultConfig::drop_segment(Some(1));
        let mut fault = SegmentationFault::new("test", config);

        let mut data = vec![0x00, 0x01, 0x04, 0x0C, 0x0C, 0x00];
        fault.corrupt_segment(&mut data);
        assert!(data.is_empty(), "Data should be cleared for drop");
    }

    #[test]
    fn test_reorder_segments() {
        let config = SegmentationFaultConfig::reorder_segments();
        let mut fault = SegmentationFault::new("test", config);

        let mut data = vec![0x08, 0x03, 0x04, 0x0C];
        let original_seq = data[1];
        fault.corrupt_segment(&mut data);
        assert_ne!(data[1], original_seq, "Sequence number should change");
    }

    #[test]
    fn test_duplicate_segment() {
        let config = SegmentationFaultConfig::duplicate_segment(None);
        let mut fault = SegmentationFault::new("test", config);

        let mut data = vec![0x08, 0x01, 0x04, 0x0C];
        let original_len = data.len();
        fault.corrupt_segment(&mut data);
        assert_eq!(data.len(), original_len * 2);
    }

    #[test]
    fn test_wrong_sequence_number() {
        let config = SegmentationFaultConfig::wrong_sequence_number(5);
        let mut fault = SegmentationFault::new("test", config);

        let mut data = vec![0x08, 0x03, 0x04, 0x0C];
        fault.corrupt_segment(&mut data);
        assert_eq!(data[1], 8); // 3 + 5
    }

    #[test]
    fn test_invalid_window_size() {
        let config = SegmentationFaultConfig::invalid_window_size(200);
        let mut fault = SegmentationFault::new("test", config);

        let mut data = vec![0x08, 0x01, 0x04, 0x0C];
        fault.corrupt_segment(&mut data);
        assert_eq!(data[2], 200);
    }

    #[test]
    fn test_wrong_more_segments_flag() {
        let config = SegmentationFaultConfig::wrong_more_segments_flag();
        let mut fault = SegmentationFault::new("test", config);

        let mut data = vec![0x08, 0x01, 0x04, 0x0C]; // segmented (bit 3=1), more=false (bit 2=0)
        fault.corrupt_segment(&mut data);
        assert_eq!(data[0], 0x0C); // bit 2 toggled: now more=true
    }

    #[test]
    fn test_oversized_segment() {
        let config = SegmentationFaultConfig::oversized_segment(50);
        let mut fault = SegmentationFault::new("test", config);

        let mut data = vec![0x08, 0x01, 0x04, 0x0C];
        let original_len = data.len();
        fault.corrupt_segment(&mut data);
        assert_eq!(data.len(), original_len + 50);
    }

    #[test]
    fn test_wrong_segment_ack_invoke_id() {
        let config = SegmentationFaultConfig {
            fault_type: SegmentationFaultType::WrongSegmentAckInvokeId {
                wrong_invoke_id: 99,
            },
            ..Default::default()
        };
        let mut fault = SegmentationFault::new("test", config);

        let mut data = vec![0x40, 0x05, 0x01, 0x04]; // SegmentAck, invoke=5
        fault.corrupt_segment(&mut data);
        assert_eq!(data[1], 99);
    }

    #[test]
    fn test_builder() {
        let fault = SegmentationFault::builder()
            .id("seg-001")
            .drop_segment(Some(2))
            .min_segments(3)
            .probability(0.2)
            .target("bacnet-*")
            .build();

        assert_eq!(fault.id(), "seg-001");
        assert!(fault
            .base
            .metadata
            .tags
            .contains(&"segmentation".to_string()));
        assert_eq!(fault.config.min_segment_count, 3);
    }
}

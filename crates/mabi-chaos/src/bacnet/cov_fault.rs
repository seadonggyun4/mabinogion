//! BACnet COV (Change of Value) notification fault injection.
//!
//! Simulates failures in COV notification delivery:
//! - Drop COV notifications entirely (subscriber never receives)
//! - Delay notifications (stale data window)
//! - Corrupt notification payloads (wrong values, missing properties)
//! - Send unsolicited notifications (no active subscription)
//! - Duplicate notifications (same sequence sent twice)
//! - Wrong process identifier (confuses subscriber routing)

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
// COV Fault Type
// =============================================================================

/// Type of COV notification fault to inject.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum CovFaultType {
    /// Drop COV notifications entirely. The subscriber will not receive any
    /// notification for the subscribed object until the subscription expires.
    #[default]
    DropNotification,

    /// Delay COV notifications, causing stale data at the subscriber.
    DelayNotification {
        /// Delay in milliseconds.
        delay_ms: u64,
    },

    /// Corrupt the present-value in the notification payload.
    CorruptPresentValue {
        /// Corruption strategy.
        strategy: CovCorruptionStrategy,
    },

    /// Send a notification with a wrong subscriber process identifier,
    /// causing the subscriber to not match the notification to any active
    /// subscription.
    WrongProcessIdentifier {
        /// If Some, use this value. Otherwise, random u32.
        forced_pid: Option<u32>,
    },

    /// Send duplicate notifications (same notification twice) to test
    /// subscriber idempotency.
    DuplicateNotification,

    /// Send an unsolicited notification for an object that the receiver
    /// did not subscribe to.
    UnsolicitedNotification {
        /// Object type to use in the unsolicited notification.
        object_type: u16,
        /// Object instance to use.
        object_instance: u32,
    },

    /// Send notification with missing required property values
    /// (e.g., omit present-value or status-flags).
    IncompleteNotification {
        /// Which property to omit.
        omit_property: OmittedCovProperty,
    },

    /// Flip the confirmed/unconfirmed status of the notification.
    WrongConfirmationStatus,
}

/// Strategy for corrupting present-value in COV notifications.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum CovCorruptionStrategy {
    /// Replace with a fixed constant value.
    #[default]
    FixedValue,

    /// Add random noise to the value.
    Noise {
        /// Noise amplitude as percentage of original value.
        amplitude_percent: f64,
    },

    /// Replace with NaN (for floating point values).
    NaN,

    /// Replace with the negative of the original value.
    Negate,

    /// Replace with zero.
    Zero,

    /// Replace with maximum value for the type.
    MaxValue,
}

/// Which property to omit from a COV notification.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum OmittedCovProperty {
    #[default]
    PresentValue,
    StatusFlags,
    Both,
}

// =============================================================================
// COV Fault Configuration
// =============================================================================

/// Configuration for COV notification fault injection.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CovFaultConfig {
    /// Type of COV fault.
    pub fault_type: CovFaultType,

    /// Object types to target (BACnet object type codes).
    /// Empty = target all object types.
    #[serde(default)]
    pub target_object_types: Vec<u16>,

    /// Only affect confirmed COV notifications.
    #[serde(default)]
    pub confirmed_only: bool,

    /// Only affect unconfirmed COV notifications.
    #[serde(default)]
    pub unconfirmed_only: bool,
}

impl Default for CovFaultConfig {
    fn default() -> Self {
        Self {
            fault_type: CovFaultType::DropNotification,
            target_object_types: Vec::new(),
            confirmed_only: false,
            unconfirmed_only: false,
        }
    }
}

impl CovFaultConfig {
    pub fn drop_notification() -> Self {
        Self::default()
    }

    pub fn delay_notification(delay_ms: u64) -> Self {
        Self {
            fault_type: CovFaultType::DelayNotification { delay_ms },
            ..Default::default()
        }
    }

    pub fn corrupt_value(strategy: CovCorruptionStrategy) -> Self {
        Self {
            fault_type: CovFaultType::CorruptPresentValue { strategy },
            ..Default::default()
        }
    }

    pub fn wrong_process_id(forced_pid: Option<u32>) -> Self {
        Self {
            fault_type: CovFaultType::WrongProcessIdentifier { forced_pid },
            ..Default::default()
        }
    }

    pub fn duplicate_notification() -> Self {
        Self {
            fault_type: CovFaultType::DuplicateNotification,
            ..Default::default()
        }
    }

    pub fn incomplete(omit: OmittedCovProperty) -> Self {
        Self {
            fault_type: CovFaultType::IncompleteNotification { omit_property: omit },
            ..Default::default()
        }
    }

    pub fn for_object_types(mut self, types: Vec<u16>) -> Self {
        self.target_object_types = types;
        self
    }

    pub fn confirmed_only(mut self) -> Self {
        self.confirmed_only = true;
        self.unconfirmed_only = false;
        self
    }

    pub fn unconfirmed_only(mut self) -> Self {
        self.unconfirmed_only = true;
        self.confirmed_only = false;
        self
    }
}

// =============================================================================
// COV Fault Implementation
// =============================================================================

/// BACnet COV notification fault.
///
/// Simulates failures in COV (Change of Value) notification delivery,
/// which is critical for real-time monitoring in BACnet systems.
///
/// # Example
///
/// ```rust,ignore
/// use mabi_chaos::bacnet::CovFault;
///
/// // Drop 10% of COV notifications for analog inputs
/// let fault = CovFault::builder()
///     .id("cov-drop-001")
///     .drop_notification()
///     .for_object_types(vec![0]) // AnalogInput
///     .probability(0.1)
///     .target("bacnet-*")
///     .build();
///
/// // Delay COV notifications by 5 seconds
/// let fault = CovFault::builder()
///     .id("cov-delay-001")
///     .delay_notification(5000)
///     .build();
/// ```
#[derive(Debug, Clone)]
pub struct CovFault {
    base: BaseFault,
    config: CovFaultConfig,
    rng: StdRng,
}

impl CovFault {
    pub fn new(id: impl Into<String>, config: CovFaultConfig) -> Self {
        let id = id.into();
        let metadata =
            FaultMetadata::new(&id, "BACnet COV Fault", FaultCategory::Protocol)
                .with_description("Injects BACnet COV notification failures")
                .with_severity(FaultSeverity::High)
                .with_tag("bacnet")
                .with_tag("cov");

        Self {
            base: BaseFault::new(metadata),
            config,
            rng: StdRng::from_entropy(),
        }
    }

    pub fn builder() -> CovFaultBuilder {
        CovFaultBuilder::default()
    }

    /// Check if an object type matches the filter.
    pub fn matches_object_type(&self, obj_type: u16) -> bool {
        if self.config.target_object_types.is_empty() {
            return true;
        }
        self.config.target_object_types.contains(&obj_type)
    }

    /// Get the FaultBehavior for this COV fault type.
    pub fn behavior(&self) -> FaultBehavior {
        match &self.config.fault_type {
            CovFaultType::DropNotification => FaultBehavior::Skip,

            CovFaultType::DelayNotification { delay_ms } => FaultBehavior::Delay {
                duration_ms: *delay_ms,
            },

            CovFaultType::CorruptPresentValue { .. }
            | CovFaultType::WrongProcessIdentifier { .. }
            | CovFaultType::DuplicateNotification
            | CovFaultType::UnsolicitedNotification { .. }
            | CovFaultType::IncompleteNotification { .. }
            | CovFaultType::WrongConfirmationStatus => FaultBehavior::Modify,
        }
    }

    /// Apply COV-specific corruption to raw notification data.
    pub fn corrupt_cov_data(&mut self, data: &mut Vec<u8>) {
        match &self.config.fault_type {
            CovFaultType::CorruptPresentValue { strategy } => {
                // Find present-value tag in notification and corrupt it.
                // BACnet COV notification contains property values as tagged data.
                // For simplicity, corrupt a portion of the payload after header.
                if data.len() > 10 {
                    match strategy {
                        CovCorruptionStrategy::FixedValue => {
                            // Replace last 4 payload bytes with a fixed pattern
                            let start = data.len().saturating_sub(4);
                            for b in &mut data[start..] {
                                *b = 0x42; // Fixed pattern
                            }
                        }
                        CovCorruptionStrategy::Noise { .. } => {
                            // XOR random noise into value bytes
                            let start = data.len().saturating_sub(4);
                            for b in &mut data[start..] {
                                *b ^= self.rng.gen_range(1..32u8);
                            }
                        }
                        CovCorruptionStrategy::NaN => {
                            // Insert IEEE 754 NaN (0x7FC00000)
                            if data.len() >= 14 {
                                let start = data.len().saturating_sub(4);
                                let nan_bytes = [0x7F, 0xC0, 0x00, 0x00];
                                for (i, b) in nan_bytes.iter().enumerate() {
                                    if start + i < data.len() {
                                        data[start + i] = *b;
                                    }
                                }
                            }
                        }
                        CovCorruptionStrategy::Zero => {
                            let start = data.len().saturating_sub(4);
                            for b in &mut data[start..] {
                                *b = 0;
                            }
                        }
                        CovCorruptionStrategy::Negate | CovCorruptionStrategy::MaxValue => {
                            let start = data.len().saturating_sub(4);
                            for b in &mut data[start..] {
                                *b = 0xFF;
                            }
                        }
                    }
                }
            }

            CovFaultType::WrongProcessIdentifier { forced_pid } => {
                // Process identifier is typically in the first tagged value.
                // Replace context tag [0] value with forced_pid.
                if data.len() > 6 {
                    let pid = forced_pid.unwrap_or_else(|| self.rng.gen());
                    let pid_bytes = pid.to_be_bytes();
                    // Write after first context tag header (approximate)
                    let offset = 4.min(data.len() - 4);
                    for (i, b) in pid_bytes.iter().enumerate() {
                        if offset + i < data.len() {
                            data[offset + i] = *b;
                        }
                    }
                }
            }

            CovFaultType::DuplicateNotification => {
                // Duplicate the entire packet
                let original = data.clone();
                data.extend_from_slice(&original);
            }

            CovFaultType::IncompleteNotification { omit_property } => {
                // Truncate to remove property values at the end
                let truncate_to = match omit_property {
                    OmittedCovProperty::PresentValue => data.len().saturating_sub(8),
                    OmittedCovProperty::StatusFlags => data.len().saturating_sub(4),
                    OmittedCovProperty::Both => data.len().saturating_sub(12),
                };
                data.truncate(truncate_to.max(6));
            }

            CovFaultType::WrongConfirmationStatus => {
                // Flip between confirmed (service 1) and unconfirmed (service 2)
                // ConfirmedCOVNotification = 1, UnconfirmedCOVNotification = 2
                if data.len() > 3 {
                    let apdu_type = (data[0] >> 4) & 0x0F;
                    if apdu_type == 0 {
                        // ConfirmedRequest → make it look like Unconfirmed
                        data[0] = (data[0] & 0x0F) | 0x10; // type=1 (UnconfirmedRequest)
                    } else if apdu_type == 1 {
                        // UnconfirmedRequest → make it look like Confirmed
                        data[0] = (data[0] & 0x0F) | 0x00; // type=0 (ConfirmedRequest)
                    }
                }
            }

            // These types don't modify raw data
            CovFaultType::DropNotification
            | CovFaultType::DelayNotification { .. }
            | CovFaultType::UnsolicitedNotification { .. } => {}
        }
    }

    pub fn config(&self) -> &CovFaultConfig {
        &self.config
    }
}

#[async_trait]
impl Fault for CovFault {
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

        // Check object type filter via metadata
        if let Some(obj_type_str) = ctx.get_metadata("bacnet_object_type") {
            if let Ok(obj_type) = obj_type_str.parse::<u16>() {
                if !self.matches_object_type(obj_type) {
                    return Ok(false);
                }
            }
        }

        // Check confirmation filter
        if self.config.confirmed_only {
            if ctx.get_metadata("bacnet_cov_confirmed") != Some("true") {
                return Ok(false);
            }
        }
        if self.config.unconfirmed_only {
            if ctx.get_metadata("bacnet_cov_confirmed") == Some("true") {
                return Ok(false);
            }
        }

        Ok(true)
    }

    async fn apply(&self, ctx: &mut FaultContext) -> ChaosResult<FaultBehavior> {
        if !self.clone().base.check_probability() {
            return Ok(FaultBehavior::Continue);
        }

        ctx.set_metadata("bacnet_cov_fault", &format!("{:?}", self.config.fault_type));
        ctx.record_applied_fault(self.id(), "cov_fault");

        tracing::debug!(
            fault_id = %self.id(),
            target = %ctx.target.device_id,
            fault_type = ?self.config.fault_type,
            "Applying BACnet COV fault"
        );

        // For delay faults, actually sleep
        if let CovFaultType::DelayNotification { delay_ms } = &self.config.fault_type {
            ctx.add_delay(*delay_ms);
            tokio::time::sleep(std::time::Duration::from_millis(*delay_ms)).await;
        }

        Ok(self.behavior())
    }

    async fn after_operation(&self, ctx: &mut FaultContext) -> ChaosResult<FaultBehavior> {
        if ctx.get_metadata("bacnet_cov_fault").is_none() {
            return Ok(FaultBehavior::Continue);
        }

        // Apply corruption to response data
        if let Some(ref mut response) = ctx.response_data {
            if let Some(ref mut raw) = response.raw {
                let mut this = self.clone();
                this.corrupt_cov_data(raw);
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
pub struct CovFaultBuilder {
    id: Option<String>,
    name: Option<String>,
    config: CovFaultConfig,
    probability: f64,
    targets: Vec<String>,
    severity: FaultSeverity,
}

impl CovFaultBuilder {
    pub fn id(mut self, id: impl Into<String>) -> Self {
        self.id = Some(id.into());
        self
    }

    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    pub fn fault_type(mut self, t: CovFaultType) -> Self {
        self.config.fault_type = t;
        self
    }

    pub fn drop_notification(mut self) -> Self {
        self.config.fault_type = CovFaultType::DropNotification;
        self
    }

    pub fn delay_notification(mut self, delay_ms: u64) -> Self {
        self.config.fault_type = CovFaultType::DelayNotification { delay_ms };
        self
    }

    pub fn corrupt_value(mut self, strategy: CovCorruptionStrategy) -> Self {
        self.config.fault_type = CovFaultType::CorruptPresentValue { strategy };
        self
    }

    pub fn wrong_process_id(mut self, forced_pid: Option<u32>) -> Self {
        self.config.fault_type = CovFaultType::WrongProcessIdentifier { forced_pid };
        self
    }

    pub fn duplicate_notification(mut self) -> Self {
        self.config.fault_type = CovFaultType::DuplicateNotification;
        self
    }

    pub fn incomplete(mut self, omit: OmittedCovProperty) -> Self {
        self.config.fault_type = CovFaultType::IncompleteNotification { omit_property: omit };
        self
    }

    pub fn wrong_confirmation(mut self) -> Self {
        self.config.fault_type = CovFaultType::WrongConfirmationStatus;
        self
    }

    pub fn for_object_types(mut self, types: Vec<u16>) -> Self {
        self.config.target_object_types = types;
        self
    }

    pub fn confirmed_only(mut self) -> Self {
        self.config.confirmed_only = true;
        self.config.unconfirmed_only = false;
        self
    }

    pub fn unconfirmed_only(mut self) -> Self {
        self.config.unconfirmed_only = true;
        self.config.confirmed_only = false;
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

    pub fn build(self) -> CovFault {
        let id = self.id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        let mut fault = CovFault::new(&id, self.config);

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
    fn test_drop_behavior() {
        let config = CovFaultConfig::drop_notification();
        let fault = CovFault::new("test", config);
        assert_eq!(fault.behavior(), FaultBehavior::Skip);
    }

    #[test]
    fn test_delay_behavior() {
        let config = CovFaultConfig::delay_notification(5000);
        let fault = CovFault::new("test", config);
        assert_eq!(fault.behavior(), FaultBehavior::Delay { duration_ms: 5000 });
    }

    #[test]
    fn test_corrupt_behavior() {
        let config = CovFaultConfig::corrupt_value(CovCorruptionStrategy::NaN);
        let fault = CovFault::new("test", config);
        assert_eq!(fault.behavior(), FaultBehavior::Modify);
    }

    #[test]
    fn test_object_type_filter() {
        let config = CovFaultConfig::drop_notification().for_object_types(vec![0, 3]); // AI, BI
        let fault = CovFault::new("test", config);

        assert!(fault.matches_object_type(0));
        assert!(fault.matches_object_type(3));
        assert!(!fault.matches_object_type(1));
    }

    #[test]
    fn test_empty_filter_matches_all() {
        let config = CovFaultConfig::drop_notification();
        let fault = CovFault::new("test", config);
        assert!(fault.matches_object_type(0));
        assert!(fault.matches_object_type(99));
    }

    #[test]
    fn test_corrupt_fixed_value() {
        let config = CovFaultConfig::corrupt_value(CovCorruptionStrategy::FixedValue);
        let mut fault = CovFault::new("test", config);

        let mut data = vec![0x00; 20];
        fault.corrupt_cov_data(&mut data);
        // Last 4 bytes should be 0x42
        assert_eq!(data[16], 0x42);
        assert_eq!(data[17], 0x42);
        assert_eq!(data[18], 0x42);
        assert_eq!(data[19], 0x42);
    }

    #[test]
    fn test_corrupt_nan() {
        let config = CovFaultConfig::corrupt_value(CovCorruptionStrategy::NaN);
        let mut fault = CovFault::new("test", config);

        let mut data = vec![0x00; 20];
        fault.corrupt_cov_data(&mut data);
        // Last 4 bytes should be NaN pattern
        assert_eq!(data[16], 0x7F);
        assert_eq!(data[17], 0xC0);
        assert_eq!(data[18], 0x00);
        assert_eq!(data[19], 0x00);
    }

    #[test]
    fn test_duplicate_notification() {
        let config = CovFaultConfig::duplicate_notification();
        let mut fault = CovFault::new("test", config);

        let mut data = vec![0x01, 0x02, 0x03, 0x04];
        let original_len = data.len();
        fault.corrupt_cov_data(&mut data);
        assert_eq!(data.len(), original_len * 2);
        assert_eq!(&data[0..4], &data[4..8]);
    }

    #[test]
    fn test_incomplete_notification() {
        let config = CovFaultConfig::incomplete(OmittedCovProperty::PresentValue);
        let mut fault = CovFault::new("test", config);

        let mut data = vec![0x00; 20];
        fault.corrupt_cov_data(&mut data);
        assert_eq!(data.len(), 12); // 20 - 8
    }

    #[test]
    fn test_wrong_confirmation_status() {
        let config = CovFaultConfig {
            fault_type: CovFaultType::WrongConfirmationStatus,
            ..Default::default()
        };
        let mut fault = CovFault::new("test", config);

        // ConfirmedRequest (type 0) → should become UnconfirmedRequest (type 1)
        let mut data = vec![0x00, 0x04, 0x01, 0x01, 0x00];
        fault.corrupt_cov_data(&mut data);
        let apdu_type = (data[0] >> 4) & 0x0F;
        assert_eq!(apdu_type, 1);
    }

    #[test]
    fn test_builder() {
        let fault = CovFault::builder()
            .id("cov-001")
            .drop_notification()
            .for_object_types(vec![0, 3, 5])
            .probability(0.1)
            .target("bacnet-*")
            .build();

        assert_eq!(fault.id(), "cov-001");
        assert!(fault.base.metadata.tags.contains(&"cov".to_string()));
    }
}

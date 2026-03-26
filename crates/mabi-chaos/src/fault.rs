//! Core fault trait and related types.
//!
//! The [`Fault`] trait is the foundation of the chaos engineering module.
//! All fault types implement this trait to provide consistent behavior.

use std::fmt;
use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::context::FaultContext;
use crate::error::ChaosResult;

// =============================================================================
// Fault Trait
// =============================================================================

/// Core trait that all fault types must implement.
///
/// The `Fault` trait defines the interface for fault injection. Faults can:
/// - Be enabled/disabled dynamically
/// - Have probability-based activation
/// - Track their own state and statistics
/// - Be applied before and after operations
///
/// # Example
///
/// ```rust,ignore
/// use mabi_chaos::prelude::*;
///
/// struct CustomFault {
///     config: CustomConfig,
/// }
///
/// #[async_trait]
/// impl Fault for CustomFault {
///     fn metadata(&self) -> &FaultMetadata {
///         // ...
///     }
///
///     async fn should_activate(&self, ctx: &FaultContext) -> ChaosResult<bool> {
///         // Determine if fault should activate
///         Ok(true)
///     }
///
///     async fn apply(&self, ctx: &mut FaultContext) -> ChaosResult<FaultBehavior> {
///         // Apply fault effects
///         Ok(FaultBehavior::Continue)
///     }
/// }
/// ```
#[async_trait]
pub trait Fault: Send + Sync + fmt::Debug {
    /// Get fault metadata.
    fn metadata(&self) -> &FaultMetadata;

    /// Get fault ID.
    fn id(&self) -> &str {
        &self.metadata().id
    }

    /// Get fault name.
    fn name(&self) -> &str {
        &self.metadata().name
    }

    /// Get fault category.
    fn category(&self) -> FaultCategory {
        self.metadata().category
    }

    /// Get fault severity.
    fn severity(&self) -> FaultSeverity {
        self.metadata().severity
    }

    /// Check if fault is enabled.
    fn is_enabled(&self) -> bool {
        self.metadata().enabled
    }

    /// Enable the fault.
    fn enable(&mut self);

    /// Disable the fault.
    fn disable(&mut self);

    /// Check if the fault should activate for the given context.
    ///
    /// This method considers probability, target matching, and other conditions.
    async fn should_activate(&self, ctx: &FaultContext) -> ChaosResult<bool>;

    /// Apply the fault to the operation.
    ///
    /// Returns a [`FaultBehavior`] indicating how the operation should proceed.
    async fn apply(&self, ctx: &mut FaultContext) -> ChaosResult<FaultBehavior>;

    /// Called before the operation (optional).
    ///
    /// Override this to inject faults before the actual operation.
    async fn before_operation(&self, ctx: &mut FaultContext) -> ChaosResult<FaultBehavior> {
        if !self.is_enabled() {
            return Ok(FaultBehavior::Continue);
        }
        if !self.should_activate(ctx).await? {
            return Ok(FaultBehavior::Continue);
        }
        self.apply(ctx).await
    }

    /// Called after the operation (optional).
    ///
    /// Override this to modify responses or inject post-operation faults.
    async fn after_operation(&self, _ctx: &mut FaultContext) -> ChaosResult<FaultBehavior> {
        Ok(FaultBehavior::Continue)
    }

    /// Reset the fault state.
    fn reset(&mut self);

    /// Get fault statistics.
    fn statistics(&self) -> FaultStatistics {
        FaultStatistics::default()
    }

    /// Clone the fault into a boxed trait object.
    fn clone_box(&self) -> Box<dyn Fault>;

    /// Convert to Any for downcasting.
    fn as_any(&self) -> &dyn std::any::Any;

    /// Convert to mutable Any for downcasting.
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any;
}

impl Clone for Box<dyn Fault> {
    fn clone(&self) -> Self {
        self.clone_box()
    }
}

/// Thread-safe fault reference.
pub type ArcFault = Arc<dyn Fault>;

/// Boxed fault type.
pub type BoxedFault = Box<dyn Fault>;

// =============================================================================
// Fault Behavior
// =============================================================================

/// Behavior to take after fault application.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FaultBehavior {
    /// Continue with the operation normally.
    Continue,

    /// Skip the operation entirely.
    Skip,

    /// Abort with an error.
    Abort { error: String },

    /// Delay the operation.
    Delay { duration_ms: u64 },

    /// Modify the request/response.
    Modify,

    /// Retry the operation.
    Retry { max_attempts: u32 },

    /// Return a specific response.
    ReturnError { error_code: i32, message: String },
}

impl Default for FaultBehavior {
    fn default() -> Self {
        Self::Continue
    }
}

impl FaultBehavior {
    /// Check if operation should proceed.
    pub fn should_proceed(&self) -> bool {
        matches!(self, Self::Continue | Self::Delay { .. } | Self::Modify)
    }

    /// Check if operation was affected by fault.
    pub fn was_affected(&self) -> bool {
        !matches!(self, Self::Continue)
    }
}

// =============================================================================
// Fault Metadata
// =============================================================================

/// Metadata describing a fault.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FaultMetadata {
    /// Unique fault identifier.
    pub id: String,

    /// Human-readable name.
    pub name: String,

    /// Description of what the fault does.
    pub description: String,

    /// Fault category.
    pub category: FaultCategory,

    /// Fault severity.
    pub severity: FaultSeverity,

    /// Whether the fault is enabled.
    pub enabled: bool,

    /// Probability of activation (0.0 - 1.0).
    pub probability: f64,

    /// Target patterns this fault applies to.
    #[serde(default)]
    pub targets: Vec<String>,

    /// Tags for categorization.
    #[serde(default)]
    pub tags: Vec<String>,

    /// Creation timestamp.
    pub created_at: DateTime<Utc>,

    /// Last update timestamp.
    pub updated_at: DateTime<Utc>,
}

impl FaultMetadata {
    /// Create new fault metadata.
    pub fn new(id: impl Into<String>, name: impl Into<String>, category: FaultCategory) -> Self {
        let now = Utc::now();
        Self {
            id: id.into(),
            name: name.into(),
            description: String::new(),
            category,
            severity: FaultSeverity::Low,
            enabled: true,
            probability: 1.0,
            targets: Vec::new(),
            tags: Vec::new(),
            created_at: now,
            updated_at: now,
        }
    }

    /// Set description.
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = description.into();
        self
    }

    /// Set severity.
    pub fn with_severity(mut self, severity: FaultSeverity) -> Self {
        self.severity = severity;
        self
    }

    /// Set probability.
    pub fn with_probability(mut self, probability: f64) -> Self {
        self.probability = probability.clamp(0.0, 1.0);
        self
    }

    /// Add target pattern.
    pub fn with_target(mut self, target: impl Into<String>) -> Self {
        self.targets.push(target.into());
        self
    }

    /// Add targets.
    pub fn with_targets(mut self, targets: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.targets.extend(targets.into_iter().map(Into::into));
        self
    }

    /// Add tag.
    pub fn with_tag(mut self, tag: impl Into<String>) -> Self {
        self.tags.push(tag.into());
        self
    }

    /// Check if a target matches any pattern.
    pub fn matches_target(&self, target: &str) -> bool {
        if self.targets.is_empty() {
            return true; // No targets = matches all
        }
        self.targets.iter().any(|pattern| {
            if pattern.contains('*') {
                // Simple glob matching
                let parts: Vec<&str> = pattern.split('*').collect();
                if parts.len() == 2 {
                    let (prefix, suffix) = (parts[0], parts[1]);
                    target.starts_with(prefix) && target.ends_with(suffix)
                } else if pattern.starts_with('*') {
                    target.ends_with(&pattern[1..])
                } else if pattern.ends_with('*') {
                    target.starts_with(&pattern[..pattern.len() - 1])
                } else {
                    target == pattern
                }
            } else {
                target == pattern
            }
        })
    }
}

impl Default for FaultMetadata {
    fn default() -> Self {
        Self::new("unknown", "Unknown Fault", FaultCategory::Other)
    }
}

// =============================================================================
// Fault Category
// =============================================================================

/// Category of fault.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum FaultCategory {
    /// Network-related faults (latency, packet loss, disconnect).
    Network,

    /// Device-related faults (offline, slow response, corrupted data).
    Device,

    /// Protocol-related faults (malformed packets, checksums, timeouts).
    Protocol,

    /// Resource-related faults (memory, CPU, disk).
    Resource,

    /// Other/custom faults.
    #[default]
    Other,
}

impl fmt::Display for FaultCategory {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Network => write!(f, "network"),
            Self::Device => write!(f, "device"),
            Self::Protocol => write!(f, "protocol"),
            Self::Resource => write!(f, "resource"),
            Self::Other => write!(f, "other"),
        }
    }
}

// =============================================================================
// Fault Severity
// =============================================================================

/// Severity level of a fault.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, Default,
)]
#[serde(rename_all = "lowercase")]
pub enum FaultSeverity {
    /// Minor impact, typically logging or metrics only.
    #[default]
    Low,

    /// Moderate impact, may cause delays or retries.
    Medium,

    /// Significant impact, may cause operation failures.
    High,

    /// Critical impact, may cause system-wide issues.
    Critical,
}

impl fmt::Display for FaultSeverity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Low => write!(f, "low"),
            Self::Medium => write!(f, "medium"),
            Self::High => write!(f, "high"),
            Self::Critical => write!(f, "critical"),
        }
    }
}

// =============================================================================
// Fault State
// =============================================================================

/// State of a fault.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum FaultState {
    /// Fault is inactive.
    #[default]
    Inactive,

    /// Fault is active and may be triggered.
    Active,

    /// Fault is paused (temporarily inactive).
    Paused,

    /// Fault is in cooldown period.
    Cooldown,
}

impl FaultState {
    /// Check if fault can be triggered.
    pub fn can_trigger(&self) -> bool {
        matches!(self, Self::Active)
    }
}

// =============================================================================
// Fault Statistics
// =============================================================================

/// Statistics for a fault.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FaultStatistics {
    /// Total number of times the fault was checked.
    pub checks_total: u64,

    /// Number of times the fault was activated.
    pub activations_total: u64,

    /// Number of times activation was skipped (probability).
    pub skips_total: u64,

    /// Number of errors during fault application.
    pub errors_total: u64,

    /// Total delay injected (in milliseconds).
    pub total_delay_ms: u64,

    /// Last activation timestamp.
    pub last_activation: Option<DateTime<Utc>>,

    /// Last error message.
    pub last_error: Option<String>,
}

impl FaultStatistics {
    /// Record a check.
    pub fn record_check(&mut self) {
        self.checks_total += 1;
    }

    /// Record an activation.
    pub fn record_activation(&mut self) {
        self.activations_total += 1;
        self.last_activation = Some(Utc::now());
    }

    /// Record a skip.
    pub fn record_skip(&mut self) {
        self.skips_total += 1;
    }

    /// Record an error.
    pub fn record_error(&mut self, error: &str) {
        self.errors_total += 1;
        self.last_error = Some(error.to_string());
    }

    /// Record delay.
    pub fn record_delay(&mut self, delay_ms: u64) {
        self.total_delay_ms += delay_ms;
    }

    /// Get activation rate.
    pub fn activation_rate(&self) -> f64 {
        if self.checks_total == 0 {
            0.0
        } else {
            self.activations_total as f64 / self.checks_total as f64
        }
    }

    /// Get error rate.
    pub fn error_rate(&self) -> f64 {
        if self.activations_total == 0 {
            0.0
        } else {
            self.errors_total as f64 / self.activations_total as f64
        }
    }
}

// =============================================================================
// Base Fault Implementation
// =============================================================================

/// Base fault implementation that provides common functionality.
///
/// This struct can be embedded in concrete fault types to avoid boilerplate.
#[derive(Debug, Clone)]
pub struct BaseFault {
    pub metadata: FaultMetadata,
    pub state: FaultState,
    pub statistics: FaultStatistics,
}

impl BaseFault {
    /// Create a new base fault.
    pub fn new(metadata: FaultMetadata) -> Self {
        Self {
            metadata,
            state: FaultState::Active,
            statistics: FaultStatistics::default(),
        }
    }

    /// Enable the fault.
    pub fn enable(&mut self) {
        self.metadata.enabled = true;
        self.state = FaultState::Active;
        self.metadata.updated_at = Utc::now();
    }

    /// Disable the fault.
    pub fn disable(&mut self) {
        self.metadata.enabled = false;
        self.state = FaultState::Inactive;
        self.metadata.updated_at = Utc::now();
    }

    /// Reset the fault.
    pub fn reset(&mut self) {
        self.state = FaultState::Active;
        self.statistics = FaultStatistics::default();
        self.metadata.updated_at = Utc::now();
    }

    /// Check if should activate based on probability.
    pub fn check_probability(&mut self) -> bool {
        self.statistics.record_check();

        if self.metadata.probability >= 1.0 {
            return true;
        }
        if self.metadata.probability <= 0.0 {
            self.statistics.record_skip();
            return false;
        }

        let activated = rand::random::<f64>() < self.metadata.probability;
        if !activated {
            self.statistics.record_skip();
        }
        activated
    }

    /// Check if target matches.
    pub fn matches_target(&self, target: &str) -> bool {
        self.metadata.matches_target(target)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fault_metadata() {
        let metadata = FaultMetadata::new("test-001", "Test Fault", FaultCategory::Network)
            .with_description("A test fault")
            .with_severity(FaultSeverity::Medium)
            .with_probability(0.5)
            .with_target("device-*");

        assert_eq!(metadata.id, "test-001");
        assert_eq!(metadata.category, FaultCategory::Network);
        assert_eq!(metadata.severity, FaultSeverity::Medium);
        assert!(metadata.matches_target("device-001"));
        assert!(!metadata.matches_target("other-001"));
    }

    #[test]
    fn test_target_matching() {
        let metadata = FaultMetadata::new("test", "Test", FaultCategory::Network)
            .with_target("modbus-*")
            .with_target("*-sensor")
            .with_target("exact-match");

        assert!(metadata.matches_target("modbus-001"));
        assert!(metadata.matches_target("modbus-server"));
        assert!(metadata.matches_target("temp-sensor"));
        assert!(metadata.matches_target("humidity-sensor"));
        assert!(metadata.matches_target("exact-match"));
        assert!(!metadata.matches_target("opcua-001"));
        assert!(!metadata.matches_target("sensor-001"));
    }

    #[test]
    fn test_fault_behavior() {
        assert!(FaultBehavior::Continue.should_proceed());
        assert!(FaultBehavior::Delay { duration_ms: 100 }.should_proceed());
        assert!(!FaultBehavior::Skip.should_proceed());
        assert!(!FaultBehavior::Abort {
            error: "test".into()
        }
        .should_proceed());

        assert!(!FaultBehavior::Continue.was_affected());
        assert!(FaultBehavior::Skip.was_affected());
    }

    #[test]
    fn test_fault_statistics() {
        let mut stats = FaultStatistics::default();

        stats.record_check();
        stats.record_check();
        stats.record_activation();
        stats.record_skip();

        assert_eq!(stats.checks_total, 2);
        assert_eq!(stats.activations_total, 1);
        assert_eq!(stats.skips_total, 1);
        assert_eq!(stats.activation_rate(), 0.5);
    }

    #[test]
    fn test_base_fault() {
        let metadata = FaultMetadata::new("test", "Test", FaultCategory::Device);
        let mut base = BaseFault::new(metadata);

        assert!(base.metadata.enabled);
        assert_eq!(base.state, FaultState::Active);

        base.disable();
        assert!(!base.metadata.enabled);
        assert_eq!(base.state, FaultState::Inactive);

        base.enable();
        assert!(base.metadata.enabled);
        assert_eq!(base.state, FaultState::Active);
    }
}

//! Device offline fault injection.

use std::any::Any;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use rand::prelude::*;
use serde::{Deserialize, Serialize};

use crate::context::FaultContext;
use crate::error::ChaosResult;
use crate::fault::{
    BaseFault, Fault, FaultBehavior, FaultCategory, FaultMetadata, FaultSeverity, FaultStatistics,
};

// =============================================================================
// Offline Pattern
// =============================================================================

/// Pattern for device offline behavior.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum OfflinePattern {
    /// Sudden offline (no warning).
    #[default]
    Sudden,

    /// Graceful offline (last message sent).
    Graceful,

    /// Flapping (intermittent online/offline).
    Flapping {
        /// Time online (ms).
        online_ms: u64,
        /// Time offline (ms).
        offline_ms: u64,
    },

    /// Degraded (partially responsive before offline).
    Degraded {
        /// Duration of degraded state (ms).
        degraded_duration_ms: u64,
        /// Response rate during degraded (0.0 - 1.0).
        response_rate: f64,
    },

    /// Scheduled (predictable offline windows).
    Scheduled {
        /// Interval between offline events (ms).
        interval_ms: u64,
        /// Duration of each offline event (ms).
        offline_duration_ms: u64,
    },
}

// =============================================================================
// Offline Configuration
// =============================================================================

/// Configuration for device offline fault.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OfflineConfig {
    /// Offline pattern.
    pub pattern: OfflinePattern,

    /// Probability of going offline.
    pub probability: f64,

    /// Duration to stay offline (ms). None = permanent until reset.
    pub offline_duration_ms: Option<u64>,

    /// Error message to return.
    #[serde(default = "default_error_message")]
    pub error_message: String,

    /// Error code to return.
    #[serde(default = "default_error_code")]
    pub error_code: i32,

    /// Whether to affect all points or just some.
    #[serde(default = "default_true")]
    pub affect_all_points: bool,
}

fn default_error_message() -> String {
    "Device offline".to_string()
}

fn default_error_code() -> i32 {
    -1
}

fn default_true() -> bool {
    true
}

impl Default for OfflineConfig {
    fn default() -> Self {
        Self {
            pattern: OfflinePattern::Sudden,
            probability: 0.1,
            offline_duration_ms: Some(5000),
            error_message: default_error_message(),
            error_code: -1,
            affect_all_points: true,
        }
    }
}

impl OfflineConfig {
    /// Create a sudden offline config.
    pub fn sudden(probability: f64, duration_ms: Option<u64>) -> Self {
        Self {
            pattern: OfflinePattern::Sudden,
            probability: probability.clamp(0.0, 1.0),
            offline_duration_ms: duration_ms,
            ..Default::default()
        }
    }

    /// Create a flapping config.
    pub fn flapping(online_ms: u64, offline_ms: u64) -> Self {
        Self {
            pattern: OfflinePattern::Flapping {
                online_ms,
                offline_ms,
            },
            probability: 1.0,
            offline_duration_ms: None,
            ..Default::default()
        }
    }

    /// Create a scheduled offline config.
    pub fn scheduled(interval_ms: u64, offline_duration_ms: u64) -> Self {
        Self {
            pattern: OfflinePattern::Scheduled {
                interval_ms,
                offline_duration_ms,
            },
            probability: 1.0,
            offline_duration_ms: Some(offline_duration_ms),
            ..Default::default()
        }
    }

    /// Set error message.
    pub fn with_error(mut self, code: i32, message: impl Into<String>) -> Self {
        self.error_code = code;
        self.error_message = message.into();
        self
    }
}

// =============================================================================
// Device Offline Fault
// =============================================================================

/// Device offline fault implementation.
///
/// Simulates a device going offline with various patterns.
///
/// # Example
///
/// ```rust,ignore
/// use mabi_chaos::device::DeviceOfflineFault;
///
/// // Random offline with 5% probability
/// let fault = DeviceOfflineFault::builder()
///     .id("offline-001")
///     .probability(0.05)
///     .duration_ms(10000)
///     .build();
///
/// // Flapping device
/// let fault = DeviceOfflineFault::builder()
///     .id("offline-002")
///     .flapping(5000, 2000)
///     .build();
/// ```
#[derive(Debug, Clone)]
pub struct DeviceOfflineFault {
    base: BaseFault,
    config: OfflineConfig,
    rng: StdRng,

    // State
    is_offline: bool,
    offline_until: Option<Instant>,
    last_state_change: Instant,
    in_degraded_mode: bool,
}

impl DeviceOfflineFault {
    /// Create a new device offline fault.
    pub fn new(id: impl Into<String>, config: OfflineConfig) -> Self {
        let id = id.into();
        let metadata = FaultMetadata::new(&id, "Device Offline", FaultCategory::Device)
            .with_description("Simulates device going offline")
            .with_severity(FaultSeverity::High);

        Self {
            base: BaseFault::new(metadata),
            config,
            rng: StdRng::from_entropy(),
            is_offline: false,
            offline_until: None,
            last_state_change: Instant::now(),
            in_degraded_mode: false,
        }
    }

    /// Create a builder.
    pub fn builder() -> OfflineFaultBuilder {
        OfflineFaultBuilder::default()
    }

    /// Update offline state based on pattern.
    fn update_state(&mut self) {
        let now = Instant::now();

        // Check if offline duration has passed
        if let Some(until) = self.offline_until {
            if now >= until {
                self.is_offline = false;
                self.offline_until = None;
                self.last_state_change = now;
            }
        }

        // Handle pattern-specific logic
        match &self.config.pattern {
            OfflinePattern::Flapping {
                online_ms,
                offline_ms,
            } => {
                let elapsed = self.last_state_change.elapsed().as_millis() as u64;
                if self.is_offline {
                    if elapsed >= *offline_ms {
                        self.is_offline = false;
                        self.last_state_change = now;
                    }
                } else if elapsed >= *online_ms {
                    self.is_offline = true;
                    self.last_state_change = now;
                }
            }

            OfflinePattern::Scheduled {
                interval_ms,
                offline_duration_ms,
            } => {
                if !self.is_offline {
                    let elapsed = self.last_state_change.elapsed().as_millis() as u64;
                    if elapsed >= *interval_ms {
                        self.is_offline = true;
                        self.offline_until = Some(now + Duration::from_millis(*offline_duration_ms));
                        self.last_state_change = now;
                    }
                }
            }

            _ => {}
        }
    }

    /// Check if device should be offline.
    pub fn check_offline(&mut self) -> bool {
        self.update_state();

        if self.is_offline {
            return true;
        }

        // Check probability for random offline
        match &self.config.pattern {
            OfflinePattern::Sudden | OfflinePattern::Graceful => {
                if self.rng.gen::<f64>() < self.config.probability {
                    self.is_offline = true;
                    self.last_state_change = Instant::now();
                    if let Some(duration) = self.config.offline_duration_ms {
                        self.offline_until = Some(Instant::now() + Duration::from_millis(duration));
                    }
                    return true;
                }
            }

            OfflinePattern::Degraded {
                degraded_duration_ms,
                response_rate,
            } => {
                if !self.in_degraded_mode && self.rng.gen::<f64>() < self.config.probability {
                    self.in_degraded_mode = true;
                    self.last_state_change = Instant::now();
                }

                if self.in_degraded_mode {
                    let elapsed = self.last_state_change.elapsed().as_millis() as u64;
                    if elapsed >= *degraded_duration_ms {
                        // Transition to fully offline
                        self.is_offline = true;
                        self.in_degraded_mode = false;
                        if let Some(duration) = self.config.offline_duration_ms {
                            self.offline_until =
                                Some(Instant::now() + Duration::from_millis(duration));
                        }
                        return true;
                    }

                    // During degraded, partially respond
                    if self.rng.gen::<f64>() > *response_rate {
                        return true; // Simulate offline for this request
                    }
                }
            }

            _ => {}
        }

        false
    }

    /// Get configuration.
    pub fn config(&self) -> &OfflineConfig {
        &self.config
    }

    /// Check if currently offline.
    pub fn is_offline(&self) -> bool {
        self.is_offline
    }

    /// Force online.
    pub fn force_online(&mut self) {
        self.is_offline = false;
        self.offline_until = None;
        self.in_degraded_mode = false;
        self.last_state_change = Instant::now();
    }

    /// Force offline.
    pub fn force_offline(&mut self, duration_ms: Option<u64>) {
        self.is_offline = true;
        self.last_state_change = Instant::now();
        self.offline_until = duration_ms.map(|d| Instant::now() + Duration::from_millis(d));
    }
}

#[async_trait]
impl Fault for DeviceOfflineFault {
    fn metadata(&self) -> &FaultMetadata {
        &self.base.metadata
    }

    fn enable(&mut self) {
        self.base.enable();
    }

    fn disable(&mut self) {
        self.base.disable();
        self.force_online();
    }

    async fn should_activate(&self, ctx: &FaultContext) -> ChaosResult<bool> {
        if !self.base.matches_target(ctx.target.identifier()) {
            return Ok(false);
        }
        Ok(true)
    }

    async fn apply(&self, ctx: &mut FaultContext) -> ChaosResult<FaultBehavior> {
        let is_offline = {
            let mut this = self.clone();
            if !this.base.check_probability() {
                return Ok(FaultBehavior::Continue);
            }
            this.check_offline()
        };

        if is_offline {
            ctx.record_applied_fault(self.id(), "offline");

            tracing::debug!(
                fault_id = %self.id(),
                target = %ctx.target.device_id,
                "Device is offline"
            );

            return Ok(FaultBehavior::ReturnError {
                error_code: self.config.error_code,
                message: self.config.error_message.clone(),
            });
        }

        Ok(FaultBehavior::Continue)
    }

    fn reset(&mut self) {
        self.base.reset();
        self.force_online();
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

/// Builder for device offline fault.
#[derive(Debug, Default)]
pub struct OfflineFaultBuilder {
    id: Option<String>,
    name: Option<String>,
    config: OfflineConfig,
    targets: Vec<String>,
    severity: FaultSeverity,
}

impl OfflineFaultBuilder {
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

    /// Set probability.
    pub fn probability(mut self, p: f64) -> Self {
        self.config.probability = p.clamp(0.0, 1.0);
        self
    }

    /// Set offline duration.
    pub fn duration_ms(mut self, ms: u64) -> Self {
        self.config.offline_duration_ms = Some(ms);
        self
    }

    /// Set permanent offline.
    pub fn permanent(mut self) -> Self {
        self.config.offline_duration_ms = None;
        self
    }

    /// Set flapping pattern.
    pub fn flapping(mut self, online_ms: u64, offline_ms: u64) -> Self {
        self.config.pattern = OfflinePattern::Flapping {
            online_ms,
            offline_ms,
        };
        self
    }

    /// Set scheduled pattern.
    pub fn scheduled(mut self, interval_ms: u64, offline_duration_ms: u64) -> Self {
        self.config.pattern = OfflinePattern::Scheduled {
            interval_ms,
            offline_duration_ms,
        };
        self
    }

    /// Set degraded pattern.
    pub fn degraded(mut self, duration_ms: u64, response_rate: f64) -> Self {
        self.config.pattern = OfflinePattern::Degraded {
            degraded_duration_ms: duration_ms,
            response_rate: response_rate.clamp(0.0, 1.0),
        };
        self
    }

    /// Set error response.
    pub fn error(mut self, code: i32, message: impl Into<String>) -> Self {
        self.config.error_code = code;
        self.config.error_message = message.into();
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
    pub fn build(self) -> DeviceOfflineFault {
        let id = self.id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        let mut fault = DeviceOfflineFault::new(&id, self.config);

        if let Some(name) = self.name {
            fault.base.metadata.name = name;
        }

        fault.base.metadata.targets = self.targets;
        fault.base.metadata.severity = self.severity;

        fault
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sudden_offline() {
        let config = OfflineConfig::sudden(1.0, Some(100));
        let mut fault = DeviceOfflineFault::new("test", config);

        assert!(fault.check_offline());
        assert!(fault.is_offline());

        // Wait for recovery
        std::thread::sleep(Duration::from_millis(150));
        fault.update_state();
        assert!(!fault.is_offline());
    }

    #[test]
    fn test_flapping() {
        let config = OfflineConfig::flapping(50, 50);
        let mut fault = DeviceOfflineFault::new("test", config);

        // Should start online
        assert!(!fault.check_offline());

        // Wait for transition to offline
        std::thread::sleep(Duration::from_millis(60));
        assert!(fault.check_offline());

        // Wait for transition back to online
        std::thread::sleep(Duration::from_millis(60));
        assert!(!fault.check_offline());
    }

    #[test]
    fn test_force_offline() {
        let config = OfflineConfig::sudden(0.0, None); // 0% probability
        let mut fault = DeviceOfflineFault::new("test", config);

        assert!(!fault.check_offline());

        fault.force_offline(Some(100));
        assert!(fault.is_offline());

        fault.force_online();
        assert!(!fault.is_offline());
    }

    #[test]
    fn test_builder() {
        let fault = DeviceOfflineFault::builder()
            .id("offline-001")
            .probability(0.1)
            .duration_ms(5000)
            .error(-1001, "Connection lost")
            .target("device-*")
            .build();

        assert_eq!(fault.id(), "offline-001");
        assert_eq!(fault.config.probability, 0.1);
        assert_eq!(fault.config.error_code, -1001);
    }
}

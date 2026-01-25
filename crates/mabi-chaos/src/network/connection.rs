//! Connection disruption fault injection.

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
// Disconnect Mode
// =============================================================================

/// Mode of connection disconnection.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum DisconnectMode {
    /// Immediate connection close.
    #[default]
    Immediate,

    /// Connection timeout (no response).
    Timeout {
        /// Timeout duration in milliseconds.
        duration_ms: u64,
    },

    /// TCP RST (reset) simulation.
    Reset,

    /// Send partial data then disconnect.
    PartialData {
        /// Bytes to send before disconnect.
        bytes_before_disconnect: usize,
    },

    /// Graceful close (FIN).
    Graceful,

    /// Connection refused.
    Refused,

    /// Connection hung (keep-alive fails).
    Hung,
}

// =============================================================================
// Connection Configuration
// =============================================================================

/// Configuration for connection disruption.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionConfig {
    /// Disconnect mode.
    pub mode: DisconnectMode,

    /// Probability of disconnection.
    pub probability: f64,

    /// Cooldown between disruptions.
    pub cooldown_ms: u64,

    /// Maximum disconnections (0 = unlimited).
    #[serde(default)]
    pub max_disconnections: usize,

    /// Duration to stay disconnected (for recovery testing).
    #[serde(default)]
    pub disconnect_duration_ms: Option<u64>,

    /// Whether to affect subsequent requests.
    #[serde(default)]
    pub persistent: bool,
}

impl Default for ConnectionConfig {
    fn default() -> Self {
        Self {
            mode: DisconnectMode::Immediate,
            probability: 0.1,
            cooldown_ms: 5000,
            max_disconnections: 0,
            disconnect_duration_ms: None,
            persistent: false,
        }
    }
}

impl ConnectionConfig {
    /// Create a config for immediate disconnect.
    pub fn immediate(probability: f64) -> Self {
        Self {
            mode: DisconnectMode::Immediate,
            probability: probability.clamp(0.0, 1.0),
            ..Default::default()
        }
    }

    /// Create a config for timeout.
    pub fn timeout(probability: f64, timeout_ms: u64) -> Self {
        Self {
            mode: DisconnectMode::Timeout {
                duration_ms: timeout_ms,
            },
            probability: probability.clamp(0.0, 1.0),
            ..Default::default()
        }
    }

    /// Create a config for reset.
    pub fn reset(probability: f64) -> Self {
        Self {
            mode: DisconnectMode::Reset,
            probability: probability.clamp(0.0, 1.0),
            ..Default::default()
        }
    }

    /// Set cooldown.
    pub fn with_cooldown(mut self, cooldown_ms: u64) -> Self {
        self.cooldown_ms = cooldown_ms;
        self
    }

    /// Set max disconnections.
    pub fn with_max(mut self, max: usize) -> Self {
        self.max_disconnections = max;
        self
    }

    /// Set disconnect duration.
    pub fn with_duration(mut self, duration_ms: u64) -> Self {
        self.disconnect_duration_ms = Some(duration_ms);
        self
    }

    /// Make persistent.
    pub fn persistent(mut self) -> Self {
        self.persistent = true;
        self
    }
}

// =============================================================================
// Connection Fault
// =============================================================================

/// Connection disruption fault implementation.
///
/// Simulates various connection failures:
/// - Immediate disconnect
/// - Timeout (no response)
/// - TCP RST
/// - Partial data
/// - Connection refused
///
/// # Example
///
/// ```rust,ignore
/// use mabi_chaos::network::ConnectionFault;
///
/// let fault = ConnectionFault::builder()
///     .id("conn-001")
///     .mode(DisconnectMode::Timeout { duration_ms: 5000 })
///     .probability(0.05)
///     .cooldown_ms(10000)
///     .build();
/// ```
#[derive(Debug, Clone)]
pub struct ConnectionFault {
    base: BaseFault,
    config: ConnectionConfig,
    rng: StdRng,

    // State
    last_disruption: Option<Instant>,
    disruption_count: usize,
    currently_disconnected: bool,
    disconnect_until: Option<Instant>,
}

impl ConnectionFault {
    /// Create a new connection fault.
    pub fn new(id: impl Into<String>, config: ConnectionConfig) -> Self {
        let id = id.into();
        let metadata = FaultMetadata::new(&id, "Connection Disruption", FaultCategory::Network)
            .with_description("Simulates connection failures and disruptions")
            .with_severity(FaultSeverity::High);

        Self {
            base: BaseFault::new(metadata),
            config,
            rng: StdRng::from_entropy(),
            last_disruption: None,
            disruption_count: 0,
            currently_disconnected: false,
            disconnect_until: None,
        }
    }

    /// Create a builder.
    pub fn builder() -> ConnectionFaultBuilder {
        ConnectionFaultBuilder::default()
    }

    /// Check if should disrupt.
    pub fn should_disrupt(&mut self) -> Option<DisconnectMode> {
        // Check if still disconnected
        if self.currently_disconnected {
            if let Some(until) = self.disconnect_until {
                if Instant::now() < until {
                    return Some(self.config.mode.clone());
                }
                self.currently_disconnected = false;
                self.disconnect_until = None;
            } else if self.config.persistent {
                return Some(self.config.mode.clone());
            }
        }

        // Check cooldown
        if let Some(last) = self.last_disruption {
            if last.elapsed() < Duration::from_millis(self.config.cooldown_ms) {
                return None;
            }
        }

        // Check max disruptions
        if self.config.max_disconnections > 0 && self.disruption_count >= self.config.max_disconnections
        {
            return None;
        }

        // Check probability
        if self.rng.gen::<f64>() < self.config.probability {
            self.last_disruption = Some(Instant::now());
            self.disruption_count += 1;

            // Handle disconnect duration
            if let Some(duration_ms) = self.config.disconnect_duration_ms {
                self.currently_disconnected = true;
                self.disconnect_until = Some(Instant::now() + Duration::from_millis(duration_ms));
            } else if self.config.persistent {
                self.currently_disconnected = true;
            }

            Some(self.config.mode.clone())
        } else {
            None
        }
    }

    /// Get configuration.
    pub fn config(&self) -> &ConnectionConfig {
        &self.config
    }

    /// Update configuration.
    pub fn set_config(&mut self, config: ConnectionConfig) {
        self.config = config;
    }

    /// Get disruption count.
    pub fn disruption_count(&self) -> usize {
        self.disruption_count
    }

    /// Check if currently in disconnected state.
    pub fn is_disconnected(&self) -> bool {
        self.currently_disconnected
    }

    /// Force reconnection.
    pub fn reconnect(&mut self) {
        self.currently_disconnected = false;
        self.disconnect_until = None;
    }
}

#[async_trait]
impl Fault for ConnectionFault {
    fn metadata(&self) -> &FaultMetadata {
        &self.base.metadata
    }

    fn enable(&mut self) {
        self.base.enable();
    }

    fn disable(&mut self) {
        self.base.disable();
        self.reconnect();
    }

    async fn should_activate(&self, ctx: &FaultContext) -> ChaosResult<bool> {
        if !self.base.matches_target(ctx.target.identifier()) {
            return Ok(false);
        }
        Ok(true)
    }

    async fn apply(&self, ctx: &mut FaultContext) -> ChaosResult<FaultBehavior> {
        let mode = {
            let mut this = self.clone();
            if !this.base.check_probability() {
                return Ok(FaultBehavior::Continue);
            }
            this.should_disrupt()
        };

        if let Some(mode) = mode {
            ctx.record_applied_fault(self.id(), &format!("{:?}", mode));

            tracing::debug!(
                fault_id = %self.id(),
                target = %ctx.target.device_id,
                mode = ?mode,
                "Disrupting connection"
            );

            match mode {
                DisconnectMode::Immediate | DisconnectMode::Reset | DisconnectMode::Refused => {
                    return Ok(FaultBehavior::Abort {
                        error: format!("Connection disruption: {:?}", mode),
                    });
                }

                DisconnectMode::Timeout { duration_ms } => {
                    // Simulate timeout by sleeping then aborting
                    tokio::time::sleep(Duration::from_millis(duration_ms)).await;
                    ctx.add_delay(duration_ms);
                    return Ok(FaultBehavior::Abort {
                        error: "Connection timeout".to_string(),
                    });
                }

                DisconnectMode::PartialData { .. } => {
                    // Mark that response should be truncated
                    ctx.set_metadata("partial_response", "true");
                    return Ok(FaultBehavior::Modify);
                }

                DisconnectMode::Graceful => {
                    return Ok(FaultBehavior::Abort {
                        error: "Connection closed gracefully".to_string(),
                    });
                }

                DisconnectMode::Hung => {
                    // Just hang indefinitely (or until timeout)
                    tokio::time::sleep(Duration::from_secs(30)).await;
                    return Ok(FaultBehavior::Abort {
                        error: "Connection hung".to_string(),
                    });
                }
            }
        }

        Ok(FaultBehavior::Continue)
    }

    fn reset(&mut self) {
        self.base.reset();
        self.last_disruption = None;
        self.disruption_count = 0;
        self.currently_disconnected = false;
        self.disconnect_until = None;
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

/// Builder for connection fault.
#[derive(Debug, Default)]
pub struct ConnectionFaultBuilder {
    id: Option<String>,
    name: Option<String>,
    config: ConnectionConfig,
    targets: Vec<String>,
    severity: FaultSeverity,
}

impl ConnectionFaultBuilder {
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

    /// Set disconnect mode.
    pub fn mode(mut self, mode: DisconnectMode) -> Self {
        self.config.mode = mode;
        self
    }

    /// Set probability.
    pub fn probability(mut self, p: f64) -> Self {
        self.config.probability = p.clamp(0.0, 1.0);
        self
    }

    /// Set cooldown.
    pub fn cooldown_ms(mut self, ms: u64) -> Self {
        self.config.cooldown_ms = ms;
        self
    }

    /// Set max disconnections.
    pub fn max_disconnections(mut self, max: usize) -> Self {
        self.config.max_disconnections = max;
        self
    }

    /// Set disconnect duration.
    pub fn disconnect_duration_ms(mut self, ms: u64) -> Self {
        self.config.disconnect_duration_ms = Some(ms);
        self
    }

    /// Make persistent.
    pub fn persistent(mut self) -> Self {
        self.config.persistent = true;
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
    pub fn build(self) -> ConnectionFault {
        let id = self.id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        let mut fault = ConnectionFault::new(&id, self.config);

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
    fn test_immediate_disconnect() {
        let config = ConnectionConfig::immediate(1.0);
        let mut fault = ConnectionFault::new("test", config);

        let mode = fault.should_disrupt();
        assert!(matches!(mode, Some(DisconnectMode::Immediate)));
    }

    #[test]
    fn test_cooldown() {
        let config = ConnectionConfig::immediate(1.0).with_cooldown(100);
        let mut fault = ConnectionFault::new("test", config);

        // First disruption should succeed
        assert!(fault.should_disrupt().is_some());

        // Immediately after should fail (cooldown)
        assert!(fault.should_disrupt().is_none());

        // After cooldown should succeed
        std::thread::sleep(Duration::from_millis(150));
        assert!(fault.should_disrupt().is_some());
    }

    #[test]
    fn test_max_disruptions() {
        let config = ConnectionConfig::immediate(1.0).with_cooldown(0).with_max(3);
        let mut fault = ConnectionFault::new("test", config);

        for _ in 0..3 {
            assert!(fault.should_disrupt().is_some());
        }

        // Should be exhausted
        assert!(fault.should_disrupt().is_none());
        assert_eq!(fault.disruption_count(), 3);
    }

    #[test]
    fn test_builder() {
        let fault = ConnectionFault::builder()
            .id("conn-001")
            .mode(DisconnectMode::Reset)
            .probability(0.2)
            .cooldown_ms(5000)
            .target("device-*")
            .build();

        assert_eq!(fault.id(), "conn-001");
        assert!(matches!(fault.config.mode, DisconnectMode::Reset));
    }
}

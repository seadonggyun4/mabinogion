//! Bandwidth throttling fault injection.

use std::any::Any;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::context::FaultContext;
use crate::error::ChaosResult;
use crate::fault::{
    BaseFault, Fault, FaultBehavior, FaultCategory, FaultMetadata, FaultSeverity, FaultStatistics,
};

// =============================================================================
// Bandwidth Configuration
// =============================================================================

/// Configuration for bandwidth throttling.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BandwidthConfig {
    /// Bandwidth limit in bytes per second.
    pub bytes_per_second: u64,

    /// Burst size (bytes allowed before throttling kicks in).
    #[serde(default)]
    pub burst_bytes: u64,

    /// Whether to apply to request (upload).
    #[serde(default = "default_true")]
    pub apply_to_request: bool,

    /// Whether to apply to response (download).
    #[serde(default = "default_true")]
    pub apply_to_response: bool,

    /// Simulate packet fragmentation.
    #[serde(default)]
    pub fragment_packets: bool,

    /// Maximum fragment size.
    #[serde(default = "default_mtu")]
    pub max_fragment_size: usize,
}

fn default_true() -> bool {
    true
}

fn default_mtu() -> usize {
    1500
}

impl Default for BandwidthConfig {
    fn default() -> Self {
        Self {
            bytes_per_second: 1_000_000, // 1 MB/s
            burst_bytes: 10_000,
            apply_to_request: true,
            apply_to_response: true,
            fragment_packets: false,
            max_fragment_size: 1500,
        }
    }
}

impl BandwidthConfig {
    /// Create a bandwidth config in KB/s.
    pub fn kbps(kbps: u64) -> Self {
        Self {
            bytes_per_second: kbps * 1024,
            ..Default::default()
        }
    }

    /// Create a bandwidth config in MB/s.
    pub fn mbps(mbps: u64) -> Self {
        Self {
            bytes_per_second: mbps * 1024 * 1024,
            ..Default::default()
        }
    }

    /// Set burst size.
    pub fn with_burst(mut self, bytes: u64) -> Self {
        self.burst_bytes = bytes;
        self
    }

    /// Enable packet fragmentation.
    pub fn with_fragmentation(mut self, max_size: usize) -> Self {
        self.fragment_packets = true;
        self.max_fragment_size = max_size;
        self
    }

    /// Only apply to uploads.
    pub fn upload_only(mut self) -> Self {
        self.apply_to_request = true;
        self.apply_to_response = false;
        self
    }

    /// Only apply to downloads.
    pub fn download_only(mut self) -> Self {
        self.apply_to_request = false;
        self.apply_to_response = true;
        self
    }
}

// =============================================================================
// Bandwidth Fault
// =============================================================================

/// Bandwidth throttling fault implementation.
///
/// Simulates limited network bandwidth by introducing delays
/// proportional to the data size.
///
/// Uses a token bucket algorithm for rate limiting.
///
/// # Example
///
/// ```rust,ignore
/// use mabi_chaos::network::BandwidthFault;
///
/// // Limit to 100 KB/s
/// let fault = BandwidthFault::builder()
///     .id("bw-001")
///     .kbps(100)
///     .build();
///
/// // Limit to 1 MB/s with 50KB burst
/// let fault = BandwidthFault::builder()
///     .id("bw-002")
///     .mbps(1)
///     .burst(50 * 1024)
///     .build();
/// ```
#[derive(Debug, Clone)]
pub struct BandwidthFault {
    base: BaseFault,
    config: BandwidthConfig,
    // Token bucket state
    tokens: f64,
    last_update: Instant,
}

impl BandwidthFault {
    /// Create a new bandwidth fault.
    pub fn new(id: impl Into<String>, config: BandwidthConfig) -> Self {
        let id = id.into();
        let metadata = FaultMetadata::new(&id, "Bandwidth Throttling", FaultCategory::Network)
            .with_description("Simulates limited network bandwidth")
            .with_severity(FaultSeverity::Medium);

        Self {
            base: BaseFault::new(metadata),
            config,
            tokens: 0.0,
            last_update: Instant::now(),
        }
    }

    /// Create a builder.
    pub fn builder() -> BandwidthFaultBuilder {
        BandwidthFaultBuilder::default()
    }

    /// Refill tokens based on elapsed time.
    fn refill_tokens(&mut self) {
        let elapsed = self.last_update.elapsed();
        self.last_update = Instant::now();

        // Add tokens based on bandwidth
        let new_tokens = elapsed.as_secs_f64() * self.config.bytes_per_second as f64;
        self.tokens = (self.tokens + new_tokens).min(self.config.burst_bytes as f64);
    }

    /// Calculate delay for given data size.
    pub fn calculate_delay(&mut self, data_size: usize) -> Duration {
        self.refill_tokens();

        let data_size = data_size as f64;

        if self.tokens >= data_size {
            // Have enough tokens, no delay
            self.tokens -= data_size;
            Duration::ZERO
        } else {
            // Need to wait for more tokens
            let needed = data_size - self.tokens;
            let wait_secs = needed / self.config.bytes_per_second as f64;
            self.tokens = 0.0;
            Duration::from_secs_f64(wait_secs)
        }
    }

    /// Get configuration.
    pub fn config(&self) -> &BandwidthConfig {
        &self.config
    }

    /// Update configuration.
    pub fn set_config(&mut self, config: BandwidthConfig) {
        self.config = config;
    }
}

#[async_trait]
impl Fault for BandwidthFault {
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
        // Estimate data size (in real implementation, would use actual data)
        let data_size = ctx
            .request_data
            .as_ref()
            .and_then(|d| d.raw.as_ref())
            .map(|r| r.len())
            .unwrap_or(100); // Default estimate

        let delay = {
            let mut this = self.clone();
            if !this.base.check_probability() {
                return Ok(FaultBehavior::Continue);
            }
            this.calculate_delay(data_size)
        };

        if delay > Duration::ZERO {
            let delay_ms = delay.as_millis() as u64;
            ctx.add_delay(delay_ms);
            ctx.record_applied_fault(self.id(), "throttle");

            tracing::debug!(
                fault_id = %self.id(),
                target = %ctx.target.device_id,
                data_size = %data_size,
                delay_ms = %delay_ms,
                "Throttling bandwidth"
            );

            tokio::time::sleep(delay).await;

            return Ok(FaultBehavior::Delay {
                duration_ms: delay_ms,
            });
        }

        Ok(FaultBehavior::Continue)
    }

    fn reset(&mut self) {
        self.base.reset();
        self.tokens = self.config.burst_bytes as f64;
        self.last_update = Instant::now();
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

/// Builder for bandwidth fault.
#[derive(Debug, Default)]
pub struct BandwidthFaultBuilder {
    id: Option<String>,
    name: Option<String>,
    config: BandwidthConfig,
    probability: f64,
    targets: Vec<String>,
    severity: FaultSeverity,
}

impl BandwidthFaultBuilder {
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

    /// Set bytes per second.
    pub fn bytes_per_second(mut self, bps: u64) -> Self {
        self.config.bytes_per_second = bps;
        self
    }

    /// Set KB/s.
    pub fn kbps(mut self, kbps: u64) -> Self {
        self.config.bytes_per_second = kbps * 1024;
        self
    }

    /// Set MB/s.
    pub fn mbps(mut self, mbps: u64) -> Self {
        self.config.bytes_per_second = mbps * 1024 * 1024;
        self
    }

    /// Set burst size.
    pub fn burst(mut self, bytes: u64) -> Self {
        self.config.burst_bytes = bytes;
        self
    }

    /// Enable fragmentation.
    pub fn fragment(mut self, max_size: usize) -> Self {
        self.config.fragment_packets = true;
        self.config.max_fragment_size = max_size;
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
    pub fn build(self) -> BandwidthFault {
        let id = self.id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        let mut fault = BandwidthFault::new(&id, self.config);

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
    fn test_token_bucket() {
        let config = BandwidthConfig {
            bytes_per_second: 1000,
            burst_bytes: 0,
            ..Default::default()
        };
        let mut fault = BandwidthFault::new("test", config);

        // No burst, so 1000 bytes should take 1 second
        let delay = fault.calculate_delay(1000);
        assert!(delay > Duration::from_millis(900));
        assert!(delay < Duration::from_millis(1100));
    }

    #[test]
    fn test_burst() {
        let config = BandwidthConfig {
            bytes_per_second: 1000,
            burst_bytes: 5000,
            ..Default::default()
        };
        let mut fault = BandwidthFault::new("test", config);

        // Reset to get full burst
        fault.reset();

        // First 5000 bytes should be instant (burst)
        let delay = fault.calculate_delay(5000);
        assert_eq!(delay, Duration::ZERO);

        // Next bytes should incur delay
        let delay = fault.calculate_delay(1000);
        assert!(delay > Duration::ZERO);
    }

    #[test]
    fn test_builder() {
        let fault = BandwidthFault::builder()
            .id("bw-001")
            .kbps(100)
            .burst(10000)
            .target("device-*")
            .build();

        assert_eq!(fault.id(), "bw-001");
        assert_eq!(fault.config.bytes_per_second, 100 * 1024);
    }
}

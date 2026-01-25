//! Configuration types for scalability module.
//!
//! This module provides comprehensive configuration for all scalability components,
//! with sensible defaults and preset profiles for common use cases.

use std::time::Duration;

use serde::{Deserialize, Serialize};

/// Preset profiles for different scale requirements.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScalabilityPreset {
    /// Development/testing (100 connections, low resources)
    Development,
    /// Small deployment (1,000 connections)
    Small,
    /// Medium deployment (10,000 connections) - Default target
    Medium,
    /// Large deployment (50,000 connections)
    Large,
    /// Enterprise deployment (100,000+ connections)
    Enterprise,
    /// Custom configuration
    Custom,
}

impl Default for ScalabilityPreset {
    fn default() -> Self {
        Self::Medium
    }
}

/// Master configuration for all scalability components.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScalabilityConfig {
    /// Preset profile used.
    pub preset: ScalabilityPreset,

    /// Connection pool configuration.
    pub connection_pool: ConnectionPoolConfig,

    /// Batch processor configuration.
    pub batch: BatchProcessorConfig,

    /// Resource limiter configuration.
    pub resources: ResourceLimiterConfig,

    /// Performance profiler configuration.
    pub profiler: ProfilerConfig,
}

impl Default for ScalabilityConfig {
    fn default() -> Self {
        Self::for_preset(ScalabilityPreset::Medium)
    }
}

impl ScalabilityConfig {
    /// Create configuration for a specific preset.
    pub fn for_preset(preset: ScalabilityPreset) -> Self {
        match preset {
            ScalabilityPreset::Development => Self::development(),
            ScalabilityPreset::Small => Self::small(),
            ScalabilityPreset::Medium => Self::medium(),
            ScalabilityPreset::Large => Self::large(),
            ScalabilityPreset::Enterprise => Self::enterprise(),
            ScalabilityPreset::Custom => Self::medium(), // Use medium as base for custom
        }
    }

    /// Create configuration optimized for a specific number of connections.
    pub fn for_connections(max_connections: usize) -> Self {
        if max_connections <= 100 {
            Self::development()
        } else if max_connections <= 1_000 {
            Self::small()
        } else if max_connections <= 10_000 {
            Self::medium()
        } else if max_connections <= 50_000 {
            Self::large()
        } else {
            Self::enterprise()
        }
    }

    /// Development preset (100 connections).
    pub fn development() -> Self {
        Self {
            preset: ScalabilityPreset::Development,
            connection_pool: ConnectionPoolConfig {
                max_connections: 100,
                shard_count: 4,
                connections_per_shard: 32,
                idle_timeout: Duration::from_secs(60),
                health_check_interval: Duration::from_secs(30),
                enable_metrics: true,
            },
            batch: BatchProcessorConfig {
                enabled: false,
                batch_size: 10,
                batch_timeout: Duration::from_millis(5),
                max_pending: 100,
                coalescing: CoalescingConfig::disabled(),
            },
            resources: ResourceLimiterConfig {
                max_memory_bytes: 256 * 1024 * 1024, // 256MB
                max_connections: 100,
                max_requests_per_second: 1_000,
                backpressure_threshold: 0.8,
                strategy: BackpressureStrategyConfig::Adaptive,
            },
            profiler: ProfilerConfig {
                enabled: true,
                sample_rate: 1.0, // Sample all in dev
                histogram_buckets: default_latency_buckets(),
                report_interval: Duration::from_secs(10),
                track_memory: true,
                track_cpu: false,
            },
        }
    }

    /// Small preset (1,000 connections).
    pub fn small() -> Self {
        Self {
            preset: ScalabilityPreset::Small,
            connection_pool: ConnectionPoolConfig {
                max_connections: 1_000,
                shard_count: 8,
                connections_per_shard: 128,
                idle_timeout: Duration::from_secs(120),
                health_check_interval: Duration::from_secs(30),
                enable_metrics: true,
            },
            batch: BatchProcessorConfig {
                enabled: true,
                batch_size: 50,
                batch_timeout: Duration::from_millis(2),
                max_pending: 1_000,
                coalescing: CoalescingConfig::default(),
            },
            resources: ResourceLimiterConfig {
                max_memory_bytes: 512 * 1024 * 1024, // 512MB
                max_connections: 1_000,
                max_requests_per_second: 10_000,
                backpressure_threshold: 0.8,
                strategy: BackpressureStrategyConfig::Adaptive,
            },
            profiler: ProfilerConfig {
                enabled: true,
                sample_rate: 0.1, // 10% sampling
                histogram_buckets: default_latency_buckets(),
                report_interval: Duration::from_secs(30),
                track_memory: true,
                track_cpu: true,
            },
        }
    }

    /// Medium preset (10,000 connections) - Primary target.
    pub fn medium() -> Self {
        Self {
            preset: ScalabilityPreset::Medium,
            connection_pool: ConnectionPoolConfig {
                max_connections: 10_000,
                shard_count: 64,
                connections_per_shard: 256,
                idle_timeout: Duration::from_secs(300),
                health_check_interval: Duration::from_secs(60),
                enable_metrics: true,
            },
            batch: BatchProcessorConfig {
                enabled: true,
                batch_size: 100,
                batch_timeout: Duration::from_millis(1),
                max_pending: 10_000,
                coalescing: CoalescingConfig {
                    enabled: true,
                    window: Duration::from_micros(500),
                    max_coalesce: 16,
                },
            },
            resources: ResourceLimiterConfig {
                max_memory_bytes: 2 * 1024 * 1024 * 1024, // 2GB
                max_connections: 10_000,
                max_requests_per_second: 100_000,
                backpressure_threshold: 0.75,
                strategy: BackpressureStrategyConfig::Adaptive,
            },
            profiler: ProfilerConfig {
                enabled: true,
                sample_rate: 0.01, // 1% sampling
                histogram_buckets: default_latency_buckets(),
                report_interval: Duration::from_secs(60),
                track_memory: true,
                track_cpu: true,
            },
        }
    }

    /// Large preset (50,000 connections).
    pub fn large() -> Self {
        Self {
            preset: ScalabilityPreset::Large,
            connection_pool: ConnectionPoolConfig {
                max_connections: 50_000,
                shard_count: 128,
                connections_per_shard: 512,
                idle_timeout: Duration::from_secs(300),
                health_check_interval: Duration::from_secs(120),
                enable_metrics: true,
            },
            batch: BatchProcessorConfig {
                enabled: true,
                batch_size: 200,
                batch_timeout: Duration::from_micros(500),
                max_pending: 50_000,
                coalescing: CoalescingConfig {
                    enabled: true,
                    window: Duration::from_micros(250),
                    max_coalesce: 32,
                },
            },
            resources: ResourceLimiterConfig {
                max_memory_bytes: 8 * 1024 * 1024 * 1024, // 8GB
                max_connections: 50_000,
                max_requests_per_second: 500_000,
                backpressure_threshold: 0.7,
                strategy: BackpressureStrategyConfig::Aggressive,
            },
            profiler: ProfilerConfig {
                enabled: true,
                sample_rate: 0.001, // 0.1% sampling
                histogram_buckets: default_latency_buckets(),
                report_interval: Duration::from_secs(120),
                track_memory: true,
                track_cpu: true,
            },
        }
    }

    /// Enterprise preset (100,000+ connections).
    pub fn enterprise() -> Self {
        Self {
            preset: ScalabilityPreset::Enterprise,
            connection_pool: ConnectionPoolConfig {
                max_connections: 100_000,
                shard_count: 256,
                connections_per_shard: 512,
                idle_timeout: Duration::from_secs(600),
                health_check_interval: Duration::from_secs(180),
                enable_metrics: true,
            },
            batch: BatchProcessorConfig {
                enabled: true,
                batch_size: 500,
                batch_timeout: Duration::from_micros(250),
                max_pending: 100_000,
                coalescing: CoalescingConfig {
                    enabled: true,
                    window: Duration::from_micros(100),
                    max_coalesce: 64,
                },
            },
            resources: ResourceLimiterConfig {
                max_memory_bytes: 16 * 1024 * 1024 * 1024, // 16GB
                max_connections: 100_000,
                max_requests_per_second: 1_000_000,
                backpressure_threshold: 0.65,
                strategy: BackpressureStrategyConfig::Aggressive,
            },
            profiler: ProfilerConfig {
                enabled: true,
                sample_rate: 0.0001, // 0.01% sampling
                histogram_buckets: default_latency_buckets(),
                report_interval: Duration::from_secs(300),
                track_memory: true,
                track_cpu: true,
            },
        }
    }

    /// Validate the configuration.
    pub fn validate(&self) -> Result<(), ConfigError> {
        self.connection_pool.validate()?;
        self.batch.validate()?;
        self.resources.validate()?;
        self.profiler.validate()?;
        Ok(())
    }
}

/// Configuration for the sharded connection pool.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionPoolConfig {
    /// Maximum total connections across all shards.
    pub max_connections: usize,

    /// Number of shards (should be power of 2 for efficient hashing).
    pub shard_count: usize,

    /// Maximum connections per shard (soft limit).
    pub connections_per_shard: usize,

    /// Idle connection timeout before cleanup.
    pub idle_timeout: Duration,

    /// Interval for health checks.
    pub health_check_interval: Duration,

    /// Enable detailed metrics collection.
    pub enable_metrics: bool,
}

impl Default for ConnectionPoolConfig {
    fn default() -> Self {
        ScalabilityConfig::medium().connection_pool
    }
}

impl ConnectionPoolConfig {
    /// Validate the configuration.
    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.max_connections == 0 {
            return Err(ConfigError::InvalidValue {
                field: "max_connections".into(),
                message: "must be greater than 0".into(),
            });
        }

        if self.shard_count == 0 {
            return Err(ConfigError::InvalidValue {
                field: "shard_count".into(),
                message: "must be greater than 0".into(),
            });
        }

        // Warn if shard_count is not a power of 2
        if !self.shard_count.is_power_of_two() {
            // This is a warning, not an error - performance may be suboptimal
            tracing::warn!(
                shard_count = self.shard_count,
                "shard_count is not a power of 2, hashing may be suboptimal"
            );
        }

        Ok(())
    }

    /// Calculate optimal shard count for a given connection limit.
    pub fn optimal_shard_count(max_connections: usize) -> usize {
        // Target ~256 connections per shard for good balance
        let target_per_shard = 256;
        let raw_count = (max_connections + target_per_shard - 1) / target_per_shard;

        // Round up to next power of 2, min 4, max 256
        let power = (raw_count as f64).log2().ceil() as u32;
        (1usize << power).clamp(4, 256)
    }
}

/// Configuration for the batch processor.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchProcessorConfig {
    /// Enable batch processing.
    pub enabled: bool,

    /// Maximum batch size before forced flush.
    pub batch_size: usize,

    /// Maximum time to wait for batch to fill.
    pub batch_timeout: Duration,

    /// Maximum pending requests before backpressure.
    pub max_pending: usize,

    /// Request coalescing configuration.
    pub coalescing: CoalescingConfig,
}

impl Default for BatchProcessorConfig {
    fn default() -> Self {
        ScalabilityConfig::medium().batch
    }
}

impl BatchProcessorConfig {
    /// Validate the configuration.
    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.enabled && self.batch_size == 0 {
            return Err(ConfigError::InvalidValue {
                field: "batch_size".into(),
                message: "must be greater than 0 when batching is enabled".into(),
            });
        }

        if self.enabled && self.batch_timeout.is_zero() {
            return Err(ConfigError::InvalidValue {
                field: "batch_timeout".into(),
                message: "must be greater than 0 when batching is enabled".into(),
            });
        }

        Ok(())
    }
}

/// Configuration for request coalescing (combining similar requests).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoalescingConfig {
    /// Enable request coalescing.
    pub enabled: bool,

    /// Time window for coalescing similar requests.
    pub window: Duration,

    /// Maximum requests to coalesce into one.
    pub max_coalesce: usize,
}

impl Default for CoalescingConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            window: Duration::from_micros(500),
            max_coalesce: 16,
        }
    }
}

impl CoalescingConfig {
    /// Disabled coalescing configuration.
    pub fn disabled() -> Self {
        Self {
            enabled: false,
            window: Duration::ZERO,
            max_coalesce: 0,
        }
    }
}

/// Configuration for resource limiting.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceLimiterConfig {
    /// Maximum memory usage in bytes.
    pub max_memory_bytes: u64,

    /// Maximum concurrent connections.
    pub max_connections: usize,

    /// Maximum requests per second (rate limit).
    pub max_requests_per_second: u64,

    /// Threshold (0.0-1.0) at which to start backpressure.
    pub backpressure_threshold: f64,

    /// Backpressure strategy.
    pub strategy: BackpressureStrategyConfig,
}

impl Default for ResourceLimiterConfig {
    fn default() -> Self {
        ScalabilityConfig::medium().resources
    }
}

impl ResourceLimiterConfig {
    /// Validate the configuration.
    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.backpressure_threshold <= 0.0 || self.backpressure_threshold > 1.0 {
            return Err(ConfigError::InvalidValue {
                field: "backpressure_threshold".into(),
                message: "must be between 0.0 (exclusive) and 1.0 (inclusive)".into(),
            });
        }

        Ok(())
    }
}

/// Backpressure strategy configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BackpressureStrategyConfig {
    /// No backpressure - reject when limits exceeded.
    None,
    /// Gradual slowdown as resources approach limits.
    Gradual,
    /// Adaptive strategy that adjusts based on system state.
    Adaptive,
    /// Aggressive strategy for high-load scenarios.
    Aggressive,
}

impl Default for BackpressureStrategyConfig {
    fn default() -> Self {
        Self::Adaptive
    }
}

/// Configuration for the performance profiler.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProfilerConfig {
    /// Enable profiling.
    pub enabled: bool,

    /// Sample rate (0.0-1.0) for latency measurements.
    pub sample_rate: f64,

    /// Histogram buckets for latency tracking (in microseconds).
    pub histogram_buckets: Vec<u64>,

    /// Interval for generating reports.
    pub report_interval: Duration,

    /// Track memory usage.
    pub track_memory: bool,

    /// Track CPU usage.
    pub track_cpu: bool,
}

impl Default for ProfilerConfig {
    fn default() -> Self {
        ScalabilityConfig::medium().profiler
    }
}

impl ProfilerConfig {
    /// Validate the configuration.
    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.sample_rate < 0.0 || self.sample_rate > 1.0 {
            return Err(ConfigError::InvalidValue {
                field: "sample_rate".into(),
                message: "must be between 0.0 and 1.0".into(),
            });
        }

        if self.enabled && self.histogram_buckets.is_empty() {
            return Err(ConfigError::InvalidValue {
                field: "histogram_buckets".into(),
                message: "must have at least one bucket when profiling is enabled".into(),
            });
        }

        Ok(())
    }
}

/// Default latency histogram buckets (in microseconds).
fn default_latency_buckets() -> Vec<u64> {
    vec![
        50,      // 50µs
        100,     // 100µs
        250,     // 250µs
        500,     // 500µs
        1_000,   // 1ms
        2_500,   // 2.5ms
        5_000,   // 5ms
        10_000,  // 10ms
        25_000,  // 25ms
        50_000,  // 50ms
        100_000, // 100ms
    ]
}

/// Configuration error.
#[derive(Debug, Clone, thiserror::Error)]
pub enum ConfigError {
    #[error("Invalid value for {field}: {message}")]
    InvalidValue { field: String, message: String },

    #[error("Missing required field: {field}")]
    MissingField { field: String },

    #[error("Inconsistent configuration: {message}")]
    Inconsistent { message: String },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = ScalabilityConfig::default();
        assert_eq!(config.preset, ScalabilityPreset::Medium);
        assert_eq!(config.connection_pool.max_connections, 10_000);
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_presets() {
        for preset in [
            ScalabilityPreset::Development,
            ScalabilityPreset::Small,
            ScalabilityPreset::Medium,
            ScalabilityPreset::Large,
            ScalabilityPreset::Enterprise,
        ] {
            let config = ScalabilityConfig::for_preset(preset);
            assert!(config.validate().is_ok(), "Preset {:?} failed validation", preset);
        }
    }

    #[test]
    fn test_for_connections() {
        let config = ScalabilityConfig::for_connections(50);
        assert_eq!(config.preset, ScalabilityPreset::Development);

        let config = ScalabilityConfig::for_connections(500);
        assert_eq!(config.preset, ScalabilityPreset::Small);

        let config = ScalabilityConfig::for_connections(5_000);
        assert_eq!(config.preset, ScalabilityPreset::Medium);

        let config = ScalabilityConfig::for_connections(30_000);
        assert_eq!(config.preset, ScalabilityPreset::Large);

        let config = ScalabilityConfig::for_connections(100_000);
        assert_eq!(config.preset, ScalabilityPreset::Enterprise);
    }

    #[test]
    fn test_optimal_shard_count() {
        assert_eq!(ConnectionPoolConfig::optimal_shard_count(100), 4);
        assert_eq!(ConnectionPoolConfig::optimal_shard_count(1_000), 4);
        assert_eq!(ConnectionPoolConfig::optimal_shard_count(10_000), 64);
        assert_eq!(ConnectionPoolConfig::optimal_shard_count(50_000), 256);
        assert_eq!(ConnectionPoolConfig::optimal_shard_count(100_000), 256);
    }

    #[test]
    fn test_validation_errors() {
        let mut config = ScalabilityConfig::default();
        config.connection_pool.max_connections = 0;
        assert!(config.connection_pool.validate().is_err());

        let mut config = ScalabilityConfig::default();
        config.resources.backpressure_threshold = 1.5;
        assert!(config.resources.validate().is_err());

        let mut config = ScalabilityConfig::default();
        config.profiler.sample_rate = -0.1;
        assert!(config.profiler.validate().is_err());
    }
}

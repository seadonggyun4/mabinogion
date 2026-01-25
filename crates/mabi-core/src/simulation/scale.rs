//! Scale configuration for simulations.
//!
//! Defines how many devices, data points, and resources to simulate.

use serde::{Deserialize, Serialize};

/// Scale configuration for simulations.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScaleConfig {
    /// Number of virtual devices to create.
    pub device_count: usize,

    /// Number of data points per device.
    pub points_per_device: usize,

    /// Batch size for device creation/deletion.
    pub batch_size: usize,

    /// Estimated memory per device in bytes.
    pub memory_per_device: usize,

    /// Number of concurrent connections.
    pub concurrent_connections: usize,

    /// Target operations per second.
    pub target_ops_per_sec: u64,
}

impl Default for ScaleConfig {
    fn default() -> Self {
        Self::medium()
    }
}

impl ScaleConfig {
    /// Create a scale config for a specific device count.
    pub fn devices(count: usize) -> Self {
        Self {
            device_count: count,
            points_per_device: 100,
            batch_size: (count / 100).max(100),
            memory_per_device: 4096,
            concurrent_connections: (count / 100).max(10),
            target_ops_per_sec: (count * 10) as u64,
        }
    }

    /// Tiny scale - for quick tests.
    pub fn tiny() -> Self {
        Self {
            device_count: 10,
            points_per_device: 10,
            batch_size: 10,
            memory_per_device: 1024,
            concurrent_connections: 5,
            target_ops_per_sec: 100,
        }
    }

    /// Small scale - for development testing.
    pub fn small() -> Self {
        Self {
            device_count: 100,
            points_per_device: 50,
            batch_size: 50,
            memory_per_device: 2048,
            concurrent_connections: 20,
            target_ops_per_sec: 1000,
        }
    }

    /// Medium scale - for integration testing.
    pub fn medium() -> Self {
        Self {
            device_count: 1_000,
            points_per_device: 100,
            batch_size: 100,
            memory_per_device: 4096,
            concurrent_connections: 100,
            target_ops_per_sec: 10_000,
        }
    }

    /// Large scale - for performance testing.
    pub fn large() -> Self {
        Self {
            device_count: 10_000,
            points_per_device: 100,
            batch_size: 500,
            memory_per_device: 4096,
            concurrent_connections: 500,
            target_ops_per_sec: 100_000,
        }
    }

    /// Extra large scale - for stress testing.
    pub fn xlarge() -> Self {
        Self {
            device_count: 50_000,
            points_per_device: 100,
            batch_size: 1000,
            memory_per_device: 4096,
            concurrent_connections: 1000,
            target_ops_per_sec: 500_000,
        }
    }

    /// Maximum scale - for capacity testing.
    pub fn max() -> Self {
        Self {
            device_count: 100_000,
            points_per_device: 100,
            batch_size: 2000,
            memory_per_device: 4096,
            concurrent_connections: 2000,
            target_ops_per_sec: 1_000_000,
        }
    }

    /// Set device count.
    pub fn with_device_count(mut self, count: usize) -> Self {
        self.device_count = count;
        self
    }

    /// Set points per device.
    pub fn with_points_per_device(mut self, count: usize) -> Self {
        self.points_per_device = count;
        self
    }

    /// Set batch size.
    pub fn with_batch_size(mut self, size: usize) -> Self {
        self.batch_size = size;
        self
    }

    /// Set memory per device.
    pub fn with_memory_per_device(mut self, bytes: usize) -> Self {
        self.memory_per_device = bytes;
        self
    }

    /// Calculate total data points.
    pub fn total_points(&self) -> usize {
        self.device_count * self.points_per_device
    }

    /// Calculate estimated total memory.
    pub fn estimated_memory(&self) -> usize {
        self.device_count * self.memory_per_device
    }

    /// Calculate estimated memory in MB.
    pub fn estimated_memory_mb(&self) -> f64 {
        self.estimated_memory() as f64 / 1024.0 / 1024.0
    }

    /// Get a description of this scale.
    pub fn description(&self) -> String {
        format!(
            "{} devices, {} points/device ({} total), ~{:.1} MB",
            self.device_count,
            self.points_per_device,
            self.total_points(),
            self.estimated_memory_mb()
        )
    }
}

/// Scale presets for common scenarios.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ScalePreset {
    /// 10 devices - quick validation.
    Tiny,
    /// 100 devices - development.
    Small,
    /// 1,000 devices - integration.
    Medium,
    /// 10,000 devices - performance.
    Large,
    /// 50,000 devices - stress.
    XLarge,
    /// 100,000 devices - capacity.
    Max,
}

impl ScalePreset {
    /// Convert to ScaleConfig.
    pub fn to_config(self) -> ScaleConfig {
        match self {
            Self::Tiny => ScaleConfig::tiny(),
            Self::Small => ScaleConfig::small(),
            Self::Medium => ScaleConfig::medium(),
            Self::Large => ScaleConfig::large(),
            Self::XLarge => ScaleConfig::xlarge(),
            Self::Max => ScaleConfig::max(),
        }
    }

    /// Get all presets.
    pub fn all() -> &'static [ScalePreset] {
        &[
            Self::Tiny,
            Self::Small,
            Self::Medium,
            Self::Large,
            Self::XLarge,
            Self::Max,
        ]
    }
}

impl From<ScalePreset> for ScaleConfig {
    fn from(preset: ScalePreset) -> Self {
        preset.to_config()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scale_presets() {
        assert_eq!(ScaleConfig::tiny().device_count, 10);
        assert_eq!(ScaleConfig::small().device_count, 100);
        assert_eq!(ScaleConfig::medium().device_count, 1_000);
        assert_eq!(ScaleConfig::large().device_count, 10_000);
        assert_eq!(ScaleConfig::xlarge().device_count, 50_000);
        assert_eq!(ScaleConfig::max().device_count, 100_000);
    }

    #[test]
    fn test_scale_devices() {
        let config = ScaleConfig::devices(5000);
        assert_eq!(config.device_count, 5000);
    }

    #[test]
    fn test_total_points() {
        let config = ScaleConfig::devices(100).with_points_per_device(50);
        assert_eq!(config.total_points(), 5000);
    }

    #[test]
    fn test_estimated_memory() {
        // 1024 devices * 1024 bytes = 1 MB exactly
        let config = ScaleConfig::devices(1024).with_memory_per_device(1024);
        assert_eq!(config.estimated_memory(), 1024 * 1024);
        assert!((config.estimated_memory_mb() - 1.0).abs() < 0.01);
    }

    #[test]
    fn test_scale_preset_conversion() {
        let config: ScaleConfig = ScalePreset::Large.into();
        assert_eq!(config.device_count, 10_000);
    }

    #[test]
    fn test_description() {
        let config = ScaleConfig::tiny();
        let desc = config.description();
        assert!(desc.contains("10 devices"));
    }
}

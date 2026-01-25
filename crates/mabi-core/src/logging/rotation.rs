//! Log rotation configuration and utilities.
//!
//! This module provides configuration for log file rotation strategies,
//! supporting both time-based and size-based rotation through `tracing-appender`.

use serde::{Deserialize, Serialize};

/// Rotation strategy for log files.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum RotationStrategy {
    /// Rotate logs daily.
    #[default]
    Daily,
    /// Rotate logs hourly.
    Hourly,
    /// Rotate logs every minute (mainly for testing).
    Minutely,
    /// Never rotate - single log file.
    Never,
}

impl RotationStrategy {
    /// Get a human-readable description of the rotation strategy.
    pub fn description(&self) -> &'static str {
        match self {
            Self::Daily => "Rotates logs once per day at midnight",
            Self::Hourly => "Rotates logs every hour",
            Self::Minutely => "Rotates logs every minute (for testing)",
            Self::Never => "Never rotates - uses single log file",
        }
    }

    /// Get the expected file suffix pattern for this strategy.
    pub fn suffix_pattern(&self) -> &'static str {
        match self {
            Self::Daily => "YYYY-MM-DD",
            Self::Hourly => "YYYY-MM-DD-HH",
            Self::Minutely => "YYYY-MM-DD-HH-mm",
            Self::Never => "(none)",
        }
    }
}

/// Log rotation configuration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RotationConfig {
    /// Rotation strategy.
    #[serde(default)]
    pub strategy: RotationStrategy,

    /// Maximum number of rotated files to keep.
    /// None means keep all files indefinitely.
    #[serde(default)]
    pub max_files: Option<u32>,

    /// Whether to compress rotated files (future feature).
    #[serde(default)]
    pub compress: bool,
}

impl Default for RotationConfig {
    fn default() -> Self {
        Self {
            strategy: RotationStrategy::Daily,
            max_files: Some(7), // Keep 7 days by default
            compress: false,
        }
    }
}

impl RotationConfig {
    /// Create a new rotation config with the given strategy.
    pub fn new(strategy: RotationStrategy) -> Self {
        Self {
            strategy,
            ..Default::default()
        }
    }

    /// Create a daily rotation config.
    pub fn daily() -> Self {
        Self::new(RotationStrategy::Daily)
    }

    /// Create an hourly rotation config.
    pub fn hourly() -> Self {
        Self::new(RotationStrategy::Hourly)
    }

    /// Create a minutely rotation config (for testing).
    pub fn minutely() -> Self {
        Self::new(RotationStrategy::Minutely)
    }

    /// Create a never-rotate config.
    pub fn never() -> Self {
        Self::new(RotationStrategy::Never)
    }

    /// Set the maximum number of rotated files to keep.
    pub fn with_max_files(mut self, max: u32) -> Self {
        self.max_files = Some(max);
        self
    }

    /// Keep all rotated files indefinitely.
    pub fn keep_all(mut self) -> Self {
        self.max_files = None;
        self
    }

    /// Enable compression of rotated files (future feature).
    pub fn with_compression(mut self, compress: bool) -> Self {
        self.compress = compress;
        self
    }

    /// Calculate the approximate disk space needed for log retention.
    ///
    /// # Arguments
    /// * `avg_log_size_mb` - Estimated average log file size in MB
    ///
    /// # Returns
    /// Estimated disk space needed in MB, or None if max_files is unlimited.
    pub fn estimated_disk_space(&self, avg_log_size_mb: f64) -> Option<f64> {
        self.max_files.map(|max| avg_log_size_mb * max as f64)
    }
}

/// Represents the retention policy for log files.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetentionPolicy {
    /// Maximum age of log files in days.
    pub max_age_days: Option<u32>,

    /// Maximum total size of log files in bytes.
    pub max_total_size_bytes: Option<u64>,

    /// Maximum number of log files.
    pub max_files: Option<u32>,
}

impl Default for RetentionPolicy {
    fn default() -> Self {
        Self {
            max_age_days: Some(30),
            max_total_size_bytes: Some(1024 * 1024 * 1024), // 1 GB
            max_files: Some(100),
        }
    }
}

impl RetentionPolicy {
    /// Create a new retention policy.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set maximum age in days.
    pub fn with_max_age_days(mut self, days: u32) -> Self {
        self.max_age_days = Some(days);
        self
    }

    /// Set maximum total size.
    pub fn with_max_total_size(mut self, bytes: u64) -> Self {
        self.max_total_size_bytes = Some(bytes);
        self
    }

    /// Set maximum number of files.
    pub fn with_max_files(mut self, files: u32) -> Self {
        self.max_files = Some(files);
        self
    }

    /// Create a retention policy that keeps everything.
    pub fn keep_all() -> Self {
        Self {
            max_age_days: None,
            max_total_size_bytes: None,
            max_files: None,
        }
    }

    /// Create a minimal retention policy for testing.
    pub fn minimal() -> Self {
        Self {
            max_age_days: Some(1),
            max_total_size_bytes: Some(10 * 1024 * 1024), // 10 MB
            max_files: Some(5),
        }
    }
}

/// Statistics about log files for a given directory.
#[derive(Debug, Clone, Default)]
pub struct LogFileStats {
    /// Total number of log files.
    pub file_count: usize,
    /// Total size of all log files in bytes.
    pub total_size_bytes: u64,
    /// Oldest file timestamp (Unix timestamp).
    pub oldest_file_timestamp: Option<u64>,
    /// Newest file timestamp (Unix timestamp).
    pub newest_file_timestamp: Option<u64>,
}

impl LogFileStats {
    /// Calculate statistics for log files in a directory.
    pub fn from_directory(
        directory: &std::path::Path,
        filename_prefix: &str,
    ) -> std::io::Result<Self> {
        let mut stats = Self::default();

        if !directory.exists() {
            return Ok(stats);
        }

        for entry in std::fs::read_dir(directory)? {
            let entry = entry?;
            let path = entry.path();

            if !path.is_file() {
                continue;
            }

            let filename = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or_default();

            if !filename.starts_with(filename_prefix) {
                continue;
            }

            stats.file_count += 1;

            if let Ok(metadata) = entry.metadata() {
                stats.total_size_bytes += metadata.len();

                if let Ok(modified) = metadata.modified() {
                    if let Ok(duration) = modified.duration_since(std::time::UNIX_EPOCH) {
                        let timestamp = duration.as_secs();

                        stats.oldest_file_timestamp = Some(
                            stats
                                .oldest_file_timestamp
                                .map(|t| t.min(timestamp))
                                .unwrap_or(timestamp),
                        );

                        stats.newest_file_timestamp = Some(
                            stats
                                .newest_file_timestamp
                                .map(|t| t.max(timestamp))
                                .unwrap_or(timestamp),
                        );
                    }
                }
            }
        }

        Ok(stats)
    }

    /// Get total size in a human-readable format.
    pub fn total_size_human_readable(&self) -> String {
        const KB: u64 = 1024;
        const MB: u64 = KB * 1024;
        const GB: u64 = MB * 1024;

        if self.total_size_bytes >= GB {
            format!("{:.2} GB", self.total_size_bytes as f64 / GB as f64)
        } else if self.total_size_bytes >= MB {
            format!("{:.2} MB", self.total_size_bytes as f64 / MB as f64)
        } else if self.total_size_bytes >= KB {
            format!("{:.2} KB", self.total_size_bytes as f64 / KB as f64)
        } else {
            format!("{} bytes", self.total_size_bytes)
        }
    }

    /// Get the age of the oldest file in days.
    pub fn oldest_file_age_days(&self) -> Option<u32> {
        self.oldest_file_timestamp.map(|ts| {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            ((now.saturating_sub(ts)) / (24 * 60 * 60)) as u32
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rotation_strategy_default() {
        assert_eq!(RotationStrategy::default(), RotationStrategy::Daily);
    }

    #[test]
    fn test_rotation_config_builders() {
        let daily = RotationConfig::daily();
        assert_eq!(daily.strategy, RotationStrategy::Daily);

        let hourly = RotationConfig::hourly().with_max_files(24);
        assert_eq!(hourly.strategy, RotationStrategy::Hourly);
        assert_eq!(hourly.max_files, Some(24));

        let never = RotationConfig::never().keep_all();
        assert_eq!(never.strategy, RotationStrategy::Never);
        assert_eq!(never.max_files, None);
    }

    #[test]
    fn test_rotation_config_serialization() {
        let config = RotationConfig::daily().with_max_files(30);
        let yaml = serde_yaml::to_string(&config).unwrap();
        let parsed: RotationConfig = serde_yaml::from_str(&yaml).unwrap();

        assert_eq!(config.strategy, parsed.strategy);
        assert_eq!(config.max_files, parsed.max_files);
    }

    #[test]
    fn test_estimated_disk_space() {
        let config = RotationConfig::daily().with_max_files(30);
        let space = config.estimated_disk_space(100.0); // 100 MB per day
        assert_eq!(space, Some(3000.0)); // 30 * 100 = 3000 MB

        let unlimited = RotationConfig::daily().keep_all();
        assert_eq!(unlimited.estimated_disk_space(100.0), None);
    }

    #[test]
    fn test_retention_policy() {
        let policy = RetentionPolicy::new()
            .with_max_age_days(7)
            .with_max_files(10);

        assert_eq!(policy.max_age_days, Some(7));
        assert_eq!(policy.max_files, Some(10));
    }

    #[test]
    fn test_log_file_stats_human_readable() {
        let mut stats = LogFileStats::default();

        stats.total_size_bytes = 500;
        assert_eq!(stats.total_size_human_readable(), "500 bytes");

        stats.total_size_bytes = 1024 * 10;
        assert!(stats.total_size_human_readable().contains("KB"));

        stats.total_size_bytes = 1024 * 1024 * 50;
        assert!(stats.total_size_human_readable().contains("MB"));

        stats.total_size_bytes = 1024 * 1024 * 1024 * 2;
        assert!(stats.total_size_human_readable().contains("GB"));
    }

    #[test]
    fn test_strategy_descriptions() {
        assert!(!RotationStrategy::Daily.description().is_empty());
        assert!(!RotationStrategy::Hourly.suffix_pattern().is_empty());
    }
}

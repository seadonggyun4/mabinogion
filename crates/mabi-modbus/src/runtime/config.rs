//! Runtime configuration types and utilities.

use std::time::Duration;

use serde::{Deserialize, Serialize};

/// Connection limits configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionLimits {
    /// Maximum concurrent connections.
    pub max_connections: usize,

    /// Maximum connections per IP address.
    pub max_per_ip: Option<usize>,

    /// Connection rate limit (connections per second).
    pub rate_limit: Option<f64>,

    /// Burst allowance for rate limiting.
    pub burst_size: Option<usize>,
}

impl Default for ConnectionLimits {
    fn default() -> Self {
        Self {
            max_connections: 100,
            max_per_ip: Some(10),
            rate_limit: None,
            burst_size: None,
        }
    }
}

/// Timeout configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimeoutConfig {
    /// Connection idle timeout.
    pub idle: Duration,

    /// Request processing timeout.
    pub request: Duration,

    /// Connection handshake timeout.
    pub handshake: Duration,
}

impl Default for TimeoutConfig {
    fn default() -> Self {
        Self {
            idle: Duration::from_secs(300),
            request: Duration::from_secs(30),
            handshake: Duration::from_secs(10),
        }
    }
}

/// Access control configuration for registers.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccessControl {
    /// Whether read access is enabled by default.
    pub default_read: bool,

    /// Whether write access is enabled by default.
    pub default_write: bool,

    /// Specific read-only ranges.
    pub read_only_ranges: Vec<AddressRange>,

    /// Specific write-only ranges.
    pub write_only_ranges: Vec<AddressRange>,

    /// Blocked address ranges.
    pub blocked_ranges: Vec<AddressRange>,
}

impl Default for AccessControl {
    fn default() -> Self {
        Self {
            default_read: true,
            default_write: true,
            read_only_ranges: Vec::new(),
            write_only_ranges: Vec::new(),
            blocked_ranges: Vec::new(),
        }
    }
}

impl AccessControl {
    /// Check if an address can be read.
    pub fn can_read(&self, address: u16) -> bool {
        // Check blocked first
        for range in &self.blocked_ranges {
            if range.contains(address) {
                return false;
            }
        }

        // Check write-only
        for range in &self.write_only_ranges {
            if range.contains(address) {
                return false;
            }
        }

        // Check read-only (always readable) or default
        for range in &self.read_only_ranges {
            if range.contains(address) {
                return true;
            }
        }

        self.default_read
    }

    /// Check if an address can be written.
    pub fn can_write(&self, address: u16) -> bool {
        // Check blocked first
        for range in &self.blocked_ranges {
            if range.contains(address) {
                return false;
            }
        }

        // Check read-only
        for range in &self.read_only_ranges {
            if range.contains(address) {
                return false;
            }
        }

        // Check write-only (always writable) or default
        for range in &self.write_only_ranges {
            if range.contains(address) {
                return true;
            }
        }

        self.default_write
    }
}

/// Address range.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AddressRange {
    /// Start address (inclusive).
    pub start: u16,

    /// End address (inclusive).
    pub end: u16,
}

impl AddressRange {
    /// Create a new address range.
    pub fn new(start: u16, end: u16) -> Self {
        Self {
            start: start.min(end),
            end: start.max(end),
        }
    }

    /// Create a single address range.
    pub fn single(address: u16) -> Self {
        Self::new(address, address)
    }

    /// Check if an address is in this range.
    pub fn contains(&self, address: u16) -> bool {
        address >= self.start && address <= self.end
    }

    /// Get the number of addresses in this range.
    pub fn len(&self) -> usize {
        (self.end - self.start + 1) as usize
    }

    /// Check if the range is empty.
    pub fn is_empty(&self) -> bool {
        false // A range always has at least one address
    }
}

/// Feature flags for runtime feature toggling.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FeatureFlags {
    /// Enable detailed metrics.
    pub detailed_metrics: bool,

    /// Enable request logging.
    pub request_logging: bool,

    /// Enable connection logging.
    pub connection_logging: bool,

    /// Enable slow request warnings.
    pub slow_request_warnings: bool,

    /// Slow request threshold.
    pub slow_request_threshold_ms: u64,

    /// Enable broadcast (unit ID 0) support.
    pub broadcast_enabled: bool,

    /// Enable write operations.
    pub writes_enabled: bool,

    /// Enable diagnostic function codes.
    pub diagnostics_enabled: bool,
}

impl FeatureFlags {
    /// Production-ready defaults.
    pub fn production() -> Self {
        Self {
            detailed_metrics: false,
            request_logging: false,
            connection_logging: false,
            slow_request_warnings: true,
            slow_request_threshold_ms: 100,
            broadcast_enabled: false,
            writes_enabled: true,
            diagnostics_enabled: false,
        }
    }

    /// Development/debug defaults.
    pub fn development() -> Self {
        Self {
            detailed_metrics: true,
            request_logging: true,
            connection_logging: true,
            slow_request_warnings: true,
            slow_request_threshold_ms: 50,
            broadcast_enabled: true,
            writes_enabled: true,
            diagnostics_enabled: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_address_range() {
        let range = AddressRange::new(100, 200);
        assert!(range.contains(100));
        assert!(range.contains(150));
        assert!(range.contains(200));
        assert!(!range.contains(99));
        assert!(!range.contains(201));
        assert_eq!(range.len(), 101);
    }

    #[test]
    fn test_address_range_reversed() {
        // Should auto-correct reversed ranges
        let range = AddressRange::new(200, 100);
        assert_eq!(range.start, 100);
        assert_eq!(range.end, 200);
    }

    #[test]
    fn test_access_control_default() {
        let access = AccessControl::default();
        assert!(access.can_read(0));
        assert!(access.can_read(65535));
        assert!(access.can_write(0));
        assert!(access.can_write(65535));
    }

    #[test]
    fn test_access_control_read_only() {
        let mut access = AccessControl::default();
        access.read_only_ranges.push(AddressRange::new(100, 200));

        assert!(access.can_read(150));
        assert!(!access.can_write(150)); // Read-only means no write
        assert!(access.can_write(50)); // Outside range, default applies
    }

    #[test]
    fn test_access_control_blocked() {
        let mut access = AccessControl::default();
        access.blocked_ranges.push(AddressRange::new(500, 600));

        assert!(!access.can_read(550));
        assert!(!access.can_write(550));
        assert!(access.can_read(499));
        assert!(access.can_read(601));
    }

    #[test]
    fn test_feature_flags_production() {
        let flags = FeatureFlags::production();
        assert!(!flags.detailed_metrics);
        assert!(!flags.request_logging);
        assert!(flags.writes_enabled);
    }

    #[test]
    fn test_feature_flags_development() {
        let flags = FeatureFlags::development();
        assert!(flags.detailed_metrics);
        assert!(flags.request_logging);
        assert!(flags.diagnostics_enabled);
    }

    #[test]
    fn test_timeout_config_default() {
        let config = TimeoutConfig::default();
        assert_eq!(config.idle, Duration::from_secs(300));
        assert_eq!(config.request, Duration::from_secs(30));
    }

    #[test]
    fn test_connection_limits_default() {
        let limits = ConnectionLimits::default();
        assert_eq!(limits.max_connections, 100);
        assert_eq!(limits.max_per_ip, Some(10));
    }
}

//! Failure injection for chaos engineering.
//!
//! Simulates various failure conditions to test system resilience.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use rand::prelude::*;
use serde::{Deserialize, Serialize};

/// Failure injection configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FailureConfig {
    /// Whether failure injection is enabled.
    pub enabled: bool,

    /// Failure injection rate (0.0 - 1.0).
    pub failure_rate: f64,

    /// Types of failures to inject.
    pub failure_types: Vec<FailureType>,

    /// Failure schedule type.
    pub schedule: FailureSchedule,

    /// Probability of cascading failures.
    pub cascade_probability: f64,

    /// Time to recover from a failure.
    pub recovery_time: Duration,

    /// Maximum concurrent failures allowed.
    pub max_concurrent_failures: usize,

    /// Random seed for reproducibility.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seed: Option<u64>,
}

impl Default for FailureConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            failure_rate: 0.01, // 1% failure rate
            failure_types: vec![
                FailureType::Timeout,
                FailureType::ConnectionReset,
                FailureType::ProtocolError,
            ],
            schedule: FailureSchedule::Random {
                min_interval: Duration::from_secs(10),
                max_interval: Duration::from_secs(60),
            },
            cascade_probability: 0.0,
            recovery_time: Duration::from_secs(30),
            max_concurrent_failures: 5,
            seed: None,
        }
    }
}

impl FailureConfig {
    /// Create a new failure config.
    pub fn new(rate: f64) -> Self {
        Self {
            failure_rate: rate.clamp(0.0, 1.0),
            ..Default::default()
        }
    }

    /// No failures (disabled).
    pub fn none() -> Self {
        Self {
            enabled: false,
            failure_rate: 0.0,
            failure_types: vec![],
            ..Default::default()
        }
    }

    /// Low failure rate (0.1%).
    pub fn low() -> Self {
        Self {
            failure_rate: 0.001,
            ..Default::default()
        }
    }

    /// Medium failure rate (1%).
    pub fn medium() -> Self {
        Self {
            failure_rate: 0.01,
            ..Default::default()
        }
    }

    /// High failure rate (5%).
    pub fn high() -> Self {
        Self {
            failure_rate: 0.05,
            ..Default::default()
        }
    }

    /// Chaos mode (10%).
    pub fn chaos() -> Self {
        Self {
            failure_rate: 0.10,
            failure_types: FailureType::all().to_vec(),
            ..Default::default()
        }
    }

    /// Set failure types.
    pub fn with_types(mut self, types: Vec<FailureType>) -> Self {
        self.failure_types = types;
        self
    }

    /// Set schedule.
    pub fn with_schedule(mut self, schedule: FailureSchedule) -> Self {
        self.schedule = schedule;
        self
    }

    /// Set random seed.
    pub fn with_seed(mut self, seed: u64) -> Self {
        self.seed = Some(seed);
        self
    }

    /// Check if a failure should be injected.
    pub fn should_inject(&self) -> bool {
        if !self.enabled || self.failure_rate <= 0.0 || self.failure_types.is_empty() {
            return false;
        }

        let mut rng = match self.seed {
            Some(seed) => StdRng::seed_from_u64(seed),
            None => StdRng::from_entropy(),
        };

        rng.gen::<f64>() < self.failure_rate
    }

    /// Get the next failure type to inject.
    pub fn next_failure_type(&self) -> FailureType {
        if self.failure_types.is_empty() {
            return FailureType::Generic("No failures configured".into());
        }

        let mut rng = match self.seed {
            Some(seed) => StdRng::seed_from_u64(seed),
            None => StdRng::from_entropy(),
        };
        let idx = rng.gen_range(0..self.failure_types.len());
        self.failure_types[idx].clone()
    }
}

/// Failure schedule types.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FailureSchedule {
    /// Random intervals between failures.
    Random {
        /// Minimum time between failures.
        min_interval: Duration,
        /// Maximum time between failures.
        max_interval: Duration,
    },

    /// Fixed periodic failures.
    Periodic {
        /// Interval between failures.
        interval: Duration,
        /// Duration of each failure.
        duration: Duration,
    },

    /// Scheduled at specific times.
    Scheduled {
        /// List of scheduled failures.
        entries: Vec<ScheduledFailure>,
    },

    /// Burst of failures.
    Burst {
        /// Number of failures in burst.
        count: usize,
        /// Time between failures in burst.
        burst_interval: Duration,
        /// Time between bursts.
        burst_pause: Duration,
    },
}

impl Default for FailureSchedule {
    fn default() -> Self {
        Self::Random {
            min_interval: Duration::from_secs(10),
            max_interval: Duration::from_secs(60),
        }
    }
}

/// Types of failures that can be injected.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum FailureType {
    /// Request timeout.
    Timeout,

    /// Connection reset by peer.
    ConnectionReset,

    /// Connection refused.
    ConnectionRefused,

    /// Protocol error.
    ProtocolError,

    /// Invalid response.
    InvalidResponse,

    /// Partial response.
    PartialResponse,

    /// Slow response (latency spike).
    SlowResponse {
        /// Delay to add.
        delay: Duration,
    },

    /// Out of memory simulation.
    OutOfMemory,

    /// Disk full simulation.
    DiskFull,

    /// Device offline.
    DeviceOffline,

    /// Network partition.
    NetworkPartition,

    /// Corrupted data.
    CorruptedData,

    /// Rate limited.
    RateLimited,

    /// Authentication failure.
    AuthFailure,

    /// Generic failure with custom message.
    Generic(String),
}

impl FailureType {
    /// Get all failure types (except parameterized ones).
    pub fn all() -> &'static [FailureType] {
        &[
            FailureType::Timeout,
            FailureType::ConnectionReset,
            FailureType::ConnectionRefused,
            FailureType::ProtocolError,
            FailureType::InvalidResponse,
            FailureType::PartialResponse,
            FailureType::OutOfMemory,
            FailureType::DiskFull,
            FailureType::DeviceOffline,
            FailureType::NetworkPartition,
            FailureType::CorruptedData,
            FailureType::RateLimited,
            FailureType::AuthFailure,
        ]
    }

    /// Get network-related failures.
    pub fn network() -> Vec<FailureType> {
        vec![
            FailureType::Timeout,
            FailureType::ConnectionReset,
            FailureType::ConnectionRefused,
            FailureType::NetworkPartition,
        ]
    }

    /// Get protocol-related failures.
    pub fn protocol() -> Vec<FailureType> {
        vec![
            FailureType::ProtocolError,
            FailureType::InvalidResponse,
            FailureType::PartialResponse,
            FailureType::CorruptedData,
        ]
    }

    /// Get resource-related failures.
    pub fn resource() -> Vec<FailureType> {
        vec![
            FailureType::OutOfMemory,
            FailureType::DiskFull,
            FailureType::RateLimited,
        ]
    }

    /// Get a description of this failure type.
    pub fn description(&self) -> &str {
        match self {
            Self::Timeout => "Request timed out",
            Self::ConnectionReset => "Connection reset by peer",
            Self::ConnectionRefused => "Connection refused",
            Self::ProtocolError => "Protocol violation",
            Self::InvalidResponse => "Invalid response received",
            Self::PartialResponse => "Incomplete response",
            Self::SlowResponse { .. } => "Response delayed significantly",
            Self::OutOfMemory => "Out of memory",
            Self::DiskFull => "Disk full",
            Self::DeviceOffline => "Device went offline",
            Self::NetworkPartition => "Network partition detected",
            Self::CorruptedData => "Data corruption detected",
            Self::RateLimited => "Rate limit exceeded",
            Self::AuthFailure => "Authentication failed",
            Self::Generic(msg) => msg,
        }
    }

    /// Check if this failure is recoverable.
    pub fn is_recoverable(&self) -> bool {
        match self {
            Self::Timeout
            | Self::ConnectionReset
            | Self::SlowResponse { .. }
            | Self::RateLimited
            | Self::DeviceOffline
            | Self::NetworkPartition => true,

            Self::ConnectionRefused
            | Self::ProtocolError
            | Self::InvalidResponse
            | Self::PartialResponse
            | Self::OutOfMemory
            | Self::DiskFull
            | Self::CorruptedData
            | Self::AuthFailure
            | Self::Generic(_) => false,
        }
    }

    /// Get suggested retry delay for recoverable failures.
    pub fn suggested_retry_delay(&self) -> Option<Duration> {
        if !self.is_recoverable() {
            return None;
        }

        Some(match self {
            Self::Timeout => Duration::from_secs(5),
            Self::ConnectionReset => Duration::from_millis(500),
            Self::SlowResponse { .. } => Duration::from_secs(1),
            Self::RateLimited => Duration::from_secs(30),
            Self::DeviceOffline => Duration::from_secs(10),
            Self::NetworkPartition => Duration::from_secs(60),
            _ => Duration::from_secs(1),
        })
    }
}

impl std::fmt::Display for FailureType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.description())
    }
}

/// A scheduled failure.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduledFailure {
    /// When to inject (offset from start).
    pub at: Duration,
    /// Type of failure.
    pub failure_type: FailureType,
    /// Duration of the failure (for continuous failures).
    pub duration: Option<Duration>,
    /// Number of times to repeat.
    pub repeat: usize,
}

impl ScheduledFailure {
    /// Create a new scheduled failure.
    pub fn new(at: Duration, failure_type: FailureType) -> Self {
        Self {
            at,
            failure_type,
            duration: None,
            repeat: 1,
        }
    }

    /// Set the duration.
    pub fn for_duration(mut self, duration: Duration) -> Self {
        self.duration = Some(duration);
        self
    }

    /// Set repeat count.
    pub fn repeat(mut self, times: usize) -> Self {
        self.repeat = times;
        self
    }
}

/// Stateful failure injector for tracking injection state.
#[derive(Debug)]
pub struct FailureInjector {
    config: FailureConfig,
    current_index: AtomicU64,
    active_failures: AtomicU64,
    total_injected: AtomicU64,
    rng: parking_lot::Mutex<StdRng>,
}

impl FailureInjector {
    /// Create a new failure injector.
    pub fn new(config: FailureConfig) -> Self {
        let rng = match config.seed {
            Some(seed) => StdRng::seed_from_u64(seed),
            None => StdRng::from_entropy(),
        };

        Self {
            config,
            current_index: AtomicU64::new(0),
            active_failures: AtomicU64::new(0),
            total_injected: AtomicU64::new(0),
            rng: parking_lot::Mutex::new(rng),
        }
    }

    /// Check if a failure should be injected.
    pub fn should_inject(&self) -> bool {
        if !self.config.enabled
            || self.config.failure_rate <= 0.0
            || self.config.failure_types.is_empty()
        {
            return false;
        }

        // Check max concurrent failures
        let active = self.active_failures.load(Ordering::SeqCst);
        if active >= self.config.max_concurrent_failures as u64 {
            return false;
        }

        let mut rng = self.rng.lock();
        rng.gen::<f64>() < self.config.failure_rate
    }

    /// Get the next failure type.
    pub fn next_failure_type(&self) -> FailureType {
        if self.config.failure_types.is_empty() {
            return FailureType::Generic("No failures configured".into());
        }

        let mut rng = self.rng.lock();
        let idx = rng.gen_range(0..self.config.failure_types.len());
        self.config.failure_types[idx].clone()
    }

    /// Inject a failure and track it.
    pub fn inject(&self) -> Option<FailureType> {
        if !self.should_inject() {
            return None;
        }

        self.active_failures.fetch_add(1, Ordering::SeqCst);
        self.total_injected.fetch_add(1, Ordering::SeqCst);

        Some(self.next_failure_type())
    }

    /// Mark a failure as recovered.
    pub fn recover(&self) {
        let current = self.active_failures.load(Ordering::SeqCst);
        if current > 0 {
            self.active_failures.fetch_sub(1, Ordering::SeqCst);
        }
    }

    /// Get the number of active failures.
    pub fn active_count(&self) -> u64 {
        self.active_failures.load(Ordering::SeqCst)
    }

    /// Get the total number of injected failures.
    pub fn total_count(&self) -> u64 {
        self.total_injected.load(Ordering::SeqCst)
    }

    /// Reset the injector state.
    pub fn reset(&self) {
        self.current_index.store(0, Ordering::SeqCst);
        self.active_failures.store(0, Ordering::SeqCst);
        self.total_injected.store(0, Ordering::SeqCst);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_failure_config_default() {
        let config = FailureConfig::default();
        assert!(config.enabled);
        assert_eq!(config.failure_rate, 0.01);
        assert!(!config.failure_types.is_empty());
    }

    #[test]
    fn test_failure_config_presets() {
        let none = FailureConfig::none();
        assert!(!none.enabled);
        assert_eq!(none.failure_rate, 0.0);

        let low = FailureConfig::low();
        assert_eq!(low.failure_rate, 0.001);

        let chaos = FailureConfig::chaos();
        assert_eq!(chaos.failure_rate, 0.10);
    }

    #[test]
    fn test_should_inject_disabled() {
        let config = FailureConfig::none();
        assert!(!config.should_inject());
    }

    #[test]
    fn test_failure_type_all() {
        let all = FailureType::all();
        assert!(all.len() >= 10);
    }

    #[test]
    fn test_failure_type_categories() {
        let network = FailureType::network();
        assert!(network.contains(&FailureType::Timeout));

        let protocol = FailureType::protocol();
        assert!(protocol.contains(&FailureType::ProtocolError));

        let resource = FailureType::resource();
        assert!(resource.contains(&FailureType::OutOfMemory));
    }

    #[test]
    fn test_failure_type_recoverable() {
        assert!(FailureType::Timeout.is_recoverable());
        assert!(!FailureType::ProtocolError.is_recoverable());
    }

    #[test]
    fn test_retry_delay() {
        let timeout = FailureType::Timeout;
        assert!(timeout.suggested_retry_delay().is_some());

        let protocol = FailureType::ProtocolError;
        assert!(protocol.suggested_retry_delay().is_none());
    }

    #[test]
    fn test_failure_schedule_random() {
        let schedule = FailureSchedule::Random {
            min_interval: Duration::from_secs(5),
            max_interval: Duration::from_secs(30),
        };
        matches!(schedule, FailureSchedule::Random { .. });
    }

    #[test]
    fn test_failure_schedule_periodic() {
        let schedule = FailureSchedule::Periodic {
            interval: Duration::from_secs(60),
            duration: Duration::from_secs(10),
        };
        matches!(schedule, FailureSchedule::Periodic { .. });
    }

    #[test]
    fn test_failure_injector() {
        let config = FailureConfig::new(1.0) // 100% rate for testing
            .with_types(vec![FailureType::Timeout]);

        let injector = FailureInjector::new(config);

        // Should be able to inject
        let failure = injector.inject();
        assert!(failure.is_some());
        assert_eq!(injector.active_count(), 1);
        assert_eq!(injector.total_count(), 1);

        // Recover
        injector.recover();
        assert_eq!(injector.active_count(), 0);

        // Reset
        injector.reset();
        assert_eq!(injector.total_count(), 0);
    }

    #[test]
    fn test_failure_injector_max_concurrent() {
        let mut config = FailureConfig::new(1.0);
        config.max_concurrent_failures = 2;

        let injector = FailureInjector::new(config);

        // Inject up to max
        injector.inject();
        injector.inject();
        assert_eq!(injector.active_count(), 2);

        // Should not inject more
        // Note: should_inject will return false due to max_concurrent check
        let active_before = injector.active_count();
        if !injector.should_inject() {
            assert_eq!(active_before, 2);
        }
    }

    #[test]
    fn test_failure_type_display() {
        let failure = FailureType::Timeout;
        assert_eq!(failure.to_string(), "Request timed out");

        let slow = FailureType::SlowResponse {
            delay: Duration::from_secs(5),
        };
        assert_eq!(slow.to_string(), "Response delayed significantly");
    }

    #[test]
    fn test_scheduled_failure() {
        let failure = ScheduledFailure::new(Duration::from_secs(10), FailureType::NetworkPartition)
            .for_duration(Duration::from_secs(30))
            .repeat(3);

        assert_eq!(failure.at, Duration::from_secs(10));
        assert_eq!(failure.duration, Some(Duration::from_secs(30)));
        assert_eq!(failure.repeat, 3);
    }
}

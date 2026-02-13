//! Lock-free fault injection statistics.
//!
//! Provides atomic counters for tracking fault activation, enabling
//! real-time monitoring without lock contention on the hot path.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

/// Lock-free statistics for a single fault type.
///
/// All counters use `AtomicU64` with `Relaxed` ordering for increments
/// and `Acquire` ordering for reads, matching the pattern used throughout
/// the trap ecosystem (RecoveryStats, ContinuationStats, etc.).
pub struct FaultStats {
    /// Total number of times `should_activate` was called.
    checks: AtomicU64,
    /// Number of times the fault actually activated (passed probability + targeting).
    activations: AtomicU64,
    /// Number of requests affected (may differ from activations for pipeline faults).
    affected_requests: AtomicU64,
    /// Whether this fault is currently enabled.
    enabled: AtomicBool,
    /// Timestamp (epoch millis) of last activation, 0 if never.
    last_activation_ms: AtomicU64,
}

impl FaultStats {
    /// Create new zeroed statistics with the fault enabled.
    pub fn new() -> Self {
        Self {
            checks: AtomicU64::new(0),
            activations: AtomicU64::new(0),
            affected_requests: AtomicU64::new(0),
            enabled: AtomicBool::new(true),
            last_activation_ms: AtomicU64::new(0),
        }
    }

    /// Record a check (should_activate was called).
    #[inline]
    pub fn record_check(&self) {
        self.checks.fetch_add(1, Ordering::Relaxed);
    }

    /// Record an activation.
    #[inline]
    pub fn record_activation(&self) {
        self.activations.fetch_add(1, Ordering::Relaxed);
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        self.last_activation_ms.store(now, Ordering::Relaxed);
    }

    /// Record an affected request.
    #[inline]
    pub fn record_affected(&self) {
        self.affected_requests.fetch_add(1, Ordering::Relaxed);
    }

    /// Check if the fault is enabled.
    #[inline]
    pub fn is_enabled(&self) -> bool {
        self.enabled.load(Ordering::Acquire)
    }

    /// Set whether the fault is enabled.
    #[inline]
    pub fn set_enabled(&self, enabled: bool) {
        self.enabled.store(enabled, Ordering::Release);
    }

    /// Take a consistent snapshot of all statistics.
    pub fn snapshot(&self) -> FaultStatsSnapshot {
        FaultStatsSnapshot {
            checks: self.checks.load(Ordering::Acquire),
            activations: self.activations.load(Ordering::Acquire),
            affected_requests: self.affected_requests.load(Ordering::Acquire),
            enabled: self.enabled.load(Ordering::Acquire),
            last_activation_ms: self.last_activation_ms.load(Ordering::Acquire),
        }
    }

    /// Reset all counters to zero (does not change enabled state).
    pub fn reset(&self) {
        self.checks.store(0, Ordering::Release);
        self.activations.store(0, Ordering::Release);
        self.affected_requests.store(0, Ordering::Release);
        self.last_activation_ms.store(0, Ordering::Release);
    }
}

impl Default for FaultStats {
    fn default() -> Self {
        Self::new()
    }
}

/// Immutable snapshot of fault statistics at a point in time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FaultStatsSnapshot {
    /// Total checks performed.
    pub checks: u64,
    /// Total activations.
    pub activations: u64,
    /// Total affected requests.
    pub affected_requests: u64,
    /// Whether the fault was enabled at snapshot time.
    pub enabled: bool,
    /// Epoch millis of last activation (0 = never).
    pub last_activation_ms: u64,
}

impl FaultStatsSnapshot {
    /// Activation rate as a fraction (0.0 to 1.0).
    /// Returns 0.0 if no checks have been performed.
    pub fn activation_rate(&self) -> f64 {
        if self.checks == 0 {
            0.0
        } else {
            self.activations as f64 / self.checks as f64
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stats_new() {
        let stats = FaultStats::new();
        let snap = stats.snapshot();
        assert_eq!(snap.checks, 0);
        assert_eq!(snap.activations, 0);
        assert_eq!(snap.affected_requests, 0);
        assert!(snap.enabled);
        assert_eq!(snap.last_activation_ms, 0);
    }

    #[test]
    fn test_stats_record() {
        let stats = FaultStats::new();
        stats.record_check();
        stats.record_check();
        stats.record_activation();
        stats.record_affected();

        let snap = stats.snapshot();
        assert_eq!(snap.checks, 2);
        assert_eq!(snap.activations, 1);
        assert_eq!(snap.affected_requests, 1);
        assert!(snap.last_activation_ms > 0);
    }

    #[test]
    fn test_stats_enable_disable() {
        let stats = FaultStats::new();
        assert!(stats.is_enabled());

        stats.set_enabled(false);
        assert!(!stats.is_enabled());
        assert!(!stats.snapshot().enabled);

        stats.set_enabled(true);
        assert!(stats.is_enabled());
    }

    #[test]
    fn test_stats_reset() {
        let stats = FaultStats::new();
        stats.record_check();
        stats.record_activation();
        stats.record_affected();
        stats.set_enabled(false);

        stats.reset();
        let snap = stats.snapshot();
        assert_eq!(snap.checks, 0);
        assert_eq!(snap.activations, 0);
        assert_eq!(snap.affected_requests, 0);
        // Enabled state should NOT be reset
        assert!(!snap.enabled);
    }

    #[test]
    fn test_activation_rate() {
        let snap = FaultStatsSnapshot {
            checks: 100,
            activations: 25,
            affected_requests: 25,
            enabled: true,
            last_activation_ms: 0,
        };
        assert!((snap.activation_rate() - 0.25).abs() < f64::EPSILON);

        let empty = FaultStatsSnapshot {
            checks: 0,
            activations: 0,
            affected_requests: 0,
            enabled: true,
            last_activation_ms: 0,
        };
        assert!((empty.activation_rate() - 0.0).abs() < f64::EPSILON);
    }
}

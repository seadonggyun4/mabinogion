//! Fault targeting and activation control.
//!
//! Provides fine-grained control over which requests trigger fault injection,
//! based on unit ID, function code, and probability.

use rand::Rng;
use serde::{Deserialize, Serialize};

/// Specifies which Modbus requests a fault should target.
///
/// All filters are AND-combined: a request must match all configured filters
/// to be eligible. Probability is checked last (after all filters pass).
///
/// # Examples
///
/// Target all requests with 50% probability:
/// ```rust,ignore
/// FaultTarget::new().with_probability(0.5)
/// ```
///
/// Target only unit ID 1, FC 0x03 with 100% probability:
/// ```rust,ignore
/// FaultTarget::new()
///     .with_unit_ids(vec![1])
///     .with_function_codes(vec![0x03])
/// ```
#[derive(Debug, Serialize, Deserialize)]
pub struct FaultTarget {
    /// Unit IDs to target. Empty = all units.
    #[serde(default)]
    pub unit_ids: Vec<u8>,

    /// Function codes to target. Empty = all function codes.
    #[serde(default)]
    pub function_codes: Vec<u8>,

    /// Activation probability (0.0 = never, 1.0 = always).
    #[serde(default = "default_probability")]
    pub probability: f64,

    /// If set, only activate after this many checks have occurred.
    /// Useful for "activate on Nth request" patterns.
    #[serde(default)]
    pub skip_count: u64,

    /// If set, limit the total number of activations.
    /// After this many activations, the fault will stop firing.
    #[serde(default)]
    pub max_activations: Option<u64>,

    /// Internal counter for skip tracking (not serialized).
    #[serde(skip)]
    check_counter: std::sync::atomic::AtomicU64,

    /// Internal counter for activation tracking (not serialized).
    #[serde(skip)]
    activation_counter: std::sync::atomic::AtomicU64,
}

impl Clone for FaultTarget {
    fn clone(&self) -> Self {
        Self {
            unit_ids: self.unit_ids.clone(),
            function_codes: self.function_codes.clone(),
            probability: self.probability,
            skip_count: self.skip_count,
            max_activations: self.max_activations,
            // Reset counters on clone (fresh copy)
            check_counter: std::sync::atomic::AtomicU64::new(0),
            activation_counter: std::sync::atomic::AtomicU64::new(0),
        }
    }
}

fn default_probability() -> f64 {
    1.0
}

impl FaultTarget {
    /// Create a new target that matches all requests with 100% probability.
    pub fn new() -> Self {
        Self {
            unit_ids: Vec::new(),
            function_codes: Vec::new(),
            probability: 1.0,
            skip_count: 0,
            max_activations: None,
            check_counter: std::sync::atomic::AtomicU64::new(0),
            activation_counter: std::sync::atomic::AtomicU64::new(0),
        }
    }

    /// Set target unit IDs (empty = all).
    pub fn with_unit_ids(mut self, ids: Vec<u8>) -> Self {
        self.unit_ids = ids;
        self
    }

    /// Set target function codes (empty = all).
    pub fn with_function_codes(mut self, codes: Vec<u8>) -> Self {
        self.function_codes = codes;
        self
    }

    /// Set activation probability (0.0 to 1.0).
    pub fn with_probability(mut self, p: f64) -> Self {
        self.probability = p.clamp(0.0, 1.0);
        self
    }

    /// Skip the first N checks before activating.
    pub fn with_skip_count(mut self, n: u64) -> Self {
        self.skip_count = n;
        self
    }

    /// Limit total activations.
    pub fn with_max_activations(mut self, n: u64) -> Self {
        self.max_activations = Some(n);
        self
    }

    /// Check if a request matches this target and should activate.
    ///
    /// Evaluates all filters in short-circuit order (cheapest first):
    /// 1. Unit ID filter
    /// 2. Function code filter
    /// 3. Skip count
    /// 4. Max activations
    /// 5. Probability roll
    pub fn should_activate(&self, unit_id: u8, function_code: u8) -> bool {
        // Unit ID filter
        if !self.unit_ids.is_empty() && !self.unit_ids.contains(&unit_id) {
            return false;
        }

        // Function code filter
        if !self.function_codes.is_empty() && !self.function_codes.contains(&function_code) {
            return false;
        }

        // Skip count
        let check_num = self
            .check_counter
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        if check_num < self.skip_count {
            return false;
        }

        // Max activations
        if let Some(max) = self.max_activations {
            let current = self
                .activation_counter
                .load(std::sync::atomic::Ordering::Acquire);
            if current >= max {
                return false;
            }
        }

        // Probability
        let activated = if (self.probability - 1.0).abs() < f64::EPSILON {
            true
        } else if self.probability <= 0.0 {
            false
        } else {
            let mut rng = rand::thread_rng();
            rng.gen::<f64>() < self.probability
        };

        if activated {
            self.activation_counter
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }

        activated
    }

    /// Reset internal counters.
    pub fn reset_counters(&self) {
        self.check_counter
            .store(0, std::sync::atomic::Ordering::Release);
        self.activation_counter
            .store(0, std::sync::atomic::Ordering::Release);
    }

    /// Get current activation count.
    pub fn activation_count(&self) -> u64 {
        self.activation_counter
            .load(std::sync::atomic::Ordering::Acquire)
    }

    /// Get current check count.
    pub fn check_count(&self) -> u64 {
        self.check_counter
            .load(std::sync::atomic::Ordering::Acquire)
    }
}

impl Default for FaultTarget {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_matches_all() {
        let target = FaultTarget::new();
        assert!(target.should_activate(1, 0x03));
        assert!(target.should_activate(255, 0x10));
        assert!(target.should_activate(0, 0x01));
    }

    #[test]
    fn test_unit_id_filter() {
        let target = FaultTarget::new().with_unit_ids(vec![1, 2]);
        assert!(target.should_activate(1, 0x03));
        assert!(target.should_activate(2, 0x03));
        assert!(!target.should_activate(3, 0x03));
    }

    #[test]
    fn test_function_code_filter() {
        let target = FaultTarget::new().with_function_codes(vec![0x03, 0x04]);
        assert!(target.should_activate(1, 0x03));
        assert!(target.should_activate(1, 0x04));
        assert!(!target.should_activate(1, 0x10));
    }

    #[test]
    fn test_combined_filters() {
        let target = FaultTarget::new()
            .with_unit_ids(vec![1])
            .with_function_codes(vec![0x03]);
        assert!(target.should_activate(1, 0x03));
        assert!(!target.should_activate(2, 0x03));
        assert!(!target.should_activate(1, 0x04));
        assert!(!target.should_activate(2, 0x04));
    }

    #[test]
    fn test_zero_probability() {
        let target = FaultTarget::new().with_probability(0.0);
        for _ in 0..100 {
            assert!(!target.should_activate(1, 0x03));
        }
    }

    #[test]
    fn test_full_probability() {
        let target = FaultTarget::new().with_probability(1.0);
        for _ in 0..100 {
            assert!(target.should_activate(1, 0x03));
        }
    }

    #[test]
    fn test_skip_count() {
        let target = FaultTarget::new().with_skip_count(3);
        assert!(!target.should_activate(1, 0x03)); // check 0 - skip
        assert!(!target.should_activate(1, 0x03)); // check 1 - skip
        assert!(!target.should_activate(1, 0x03)); // check 2 - skip
        assert!(target.should_activate(1, 0x03)); // check 3 - activate
        assert!(target.should_activate(1, 0x03)); // check 4 - activate
    }

    #[test]
    fn test_max_activations() {
        let target = FaultTarget::new().with_max_activations(2);
        assert!(target.should_activate(1, 0x03)); // activation 1
        assert!(target.should_activate(1, 0x03)); // activation 2
        assert!(!target.should_activate(1, 0x03)); // exceeded max
        assert!(!target.should_activate(1, 0x03)); // still exceeded
    }

    #[test]
    fn test_reset_counters() {
        let target = FaultTarget::new()
            .with_skip_count(1)
            .with_max_activations(1);
        assert!(!target.should_activate(1, 0x03)); // skip
        assert!(target.should_activate(1, 0x03)); // activate
        assert!(!target.should_activate(1, 0x03)); // max reached

        target.reset_counters();
        assert!(!target.should_activate(1, 0x03)); // skip again
        assert!(target.should_activate(1, 0x03)); // activate again
    }

    #[test]
    fn test_probability_clamp() {
        let target = FaultTarget::new().with_probability(1.5);
        assert!((target.probability - 1.0).abs() < f64::EPSILON);

        let target = FaultTarget::new().with_probability(-0.5);
        assert!((target.probability - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_counter_tracking() {
        let target = FaultTarget::new().with_skip_count(2);
        assert_eq!(target.check_count(), 0);
        assert_eq!(target.activation_count(), 0);

        target.should_activate(1, 0x03); // skip
        target.should_activate(1, 0x03); // skip
        assert_eq!(target.check_count(), 2);
        assert_eq!(target.activation_count(), 0);

        target.should_activate(1, 0x03); // activate
        assert_eq!(target.check_count(), 3);
        assert_eq!(target.activation_count(), 1);
    }
}

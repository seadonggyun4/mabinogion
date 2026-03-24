//! Memory simulation patterns.
//!
//! Defines various memory allocation patterns to test different scenarios.

use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::profiling::Profiler;

/// Memory allocation pattern for simulation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MemoryPattern {
    /// Steady state - no significant changes.
    Steady,

    /// Continuous memory growth (simulates leak).
    GrowthOnly,

    /// Growth followed by periodic release.
    GrowthAndRelease,

    /// High allocation/deallocation churn.
    HighChurn,

    /// Memory fragmentation pattern.
    Fragmentation,

    /// Custom pattern.
    #[serde(skip)]
    Custom(Arc<dyn CustomMemoryPattern>),
}

impl Default for MemoryPattern {
    fn default() -> Self {
        Self::Steady
    }
}

impl MemoryPattern {
    /// Get a description of this pattern.
    pub fn description(&self) -> &str {
        match self {
            Self::Steady => "Steady state with minimal memory changes",
            Self::GrowthOnly => "Continuous growth simulating memory leak",
            Self::GrowthAndRelease => "Periodic growth and release cycles",
            Self::HighChurn => "High allocation/deallocation frequency",
            Self::Fragmentation => "Varied sizes causing fragmentation",
            Self::Custom(_) => "Custom memory pattern",
        }
    }

    /// Apply the pattern to the profiler.
    pub fn apply(&self, profiler: &Profiler, elapsed: Duration) {
        match self {
            Self::Custom(pattern) => pattern.apply(profiler, elapsed),
            _ => {} // Handled in Simulator
        }
    }
}

/// Trait for custom memory patterns.
pub trait CustomMemoryPattern: Send + Sync + std::fmt::Debug {
    /// Apply the pattern for the given elapsed time.
    fn apply(&self, profiler: &Profiler, elapsed: Duration);

    /// Get a description.
    fn description(&self) -> &str;
}

/// Sawtooth memory pattern - grows then drops.
#[derive(Debug, Clone)]
pub struct SawtoothPattern {
    /// Period of each cycle.
    pub period: Duration,
    /// Peak allocation per cycle.
    pub peak_bytes: usize,
    /// Region name.
    pub region: String,
}

impl Default for SawtoothPattern {
    fn default() -> Self {
        Self {
            period: Duration::from_secs(30),
            peak_bytes: 1024 * 1024, // 1MB
            region: "sawtooth".into(),
        }
    }
}

impl CustomMemoryPattern for SawtoothPattern {
    fn apply(&self, profiler: &Profiler, elapsed: Duration) {
        let cycle_pos =
            (elapsed.as_millis() % self.period.as_millis()) as f64 / self.period.as_millis() as f64;

        if cycle_pos < 0.9 {
            // Growing phase (90% of cycle)
            let growth = (self.peak_bytes as f64 * cycle_pos / 0.9) as usize;
            let increment = growth / 100;
            if increment > 0 {
                profiler.record_allocation(&self.region, increment);
            }
        } else {
            // Release phase (10% of cycle)
            let release_progress = (cycle_pos - 0.9) / 0.1;
            let release = (self.peak_bytes as f64 * release_progress) as usize;
            let decrement = release / 10;
            if decrement > 0 {
                profiler.record_deallocation(&self.region, decrement);
            }
        }
    }

    fn description(&self) -> &str {
        "Sawtooth pattern: gradual growth then rapid release"
    }
}

/// Stepped memory pattern - grows in discrete steps.
#[derive(Debug, Clone)]
pub struct SteppedPattern {
    /// Duration of each step.
    pub step_duration: Duration,
    /// Memory increment per step.
    pub step_bytes: usize,
    /// Maximum steps before reset.
    pub max_steps: usize,
    /// Region name.
    pub region: String,
}

impl Default for SteppedPattern {
    fn default() -> Self {
        Self {
            step_duration: Duration::from_secs(10),
            step_bytes: 256 * 1024, // 256KB
            max_steps: 10,
            region: "stepped".into(),
        }
    }
}

impl CustomMemoryPattern for SteppedPattern {
    fn apply(&self, profiler: &Profiler, elapsed: Duration) {
        let total_period = self.step_duration.as_millis() * self.max_steps as u128;
        let cycle_ms = elapsed.as_millis() % total_period;
        let current_step = (cycle_ms / self.step_duration.as_millis()) as usize;

        // Only allocate at step boundaries
        let step_boundary = cycle_ms % self.step_duration.as_millis();
        if step_boundary < 100 && current_step > 0 {
            if current_step == self.max_steps - 1 {
                // Reset at last step
                profiler.record_deallocation(&self.region, self.step_bytes * self.max_steps);
            } else {
                profiler.record_allocation(&self.region, self.step_bytes);
            }
        }
    }

    fn description(&self) -> &str {
        "Stepped pattern: discrete memory increments"
    }
}

/// Burst memory pattern - sudden spikes.
#[derive(Debug, Clone)]
pub struct BurstPattern {
    /// Time between bursts.
    pub burst_interval: Duration,
    /// Size of each burst.
    pub burst_size: usize,
    /// How long before burst is released.
    pub hold_duration: Duration,
    /// Region name.
    pub region: String,
}

impl Default for BurstPattern {
    fn default() -> Self {
        Self {
            burst_interval: Duration::from_secs(20),
            burst_size: 5 * 1024 * 1024, // 5MB
            hold_duration: Duration::from_secs(5),
            region: "burst".into(),
        }
    }
}

impl CustomMemoryPattern for BurstPattern {
    fn apply(&self, profiler: &Profiler, elapsed: Duration) {
        let cycle = elapsed.as_millis() % self.burst_interval.as_millis();

        if cycle < 100 {
            // Burst allocation at start of cycle
            profiler.record_allocation(&self.region, self.burst_size);
        } else if cycle >= self.hold_duration.as_millis()
            && cycle < self.hold_duration.as_millis() + 100
        {
            // Release after hold duration
            profiler.record_deallocation(&self.region, self.burst_size);
        }
    }

    fn description(&self) -> &str {
        "Burst pattern: sudden spikes held briefly"
    }
}

/// Memory leak simulation pattern.
#[derive(Debug, Clone)]
pub struct LeakPattern {
    /// Bytes leaked per second.
    pub leak_rate_per_sec: usize,
    /// Probability of leak each tick.
    pub leak_probability: f64,
    /// Region name.
    pub region: String,
    /// Whether to occasionally "fix" the leak (for testing detection).
    pub sporadic_fix: bool,
}

impl Default for LeakPattern {
    fn default() -> Self {
        Self {
            leak_rate_per_sec: 10 * 1024, // 10KB/s
            leak_probability: 0.1,
            region: "leak".into(),
            sporadic_fix: false,
        }
    }
}

impl CustomMemoryPattern for LeakPattern {
    fn apply(&self, profiler: &Profiler, elapsed: Duration) {
        // Calculate expected leaked bytes based on elapsed time
        let expected_leaked = (elapsed.as_secs_f64() * self.leak_rate_per_sec as f64) as usize;

        // Leak incrementally (small amounts frequently)
        let leak_amount = self.leak_rate_per_sec / 100; // Per tick (~10ms)
        if leak_amount > 0 {
            profiler.record_allocation(&self.region, leak_amount);
        }

        // Sporadic "fix" to test leak detector's ability to detect trends
        if self.sporadic_fix && elapsed.as_secs() % 60 == 30 {
            profiler.record_deallocation(&self.region, expected_leaked / 4);
        }
    }

    fn description(&self) -> &str {
        "Leak pattern: gradual memory leak simulation"
    }
}

/// Factory for creating memory patterns.
pub struct MemoryPatternFactory;

impl MemoryPatternFactory {
    /// Create a steady pattern.
    pub fn steady() -> MemoryPattern {
        MemoryPattern::Steady
    }

    /// Create a growth-only pattern (leak simulation).
    pub fn growth_only() -> MemoryPattern {
        MemoryPattern::GrowthOnly
    }

    /// Create a sawtooth pattern.
    pub fn sawtooth(period: Duration, peak_bytes: usize) -> MemoryPattern {
        MemoryPattern::Custom(Arc::new(SawtoothPattern {
            period,
            peak_bytes,
            region: "sawtooth".into(),
        }))
    }

    /// Create a stepped pattern.
    pub fn stepped(step_duration: Duration, step_bytes: usize, max_steps: usize) -> MemoryPattern {
        MemoryPattern::Custom(Arc::new(SteppedPattern {
            step_duration,
            step_bytes,
            max_steps,
            region: "stepped".into(),
        }))
    }

    /// Create a burst pattern.
    pub fn burst(interval: Duration, size: usize, hold: Duration) -> MemoryPattern {
        MemoryPattern::Custom(Arc::new(BurstPattern {
            burst_interval: interval,
            burst_size: size,
            hold_duration: hold,
            region: "burst".into(),
        }))
    }

    /// Create a leak pattern.
    pub fn leak(rate_per_sec: usize) -> MemoryPattern {
        MemoryPattern::Custom(Arc::new(LeakPattern {
            leak_rate_per_sec: rate_per_sec,
            ..Default::default()
        }))
    }

    /// Create a combined pattern.
    pub fn combined(patterns: Vec<MemoryPattern>) -> MemoryPattern {
        MemoryPattern::Custom(Arc::new(CombinedPattern { patterns }))
    }
}

/// Combined pattern that applies multiple patterns.
#[derive(Debug)]
struct CombinedPattern {
    patterns: Vec<MemoryPattern>,
}

impl CustomMemoryPattern for CombinedPattern {
    fn apply(&self, profiler: &Profiler, elapsed: Duration) {
        for pattern in &self.patterns {
            pattern.apply(profiler, elapsed);
        }
    }

    fn description(&self) -> &str {
        "Combined pattern: multiple patterns applied together"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profiling::ProfilerConfig;

    #[test]
    fn test_memory_pattern_descriptions() {
        assert!(!MemoryPattern::Steady.description().is_empty());
        assert!(!MemoryPattern::GrowthOnly.description().is_empty());
        assert!(!MemoryPattern::HighChurn.description().is_empty());
    }

    #[test]
    fn test_sawtooth_pattern() {
        let profiler = Profiler::new(ProfilerConfig::default());
        profiler.start();

        let pattern = SawtoothPattern::default();
        pattern.apply(&profiler, Duration::from_secs(5));

        // Should have some allocations
        let snapshot = profiler.snapshot();
        assert!(snapshot.allocation_count > 0 || snapshot.current_bytes > 0);
    }

    #[test]
    fn test_stepped_pattern() {
        let profiler = Profiler::new(ProfilerConfig::default());
        profiler.start();

        let pattern = SteppedPattern::default();

        // Apply at different times
        pattern.apply(&profiler, Duration::from_secs(0));
        pattern.apply(&profiler, Duration::from_secs(10));
        pattern.apply(&profiler, Duration::from_secs(20));
    }

    #[test]
    fn test_burst_pattern() {
        let profiler = Profiler::new(ProfilerConfig::default());
        profiler.start();

        let pattern = BurstPattern {
            burst_interval: Duration::from_secs(10),
            burst_size: 1024,
            hold_duration: Duration::from_secs(2),
            region: "test".into(),
        };

        // At start of cycle - should allocate
        pattern.apply(&profiler, Duration::from_millis(50));
        let snapshot1 = profiler.snapshot();

        // After hold - should deallocate
        pattern.apply(&profiler, Duration::from_millis(2050));
        let _snapshot2 = profiler.snapshot();

        assert!(snapshot1.allocation_count > 0);
    }

    #[test]
    fn test_leak_pattern() {
        let profiler = Profiler::new(ProfilerConfig::default());
        profiler.start();

        let pattern = LeakPattern {
            leak_rate_per_sec: 1024,
            leak_probability: 1.0,
            region: "test_leak".into(),
            sporadic_fix: false,
        };

        // Apply multiple times
        for i in 0..10 {
            pattern.apply(&profiler, Duration::from_millis(i * 100));
        }

        let snapshot = profiler.snapshot();
        assert!(snapshot.allocation_count > 0);
    }

    #[test]
    fn test_pattern_factory() {
        let _steady = MemoryPatternFactory::steady();
        let _growth = MemoryPatternFactory::growth_only();
        let _sawtooth = MemoryPatternFactory::sawtooth(Duration::from_secs(10), 1024);
        let _stepped = MemoryPatternFactory::stepped(Duration::from_secs(5), 512, 5);
        let _burst =
            MemoryPatternFactory::burst(Duration::from_secs(20), 4096, Duration::from_secs(3));
        let _leak = MemoryPatternFactory::leak(1024);
    }

    #[test]
    fn test_combined_pattern() {
        let profiler = Profiler::new(ProfilerConfig::default());
        profiler.start();

        let pattern = MemoryPatternFactory::combined(vec![
            MemoryPattern::HighChurn,
            MemoryPatternFactory::leak(512),
        ]);

        pattern.apply(&profiler, Duration::from_secs(1));
    }
}

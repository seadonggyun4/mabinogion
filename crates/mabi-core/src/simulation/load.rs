//! Load patterns for simulation.
//!
//! Defines various load profiles to test system performance under different conditions.

use std::time::Duration;

use serde::{Deserialize, Serialize};

/// Load pattern for simulations.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum LoadPattern {
    /// Constant load.
    Steady {
        /// Operations per second.
        ops_per_sec: u64,
    },

    /// Linear ramp from start to end.
    Ramp {
        /// Starting operations per second.
        start_ops: u64,
        /// Ending operations per second.
        end_ops: u64,
        /// Duration of the ramp.
        duration: Duration,
    },

    /// Step function (sudden changes).
    Step {
        /// Initial level.
        initial: u64,
        /// Steps as (time_offset, new_level) pairs.
        steps: Vec<(Duration, u64)>,
    },

    /// Sinusoidal wave pattern.
    Wave {
        /// Base operations per second.
        baseline: u64,
        /// Amplitude of the wave.
        amplitude: u64,
        /// Period of one complete cycle.
        period: Duration,
    },

    /// Sudden spike pattern.
    Spike {
        /// Normal operations per second.
        baseline: u64,
        /// Peak operations per second.
        peak: u64,
        /// Duration of the spike.
        spike_duration: Duration,
    },

    /// Burst pattern (periodic high load).
    Burst {
        /// Normal operations per second.
        baseline: u64,
        /// Burst operations per second.
        burst_ops: u64,
        /// Duration of each burst.
        burst_duration: Duration,
        /// Time between bursts.
        interval: Duration,
    },

    /// Random load within bounds.
    Random {
        /// Minimum operations per second.
        min_ops: u64,
        /// Maximum operations per second.
        max_ops: u64,
    },

    /// Realistic daily traffic pattern.
    DailyTraffic {
        /// Peak operations per second.
        peak: u64,
        /// Off-peak operations per second.
        off_peak: u64,
        /// Simulated day duration.
        day_duration: Duration,
    },
}

impl Default for LoadPattern {
    fn default() -> Self {
        Self::Steady { ops_per_sec: 1000 }
    }
}

impl LoadPattern {
    /// Calculate operations per second for the given elapsed time.
    pub fn ops_for_elapsed(&self, elapsed: Duration) -> u64 {
        match self {
            Self::Steady { ops_per_sec } => *ops_per_sec,

            Self::Ramp {
                start_ops,
                end_ops,
                duration,
            } => {
                let progress = (elapsed.as_secs_f64() / duration.as_secs_f64()).min(1.0);
                let diff = *end_ops as f64 - *start_ops as f64;
                (*start_ops as f64 + diff * progress) as u64
            }

            Self::Step { initial, steps } => {
                let mut current = *initial;
                for (time, level) in steps {
                    if elapsed >= *time {
                        current = *level;
                    }
                }
                current
            }

            Self::Wave {
                baseline,
                amplitude,
                period,
            } => {
                let phase = (elapsed.as_secs_f64() / period.as_secs_f64()) * 2.0 * std::f64::consts::PI;
                let wave = phase.sin();
                (*baseline as f64 + *amplitude as f64 * wave) as u64
            }

            Self::Spike {
                baseline,
                peak,
                spike_duration,
            } => {
                // Spike at the midpoint of the simulation
                // For now, spike if within first spike_duration
                if elapsed < *spike_duration {
                    *peak
                } else {
                    *baseline
                }
            }

            Self::Burst {
                baseline,
                burst_ops,
                burst_duration,
                interval,
            } => {
                let cycle = elapsed.as_millis() % interval.as_millis();
                if cycle < burst_duration.as_millis() {
                    *burst_ops
                } else {
                    *baseline
                }
            }

            Self::Random { min_ops, max_ops } => {
                // Use elapsed time as pseudo-random seed
                let seed = elapsed.as_micros() as u64;
                let range = *max_ops - *min_ops;
                *min_ops + (seed % (range + 1))
            }

            Self::DailyTraffic {
                peak,
                off_peak,
                day_duration,
            } => {
                // Simulate 24-hour traffic pattern
                let day_progress = (elapsed.as_secs_f64() % day_duration.as_secs_f64())
                    / day_duration.as_secs_f64();

                // Peak around 0.5 (noon), low at 0.0 and 1.0 (midnight)
                let hour = day_progress * 24.0;
                let traffic_factor = if hour >= 8.0 && hour <= 20.0 {
                    // Business hours
                    let peak_at_noon = 1.0 - ((hour - 14.0).abs() / 6.0);
                    0.5 + 0.5 * peak_at_noon
                } else {
                    // Off hours
                    0.2
                };

                let range = *peak - *off_peak;
                (*off_peak as f64 + range as f64 * traffic_factor) as u64
            }
        }
    }

    /// Get a description of this pattern.
    pub fn description(&self) -> String {
        match self {
            Self::Steady { ops_per_sec } => format!("Steady {} ops/s", ops_per_sec),
            Self::Ramp {
                start_ops,
                end_ops,
                duration,
            } => format!(
                "Ramp {} -> {} ops/s over {:?}",
                start_ops, end_ops, duration
            ),
            Self::Step { initial, steps } => {
                format!("Step pattern starting at {} with {} steps", initial, steps.len())
            }
            Self::Wave {
                baseline,
                amplitude,
                period,
            } => format!(
                "Wave {}±{} ops/s, period {:?}",
                baseline, amplitude, period
            ),
            Self::Spike {
                baseline,
                peak,
                spike_duration,
            } => format!(
                "Spike {} -> {} ops/s for {:?}",
                baseline, peak, spike_duration
            ),
            Self::Burst {
                baseline,
                burst_ops,
                burst_duration,
                interval,
            } => format!(
                "Burst {}->{} ops/s for {:?} every {:?}",
                baseline, burst_ops, burst_duration, interval
            ),
            Self::Random { min_ops, max_ops } => format!("Random {}-{} ops/s", min_ops, max_ops),
            Self::DailyTraffic { peak, off_peak, .. } => {
                format!("Daily traffic {}-{} ops/s", off_peak, peak)
            }
        }
    }

    /// Create common load patterns.
    pub fn steady(ops_per_sec: u64) -> Self {
        Self::Steady { ops_per_sec }
    }

    /// Create a ramp pattern.
    pub fn ramp(start: u64, end: u64, duration: Duration) -> Self {
        Self::Ramp {
            start_ops: start,
            end_ops: end,
            duration,
        }
    }

    /// Create a wave pattern.
    pub fn wave(baseline: u64, amplitude: u64, period: Duration) -> Self {
        Self::Wave {
            baseline,
            amplitude,
            period,
        }
    }

    /// Create a spike pattern.
    pub fn spike(baseline: u64, peak: u64, spike_duration: Duration) -> Self {
        Self::Spike {
            baseline,
            peak,
            spike_duration,
        }
    }

    /// Create a burst pattern.
    pub fn burst(
        baseline: u64,
        burst_ops: u64,
        burst_duration: Duration,
        interval: Duration,
    ) -> Self {
        Self::Burst {
            baseline,
            burst_ops,
            burst_duration,
            interval,
        }
    }
}

/// Load pattern builder for complex patterns.
#[derive(Default)]
pub struct LoadPatternBuilder {
    steps: Vec<(Duration, u64)>,
    initial: u64,
}

impl LoadPatternBuilder {
    /// Create a new builder.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the initial load.
    pub fn initial(mut self, ops: u64) -> Self {
        self.initial = ops;
        self
    }

    /// Add a step at a specific time.
    pub fn step_at(mut self, time: Duration, ops: u64) -> Self {
        self.steps.push((time, ops));
        self
    }

    /// Build the step pattern.
    pub fn build_step(self) -> LoadPattern {
        LoadPattern::Step {
            initial: self.initial,
            steps: self.steps,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_steady_pattern() {
        let pattern = LoadPattern::steady(1000);

        assert_eq!(pattern.ops_for_elapsed(Duration::from_secs(0)), 1000);
        assert_eq!(pattern.ops_for_elapsed(Duration::from_secs(100)), 1000);
    }

    #[test]
    fn test_ramp_pattern() {
        let pattern = LoadPattern::ramp(100, 1000, Duration::from_secs(10));

        assert_eq!(pattern.ops_for_elapsed(Duration::from_secs(0)), 100);
        assert_eq!(pattern.ops_for_elapsed(Duration::from_secs(5)), 550);
        assert_eq!(pattern.ops_for_elapsed(Duration::from_secs(10)), 1000);
        assert_eq!(pattern.ops_for_elapsed(Duration::from_secs(20)), 1000); // Capped at end
    }

    #[test]
    fn test_step_pattern() {
        let pattern = LoadPattern::Step {
            initial: 100,
            steps: vec![
                (Duration::from_secs(5), 500),
                (Duration::from_secs(10), 1000),
            ],
        };

        assert_eq!(pattern.ops_for_elapsed(Duration::from_secs(0)), 100);
        assert_eq!(pattern.ops_for_elapsed(Duration::from_secs(3)), 100);
        assert_eq!(pattern.ops_for_elapsed(Duration::from_secs(5)), 500);
        assert_eq!(pattern.ops_for_elapsed(Duration::from_secs(7)), 500);
        assert_eq!(pattern.ops_for_elapsed(Duration::from_secs(10)), 1000);
    }

    #[test]
    fn test_wave_pattern() {
        let pattern = LoadPattern::wave(1000, 500, Duration::from_secs(10));

        let at_0 = pattern.ops_for_elapsed(Duration::from_secs(0));
        let at_quarter = pattern.ops_for_elapsed(Duration::from_millis(2500));
        let at_half = pattern.ops_for_elapsed(Duration::from_secs(5));

        // At 0: sin(0) = 0, so baseline
        assert_eq!(at_0, 1000);

        // At quarter period: sin(π/2) = 1, so baseline + amplitude
        assert!((at_quarter as i64 - 1500).abs() < 10);

        // At half period: sin(π) = 0, so baseline
        assert!((at_half as i64 - 1000).abs() < 10);
    }

    #[test]
    fn test_spike_pattern() {
        let pattern = LoadPattern::spike(100, 10000, Duration::from_secs(5));

        assert_eq!(pattern.ops_for_elapsed(Duration::from_secs(0)), 10000); // During spike
        assert_eq!(pattern.ops_for_elapsed(Duration::from_secs(3)), 10000); // During spike
        assert_eq!(pattern.ops_for_elapsed(Duration::from_secs(6)), 100); // After spike
    }

    #[test]
    fn test_burst_pattern() {
        let pattern = LoadPattern::burst(
            100,
            5000,
            Duration::from_secs(2),
            Duration::from_secs(10),
        );

        // During burst (0-2s in each 10s cycle)
        assert_eq!(pattern.ops_for_elapsed(Duration::from_secs(0)), 5000);
        assert_eq!(pattern.ops_for_elapsed(Duration::from_secs(1)), 5000);

        // After burst (2-10s)
        assert_eq!(pattern.ops_for_elapsed(Duration::from_secs(3)), 100);
        assert_eq!(pattern.ops_for_elapsed(Duration::from_secs(9)), 100);

        // Next cycle
        assert_eq!(pattern.ops_for_elapsed(Duration::from_secs(10)), 5000);
    }

    #[test]
    fn test_random_pattern() {
        let pattern = LoadPattern::Random {
            min_ops: 100,
            max_ops: 1000,
        };

        for i in 0..10 {
            let ops = pattern.ops_for_elapsed(Duration::from_secs(i));
            assert!(ops >= 100 && ops <= 1000);
        }
    }

    #[test]
    fn test_daily_traffic_pattern() {
        let pattern = LoadPattern::DailyTraffic {
            peak: 10000,
            off_peak: 1000,
            day_duration: Duration::from_secs(86400), // 24 hours
        };

        // Simulate different times of day
        let midnight = pattern.ops_for_elapsed(Duration::from_secs(0));
        let noon = pattern.ops_for_elapsed(Duration::from_secs(43200)); // 12 hours
        let evening = pattern.ops_for_elapsed(Duration::from_secs(72000)); // 20 hours

        // Midnight should be low
        assert!(midnight < 5000);

        // Noon should be high
        assert!(noon > 5000);

        // Evening declining
        assert!(evening < noon);
    }

    #[test]
    fn test_pattern_builder() {
        let pattern = LoadPatternBuilder::new()
            .initial(100)
            .step_at(Duration::from_secs(5), 500)
            .step_at(Duration::from_secs(10), 1000)
            .build_step();

        assert_eq!(pattern.ops_for_elapsed(Duration::from_secs(0)), 100);
        assert_eq!(pattern.ops_for_elapsed(Duration::from_secs(5)), 500);
        assert_eq!(pattern.ops_for_elapsed(Duration::from_secs(10)), 1000);
    }

    #[test]
    fn test_pattern_descriptions() {
        let patterns = vec![
            LoadPattern::steady(1000),
            LoadPattern::ramp(100, 1000, Duration::from_secs(10)),
            LoadPattern::wave(500, 200, Duration::from_secs(30)),
            LoadPattern::spike(100, 5000, Duration::from_secs(5)),
        ];

        for pattern in patterns {
            let desc = pattern.description();
            assert!(!desc.is_empty());
        }
    }
}

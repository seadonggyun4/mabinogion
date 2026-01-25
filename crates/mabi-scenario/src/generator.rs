//! Pattern generators for scenario simulation.

use rand::prelude::*;
use rand_distr::{Normal, Uniform};

use crate::schema::PatternConfig;

/// Pattern type enumeration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PatternType {
    Constant,
    Sine,
    Cosine,
    Ramp,
    Step,
    Random,
    Noise,
    Follow,
    Replay,
}

/// Pattern generator.
pub struct PatternGenerator {
    config: PatternConfig,
    rng: StdRng,
    start_time: std::time::Instant,
    last_value: f64,
}

impl PatternGenerator {
    /// Create a new pattern generator.
    pub fn new(config: PatternConfig) -> Self {
        Self {
            config,
            rng: StdRng::from_entropy(),
            start_time: std::time::Instant::now(),
            last_value: 0.0,
        }
    }

    /// Reset the generator.
    pub fn reset(&mut self) {
        self.start_time = std::time::Instant::now();
        self.last_value = 0.0;
    }

    /// Generate the next value.
    pub fn generate(&mut self) -> f64 {
        let elapsed = self.start_time.elapsed().as_secs_f64();
        self.generate_at(elapsed)
    }

    /// Generate value at specific time.
    pub fn generate_at(&mut self, time_secs: f64) -> f64 {
        let value = match &self.config {
            PatternConfig::Constant { value } => *value,

            PatternConfig::Sine {
                amplitude,
                offset,
                period_secs,
                phase,
            } => {
                let angle = (2.0 * std::f64::consts::PI * time_secs / period_secs) + phase;
                offset + amplitude * angle.sin()
            }

            PatternConfig::Cosine {
                amplitude,
                offset,
                period_secs,
                phase,
            } => {
                let angle = (2.0 * std::f64::consts::PI * time_secs / period_secs) + phase;
                offset + amplitude * angle.cos()
            }

            PatternConfig::Ramp {
                start,
                end,
                duration_secs,
                repeat,
            } => {
                let t = if *repeat {
                    (time_secs % duration_secs) / duration_secs
                } else {
                    (time_secs / duration_secs).min(1.0)
                };
                start + (end - start) * t
            }

            PatternConfig::Step {
                levels,
                step_duration_secs,
            } => {
                if levels.is_empty() {
                    0.0
                } else {
                    let total_duration = step_duration_secs * levels.len() as f64;
                    let t = time_secs % total_duration;
                    let index = (t / step_duration_secs) as usize;
                    levels[index.min(levels.len() - 1)]
                }
            }

            PatternConfig::Random {
                min,
                max,
                distribution,
            } => {
                match distribution.as_str() {
                    "uniform" => {
                        let dist = Uniform::new(*min, *max);
                        dist.sample(&mut self.rng)
                    }
                    "normal" | "gaussian" => {
                        let mean = (min + max) / 2.0;
                        let std_dev = (max - min) / 6.0; // 99.7% within range
                        let dist = Normal::new(mean, std_dev).unwrap();
                        dist.sample(&mut self.rng).clamp(*min, *max)
                    }
                    _ => {
                        let dist = Uniform::new(*min, *max);
                        dist.sample(&mut self.rng)
                    }
                }
            }

            PatternConfig::Noise { mean, std_dev } => {
                let dist = Normal::new(*mean, *std_dev).unwrap();
                dist.sample(&mut self.rng)
            }

            PatternConfig::Follow { offset, gain, .. } => {
                // Follow requires external source value
                self.last_value * gain + offset
            }

            PatternConfig::Replay { .. } => {
                // Replay requires external data
                self.last_value
            }
        };

        self.last_value = value;
        value
    }

    /// Set source value for Follow pattern.
    pub fn set_source_value(&mut self, value: f64) {
        self.last_value = value;
    }

    /// Get pattern type.
    pub fn pattern_type(&self) -> PatternType {
        match &self.config {
            PatternConfig::Constant { .. } => PatternType::Constant,
            PatternConfig::Sine { .. } => PatternType::Sine,
            PatternConfig::Cosine { .. } => PatternType::Cosine,
            PatternConfig::Ramp { .. } => PatternType::Ramp,
            PatternConfig::Step { .. } => PatternType::Step,
            PatternConfig::Random { .. } => PatternType::Random,
            PatternConfig::Noise { .. } => PatternType::Noise,
            PatternConfig::Follow { .. } => PatternType::Follow,
            PatternConfig::Replay { .. } => PatternType::Replay,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_constant_pattern() {
        let mut gen = PatternGenerator::new(PatternConfig::Constant { value: 42.0 });
        assert!((gen.generate() - 42.0).abs() < 0.001);
    }

    #[test]
    fn test_sine_pattern() {
        let mut gen = PatternGenerator::new(PatternConfig::Sine {
            amplitude: 10.0,
            offset: 20.0,
            period_secs: 4.0,
            phase: 0.0,
        });

        // At t=0, sin(0) = 0, so value = 20
        let v0 = gen.generate_at(0.0);
        assert!((v0 - 20.0).abs() < 0.001);

        // At t=1 (quarter period), sin(π/2) = 1, so value = 30
        let v1 = gen.generate_at(1.0);
        assert!((v1 - 30.0).abs() < 0.001);
    }

    #[test]
    fn test_ramp_pattern() {
        let mut gen = PatternGenerator::new(PatternConfig::Ramp {
            start: 0.0,
            end: 100.0,
            duration_secs: 10.0,
            repeat: false,
        });

        assert!((gen.generate_at(0.0) - 0.0).abs() < 0.001);
        assert!((gen.generate_at(5.0) - 50.0).abs() < 0.001);
        assert!((gen.generate_at(10.0) - 100.0).abs() < 0.001);
        assert!((gen.generate_at(15.0) - 100.0).abs() < 0.001); // Clamped
    }

    #[test]
    fn test_step_pattern() {
        let mut gen = PatternGenerator::new(PatternConfig::Step {
            levels: vec![10.0, 20.0, 30.0],
            step_duration_secs: 1.0,
        });

        assert!((gen.generate_at(0.5) - 10.0).abs() < 0.001);
        assert!((gen.generate_at(1.5) - 20.0).abs() < 0.001);
        assert!((gen.generate_at(2.5) - 30.0).abs() < 0.001);
    }

    #[test]
    fn test_random_pattern() {
        let mut gen = PatternGenerator::new(PatternConfig::Random {
            min: 0.0,
            max: 100.0,
            distribution: "uniform".to_string(),
        });

        for _ in 0..100 {
            let value = gen.generate();
            assert!(value >= 0.0 && value <= 100.0);
        }
    }
}

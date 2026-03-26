//! RS485/RTU timing violation fault injection.
//!
//! Simulates timing violations in Modbus RTU serial communication,
//! testing inter-frame delay detection, inter-character gap handling,
//! and bus collision scenarios in trap-modbus.

use std::time::Duration;

use rand::Rng;
use serde::{Deserialize, Serialize};

/// Configuration for RTU timing violation faults.
///
/// Modbus RTU relies on precise timing for frame detection:
/// - **Inter-frame delay**: 3.5 character times between frames
/// - **Inter-character timeout**: 1.5 character times within a frame
///
/// Violations of these timing constraints test the gateway's ability
/// to correctly detect frame boundaries and handle malformed timing.
///
/// # Timing Reference (at 9600 baud)
///
/// - 1 character time ≈ 1.042ms (11 bits per character)
/// - 1.5 character times ≈ 1.563ms (inter-char timeout)
/// - 3.5 character times ≈ 3.646ms (inter-frame delay)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RtuTimingFaultConfig {
    /// Respond faster than the required 3.5 character time inter-frame delay.
    /// If set, the response will be sent after this delay instead of the
    /// protocol-required minimum.
    ///
    /// Set to a duration less than 3.5 char times to violate the spec.
    /// A well-implemented client should still accept the response.
    #[serde(default)]
    pub violate_interframe_delay: Option<Duration>,

    /// Inject an inter-character gap within the response frame.
    /// This simulates a slow or interrupted serial transmission.
    #[serde(default)]
    pub inject_interchar_gap: Option<InterCharGapConfig>,

    /// Simulate bus collision (overlapping transmissions).
    #[serde(default)]
    pub bus_collision: Option<BusCollisionConfig>,

    /// Byte-level transmission jitter.
    /// Adds random delay between individual bytes.
    #[serde(default)]
    pub byte_jitter: Option<ByteJitterConfig>,
}

impl Default for RtuTimingFaultConfig {
    fn default() -> Self {
        Self {
            violate_interframe_delay: None,
            inject_interchar_gap: None,
            bus_collision: None,
            byte_jitter: None,
        }
    }
}

impl RtuTimingFaultConfig {
    /// Create a new default configuration (all timing faults disabled).
    pub fn new() -> Self {
        Self::default()
    }

    /// Set inter-frame delay violation.
    pub fn with_interframe_violation(mut self, delay: Duration) -> Self {
        self.violate_interframe_delay = Some(delay);
        self
    }

    /// Set inter-character gap injection.
    pub fn with_interchar_gap(mut self, config: InterCharGapConfig) -> Self {
        self.inject_interchar_gap = Some(config);
        self
    }

    /// Set bus collision simulation.
    pub fn with_bus_collision(mut self, config: BusCollisionConfig) -> Self {
        self.bus_collision = Some(config);
        self
    }

    /// Set byte jitter.
    pub fn with_byte_jitter(mut self, config: ByteJitterConfig) -> Self {
        self.byte_jitter = Some(config);
        self
    }

    /// Check if any timing faults are configured.
    pub fn is_active(&self) -> bool {
        self.violate_interframe_delay.is_some()
            || self.inject_interchar_gap.is_some()
            || self.bus_collision.is_some()
            || self.byte_jitter.is_some()
    }
}

/// Configuration for inter-character gap injection.
///
/// Injects a delay exceeding 1.5 character times within a response frame,
/// potentially causing the receiver to detect a frame boundary where there
/// shouldn't be one.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InterCharGapConfig {
    /// Position within the frame to inject the gap.
    pub position: GapPosition,

    /// Duration of the gap.
    /// Must exceed 1.5 character times to be a real violation.
    pub gap_duration: Duration,
}

impl InterCharGapConfig {
    /// Create a gap at a fixed byte offset.
    pub fn at_offset(offset: usize, duration: Duration) -> Self {
        Self {
            position: GapPosition::FixedOffset(offset),
            gap_duration: duration,
        }
    }

    /// Create a gap at a random position.
    pub fn random(duration: Duration) -> Self {
        Self {
            position: GapPosition::Random,
            gap_duration: duration,
        }
    }

    /// Create a gap after the function code byte.
    pub fn after_fc(duration: Duration) -> Self {
        Self {
            position: GapPosition::AfterFunctionCode,
            gap_duration: duration,
        }
    }

    /// Compute the byte offset where the gap should be injected.
    pub fn compute_offset(&self, frame_length: usize) -> usize {
        match self.position {
            GapPosition::FixedOffset(offset) => offset.min(frame_length.saturating_sub(1)),
            GapPosition::Random => {
                if frame_length <= 2 {
                    1
                } else {
                    let mut rng = rand::thread_rng();
                    rng.gen_range(1..frame_length - 1)
                }
            }
            GapPosition::AfterFunctionCode => {
                // Function code is at byte 1 (after unit_id)
                2.min(frame_length.saturating_sub(1))
            }
            GapPosition::BeforeCrc => {
                // CRC is last 2 bytes
                frame_length.saturating_sub(2)
            }
        }
    }
}

/// Where within the frame to inject the inter-character gap.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GapPosition {
    /// Fixed byte offset from the start of the frame.
    FixedOffset(usize),
    /// Random position within the frame.
    Random,
    /// After the function code byte (offset 2).
    AfterFunctionCode,
    /// Before the CRC bytes.
    BeforeCrc,
}

/// Configuration for bus collision simulation.
///
/// Simulates scenarios where two devices transmit simultaneously,
/// causing garbled data on the bus.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BusCollisionConfig {
    /// The collision mode.
    pub mode: CollisionMode,

    /// Random garbage bytes to inject (for Garbage mode).
    #[serde(default = "default_garbage_bytes")]
    pub garbage_byte_count: usize,

    /// Probability of collision (0.0 to 1.0).
    #[serde(default = "default_collision_probability")]
    pub probability: f64,
}

fn default_garbage_bytes() -> usize {
    8
}

fn default_collision_probability() -> f64 {
    0.1
}

impl BusCollisionConfig {
    /// Create a collision config with garbage injection.
    pub fn garbage(byte_count: usize, probability: f64) -> Self {
        Self {
            mode: CollisionMode::GarbageInject,
            garbage_byte_count: byte_count,
            probability: probability.clamp(0.0, 1.0),
        }
    }

    /// Create a collision config that overlaps with the response.
    pub fn overlap(probability: f64) -> Self {
        Self {
            mode: CollisionMode::OverlapResponse,
            garbage_byte_count: 0,
            probability: probability.clamp(0.0, 1.0),
        }
    }

    /// Check if the collision should trigger (probabilistic).
    pub fn should_collide(&self) -> bool {
        if (self.probability - 1.0).abs() < f64::EPSILON {
            return true;
        }
        if self.probability <= 0.0 {
            return false;
        }
        let mut rng = rand::thread_rng();
        rng.gen::<f64>() < self.probability
    }

    /// Generate garbage bytes for collision.
    pub fn generate_garbage(&self) -> Vec<u8> {
        let mut rng = rand::thread_rng();
        (0..self.garbage_byte_count)
            .map(|_| rng.gen::<u8>())
            .collect()
    }
}

/// Bus collision modes.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CollisionMode {
    /// Inject random garbage bytes before the response.
    GarbageInject,
    /// Send garbage bytes overlapping with the response start.
    OverlapResponse,
    /// Send a second (different) valid frame immediately after.
    EchoFrame,
}

/// Configuration for byte-level transmission jitter.
///
/// Adds small, random delays between individual bytes to simulate
/// slow or unreliable serial hardware.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ByteJitterConfig {
    /// Minimum delay between bytes.
    pub min_delay: Duration,

    /// Maximum delay between bytes.
    pub max_delay: Duration,

    /// Apply jitter to every Nth byte (1 = every byte).
    #[serde(default = "default_jitter_interval")]
    pub interval: usize,
}

fn default_jitter_interval() -> usize {
    1
}

impl ByteJitterConfig {
    /// Create a byte jitter config.
    pub fn new(min_delay: Duration, max_delay: Duration) -> Self {
        Self {
            min_delay,
            max_delay,
            interval: 1,
        }
    }

    /// Set the interval (apply jitter every Nth byte).
    pub fn with_interval(mut self, n: usize) -> Self {
        self.interval = n.max(1);
        self
    }

    /// Compute a random jitter delay.
    pub fn compute_delay(&self) -> Duration {
        if self.min_delay >= self.max_delay {
            return self.min_delay;
        }
        let mut rng = rand::thread_rng();
        let min_us = self.min_delay.as_micros() as u64;
        let max_us = self.max_delay.as_micros() as u64;
        Duration::from_micros(rng.gen_range(min_us..=max_us))
    }
}

/// Plan for transmitting a response with timing faults.
///
/// The server integration layer uses this to know how to time
/// the transmission of each byte or chunk of the response.
#[derive(Debug)]
pub struct TimingPlan {
    /// Segments of the response to send.
    pub segments: Vec<TimingSegment>,
}

/// A segment of the response with timing.
#[derive(Debug)]
pub struct TimingSegment {
    /// The bytes in this segment.
    pub data: Vec<u8>,

    /// Delay before sending this segment.
    pub delay_before: Duration,
}

impl RtuTimingFaultConfig {
    /// Build a timing plan for sending a response frame.
    ///
    /// Takes the complete wire-format frame bytes (including CRC) and
    /// returns a plan describing how to send them with timing faults.
    pub fn build_timing_plan(&self, frame_bytes: &[u8]) -> TimingPlan {
        let mut segments = Vec::new();

        // Start with the inter-frame delay override
        let initial_delay = self.violate_interframe_delay.unwrap_or(Duration::ZERO);

        // Check for bus collision (prepend garbage)
        if let Some(ref collision) = self.bus_collision {
            if collision.should_collide() {
                let garbage = collision.generate_garbage();
                if !garbage.is_empty() {
                    segments.push(TimingSegment {
                        data: garbage,
                        delay_before: initial_delay,
                    });
                    // Small gap between garbage and real frame
                    // (less than 3.5 char time to cause confusion)
                    let gap = Duration::from_micros(500);

                    // Handle inter-char gap injection within the real frame
                    self.add_frame_segments(&mut segments, frame_bytes, gap);
                    return TimingPlan { segments };
                }
            }
        }

        // Handle inter-character gap injection
        if let Some(ref gap_config) = self.inject_interchar_gap {
            let offset = gap_config.compute_offset(frame_bytes.len());

            if offset > 0 && offset < frame_bytes.len() {
                // First part: bytes before the gap
                segments.push(TimingSegment {
                    data: frame_bytes[..offset].to_vec(),
                    delay_before: initial_delay,
                });

                // Second part: bytes after the gap (with the inter-char violation)
                segments.push(TimingSegment {
                    data: frame_bytes[offset..].to_vec(),
                    delay_before: gap_config.gap_duration,
                });

                return TimingPlan { segments };
            }
        }

        // Handle byte jitter
        if let Some(ref jitter) = self.byte_jitter {
            let mut current_delay = initial_delay;
            for (i, &byte) in frame_bytes.iter().enumerate() {
                segments.push(TimingSegment {
                    data: vec![byte],
                    delay_before: current_delay,
                });
                current_delay = if jitter.interval > 0 && (i + 1) % jitter.interval == 0 {
                    jitter.compute_delay()
                } else {
                    Duration::ZERO
                };
            }
            return TimingPlan { segments };
        }

        // No complex timing: single segment
        segments.push(TimingSegment {
            data: frame_bytes.to_vec(),
            delay_before: initial_delay,
        });

        TimingPlan { segments }
    }

    /// Helper: add frame segments after collision garbage.
    fn add_frame_segments(
        &self,
        segments: &mut Vec<TimingSegment>,
        frame_bytes: &[u8],
        initial_delay: Duration,
    ) {
        if let Some(ref gap_config) = self.inject_interchar_gap {
            let offset = gap_config.compute_offset(frame_bytes.len());
            if offset > 0 && offset < frame_bytes.len() {
                segments.push(TimingSegment {
                    data: frame_bytes[..offset].to_vec(),
                    delay_before: initial_delay,
                });
                segments.push(TimingSegment {
                    data: frame_bytes[offset..].to_vec(),
                    delay_before: gap_config.gap_duration,
                });
                return;
            }
        }

        segments.push(TimingSegment {
            data: frame_bytes.to_vec(),
            delay_before: initial_delay,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = RtuTimingFaultConfig::default();
        assert!(!config.is_active());
    }

    #[test]
    fn test_interframe_violation() {
        let config =
            RtuTimingFaultConfig::new().with_interframe_violation(Duration::from_micros(500));

        assert!(config.is_active());

        let frame = vec![0x01, 0x03, 0x02, 0x00, 0x64, 0xB8, 0x44];
        let plan = config.build_timing_plan(&frame);

        assert_eq!(plan.segments.len(), 1);
        assert_eq!(plan.segments[0].delay_before, Duration::from_micros(500));
        assert_eq!(plan.segments[0].data, frame);
    }

    #[test]
    fn test_interchar_gap_after_fc() {
        let config = RtuTimingFaultConfig::new()
            .with_interchar_gap(InterCharGapConfig::after_fc(Duration::from_millis(5)));

        let frame = vec![0x01, 0x03, 0x02, 0x00, 0x64, 0xB8, 0x44];
        let plan = config.build_timing_plan(&frame);

        assert_eq!(plan.segments.len(), 2);
        assert_eq!(plan.segments[0].data, vec![0x01, 0x03]); // unit_id + FC
        assert_eq!(plan.segments[1].data, vec![0x02, 0x00, 0x64, 0xB8, 0x44]); // rest
        assert_eq!(plan.segments[1].delay_before, Duration::from_millis(5));
    }

    #[test]
    fn test_interchar_gap_fixed_offset() {
        let config = RtuTimingFaultConfig::new()
            .with_interchar_gap(InterCharGapConfig::at_offset(3, Duration::from_millis(2)));

        let frame = vec![0x01, 0x03, 0x02, 0x00, 0x64, 0xB8, 0x44];
        let plan = config.build_timing_plan(&frame);

        assert_eq!(plan.segments.len(), 2);
        assert_eq!(plan.segments[0].data.len(), 3);
        assert_eq!(plan.segments[1].data.len(), 4);
    }

    #[test]
    fn test_byte_jitter() {
        let config = RtuTimingFaultConfig::new().with_byte_jitter(ByteJitterConfig::new(
            Duration::from_micros(100),
            Duration::from_micros(500),
        ));

        let frame = vec![0x01, 0x03, 0x02];
        let plan = config.build_timing_plan(&frame);

        assert_eq!(plan.segments.len(), 3); // One segment per byte
        assert_eq!(plan.segments[0].data, vec![0x01]);
        assert_eq!(plan.segments[1].data, vec![0x03]);
        assert_eq!(plan.segments[2].data, vec![0x02]);
    }

    #[test]
    fn test_bus_collision_garbage() {
        let config =
            RtuTimingFaultConfig::new().with_bus_collision(BusCollisionConfig::garbage(4, 1.0)); // Always collide

        let frame = vec![0x01, 0x03, 0x02, 0x00, 0x64, 0xB8, 0x44];
        let plan = config.build_timing_plan(&frame);

        // Should have at least 2 segments: garbage + frame
        assert!(plan.segments.len() >= 2);
        assert_eq!(plan.segments[0].data.len(), 4); // 4 garbage bytes
    }

    #[test]
    fn test_bus_collision_no_trigger() {
        let config =
            RtuTimingFaultConfig::new().with_bus_collision(BusCollisionConfig::garbage(4, 0.0)); // Never collide

        let frame = vec![0x01, 0x03, 0x02];
        let plan = config.build_timing_plan(&frame);

        // No collision: single segment
        assert_eq!(plan.segments.len(), 1);
        assert_eq!(plan.segments[0].data, frame);
    }

    #[test]
    fn test_combined_interframe_and_gap() {
        let config = RtuTimingFaultConfig::new()
            .with_interframe_violation(Duration::from_micros(100))
            .with_interchar_gap(InterCharGapConfig::at_offset(2, Duration::from_millis(3)));

        let frame = vec![0x01, 0x03, 0x02, 0x00, 0x64];
        let plan = config.build_timing_plan(&frame);

        assert_eq!(plan.segments.len(), 2);
        // First segment should have the interframe delay override
        assert_eq!(plan.segments[0].delay_before, Duration::from_micros(100));
        // Second segment should have the interchar gap
        assert_eq!(plan.segments[1].delay_before, Duration::from_millis(3));
    }

    #[test]
    fn test_gap_position_computation() {
        let gap = InterCharGapConfig::at_offset(10, Duration::ZERO);
        // Offset exceeds frame length
        assert_eq!(gap.compute_offset(5), 4);

        let gap = InterCharGapConfig::after_fc(Duration::ZERO);
        assert_eq!(gap.compute_offset(10), 2);
        assert_eq!(gap.compute_offset(1), 0); // clamp to 0 for very short frames

        let gap = InterCharGapConfig {
            position: GapPosition::BeforeCrc,
            gap_duration: Duration::ZERO,
        };
        assert_eq!(gap.compute_offset(7), 5); // 7 - 2 = 5
    }

    #[test]
    fn test_byte_jitter_interval() {
        let jitter = ByteJitterConfig::new(Duration::from_micros(100), Duration::from_micros(100))
            .with_interval(2);

        let config = RtuTimingFaultConfig::new().with_byte_jitter(jitter);
        let frame = vec![0x01, 0x02, 0x03, 0x04];
        let plan = config.build_timing_plan(&frame);

        assert_eq!(plan.segments.len(), 4);
        // Byte 0: initial delay (0)
        // Byte 1: no jitter (not on interval)
        // Byte 2: jitter (after byte 1, index 1, interval 2)
        assert_eq!(plan.segments[2].delay_before, Duration::from_micros(100));
    }

    #[test]
    fn test_byte_jitter_delay_range() {
        let jitter = ByteJitterConfig::new(Duration::from_micros(100), Duration::from_micros(500));

        for _ in 0..100 {
            let delay = jitter.compute_delay();
            assert!(delay >= Duration::from_micros(100));
            assert!(delay <= Duration::from_micros(500));
        }
    }
}

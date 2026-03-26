//! Follow pattern implementation with source tracking and delay buffer.
//!
//! The Follow pattern allows a data point to follow another point's value
//! with optional transformations (gain, offset) and time delay.

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::RwLock;

/// Maximum delay buffer size.
const MAX_BUFFER_SIZE: usize = 10_000;

/// Timestamped value for delay buffer.
#[derive(Debug, Clone)]
pub struct TimestampedValue {
    /// The value.
    pub value: f64,
    /// When the value was recorded.
    pub timestamp: Instant,
}

impl TimestampedValue {
    /// Create a new timestamped value.
    pub fn new(value: f64) -> Self {
        Self {
            value,
            timestamp: Instant::now(),
        }
    }

    /// Create with explicit timestamp.
    pub fn with_timestamp(value: f64, timestamp: Instant) -> Self {
        Self { value, timestamp }
    }
}

/// Delay buffer for storing historical values.
#[derive(Debug)]
pub struct DelayBuffer {
    /// Circular buffer of values.
    buffer: VecDeque<TimestampedValue>,
    /// Maximum buffer size.
    max_size: usize,
    /// Target delay duration.
    delay: Duration,
}

impl DelayBuffer {
    /// Create a new delay buffer.
    pub fn new(delay: Duration) -> Self {
        Self {
            buffer: VecDeque::with_capacity(1024),
            max_size: MAX_BUFFER_SIZE,
            delay,
        }
    }

    /// Create with custom max size.
    pub fn with_max_size(delay: Duration, max_size: usize) -> Self {
        Self {
            buffer: VecDeque::with_capacity(max_size.min(1024)),
            max_size: max_size.min(MAX_BUFFER_SIZE),
            delay,
        }
    }

    /// Push a new value to the buffer.
    pub fn push(&mut self, value: f64) {
        self.push_with_timestamp(value, Instant::now());
    }

    /// Push a value with explicit timestamp.
    pub fn push_with_timestamp(&mut self, value: f64, timestamp: Instant) {
        // Evict old entries if buffer is full
        if self.buffer.len() >= self.max_size {
            self.buffer.pop_front();
        }

        self.buffer
            .push_back(TimestampedValue::with_timestamp(value, timestamp));
    }

    /// Get the delayed value with interpolation.
    pub fn get_delayed(&self) -> Option<f64> {
        self.get_delayed_at(Instant::now())
    }

    /// Get the delayed value at a specific time.
    pub fn get_delayed_at(&self, now: Instant) -> Option<f64> {
        if self.buffer.is_empty() {
            return None;
        }

        let target_time = now.checked_sub(self.delay)?;

        // Find the two values surrounding the target time for interpolation
        let mut before: Option<&TimestampedValue> = None;
        let mut after: Option<&TimestampedValue> = None;

        for entry in &self.buffer {
            if entry.timestamp <= target_time {
                before = Some(entry);
            } else {
                after = Some(entry);
                break;
            }
        }

        match (before, after) {
            (Some(b), Some(a)) => {
                // Linear interpolation
                let time_span = a.timestamp.duration_since(b.timestamp).as_secs_f64();
                if time_span < f64::EPSILON {
                    Some(b.value)
                } else {
                    let t = target_time.duration_since(b.timestamp).as_secs_f64() / time_span;
                    Some(b.value + (a.value - b.value) * t)
                }
            }
            (Some(b), None) => {
                // Use last known value before target
                Some(b.value)
            }
            (None, Some(a)) => {
                // Target is before first entry, use first value
                Some(a.value)
            }
            (None, None) => None,
        }
    }

    /// Clear the buffer.
    pub fn clear(&mut self) {
        self.buffer.clear();
    }

    /// Get buffer length.
    pub fn len(&self) -> usize {
        self.buffer.len()
    }

    /// Check if buffer is empty.
    pub fn is_empty(&self) -> bool {
        self.buffer.is_empty()
    }

    /// Get the delay duration.
    pub fn delay(&self) -> Duration {
        self.delay
    }

    /// Set a new delay duration.
    pub fn set_delay(&mut self, delay: Duration) {
        self.delay = delay;
    }
}

/// Follow pattern configuration.
#[derive(Debug, Clone)]
pub struct FollowConfig {
    /// Source point ID.
    pub source: String,
    /// Value offset.
    pub offset: f64,
    /// Value gain/multiplier.
    pub gain: f64,
    /// Delay in milliseconds.
    pub delay_ms: u64,
}

impl FollowConfig {
    /// Create a new follow configuration.
    pub fn new(source: impl Into<String>) -> Self {
        Self {
            source: source.into(),
            offset: 0.0,
            gain: 1.0,
            delay_ms: 0,
        }
    }

    /// Set offset.
    pub fn with_offset(mut self, offset: f64) -> Self {
        self.offset = offset;
        self
    }

    /// Set gain.
    pub fn with_gain(mut self, gain: f64) -> Self {
        self.gain = gain;
        self
    }

    /// Set delay.
    pub fn with_delay_ms(mut self, delay_ms: u64) -> Self {
        self.delay_ms = delay_ms;
        self
    }
}

/// Follow pattern state for a single follower.
#[derive(Debug)]
pub struct FollowState {
    /// Configuration.
    config: FollowConfig,
    /// Delay buffer (only if delay > 0).
    buffer: Option<DelayBuffer>,
    /// Last computed value.
    last_value: f64,
}

impl FollowState {
    /// Create a new follow state.
    pub fn new(config: FollowConfig) -> Self {
        let buffer = if config.delay_ms > 0 {
            Some(DelayBuffer::new(Duration::from_millis(config.delay_ms)))
        } else {
            None
        };

        Self {
            config,
            buffer,
            last_value: 0.0,
        }
    }

    /// Update with source value.
    pub fn update(&mut self, source_value: f64) -> f64 {
        if let Some(ref mut buffer) = self.buffer {
            // Push to buffer for delayed retrieval
            buffer.push(source_value);

            // Get delayed value
            if let Some(delayed) = buffer.get_delayed() {
                self.last_value = self.transform(delayed);
            }
        } else {
            // No delay, apply transformation directly
            self.last_value = self.transform(source_value);
        }

        self.last_value
    }

    /// Apply gain and offset transformation.
    fn transform(&self, value: f64) -> f64 {
        value * self.config.gain + self.config.offset
    }

    /// Get last computed value.
    pub fn last_value(&self) -> f64 {
        self.last_value
    }

    /// Get source point ID.
    pub fn source(&self) -> &str {
        &self.config.source
    }

    /// Reset state.
    pub fn reset(&mut self) {
        self.last_value = 0.0;
        if let Some(ref mut buffer) = self.buffer {
            buffer.clear();
        }
    }
}

/// Source value registry for follow pattern coordination.
#[derive(Debug, Default)]
pub struct SourceRegistry {
    /// Current source values.
    values: HashMap<String, f64>,
    /// Subscribers waiting for source updates.
    subscribers: HashMap<String, Vec<String>>,
}

impl SourceRegistry {
    /// Create a new source registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a source value.
    pub fn set_value(&mut self, source_id: impl Into<String>, value: f64) {
        self.values.insert(source_id.into(), value);
    }

    /// Get a source value.
    pub fn get_value(&self, source_id: &str) -> Option<f64> {
        self.values.get(source_id).copied()
    }

    /// Register a subscriber for a source.
    pub fn subscribe(&mut self, source_id: impl Into<String>, subscriber_id: impl Into<String>) {
        self.subscribers
            .entry(source_id.into())
            .or_default()
            .push(subscriber_id.into());
    }

    /// Get subscribers for a source.
    pub fn get_subscribers(&self, source_id: &str) -> Option<&[String]> {
        self.subscribers.get(source_id).map(|v| v.as_slice())
    }

    /// Clear all values.
    pub fn clear(&mut self) {
        self.values.clear();
    }
}

/// Follow pattern manager for coordinating multiple follow relationships.
pub struct FollowManager {
    /// Source value registry.
    sources: Arc<RwLock<SourceRegistry>>,
    /// Follow states by point ID.
    states: HashMap<String, FollowState>,
}

impl FollowManager {
    /// Create a new follow manager.
    pub fn new() -> Self {
        Self {
            sources: Arc::new(RwLock::new(SourceRegistry::new())),
            states: HashMap::new(),
        }
    }

    /// Create with shared source registry.
    pub fn with_sources(sources: Arc<RwLock<SourceRegistry>>) -> Self {
        Self {
            sources,
            states: HashMap::new(),
        }
    }

    /// Register a follow relationship.
    pub fn register(&mut self, point_id: impl Into<String>, config: FollowConfig) {
        let point_id = point_id.into();
        let source = config.source.clone();

        // Register subscriber
        self.sources.write().subscribe(&source, &point_id);

        // Create follow state
        self.states.insert(point_id, FollowState::new(config));
    }

    /// Update source value and propagate to followers.
    pub fn update_source(&mut self, source_id: &str, value: f64) -> Vec<(String, f64)> {
        // Update source registry
        self.sources.write().set_value(source_id, value);

        // Propagate to followers
        let mut updates = Vec::new();

        for (point_id, state) in &mut self.states {
            if state.source() == source_id {
                let new_value = state.update(value);
                updates.push((point_id.clone(), new_value));
            }
        }

        updates
    }

    /// Get current value for a follow point.
    pub fn get_value(&self, point_id: &str) -> Option<f64> {
        self.states.get(point_id).map(|s| s.last_value())
    }

    /// Process tick - update all followers from their sources.
    pub fn tick(&mut self) -> Vec<(String, f64)> {
        let sources = self.sources.read();
        let mut updates = Vec::new();

        for (point_id, state) in &mut self.states {
            if let Some(source_value) = sources.get_value(state.source()) {
                let new_value = state.update(source_value);
                updates.push((point_id.clone(), new_value));
            }
        }

        updates
    }

    /// Reset all follow states.
    pub fn reset(&mut self) {
        for state in self.states.values_mut() {
            state.reset();
        }
        self.sources.write().clear();
    }

    /// Get shared source registry.
    pub fn sources(&self) -> Arc<RwLock<SourceRegistry>> {
        Arc::clone(&self.sources)
    }
}

impl Default for FollowManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_delay_buffer_no_delay() {
        let mut buffer = DelayBuffer::new(Duration::ZERO);
        buffer.push(10.0);
        buffer.push(20.0);
        buffer.push(30.0);

        // With zero delay, should return latest
        let value = buffer.get_delayed();
        assert!(value.is_some());
    }

    #[test]
    fn test_delay_buffer_with_delay() {
        let mut buffer = DelayBuffer::new(Duration::from_millis(100));

        let now = Instant::now();
        buffer.push_with_timestamp(10.0, now);
        buffer.push_with_timestamp(20.0, now + Duration::from_millis(50));
        buffer.push_with_timestamp(30.0, now + Duration::from_millis(100));

        // Query at now + 150ms should get value from ~50ms ago (interpolated)
        let query_time = now + Duration::from_millis(150);
        let value = buffer.get_delayed_at(query_time);
        assert!(value.is_some());
        // Should be around 20 (value at 50ms)
        let v = value.unwrap();
        assert!(v >= 15.0 && v <= 25.0);
    }

    #[test]
    fn test_follow_state_no_delay() {
        let config = FollowConfig::new("source-1")
            .with_gain(2.0)
            .with_offset(5.0);

        let mut state = FollowState::new(config);

        let result = state.update(10.0);
        assert!((result - 25.0).abs() < 0.001); // 10 * 2 + 5 = 25
    }

    #[test]
    fn test_follow_manager() {
        let mut manager = FollowManager::new();

        manager.register("follower-1", FollowConfig::new("source-1").with_gain(1.5));

        manager.register(
            "follower-2",
            FollowConfig::new("source-1").with_offset(10.0),
        );

        // Update source
        let updates = manager.update_source("source-1", 20.0);
        assert_eq!(updates.len(), 2);

        // Check values
        let v1 = manager.get_value("follower-1").unwrap();
        assert!((v1 - 30.0).abs() < 0.001); // 20 * 1.5 = 30

        let v2 = manager.get_value("follower-2").unwrap();
        assert!((v2 - 30.0).abs() < 0.001); // 20 * 1 + 10 = 30
    }

    #[test]
    fn test_source_registry() {
        let mut registry = SourceRegistry::new();

        registry.set_value("temp-1", 25.5);
        registry.subscribe("temp-1", "follower-1");
        registry.subscribe("temp-1", "follower-2");

        assert_eq!(registry.get_value("temp-1"), Some(25.5));
        assert_eq!(registry.get_subscribers("temp-1").map(|s| s.len()), Some(2));
    }

    #[test]
    fn test_follow_config_builder() {
        let config = FollowConfig::new("source")
            .with_gain(2.0)
            .with_offset(5.0)
            .with_delay_ms(100);

        assert_eq!(config.source, "source");
        assert!((config.gain - 2.0).abs() < f64::EPSILON);
        assert!((config.offset - 5.0).abs() < f64::EPSILON);
        assert_eq!(config.delay_ms, 100);
    }
}

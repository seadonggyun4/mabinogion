//! Chaos scheduler for time-based fault injection.

use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

// =============================================================================
// Chaos Type
// =============================================================================

/// Types of chaos that can be scheduled.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ChaosType {
    /// Network latency injection.
    NetworkLatency { base_ms: u64, jitter_ms: u64 },

    /// Packet loss.
    PacketLoss { rate: f64 },

    /// Connection disconnect.
    Disconnect,

    /// Device goes offline.
    DeviceOffline,

    /// Corrupted data in responses.
    CorruptedData { corruption_rate: f64 },

    /// Load spike simulation.
    LoadSpike { multiplier: f64 },

    /// Slow device response.
    SlowResponse { delay_ms: u64 },

    /// Protocol timeout.
    Timeout { timeout_ms: u64 },

    /// Malformed packets.
    MalformedPacket,

    /// Invalid checksum.
    InvalidChecksum,

    /// Custom fault by ID.
    Custom { fault_id: String },
}

// =============================================================================
// Chaos Entry
// =============================================================================

/// A scheduled chaos entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChaosEntry {
    /// Start time (seconds from schedule start).
    pub start_secs: f64,

    /// Duration of the chaos event (seconds).
    pub duration_secs: f64,

    /// Type of chaos.
    pub chaos_type: ChaosType,

    /// Target devices/protocols (empty = all).
    #[serde(default)]
    pub targets: Vec<String>,

    /// Intensity (0.0 - 1.0).
    #[serde(default = "default_intensity")]
    pub intensity: f64,

    /// Description of this chaos entry.
    #[serde(default)]
    pub description: String,
}

fn default_intensity() -> f64 {
    1.0
}

impl ChaosEntry {
    /// Create a new chaos entry.
    pub fn new(start_secs: f64, duration_secs: f64, chaos_type: ChaosType) -> Self {
        Self {
            start_secs,
            duration_secs,
            chaos_type,
            targets: Vec::new(),
            intensity: 1.0,
            description: String::new(),
        }
    }

    /// Create a builder.
    pub fn builder() -> ChaosEntryBuilder {
        ChaosEntryBuilder::default()
    }

    /// Set targets.
    pub fn with_targets(mut self, targets: Vec<String>) -> Self {
        self.targets = targets;
        self
    }

    /// Set intensity.
    pub fn with_intensity(mut self, intensity: f64) -> Self {
        self.intensity = intensity.clamp(0.0, 1.0);
        self
    }
}

/// Builder for chaos entry.
#[derive(Debug, Default)]
pub struct ChaosEntryBuilder {
    start_secs: f64,
    duration_secs: f64,
    chaos_type: Option<ChaosType>,
    targets: Vec<String>,
    intensity: f64,
    description: String,
}

impl ChaosEntryBuilder {
    /// Set start time.
    pub fn start_secs(mut self, secs: f64) -> Self {
        self.start_secs = secs;
        self
    }

    /// Set duration.
    pub fn duration_secs(mut self, secs: f64) -> Self {
        self.duration_secs = secs;
        self
    }

    /// Set chaos type.
    pub fn chaos_type(mut self, chaos_type: ChaosType) -> Self {
        self.chaos_type = Some(chaos_type);
        self
    }

    /// Set latency chaos.
    pub fn latency(mut self, base_ms: u64, jitter_ms: u64) -> Self {
        self.chaos_type = Some(ChaosType::NetworkLatency { base_ms, jitter_ms });
        self
    }

    /// Set packet loss chaos.
    pub fn packet_loss(mut self, rate: f64) -> Self {
        self.chaos_type = Some(ChaosType::PacketLoss { rate });
        self
    }

    /// Set device offline chaos.
    pub fn device_offline(mut self) -> Self {
        self.chaos_type = Some(ChaosType::DeviceOffline);
        self
    }

    /// Add target.
    pub fn target(mut self, target: impl Into<String>) -> Self {
        self.targets.push(target.into());
        self
    }

    /// Set intensity.
    pub fn intensity(mut self, intensity: f64) -> Self {
        self.intensity = intensity.clamp(0.0, 1.0);
        self
    }

    /// Set description.
    pub fn description(mut self, desc: impl Into<String>) -> Self {
        self.description = desc.into();
        self
    }

    /// Build the entry.
    pub fn build(self) -> ChaosEntry {
        ChaosEntry {
            start_secs: self.start_secs,
            duration_secs: self.duration_secs,
            chaos_type: self.chaos_type.unwrap_or(ChaosType::DeviceOffline),
            targets: self.targets,
            intensity: if self.intensity == 0.0 {
                1.0
            } else {
                self.intensity
            },
            description: self.description,
        }
    }
}

// =============================================================================
// Chaos Schedule
// =============================================================================

/// A complete chaos schedule.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChaosSchedule {
    /// Schedule name.
    pub name: String,

    /// Description.
    #[serde(default)]
    pub description: String,

    /// Schedule entries (should be sorted by start_secs).
    pub entries: Vec<ChaosEntry>,

    /// Whether to loop the schedule.
    #[serde(default)]
    pub loop_schedule: bool,

    /// Total duration (for looping).
    #[serde(default)]
    pub total_duration_secs: Option<f64>,
}

impl ChaosSchedule {
    /// Create a new schedule.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: String::new(),
            entries: Vec::new(),
            loop_schedule: false,
            total_duration_secs: None,
        }
    }

    /// Create a builder.
    pub fn builder() -> ChaosScheduleBuilder {
        ChaosScheduleBuilder::default()
    }

    /// Add an entry.
    pub fn add_entry(&mut self, entry: ChaosEntry) {
        self.entries.push(entry);
        self.entries
            .sort_by(|a, b| a.start_secs.partial_cmp(&b.start_secs).unwrap());
    }
}

/// Builder for chaos schedule.
#[derive(Debug, Default)]
pub struct ChaosScheduleBuilder {
    name: Option<String>,
    description: String,
    entries: Vec<ChaosEntry>,
    loop_schedule: bool,
    total_duration_secs: Option<f64>,
}

impl ChaosScheduleBuilder {
    /// Set name.
    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    /// Set description.
    pub fn description(mut self, desc: impl Into<String>) -> Self {
        self.description = desc.into();
        self
    }

    /// Add entry.
    pub fn add_entry(mut self, entry: ChaosEntry) -> Self {
        self.entries.push(entry);
        self
    }

    /// Enable looping.
    pub fn looping(mut self, total_duration_secs: f64) -> Self {
        self.loop_schedule = true;
        self.total_duration_secs = Some(total_duration_secs);
        self
    }

    /// Build the schedule.
    pub fn build(mut self) -> ChaosSchedule {
        self.entries
            .sort_by(|a, b| a.start_secs.partial_cmp(&b.start_secs).unwrap());

        ChaosSchedule {
            name: self.name.unwrap_or_else(|| "unnamed".to_string()),
            description: self.description,
            entries: self.entries,
            loop_schedule: self.loop_schedule,
            total_duration_secs: self.total_duration_secs,
        }
    }
}

// =============================================================================
// Schedule State
// =============================================================================

/// State of the scheduler.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ScheduleState {
    /// Scheduler is stopped.
    #[default]
    Stopped,

    /// Scheduler is running.
    Running,

    /// Scheduler is paused.
    Paused,

    /// Schedule completed.
    Completed,
}

// =============================================================================
// Active Chaos
// =============================================================================

/// An active chaos event.
#[derive(Debug, Clone)]
pub struct ActiveChaos {
    /// The chaos entry.
    pub entry: ChaosEntry,

    /// When the chaos started.
    pub started_at: Instant,
}

impl ActiveChaos {
    /// Check if the chaos has expired.
    pub fn is_expired(&self) -> bool {
        self.started_at.elapsed() >= Duration::from_secs_f64(self.entry.duration_secs)
    }

    /// Get remaining duration.
    pub fn remaining(&self) -> Duration {
        let elapsed = self.started_at.elapsed();
        let total = Duration::from_secs_f64(self.entry.duration_secs);
        total.saturating_sub(elapsed)
    }
}

// =============================================================================
// Chaos Event
// =============================================================================

/// Events emitted by the scheduler.
#[derive(Debug, Clone)]
pub enum ChaosEvent {
    /// A chaos entry started.
    Started(ChaosEntry),

    /// A chaos entry ended.
    Ended(ChaosEntry),

    /// Schedule completed (if not looping).
    ScheduleCompleted,

    /// Schedule looped.
    ScheduleLooped,
}

// =============================================================================
// Chaos Scheduler
// =============================================================================

/// Scheduler for time-based chaos injection.
#[derive(Debug)]
pub struct ChaosScheduler {
    schedule: ChaosSchedule,
    state: ScheduleState,
    active_chaos: Vec<ActiveChaos>,
    start_time: Option<Instant>,
    next_entry_index: usize,
    loop_count: u32,
}

impl ChaosScheduler {
    /// Create a new scheduler.
    pub fn new(schedule: ChaosSchedule) -> Self {
        Self {
            schedule,
            state: ScheduleState::Stopped,
            active_chaos: Vec::new(),
            start_time: None,
            next_entry_index: 0,
            loop_count: 0,
        }
    }

    /// Get the schedule.
    pub fn schedule(&self) -> &ChaosSchedule {
        &self.schedule
    }

    /// Get current state.
    pub fn state(&self) -> ScheduleState {
        self.state
    }

    /// Get active chaos events.
    pub fn active(&self) -> &[ActiveChaos] {
        &self.active_chaos
    }

    /// Get loop count.
    pub fn loop_count(&self) -> u32 {
        self.loop_count
    }

    /// Start the scheduler.
    pub fn start(&mut self) {
        self.start_time = Some(Instant::now());
        self.next_entry_index = 0;
        self.active_chaos.clear();
        self.state = ScheduleState::Running;
        self.loop_count = 0;
    }

    /// Stop the scheduler.
    pub fn stop(&mut self) {
        self.start_time = None;
        self.active_chaos.clear();
        self.state = ScheduleState::Stopped;
    }

    /// Pause the scheduler.
    pub fn pause(&mut self) {
        if self.state == ScheduleState::Running {
            self.state = ScheduleState::Paused;
        }
    }

    /// Resume the scheduler.
    pub fn resume(&mut self) {
        if self.state == ScheduleState::Paused {
            self.state = ScheduleState::Running;
        }
    }

    /// Process one tick.
    pub fn tick(&mut self) -> Vec<ChaosEvent> {
        if self.state != ScheduleState::Running {
            return Vec::new();
        }

        let Some(start_time) = self.start_time else {
            return Vec::new();
        };

        let mut elapsed = start_time.elapsed().as_secs_f64();
        let mut events = Vec::new();

        // Handle looping
        if self.schedule.loop_schedule {
            if let Some(total) = self.schedule.total_duration_secs {
                if elapsed >= total {
                    let loops = (elapsed / total) as u32;
                    if loops > self.loop_count {
                        self.loop_count = loops;
                        self.next_entry_index = 0;
                        events.push(ChaosEvent::ScheduleLooped);
                    }
                    elapsed %= total;
                }
            }
        }

        // Check for new entries to activate
        while self.next_entry_index < self.schedule.entries.len() {
            let entry = &self.schedule.entries[self.next_entry_index];
            if entry.start_secs <= elapsed {
                self.active_chaos.push(ActiveChaos {
                    entry: entry.clone(),
                    started_at: Instant::now(),
                });
                events.push(ChaosEvent::Started(entry.clone()));
                self.next_entry_index += 1;
            } else {
                break;
            }
        }

        // Check for expired entries
        let mut i = 0;
        while i < self.active_chaos.len() {
            if self.active_chaos[i].is_expired() {
                let removed = self.active_chaos.remove(i);
                events.push(ChaosEvent::Ended(removed.entry));
            } else {
                i += 1;
            }
        }

        // Check for schedule completion
        if !self.schedule.loop_schedule
            && self.next_entry_index >= self.schedule.entries.len()
            && self.active_chaos.is_empty()
        {
            self.state = ScheduleState::Completed;
            events.push(ChaosEvent::ScheduleCompleted);
        }

        events
    }

    /// Check if a chaos type is active for a target.
    pub fn is_active(&self, target: &str, chaos_type_name: &str) -> bool {
        self.active_chaos.iter().any(|ac| {
            let type_matches = match (&ac.entry.chaos_type, chaos_type_name) {
                (ChaosType::NetworkLatency { .. }, "latency" | "network_latency") => true,
                (ChaosType::PacketLoss { .. }, "packet_loss") => true,
                (ChaosType::Disconnect, "disconnect") => true,
                (ChaosType::DeviceOffline, "device_offline" | "offline") => true,
                (ChaosType::CorruptedData { .. }, "corrupted_data" | "corruption") => true,
                (ChaosType::LoadSpike { .. }, "load_spike") => true,
                (ChaosType::SlowResponse { .. }, "slow_response") => true,
                (ChaosType::Timeout { .. }, "timeout") => true,
                (ChaosType::MalformedPacket, "malformed" | "malformed_packet") => true,
                (ChaosType::InvalidChecksum, "checksum" | "invalid_checksum") => true,
                (ChaosType::Custom { fault_id }, name) => fault_id == name,
                _ => false,
            };

            let target_matches = ac.entry.targets.is_empty()
                || ac.entry.targets.iter().any(|t| {
                    if t.contains('*') {
                        glob_match(t, target)
                    } else {
                        t == target
                    }
                });

            type_matches && target_matches
        })
    }

    /// Get active chaos parameters for a target.
    pub fn get_active_params(&self, target: &str) -> Vec<&ChaosEntry> {
        self.active_chaos
            .iter()
            .filter(|ac| {
                ac.entry.targets.is_empty()
                    || ac.entry.targets.iter().any(|t| {
                        if t.contains('*') {
                            glob_match(t, target)
                        } else {
                            t == target
                        }
                    })
            })
            .map(|ac| &ac.entry)
            .collect()
    }
}

/// Simple glob matching.
fn glob_match(pattern: &str, text: &str) -> bool {
    if pattern == "*" {
        return true;
    }
    if pattern.starts_with('*') {
        return text.ends_with(&pattern[1..]);
    }
    if pattern.ends_with('*') {
        return text.starts_with(&pattern[..pattern.len() - 1]);
    }
    pattern == text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_schedule_builder() {
        let schedule = ChaosSchedule::builder()
            .name("test")
            .add_entry(
                ChaosEntry::builder()
                    .start_secs(0.0)
                    .duration_secs(10.0)
                    .latency(100, 50)
                    .build(),
            )
            .add_entry(
                ChaosEntry::builder()
                    .start_secs(5.0)
                    .duration_secs(5.0)
                    .packet_loss(0.1)
                    .build(),
            )
            .build();

        assert_eq!(schedule.name, "test");
        assert_eq!(schedule.entries.len(), 2);
        assert!(schedule.entries[0].start_secs <= schedule.entries[1].start_secs);
    }

    #[test]
    fn test_scheduler_start_stop() {
        let schedule = ChaosSchedule::new("test");
        let mut scheduler = ChaosScheduler::new(schedule);

        assert_eq!(scheduler.state(), ScheduleState::Stopped);

        scheduler.start();
        assert_eq!(scheduler.state(), ScheduleState::Running);

        scheduler.pause();
        assert_eq!(scheduler.state(), ScheduleState::Paused);

        scheduler.resume();
        assert_eq!(scheduler.state(), ScheduleState::Running);

        scheduler.stop();
        assert_eq!(scheduler.state(), ScheduleState::Stopped);
    }

    #[test]
    fn test_scheduler_tick() {
        let schedule = ChaosSchedule::builder()
            .name("test")
            .add_entry(
                ChaosEntry::builder()
                    .start_secs(0.0)
                    .duration_secs(0.1)
                    .latency(100, 50)
                    .build(),
            )
            .build();

        let mut scheduler = ChaosScheduler::new(schedule);
        scheduler.start();

        let events = scheduler.tick();
        assert!(events.iter().any(|e| matches!(e, ChaosEvent::Started(_))));

        assert!(scheduler.is_active("any", "latency"));

        std::thread::sleep(Duration::from_millis(150));
        let events = scheduler.tick();
        assert!(events.iter().any(|e| matches!(e, ChaosEvent::Ended(_))));
    }

    #[test]
    fn test_target_matching() {
        let schedule = ChaosSchedule::builder()
            .name("test")
            .add_entry(
                ChaosEntry::builder()
                    .start_secs(0.0)
                    .duration_secs(10.0)
                    .latency(100, 50)
                    .target("modbus-*")
                    .build(),
            )
            .build();

        let mut scheduler = ChaosScheduler::new(schedule);
        scheduler.start();
        scheduler.tick();

        assert!(scheduler.is_active("modbus-001", "latency"));
        assert!(scheduler.is_active("modbus-server", "latency"));
        assert!(!scheduler.is_active("opcua-001", "latency"));
    }
}

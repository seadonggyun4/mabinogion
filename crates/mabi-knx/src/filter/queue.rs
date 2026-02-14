//! QueueFilter — 3-priority FIFO with WaitingForAck backpressure.
//!
//! The QueueFilter provides priority-based frame queuing that simulates
//! real KNX gateway behavior. When the tunnel connection is in
//! WaitingForAck state, frames are queued instead of being sent directly.
//!
//! ## Priority Levels
//!
//! - **High**: L_Data.con confirmations, M_Reset.ind, system frames
//! - **Normal**: L_Data.req group value writes/reads, L_Data.ind broadcasts
//! - **Low**: Bus monitor frames, property service responses, diagnostics
//!
//! ## Backpressure
//!
//! When `waiting_for_ack` is active for a channel, the QueueFilter holds
//! frames instead of passing them through. This prevents frame loss when
//! the client hasn't yet acknowledged the previous frame.
//!
//! ## Queue Limits
//!
//! Each priority level has a configurable maximum depth. When a queue is
//! full, the oldest frame in the lowest priority queue is dropped to make
//! room (priority-based eviction).

use std::collections::{HashMap, VecDeque};
use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use tracing::{debug, trace};

use crate::cemi::MessageCode;

use super::chain::{FilterResult, FrameEnvelope};

// ============================================================================
// Queue Priority
// ============================================================================

/// Frame priority for queue ordering.
///
/// Frames are dequeued in strict priority order: High → Normal → Low.
/// Within the same priority, frames are dequeued in FIFO order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum QueuePriority {
    /// Lowest priority: diagnostics, bus monitor, property responses.
    Low = 0,
    /// Default priority: regular data frames.
    Normal = 1,
    /// Highest priority: confirmations, resets, system frames.
    High = 2,
}

impl QueuePriority {
    /// Classify a frame by its message code into a priority level.
    pub fn classify(message_code: MessageCode) -> Self {
        match message_code {
            // High priority: confirmations and resets
            MessageCode::LDataCon
            | MessageCode::LDataReq  // Original requests get high priority for retransmit
            | MessageCode::MResetInd
            | MessageCode::MResetReq => Self::High,

            // Normal priority: data indications and standard requests
            MessageCode::LDataInd
            | MessageCode::MPropReadReq
            | MessageCode::MPropWriteReq
            | MessageCode::MPropReadCon
            | MessageCode::MPropWriteCon => Self::Normal,

            // Low priority: bus monitor, raw, and everything else
            MessageCode::LBusmonInd
            | MessageCode::LRawReq
            | MessageCode::LRawCon
            | MessageCode::LRawInd => Self::Low,
        }
    }

    /// All priority levels in descending order (highest first).
    pub fn all_descending() -> &'static [QueuePriority] {
        &[Self::High, Self::Normal, Self::Low]
    }
}

impl fmt::Display for QueuePriority {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Low => write!(f, "Low"),
            Self::Normal => write!(f, "Normal"),
            Self::High => write!(f, "High"),
        }
    }
}

// ============================================================================
// Per-Channel Queue State
// ============================================================================

/// Per-channel queue state tracking.
#[derive(Debug)]
struct ChannelQueueState {
    /// Whether this channel is currently waiting for an ACK.
    waiting_for_ack: bool,
    /// Per-priority FIFO queues.
    queues: [VecDeque<FrameEnvelope>; 3],
}

impl ChannelQueueState {
    fn new() -> Self {
        Self {
            waiting_for_ack: false,
            queues: [
                VecDeque::new(), // Low (index 0)
                VecDeque::new(), // Normal (index 1)
                VecDeque::new(), // High (index 2)
            ],
        }
    }

    /// Get the queue for a given priority level.
    fn queue_mut(&mut self, priority: QueuePriority) -> &mut VecDeque<FrameEnvelope> {
        &mut self.queues[priority as usize]
    }

    /// Get the total number of queued frames across all priorities.
    fn total_queued(&self) -> usize {
        self.queues.iter().map(|q| q.len()).sum()
    }

    /// Drain up to `max_count` frames in priority order (High → Normal → Low).
    fn drain(&mut self, max_count: usize) -> Vec<FrameEnvelope> {
        let mut result = Vec::with_capacity(max_count);

        for priority in QueuePriority::all_descending() {
            let queue = &mut self.queues[*priority as usize];
            while result.len() < max_count {
                match queue.pop_front() {
                    Some(envelope) => result.push(envelope),
                    None => break,
                }
            }
            if result.len() >= max_count {
                break;
            }
        }

        result
    }

    /// Drop the oldest frame from the lowest priority queue that has frames.
    /// Returns the priority level of the dropped frame, or None if all queues are empty.
    fn evict_lowest(&mut self) -> Option<QueuePriority> {
        // Try Low first, then Normal (never evict High)
        for &priority in &[QueuePriority::Low, QueuePriority::Normal] {
            let queue = &mut self.queues[priority as usize];
            if queue.pop_front().is_some() {
                return Some(priority);
            }
        }
        None
    }
}

// ============================================================================
// QueueFilter Configuration
// ============================================================================

/// QueueFilter configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueueFilterConfig {
    /// Whether the QueueFilter is enabled.
    #[serde(default = "default_true")]
    pub enabled: bool,

    /// Maximum queue depth per priority level per channel.
    #[serde(default = "default_max_queue_depth")]
    pub max_queue_depth: usize,

    /// Whether to enable WaitingForAck backpressure.
    /// When true, frames are queued when the connection is WaitingForAck.
    #[serde(default = "default_true")]
    pub backpressure_enabled: bool,

    /// Whether to evict lower-priority frames when queue is full.
    /// When false, new frames are dropped if queue is full.
    #[serde(default = "default_true")]
    pub priority_eviction_enabled: bool,

    /// Maximum total frames across all channels and priorities.
    /// 0 = unlimited (bounded only by per-channel limits).
    #[serde(default = "default_max_total_frames")]
    pub max_total_frames: usize,
}

fn default_true() -> bool {
    true
}

fn default_max_queue_depth() -> usize {
    64
}

fn default_max_total_frames() -> usize {
    1024
}

impl Default for QueueFilterConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            max_queue_depth: default_max_queue_depth(),
            backpressure_enabled: true,
            priority_eviction_enabled: true,
            max_total_frames: default_max_total_frames(),
        }
    }
}

impl QueueFilterConfig {
    /// Validate the configuration.
    pub fn validate(&self) -> Result<(), String> {
        if self.max_queue_depth == 0 {
            return Err("QueueFilter max_queue_depth must be > 0".to_string());
        }
        Ok(())
    }
}

// ============================================================================
// QueueFilter Statistics
// ============================================================================

/// Lock-free QueueFilter statistics.
#[derive(Debug, Default)]
pub struct QueueFilterStats {
    /// Frames that passed through without queuing.
    pub direct_pass: AtomicU64,
    /// Frames that were queued due to backpressure.
    pub queued_frames: AtomicU64,
    /// Frames that were dropped because queue was full.
    pub dropped_full: AtomicU64,
    /// Frames evicted from lower-priority queues to make room.
    pub evicted_frames: AtomicU64,
    /// Frames successfully drained from queues.
    pub drained_frames: AtomicU64,
    /// High-priority frames processed.
    pub high_priority: AtomicU64,
    /// Normal-priority frames processed.
    pub normal_priority: AtomicU64,
    /// Low-priority frames processed.
    pub low_priority: AtomicU64,
}

/// Snapshot of QueueFilter statistics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QueueFilterStatsSnapshot {
    pub direct_pass: u64,
    pub queued_frames: u64,
    pub dropped_full: u64,
    pub evicted_frames: u64,
    pub drained_frames: u64,
    pub high_priority: u64,
    pub normal_priority: u64,
    pub low_priority: u64,
}

// ============================================================================
// QueueFilter
// ============================================================================

/// 3-priority FIFO queue with WaitingForAck backpressure.
///
/// The QueueFilter classifies frames by priority and queues them when
/// the target connection is in WaitingForAck state. This prevents
/// frame loss from the server overwhelming the client before it has
/// acknowledged the previous frame.
///
/// When backpressure is released (ACK received), queued frames are
/// drained in strict priority order: High → Normal → Low.
pub struct QueueFilter {
    config: QueueFilterConfig,
    /// Per-channel queue state.
    channels: RwLock<HashMap<u8, ChannelQueueState>>,
    stats: QueueFilterStats,
}

impl QueueFilter {
    /// Create a new QueueFilter with the given configuration.
    pub fn new(config: QueueFilterConfig) -> Self {
        Self {
            config,
            channels: RwLock::new(HashMap::new()),
            stats: QueueFilterStats::default(),
        }
    }

    /// Process a frame in the send direction.
    ///
    /// 1. Classify the frame's priority based on message code
    /// 2. If backpressure is active (WaitingForAck), queue the frame
    /// 3. Otherwise, pass through with priority set on envelope
    pub fn process_send(&self, envelope: &mut FrameEnvelope) -> FilterResult {
        if !self.config.enabled {
            return FilterResult::pass();
        }

        // Classify priority
        let priority = QueuePriority::classify(envelope.cemi.message_code);
        envelope.priority = priority;

        // Record priority stats
        match priority {
            QueuePriority::High => self.stats.high_priority.fetch_add(1, Ordering::Relaxed),
            QueuePriority::Normal => self.stats.normal_priority.fetch_add(1, Ordering::Relaxed),
            QueuePriority::Low => self.stats.low_priority.fetch_add(1, Ordering::Relaxed),
        };

        // Check backpressure
        if self.config.backpressure_enabled {
            let mut channels = self.channels.write();
            let total_pending: usize = channels.values().map(|cs| cs.total_queued()).sum();

            let channel_state = channels
                .entry(envelope.channel_id)
                .or_insert_with(ChannelQueueState::new);

            if channel_state.waiting_for_ack {
                // Backpressure active — queue the frame
                return self.enqueue_frame(channel_state, envelope.clone(), total_pending);
            }
        }

        self.stats.direct_pass.fetch_add(1, Ordering::Relaxed);
        FilterResult::pass()
    }

    /// Process a frame in the recv direction.
    ///
    /// On the receive path, we just pass through. The actual backpressure
    /// release happens via `on_ack_received()`.
    pub fn process_recv(&self, _envelope: &FrameEnvelope) -> FilterResult {
        FilterResult::pass()
    }

    /// Enqueue a frame with priority-based eviction.
    ///
    /// `total_pending` is the total number of frames across all channels,
    /// computed before this call to avoid deadlocking on the channels lock.
    fn enqueue_frame(
        &self,
        channel_state: &mut ChannelQueueState,
        envelope: FrameEnvelope,
        total_pending: usize,
    ) -> FilterResult {
        let priority = envelope.priority;
        let queue = channel_state.queue_mut(priority);

        // Check per-priority queue depth
        if queue.len() >= self.config.max_queue_depth {
            if self.config.priority_eviction_enabled {
                // Try to evict from a lower-priority queue
                if let Some(evicted_priority) = channel_state.evict_lowest() {
                    self.stats.evicted_frames.fetch_add(1, Ordering::Relaxed);
                    debug!(
                        channel_id = envelope.channel_id,
                        evicted_priority = %evicted_priority,
                        enqueue_priority = %priority,
                        "QueueFilter: evicted lower-priority frame to make room"
                    );
                } else {
                    // All lower queues empty, and current queue is full — drop
                    self.stats.dropped_full.fetch_add(1, Ordering::Relaxed);
                    return FilterResult::Dropped {
                        reason: format!(
                            "QueueFilter: {} queue full ({} frames), no lower priority to evict",
                            priority,
                            self.config.max_queue_depth,
                        ),
                    };
                }
            } else {
                // No eviction — drop
                self.stats.dropped_full.fetch_add(1, Ordering::Relaxed);
                return FilterResult::Dropped {
                    reason: format!(
                        "QueueFilter: {} queue full ({} frames)",
                        priority,
                        self.config.max_queue_depth,
                    ),
                };
            }
        }

        // Check total frame limit
        if self.config.max_total_frames > 0 && total_pending >= self.config.max_total_frames {
            self.stats.dropped_full.fetch_add(1, Ordering::Relaxed);
            return FilterResult::Dropped {
                reason: format!(
                    "QueueFilter: total frame limit reached ({} >= {})",
                    total_pending, self.config.max_total_frames,
                ),
            };
        }

        // Enqueue
        let queue = channel_state.queue_mut(priority);
        queue.push_back(envelope);
        self.stats.queued_frames.fetch_add(1, Ordering::Relaxed);

        trace!(
            priority = %priority,
            queue_depth = queue.len(),
            "QueueFilter: frame queued"
        );

        FilterResult::Queued
    }

    /// Set a channel's WaitingForAck backpressure flag.
    pub fn set_waiting_for_ack(&self, channel_id: u8, waiting: bool) {
        if !self.config.enabled || !self.config.backpressure_enabled {
            return;
        }

        let mut channels = self.channels.write();
        let channel_state = channels
            .entry(channel_id)
            .or_insert_with(ChannelQueueState::new);
        channel_state.waiting_for_ack = waiting;

        trace!(
            channel_id,
            waiting_for_ack = waiting,
            "QueueFilter: backpressure state changed"
        );
    }

    /// Notify that an ACK was received for a channel.
    ///
    /// This releases backpressure and allows queued frames to be drained.
    pub fn on_ack_received(&self, channel_id: u8) {
        if !self.config.enabled {
            return;
        }

        let mut channels = self.channels.write();
        if let Some(channel_state) = channels.get_mut(&channel_id) {
            channel_state.waiting_for_ack = false;
            trace!(
                channel_id,
                pending = channel_state.total_queued(),
                "QueueFilter: ACK received, backpressure released"
            );
        }
    }

    /// Notify that a send error occurred for a channel.
    ///
    /// This does not change backpressure state — the connection may retry.
    pub fn on_send_error(&self, channel_id: u8) {
        trace!(channel_id, "QueueFilter: send error recorded");
    }

    /// Check if there are pending frames for a channel.
    pub fn has_pending(&self, channel_id: u8) -> bool {
        let channels = self.channels.read();
        channels
            .get(&channel_id)
            .map(|cs| cs.total_queued() > 0)
            .unwrap_or(false)
    }

    /// Get the total number of pending frames across all channels.
    pub fn total_pending(&self) -> usize {
        let channels = self.channels.read();
        channels.values().map(|cs| cs.total_queued()).sum()
    }

    /// Drain up to `max_count` frames from a channel in priority order.
    ///
    /// Returns frames in descending priority order: High → Normal → Low.
    pub fn drain(&self, channel_id: u8, max_count: usize) -> Vec<FrameEnvelope> {
        if !self.config.enabled {
            return Vec::new();
        }

        let mut channels = self.channels.write();
        let result = match channels.get_mut(&channel_id) {
            Some(channel_state) => {
                let drained = channel_state.drain(max_count);
                self.stats
                    .drained_frames
                    .fetch_add(drained.len() as u64, Ordering::Relaxed);

                trace!(
                    channel_id,
                    drained_count = drained.len(),
                    remaining = channel_state.total_queued(),
                    "QueueFilter: frames drained"
                );

                drained
            }
            None => Vec::new(),
        };

        result
    }

    /// Get pending frame counts per priority for a channel.
    pub fn pending_by_priority(&self, channel_id: u8) -> [usize; 3] {
        let channels = self.channels.read();
        match channels.get(&channel_id) {
            Some(cs) => [
                cs.queues[0].len(), // Low
                cs.queues[1].len(), // Normal
                cs.queues[2].len(), // High
            ],
            None => [0, 0, 0],
        }
    }

    /// Remove all queued frames for a channel (e.g., on disconnect).
    pub fn clear_channel(&self, channel_id: u8) {
        let mut channels = self.channels.write();
        channels.remove(&channel_id);
    }

    /// Get a snapshot of the statistics.
    pub fn stats_snapshot(&self) -> QueueFilterStatsSnapshot {
        QueueFilterStatsSnapshot {
            direct_pass: self.stats.direct_pass.load(Ordering::Relaxed),
            queued_frames: self.stats.queued_frames.load(Ordering::Relaxed),
            dropped_full: self.stats.dropped_full.load(Ordering::Relaxed),
            evicted_frames: self.stats.evicted_frames.load(Ordering::Relaxed),
            drained_frames: self.stats.drained_frames.load(Ordering::Relaxed),
            high_priority: self.stats.high_priority.load(Ordering::Relaxed),
            normal_priority: self.stats.normal_priority.load(Ordering::Relaxed),
            low_priority: self.stats.low_priority.load(Ordering::Relaxed),
        }
    }
}

impl fmt::Debug for QueueFilter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("QueueFilter")
            .field("enabled", &self.config.enabled)
            .field("max_queue_depth", &self.config.max_queue_depth)
            .field("backpressure_enabled", &self.config.backpressure_enabled)
            .field("total_pending", &self.total_pending())
            .finish()
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::address::{GroupAddress, IndividualAddress};
    use crate::cemi::CemiFrame;

    fn make_envelope(channel_id: u8) -> FrameEnvelope {
        let cemi = CemiFrame::group_value_write(
            IndividualAddress::new(1, 1, 1),
            GroupAddress::three_level(1, 0, 1),
            vec![0x01],
        );
        FrameEnvelope::new(cemi, channel_id, "192.168.1.100:3671".parse().unwrap())
    }

    fn make_confirmation_envelope(channel_id: u8) -> FrameEnvelope {
        let mut cemi = CemiFrame::group_value_write(
            IndividualAddress::new(1, 1, 1),
            GroupAddress::three_level(1, 0, 1),
            vec![0x01],
        );
        cemi.message_code = MessageCode::LDataCon;
        FrameEnvelope::new(cemi, channel_id, "192.168.1.100:3671".parse().unwrap())
    }

    fn make_busmon_envelope(channel_id: u8) -> FrameEnvelope {
        let cemi = CemiFrame::bus_monitor_indication(&[0x11, 0x00, 0x00], 0x00, 0);
        FrameEnvelope::new(cemi, channel_id, "192.168.1.100:3671".parse().unwrap())
    }

    #[test]
    fn test_queue_priority_classify() {
        assert_eq!(
            QueuePriority::classify(MessageCode::LDataCon),
            QueuePriority::High
        );
        assert_eq!(
            QueuePriority::classify(MessageCode::MResetInd),
            QueuePriority::High
        );
        assert_eq!(
            QueuePriority::classify(MessageCode::LDataInd),
            QueuePriority::Normal
        );
        assert_eq!(
            QueuePriority::classify(MessageCode::MPropReadReq),
            QueuePriority::Normal
        );
        assert_eq!(
            QueuePriority::classify(MessageCode::LBusmonInd),
            QueuePriority::Low
        );
        assert_eq!(
            QueuePriority::classify(MessageCode::LRawReq),
            QueuePriority::Low
        );
    }

    #[test]
    fn test_queue_priority_ordering() {
        assert!(QueuePriority::High > QueuePriority::Normal);
        assert!(QueuePriority::Normal > QueuePriority::Low);
    }

    #[test]
    fn test_queue_priority_display() {
        assert_eq!(QueuePriority::High.to_string(), "High");
        assert_eq!(QueuePriority::Normal.to_string(), "Normal");
        assert_eq!(QueuePriority::Low.to_string(), "Low");
    }

    #[test]
    fn test_queue_filter_disabled() {
        let mut config = QueueFilterConfig::default();
        config.enabled = false;
        let filter = QueueFilter::new(config);

        let mut envelope = make_envelope(1);
        let result = filter.process_send(&mut envelope);
        assert!(result.should_continue());
    }

    #[test]
    fn test_queue_filter_passthrough_no_backpressure() {
        let config = QueueFilterConfig::default();
        let filter = QueueFilter::new(config);

        let mut envelope = make_envelope(1);
        let result = filter.process_send(&mut envelope);
        assert!(result.should_continue());

        let stats = filter.stats_snapshot();
        assert_eq!(stats.direct_pass, 1);
        assert_eq!(stats.queued_frames, 0);
    }

    #[test]
    fn test_queue_filter_backpressure_queues_frames() {
        let config = QueueFilterConfig::default();
        let filter = QueueFilter::new(config);

        // Enable backpressure for channel 1
        filter.set_waiting_for_ack(1, true);

        let mut envelope = make_envelope(1);
        let result = filter.process_send(&mut envelope);
        assert!(matches!(result, FilterResult::Queued));

        let stats = filter.stats_snapshot();
        assert_eq!(stats.queued_frames, 1);
        assert!(filter.has_pending(1));
        assert_eq!(filter.total_pending(), 1);
    }

    #[test]
    fn test_queue_filter_backpressure_release() {
        let config = QueueFilterConfig::default();
        let filter = QueueFilter::new(config);

        // Enable backpressure
        filter.set_waiting_for_ack(1, true);

        // Queue some frames
        for _ in 0..3 {
            let mut envelope = make_envelope(1);
            filter.process_send(&mut envelope);
        }
        assert_eq!(filter.total_pending(), 3);

        // Release backpressure
        filter.on_ack_received(1);

        // Drain queued frames
        let drained = filter.drain(1, 10);
        assert_eq!(drained.len(), 3);
        assert_eq!(filter.total_pending(), 0);
    }

    #[test]
    fn test_queue_filter_priority_ordering() {
        let config = QueueFilterConfig::default();
        let filter = QueueFilter::new(config);

        // Enable backpressure
        filter.set_waiting_for_ack(1, true);

        // Queue frames of different priorities
        let mut low_env = make_busmon_envelope(1);
        filter.process_send(&mut low_env);

        let mut normal_env = make_envelope(1);
        filter.process_send(&mut normal_env);

        let mut high_env = make_confirmation_envelope(1);
        filter.process_send(&mut high_env);

        // Drain should return High → Normal → Low
        let drained = filter.drain(1, 10);
        assert_eq!(drained.len(), 3);
        assert_eq!(drained[0].priority, QueuePriority::High);
        assert_eq!(drained[1].priority, QueuePriority::Normal);
        assert_eq!(drained[2].priority, QueuePriority::Low);
    }

    #[test]
    fn test_queue_filter_queue_full_drop() {
        let mut config = QueueFilterConfig::default();
        config.max_queue_depth = 2;
        config.priority_eviction_enabled = false; // Disable eviction
        let filter = QueueFilter::new(config);

        filter.set_waiting_for_ack(1, true);

        // Fill the queue
        for _ in 0..2 {
            let mut envelope = make_envelope(1);
            let result = filter.process_send(&mut envelope);
            assert!(matches!(result, FilterResult::Queued));
        }

        // Third frame should be dropped
        let mut envelope = make_envelope(1);
        let result = filter.process_send(&mut envelope);
        assert!(matches!(result, FilterResult::Dropped { .. }));

        let stats = filter.stats_snapshot();
        assert_eq!(stats.dropped_full, 1);
    }

    #[test]
    fn test_queue_filter_priority_eviction() {
        let mut config = QueueFilterConfig::default();
        config.max_queue_depth = 1;
        config.priority_eviction_enabled = true;
        let filter = QueueFilter::new(config);

        filter.set_waiting_for_ack(1, true);

        // Queue a low-priority frame (fills Low queue to max_queue_depth=1)
        let mut low_env = make_busmon_envelope(1);
        let result = filter.process_send(&mut low_env);
        assert!(matches!(result, FilterResult::Queued));

        // Queue a normal-priority frame (Normal queue is empty, direct enqueue)
        let mut normal_env1 = make_envelope(1);
        let result = filter.process_send(&mut normal_env1);
        assert!(matches!(result, FilterResult::Queued));

        // Verify both queued: Low=1, Normal=1
        let counts = filter.pending_by_priority(1);
        assert_eq!(counts[0], 1); // Low: still present
        assert_eq!(counts[1], 1); // Normal: queued

        // Now queue ANOTHER normal-priority frame — Normal queue is full (1 >= max_queue_depth=1)
        // This triggers eviction from the Low queue to make room
        let mut normal_env2 = make_envelope(1);
        let result = filter.process_send(&mut normal_env2);
        assert!(matches!(result, FilterResult::Queued));

        // Low was evicted, second normal enqueued into Normal queue
        let counts = filter.pending_by_priority(1);
        assert_eq!(counts[0], 0); // Low: evicted
        assert_eq!(counts[1], 2); // Normal: 2 frames (the eviction freed a slot, allowing enqueue)

        let stats = filter.stats_snapshot();
        assert_eq!(stats.evicted_frames, 1);
    }

    #[test]
    fn test_queue_filter_multi_channel() {
        let config = QueueFilterConfig::default();
        let filter = QueueFilter::new(config);

        filter.set_waiting_for_ack(1, true);
        filter.set_waiting_for_ack(2, true);

        // Queue frames for both channels
        let mut env1 = make_envelope(1);
        filter.process_send(&mut env1);
        let mut env2 = make_envelope(2);
        filter.process_send(&mut env2);

        assert_eq!(filter.total_pending(), 2);
        assert!(filter.has_pending(1));
        assert!(filter.has_pending(2));

        // Drain channel 1 only
        let drained = filter.drain(1, 10);
        assert_eq!(drained.len(), 1);
        assert!(!filter.has_pending(1));
        assert!(filter.has_pending(2));
    }

    #[test]
    fn test_queue_filter_clear_channel() {
        let config = QueueFilterConfig::default();
        let filter = QueueFilter::new(config);

        filter.set_waiting_for_ack(1, true);
        let mut env = make_envelope(1);
        filter.process_send(&mut env);

        filter.clear_channel(1);
        assert!(!filter.has_pending(1));
        assert_eq!(filter.total_pending(), 0);
    }

    #[test]
    fn test_queue_filter_pending_by_priority() {
        let config = QueueFilterConfig::default();
        let filter = QueueFilter::new(config);

        filter.set_waiting_for_ack(1, true);

        // Queue frames of different types
        let mut env1 = make_busmon_envelope(1);    // Low
        let mut env2 = make_envelope(1);           // Normal
        let mut env3 = make_confirmation_envelope(1); // High

        filter.process_send(&mut env1);
        filter.process_send(&mut env2);
        filter.process_send(&mut env3);

        let counts = filter.pending_by_priority(1);
        assert_eq!(counts[0], 1); // Low
        assert_eq!(counts[1], 1); // Normal
        assert_eq!(counts[2], 1); // High
    }

    #[test]
    fn test_queue_filter_config_validate() {
        let config = QueueFilterConfig::default();
        assert!(config.validate().is_ok());

        let mut bad_config = QueueFilterConfig::default();
        bad_config.max_queue_depth = 0;
        assert!(bad_config.validate().is_err());
    }

    #[test]
    fn test_queue_filter_recv_passthrough() {
        let config = QueueFilterConfig::default();
        let filter = QueueFilter::new(config);

        let envelope = make_envelope(1);
        let result = filter.process_recv(&envelope);
        assert!(result.should_continue());
    }

    #[test]
    fn test_queue_filter_debug() {
        let config = QueueFilterConfig::default();
        let filter = QueueFilter::new(config);
        let debug_str = format!("{:?}", filter);
        assert!(debug_str.contains("QueueFilter"));
        assert!(debug_str.contains("enabled"));
    }

    #[test]
    fn test_queue_filter_stats() {
        let config = QueueFilterConfig::default();
        let filter = QueueFilter::new(config);

        // Several operations
        let mut env = make_envelope(1);
        filter.process_send(&mut env);

        filter.set_waiting_for_ack(1, true);
        let mut env2 = make_envelope(1);
        filter.process_send(&mut env2);

        let stats = filter.stats_snapshot();
        assert_eq!(stats.direct_pass, 1);
        assert_eq!(stats.queued_frames, 1);
    }

    #[test]
    fn test_max_total_frames_limit() {
        let mut config = QueueFilterConfig::default();
        config.max_total_frames = 2;
        let filter = QueueFilter::new(config);

        filter.set_waiting_for_ack(1, true);

        // Queue 2 frames
        for _ in 0..2 {
            let mut env = make_envelope(1);
            let result = filter.process_send(&mut env);
            assert!(matches!(result, FilterResult::Queued));
        }

        // Third should be dropped (total limit)
        let mut env = make_envelope(1);
        let result = filter.process_send(&mut env);
        assert!(matches!(result, FilterResult::Dropped { .. }));
    }
}

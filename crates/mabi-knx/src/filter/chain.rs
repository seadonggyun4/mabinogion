//! Bidirectional filter chain pipeline.
//!
//! The [`FilterChain`] orchestrates frame processing through a pipeline of
//! filters in both send (forward) and receive (reverse) directions.
//!
//! ```text
//! send():  Client → Filter[0] → Filter[1] → ... → Filter[N] → Bus
//! recv():  Client ← Filter[0] ← Filter[1] ← ... ← Filter[N] ← Bus
//! ```
//!
//! Each filter can:
//! - **Pass** the frame through (possibly with delay)
//! - **Queue** the frame for later delivery
//! - **Drop** the frame (e.g., circuit breaker open)
//! - **Error** with a specific reason

use std::fmt;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tracing::{debug, trace};

use crate::cemi::CemiFrame;

use super::pace::{PaceFilter, PaceFilterConfig};
use super::queue::{QueueFilter, QueueFilterConfig, QueuePriority};
use super::retry::{RetryFilter, RetryFilterConfig};

// ============================================================================
// Frame Envelope — carries frame + metadata through the pipeline
// ============================================================================

/// Frame envelope carrying a cEMI frame with pipeline metadata.
///
/// The envelope tracks the frame's journey through the filter chain,
/// including original timestamps, priority, channel association, and
/// the number of pipeline stages it has passed through.
#[derive(Debug, Clone)]
pub struct FrameEnvelope {
    /// The cEMI frame payload.
    pub cemi: CemiFrame,
    /// Channel ID of the originating connection.
    pub channel_id: u8,
    /// Target client address for delivery.
    pub target_addr: SocketAddr,
    /// Frame priority (determined by QueueFilter classification).
    pub priority: QueuePriority,
    /// Timestamp when the frame entered the pipeline.
    pub enqueued_at: Instant,
    /// Accumulated delay from all filters in the chain.
    pub accumulated_delay: Duration,
    /// Number of retry attempts (managed by RetryFilter).
    pub retry_count: u8,
    /// Whether this frame requires an ACK from the client.
    pub requires_ack: bool,
    /// Original frame size in bytes (for PaceFilter timing calculation).
    pub frame_size_bytes: usize,
}

impl FrameEnvelope {
    /// Create a new frame envelope.
    pub fn new(
        cemi: CemiFrame,
        channel_id: u8,
        target_addr: SocketAddr,
    ) -> Self {
        let frame_size_bytes = cemi.encode().len();
        Self {
            cemi,
            channel_id,
            target_addr,
            priority: QueuePriority::Normal,
            enqueued_at: Instant::now(),
            accumulated_delay: Duration::ZERO,
            retry_count: 0,
            requires_ack: true,
            frame_size_bytes,
        }
    }

    /// Create with explicit priority.
    pub fn with_priority(mut self, priority: QueuePriority) -> Self {
        self.priority = priority;
        self
    }

    /// Create with ACK requirement flag.
    pub fn with_ack_required(mut self, requires_ack: bool) -> Self {
        self.requires_ack = requires_ack;
        self
    }

    /// Get total time in pipeline.
    pub fn pipeline_duration(&self) -> Duration {
        self.enqueued_at.elapsed()
    }
}

// ============================================================================
// Filter Result
// ============================================================================

/// Result of a filter processing a frame.
#[derive(Debug, Clone)]
pub enum FilterResult {
    /// Frame passed through, optionally with added delay.
    Pass {
        /// Additional delay to apply before actual transmission.
        delay: Duration,
    },
    /// Frame was queued internally — will be delivered later.
    /// The pipeline should not proceed; the filter owns the frame.
    Queued,
    /// Frame was dropped (e.g., circuit breaker open, queue full).
    Dropped {
        /// Human-readable reason for dropping.
        reason: String,
    },
    /// An error occurred during filtering.
    Error {
        /// Error description.
        message: String,
    },
}

impl FilterResult {
    /// Convenience for pass with no delay.
    pub fn pass() -> Self {
        Self::Pass {
            delay: Duration::ZERO,
        }
    }

    /// Convenience for pass with delay.
    pub fn pass_with_delay(delay: Duration) -> Self {
        Self::Pass { delay }
    }

    /// Whether the frame should continue through the pipeline.
    pub fn should_continue(&self) -> bool {
        matches!(self, Self::Pass { .. })
    }
}

// ============================================================================
// Filter Direction
// ============================================================================

/// Direction of frame flow through the filter chain.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilterDirection {
    /// Client → Bus (outbound). Filters are applied in forward order.
    Send,
    /// Bus → Client (inbound). Filters are applied in reverse order.
    Recv,
}

impl fmt::Display for FilterDirection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Send => write!(f, "send"),
            Self::Recv => write!(f, "recv"),
        }
    }
}

// ============================================================================
// Filter Chain Configuration
// ============================================================================

/// Configuration for the complete filter chain.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FilterChainConfig {
    /// Whether the filter chain is enabled at all.
    /// When disabled, frames pass through with zero overhead.
    #[serde(default)]
    pub enabled: bool,

    /// PaceFilter configuration (bus timing simulation).
    #[serde(default)]
    pub pace: PaceFilterConfig,

    /// QueueFilter configuration (priority queuing).
    #[serde(default)]
    pub queue: QueueFilterConfig,

    /// RetryFilter configuration (retry + circuit breaker).
    #[serde(default)]
    pub retry: RetryFilterConfig,
}

impl Default for FilterChainConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            pace: PaceFilterConfig::default(),
            queue: QueueFilterConfig::default(),
            retry: RetryFilterConfig::default(),
        }
    }
}

impl FilterChainConfig {
    /// Create an enabled filter chain with default sub-filter configs.
    pub fn enabled() -> Self {
        Self {
            enabled: true,
            ..Default::default()
        }
    }

    /// Validate the configuration.
    pub fn validate(&self) -> Result<(), String> {
        self.pace.validate()?;
        self.queue.validate()?;
        self.retry.validate()?;
        Ok(())
    }
}

// ============================================================================
// Filter Chain Statistics
// ============================================================================

/// Lock-free filter chain statistics.
#[derive(Debug, Default)]
pub struct FilterChainStats {
    /// Total frames processed in send direction.
    pub frames_sent: AtomicU64,
    /// Total frames processed in recv direction.
    pub frames_received: AtomicU64,
    /// Total frames dropped across all filters.
    pub frames_dropped: AtomicU64,
    /// Total frames queued across all filters.
    pub frames_queued: AtomicU64,
    /// Total accumulated delay applied (in microseconds).
    pub total_delay_us: AtomicU64,
    /// Total filter chain bypass (disabled) count.
    pub bypass_count: AtomicU64,
}

/// Snapshot of filter chain statistics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FilterChainStatsSnapshot {
    pub frames_sent: u64,
    pub frames_received: u64,
    pub frames_dropped: u64,
    pub frames_queued: u64,
    pub total_delay_us: u64,
    pub bypass_count: u64,
}

// ============================================================================
// Filter Chain
// ============================================================================

/// Bidirectional filter chain for KNX bus flow control.
///
/// The chain processes frames through three stages:
/// 1. **QueueFilter**: Priority-based queuing with backpressure
/// 2. **PaceFilter**: Bus timing enforcement (inter-frame delay)
/// 3. **RetryFilter**: Retry logic with circuit breaker protection
///
/// In the send direction (Client → Bus), filters are applied in order:
///   Queue → Pace → Retry
///
/// In the recv direction (Bus → Client), filters are applied in reverse:
///   Retry → Pace → Queue
pub struct FilterChain {
    config: FilterChainConfig,
    queue_filter: QueueFilter,
    pace_filter: PaceFilter,
    retry_filter: RetryFilter,
    stats: Arc<FilterChainStats>,
}

impl FilterChain {
    /// Create a new filter chain with the given configuration.
    pub fn new(config: FilterChainConfig) -> Self {
        Self {
            queue_filter: QueueFilter::new(config.queue.clone()),
            pace_filter: PaceFilter::new(config.pace.clone()),
            retry_filter: RetryFilter::new(config.retry.clone()),
            config,
            stats: Arc::new(FilterChainStats::default()),
        }
    }

    /// Check if the filter chain is enabled.
    pub fn is_enabled(&self) -> bool {
        self.config.enabled
    }

    /// Process a frame through the send (forward) direction.
    ///
    /// Pipeline order: Queue → Pace → Retry
    ///
    /// Returns the total delay to apply before transmission, or an error
    /// if the frame was dropped/queued.
    pub fn send(&self, envelope: &mut FrameEnvelope) -> FilterResult {
        if !self.config.enabled {
            self.stats.bypass_count.fetch_add(1, Ordering::Relaxed);
            return FilterResult::pass();
        }

        self.stats.frames_sent.fetch_add(1, Ordering::Relaxed);

        // Stage 1: QueueFilter — priority classification + backpressure
        let queue_result = self.queue_filter.process_send(envelope);
        if !queue_result.should_continue() {
            match &queue_result {
                FilterResult::Queued => {
                    self.stats.frames_queued.fetch_add(1, Ordering::Relaxed);
                    trace!(
                        channel_id = envelope.channel_id,
                        priority = ?envelope.priority,
                        "Frame queued by QueueFilter"
                    );
                }
                FilterResult::Dropped { reason } => {
                    self.stats.frames_dropped.fetch_add(1, Ordering::Relaxed);
                    debug!(
                        channel_id = envelope.channel_id,
                        reason = %reason,
                        "Frame dropped by QueueFilter"
                    );
                }
                _ => {}
            }
            return queue_result;
        }

        // Stage 2: PaceFilter — bus timing enforcement
        let pace_result = self.pace_filter.process_send(envelope);
        if !pace_result.should_continue() {
            match &pace_result {
                FilterResult::Dropped { reason } => {
                    self.stats.frames_dropped.fetch_add(1, Ordering::Relaxed);
                    debug!(
                        channel_id = envelope.channel_id,
                        reason = %reason,
                        "Frame dropped by PaceFilter"
                    );
                }
                _ => {}
            }
            return pace_result;
        }
        if let FilterResult::Pass { delay } = &pace_result {
            envelope.accumulated_delay += *delay;
        }

        // Stage 3: RetryFilter — circuit breaker check
        let retry_result = self.retry_filter.process_send(envelope);
        if !retry_result.should_continue() {
            match &retry_result {
                FilterResult::Dropped { reason } => {
                    self.stats.frames_dropped.fetch_add(1, Ordering::Relaxed);
                    debug!(
                        channel_id = envelope.channel_id,
                        reason = %reason,
                        "Frame dropped by RetryFilter (circuit breaker)"
                    );
                }
                _ => {}
            }
            return retry_result;
        }
        if let FilterResult::Pass { delay } = &retry_result {
            envelope.accumulated_delay += *delay;
        }

        // Record total accumulated delay
        let total_delay = envelope.accumulated_delay;
        if total_delay > Duration::ZERO {
            self.stats
                .total_delay_us
                .fetch_add(total_delay.as_micros() as u64, Ordering::Relaxed);
        }

        trace!(
            channel_id = envelope.channel_id,
            delay_ms = total_delay.as_millis(),
            priority = ?envelope.priority,
            "Frame passed through filter chain (send)"
        );

        FilterResult::pass_with_delay(total_delay)
    }

    /// Process a frame through the recv (reverse) direction.
    ///
    /// Pipeline order: Retry → Pace → Queue
    ///
    /// For the receive path, we apply filters in reverse to maintain
    /// the bidirectional pipeline semantics.
    pub fn recv(&self, envelope: &mut FrameEnvelope) -> FilterResult {
        if !self.config.enabled {
            self.stats.bypass_count.fetch_add(1, Ordering::Relaxed);
            return FilterResult::pass();
        }

        self.stats.frames_received.fetch_add(1, Ordering::Relaxed);

        // Stage 1 (reverse): RetryFilter — record successful delivery
        let retry_result = self.retry_filter.process_recv(envelope);
        if !retry_result.should_continue() {
            return retry_result;
        }

        // Stage 2 (reverse): PaceFilter — update timing state
        let pace_result = self.pace_filter.process_recv(envelope);
        if !pace_result.should_continue() {
            return pace_result;
        }

        // Stage 3 (reverse): QueueFilter — dequeue acknowledgment
        let queue_result = self.queue_filter.process_recv(envelope);
        if !queue_result.should_continue() {
            return queue_result;
        }

        trace!(
            channel_id = envelope.channel_id,
            "Frame passed through filter chain (recv)"
        );

        FilterResult::pass()
    }

    /// Notify the filter chain that a frame transmission succeeded.
    ///
    /// This feeds back into the RetryFilter's circuit breaker and
    /// the PaceFilter's state machine.
    pub fn on_send_success(&self, channel_id: u8) {
        if !self.config.enabled {
            return;
        }
        self.retry_filter.on_success();
        self.pace_filter.on_frame_completed();
        self.queue_filter.on_ack_received(channel_id);

        trace!(channel_id, "Filter chain: send success");
    }

    /// Notify the filter chain that a frame transmission failed.
    ///
    /// This feeds into the RetryFilter's circuit breaker to potentially
    /// trip it open.
    pub fn on_send_failure(&self, channel_id: u8, error: &str) {
        if !self.config.enabled {
            return;
        }
        self.retry_filter.on_failure();
        self.queue_filter.on_send_error(channel_id);

        debug!(
            channel_id,
            error,
            "Filter chain: send failure"
        );
    }

    /// Drain pending frames from the QueueFilter for a specific channel.
    ///
    /// Returns frames in priority order (High → Normal → Low).
    /// Used by the server to poll for queued frames when the connection
    /// transitions from WaitingForAck back to Idle.
    pub fn drain_pending(&self, channel_id: u8, max_count: usize) -> Vec<FrameEnvelope> {
        if !self.config.enabled {
            return Vec::new();
        }
        self.queue_filter.drain(channel_id, max_count)
    }

    /// Check if the chain has pending frames for a channel.
    pub fn has_pending(&self, channel_id: u8) -> bool {
        if !self.config.enabled {
            return false;
        }
        self.queue_filter.has_pending(channel_id)
    }

    /// Get the total pending frame count across all channels.
    pub fn pending_count(&self) -> usize {
        if !self.config.enabled {
            return 0;
        }
        self.queue_filter.total_pending()
    }

    /// Get the current PaceFilter state.
    pub fn pace_state(&self) -> super::pace::PaceState {
        self.pace_filter.state()
    }

    /// Get the current CircuitBreaker state.
    pub fn circuit_breaker_state(&self) -> super::retry::CircuitBreakerState {
        self.retry_filter.circuit_state()
    }

    /// Get a reference to the QueueFilter for inspection.
    pub fn queue_filter(&self) -> &QueueFilter {
        &self.queue_filter
    }

    /// Get a reference to the PaceFilter for inspection.
    pub fn pace_filter(&self) -> &PaceFilter {
        &self.pace_filter
    }

    /// Get a reference to the RetryFilter for inspection.
    pub fn retry_filter(&self) -> &RetryFilter {
        &self.retry_filter
    }

    /// Get a snapshot of the filter chain statistics.
    pub fn stats_snapshot(&self) -> FilterChainStatsSnapshot {
        FilterChainStatsSnapshot {
            frames_sent: self.stats.frames_sent.load(Ordering::Relaxed),
            frames_received: self.stats.frames_received.load(Ordering::Relaxed),
            frames_dropped: self.stats.frames_dropped.load(Ordering::Relaxed),
            frames_queued: self.stats.frames_queued.load(Ordering::Relaxed),
            total_delay_us: self.stats.total_delay_us.load(Ordering::Relaxed),
            bypass_count: self.stats.bypass_count.load(Ordering::Relaxed),
        }
    }
}

impl fmt::Debug for FilterChain {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FilterChain")
            .field("enabled", &self.config.enabled)
            .field("pace_state", &self.pace_filter.state())
            .field("circuit_breaker", &self.retry_filter.circuit_state())
            .field("pending_frames", &self.queue_filter.total_pending())
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

    fn make_envelope() -> FrameEnvelope {
        let cemi = CemiFrame::group_value_write(
            IndividualAddress::new(1, 1, 1),
            GroupAddress::three_level(1, 0, 1),
            vec![0x01],
        );
        FrameEnvelope::new(
            cemi,
            1,
            "192.168.1.100:3671".parse().unwrap(),
        )
    }

    #[test]
    fn test_filter_chain_disabled() {
        let chain = FilterChain::new(FilterChainConfig::default());
        assert!(!chain.is_enabled());

        let mut envelope = make_envelope();
        let result = chain.send(&mut envelope);

        assert!(matches!(result, FilterResult::Pass { delay } if delay == Duration::ZERO));

        let stats = chain.stats_snapshot();
        assert_eq!(stats.bypass_count, 1);
        assert_eq!(stats.frames_sent, 0);
    }

    #[test]
    fn test_filter_chain_enabled_passthrough() {
        let chain = FilterChain::new(FilterChainConfig::enabled());
        assert!(chain.is_enabled());

        let mut envelope = make_envelope();
        let result = chain.send(&mut envelope);

        assert!(result.should_continue());

        let stats = chain.stats_snapshot();
        assert_eq!(stats.frames_sent, 1);
        assert_eq!(stats.bypass_count, 0);
    }

    #[test]
    fn test_filter_chain_send_recv_cycle() {
        let chain = FilterChain::new(FilterChainConfig::enabled());

        // Send
        let mut send_env = make_envelope();
        let send_result = chain.send(&mut send_env);
        assert!(send_result.should_continue());

        // Recv
        let mut recv_env = make_envelope();
        let recv_result = chain.recv(&mut recv_env);
        assert!(recv_result.should_continue());

        let stats = chain.stats_snapshot();
        assert_eq!(stats.frames_sent, 1);
        assert_eq!(stats.frames_received, 1);
    }

    #[test]
    fn test_filter_chain_success_callback() {
        let chain = FilterChain::new(FilterChainConfig::enabled());

        let mut envelope = make_envelope();
        chain.send(&mut envelope);
        chain.on_send_success(1);

        // No assertion needed — just ensure no panic
    }

    #[test]
    fn test_filter_chain_failure_callback() {
        let chain = FilterChain::new(FilterChainConfig::enabled());

        let mut envelope = make_envelope();
        chain.send(&mut envelope);
        chain.on_send_failure(1, "test error");

        // No assertion needed — just ensure no panic
    }

    #[test]
    fn test_filter_chain_debug() {
        let chain = FilterChain::new(FilterChainConfig::enabled());
        let debug_str = format!("{:?}", chain);
        assert!(debug_str.contains("FilterChain"));
        assert!(debug_str.contains("enabled"));
    }

    #[test]
    fn test_filter_chain_config_validate() {
        let config = FilterChainConfig::enabled();
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_frame_envelope_creation() {
        let envelope = make_envelope();
        assert_eq!(envelope.channel_id, 1);
        assert_eq!(envelope.priority, QueuePriority::Normal);
        assert!(envelope.requires_ack);
        assert!(envelope.frame_size_bytes > 0);
        assert_eq!(envelope.retry_count, 0);
        assert_eq!(envelope.accumulated_delay, Duration::ZERO);
    }

    #[test]
    fn test_frame_envelope_builders() {
        let envelope = make_envelope()
            .with_priority(QueuePriority::High)
            .with_ack_required(false);

        assert_eq!(envelope.priority, QueuePriority::High);
        assert!(!envelope.requires_ack);
    }

    #[test]
    fn test_filter_result_variants() {
        let pass = FilterResult::pass();
        assert!(pass.should_continue());

        let delayed = FilterResult::pass_with_delay(Duration::from_millis(50));
        assert!(delayed.should_continue());

        let queued = FilterResult::Queued;
        assert!(!queued.should_continue());

        let dropped = FilterResult::Dropped {
            reason: "test".to_string(),
        };
        assert!(!dropped.should_continue());

        let error = FilterResult::Error {
            message: "test error".to_string(),
        };
        assert!(!error.should_continue());
    }

    #[test]
    fn test_filter_direction_display() {
        assert_eq!(FilterDirection::Send.to_string(), "send");
        assert_eq!(FilterDirection::Recv.to_string(), "recv");
    }

    #[test]
    fn test_drain_pending_disabled() {
        let chain = FilterChain::new(FilterChainConfig::default());
        let drained = chain.drain_pending(1, 10);
        assert!(drained.is_empty());
    }

    #[test]
    fn test_has_pending_disabled() {
        let chain = FilterChain::new(FilterChainConfig::default());
        assert!(!chain.has_pending(1));
    }

    #[test]
    fn test_pending_count_disabled() {
        let chain = FilterChain::new(FilterChainConfig::default());
        assert_eq!(chain.pending_count(), 0);
    }
}

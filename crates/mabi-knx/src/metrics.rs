//! KNXnet/IP server metrics collection and Prometheus exposition.
//!
//! This module provides production-grade observability for the KNXnet/IP
//! simulator by aggregating statistics from all server components into
//! a unified snapshot and exposing them in Prometheus exposition format.
//!
//! ## Architecture
//!
//! ```text
//! KnxServer
//! ├── ConnectionManager       → connection_* metrics
//! │   └── TunnelConnection[]
//! │       ├── SequenceTracker → sequence_* metrics
//! │       ├── TunnelFsm       → fsm_* metrics
//! │       └── AckWaiter       → ack_* metrics (per-connection)
//! ├── FilterChain             → filter_chain_* metrics
//! │   ├── PaceFilter          → pace_* metrics
//! │   ├── QueueFilter         → queue_* metrics
//! │   └── RetryFilter         → retry_* metrics
//! ├── HeartbeatScheduler      → heartbeat_* metrics
//! ├── GroupValueCache         → cache_* metrics
//! └── SendErrorTracker        → error_tracker_* metrics
//! ```
//!
//! ## Prometheus Exposition Format
//!
//! All metrics are prefixed with `knx_` and follow Prometheus naming conventions:
//! - Counters use `_total` suffix
//! - Gauges represent current state values
//! - Rates are computed as derived metrics (hit_rate, fault_rate, error_rate)
//!
//! ## Usage
//!
//! ```rust,ignore
//! use mabi_knx::metrics::{KnxMetricsCollector, KnxMetricsSnapshot};
//!
//! let collector = KnxMetricsCollector::new(&server);
//! let snapshot = collector.collect();
//!
//! // Prometheus text format
//! let text = snapshot.to_prometheus();
//!
//! // JSON format
//! let json = serde_json::to_string_pretty(&snapshot)?;
//! ```

use std::fmt;
use std::time::Instant;

use serde::{Deserialize, Serialize};

use crate::error_tracker::TrackerStatsSnapshot;
use crate::filter::{
    FilterChainStatsSnapshot, PaceFilterStatsSnapshot, QueueFilterStatsSnapshot,
    RetryFilterStatsSnapshot,
};
use crate::group_cache::CacheStatsSnapshot;
use crate::heartbeat::HeartbeatStatsSnapshot;
use crate::tunnel::{FsmStatsSnapshot, SequenceStatsSnapshot};

// ============================================================================
// KnxMetricsSnapshot — unified metrics snapshot
// ============================================================================

/// Unified metrics snapshot aggregating all server component statistics.
///
/// Contains 60+ fields organized by component. Supports serialization
/// to JSON/YAML for external consumption and Prometheus exposition format
/// for standard monitoring integration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnxMetricsSnapshot {
    /// Snapshot timestamp (milliseconds since collector creation).
    pub timestamp_ms: u64,

    // ========================================================================
    // Server-level metrics
    // ========================================================================
    /// Current server state (Stopped=0, Starting=1, Running=2, Stopping=3).
    pub server_state: u8,
    /// Number of active tunnel connections.
    pub active_connections: u64,
    /// Maximum connection capacity.
    pub max_connections: u64,

    // ========================================================================
    // Heartbeat metrics (Phase 4)
    // ========================================================================
    /// Total heartbeat requests processed.
    pub heartbeat_total_requests: u64,
    /// Number of Continue (0x00) responses.
    pub heartbeat_continue_count: u64,
    /// Number of ImmediateReconnect (0x21) responses.
    pub heartbeat_immediate_reconnect_count: u64,
    /// Number of AbandonTunnel (0x27) responses.
    pub heartbeat_abandon_tunnel_count: u64,
    /// Number of DelayedReconnect (0x29) responses.
    pub heartbeat_delayed_reconnect_count: u64,
    /// Number of NoResponse (timeout simulation) instances.
    pub heartbeat_no_response_count: u64,
    /// Heartbeat fault rate (non-Continue / total). 0.0-1.0.
    pub heartbeat_fault_rate: f64,

    // ========================================================================
    // Group Value Cache metrics (Phase 4)
    // ========================================================================
    /// Total cache hits.
    pub cache_hits_total: u64,
    /// Total cache misses.
    pub cache_misses_total: u64,
    /// Cache hit rate (0.0-1.0).
    pub cache_hit_rate: f64,
    /// Total cache lookups.
    pub cache_lookups_total: u64,
    /// Total cache evictions (LRU).
    pub cache_evictions_total: u64,
    /// Total cache expirations (TTL).
    pub cache_expirations_total: u64,
    /// Total cache updates.
    pub cache_updates_total: u64,
    /// Cache updates from L_Data.ind indications.
    pub cache_indication_updates: u64,
    /// Cache updates from GroupValueWrite.
    pub cache_write_updates: u64,
    /// Current number of cached entries.
    pub cache_entries_current: u64,

    // ========================================================================
    // Send Error Tracker metrics (Phase 4)
    // ========================================================================
    /// Total successful sends across all channels.
    pub error_tracker_successes_total: u64,
    /// Total failed sends across all channels.
    pub error_tracker_failures_total: u64,
    /// Overall error rate (0.0-1.0).
    pub error_tracker_error_rate: f64,
    /// Total events (successes + failures).
    pub error_tracker_events_total: u64,
    /// Number of times consecutive error threshold was exceeded.
    pub error_tracker_consecutive_triggers: u64,
    /// Number of times sliding window rate threshold was exceeded.
    pub error_tracker_rate_triggers: u64,

    // ========================================================================
    // Filter Chain metrics (Phase 3)
    // ========================================================================
    /// Total frames processed in send direction.
    pub filter_chain_frames_sent: u64,
    /// Total frames processed in recv direction.
    pub filter_chain_frames_received: u64,
    /// Total frames dropped by the filter chain.
    pub filter_chain_frames_dropped: u64,
    /// Total frames queued by the filter chain.
    pub filter_chain_frames_queued: u64,
    /// Total accumulated delay (microseconds).
    pub filter_chain_total_delay_us: u64,
    /// Total filter chain bypass count (chain disabled).
    pub filter_chain_bypass_count: u64,

    // ========================================================================
    // PaceFilter metrics (Phase 3)
    // ========================================================================
    /// Frames that passed immediately (no delay needed).
    pub pace_immediate_pass: u64,
    /// Frames that were delayed for bus timing.
    pub pace_delayed_frames: u64,
    /// Frames dropped by the pace filter.
    pub pace_dropped_frames: u64,
    /// Total delay applied by pace filter (microseconds).
    pub pace_total_delay_us: u64,
    /// Busy→Idle state transitions.
    pub pace_busy_to_idle: u64,
    /// Idle→Down state transitions.
    pub pace_idle_to_down: u64,

    // ========================================================================
    // QueueFilter metrics (Phase 3)
    // ========================================================================
    /// Frames that passed directly (no queuing).
    pub queue_direct_pass: u64,
    /// Frames enqueued due to backpressure.
    pub queue_queued_frames: u64,
    /// Frames dropped because queue was full.
    pub queue_dropped_full: u64,
    /// Frames evicted from lower-priority queues.
    pub queue_evicted_frames: u64,
    /// Frames drained from the queue.
    pub queue_drained_frames: u64,
    /// High-priority frames processed.
    pub queue_high_priority: u64,
    /// Normal-priority frames processed.
    pub queue_normal_priority: u64,
    /// Low-priority frames processed.
    pub queue_low_priority: u64,

    // ========================================================================
    // RetryFilter metrics (Phase 3)
    // ========================================================================
    /// Frames that passed directly (circuit closed).
    pub retry_direct_pass: u64,
    /// Frames dropped because circuit breaker was open.
    pub retry_circuit_open_drops: u64,
    /// Probe frames sent during half-open state.
    pub retry_probe_frames: u64,
    /// Total retry attempts.
    pub retry_attempts: u64,
    /// Successful transmissions tracked by retry filter.
    pub retry_successes: u64,
    /// Failed transmissions tracked by retry filter.
    pub retry_failures: u64,
    /// Circuit breaker state transitions.
    pub retry_state_transitions: u64,
    /// Number of times circuit breaker tripped (Closed→Open).
    pub retry_circuit_trips: u64,
    /// Number of times circuit breaker reset (HalfOpen→Closed).
    pub retry_circuit_resets: u64,

    // ========================================================================
    // Aggregated per-connection tunnel metrics
    // ========================================================================
    /// Total frames sent across all connections (sequence tracker).
    pub tunnel_frames_sent_total: u64,
    /// Total frames received across all connections.
    pub tunnel_frames_received_total: u64,
    /// Total duplicate frames detected.
    pub tunnel_duplicates_total: u64,
    /// Total out-of-order frames detected.
    pub tunnel_out_of_order_total: u64,
    /// Total fatal desync events.
    pub tunnel_fatal_desyncs_total: u64,
    /// Total sequence resets.
    pub tunnel_resets_total: u64,
    /// Total FSM state transitions across all connections.
    pub tunnel_fsm_transitions_total: u64,
}

impl KnxMetricsSnapshot {
    /// Create a zero-valued snapshot.
    pub fn zero() -> Self {
        Self {
            timestamp_ms: 0,
            server_state: 0,
            active_connections: 0,
            max_connections: 0,
            heartbeat_total_requests: 0,
            heartbeat_continue_count: 0,
            heartbeat_immediate_reconnect_count: 0,
            heartbeat_abandon_tunnel_count: 0,
            heartbeat_delayed_reconnect_count: 0,
            heartbeat_no_response_count: 0,
            heartbeat_fault_rate: 0.0,
            cache_hits_total: 0,
            cache_misses_total: 0,
            cache_hit_rate: 0.0,
            cache_lookups_total: 0,
            cache_evictions_total: 0,
            cache_expirations_total: 0,
            cache_updates_total: 0,
            cache_indication_updates: 0,
            cache_write_updates: 0,
            cache_entries_current: 0,
            error_tracker_successes_total: 0,
            error_tracker_failures_total: 0,
            error_tracker_error_rate: 0.0,
            error_tracker_events_total: 0,
            error_tracker_consecutive_triggers: 0,
            error_tracker_rate_triggers: 0,
            filter_chain_frames_sent: 0,
            filter_chain_frames_received: 0,
            filter_chain_frames_dropped: 0,
            filter_chain_frames_queued: 0,
            filter_chain_total_delay_us: 0,
            filter_chain_bypass_count: 0,
            pace_immediate_pass: 0,
            pace_delayed_frames: 0,
            pace_dropped_frames: 0,
            pace_total_delay_us: 0,
            pace_busy_to_idle: 0,
            pace_idle_to_down: 0,
            queue_direct_pass: 0,
            queue_queued_frames: 0,
            queue_dropped_full: 0,
            queue_evicted_frames: 0,
            queue_drained_frames: 0,
            queue_high_priority: 0,
            queue_normal_priority: 0,
            queue_low_priority: 0,
            retry_direct_pass: 0,
            retry_circuit_open_drops: 0,
            retry_probe_frames: 0,
            retry_attempts: 0,
            retry_successes: 0,
            retry_failures: 0,
            retry_state_transitions: 0,
            retry_circuit_trips: 0,
            retry_circuit_resets: 0,
            tunnel_frames_sent_total: 0,
            tunnel_frames_received_total: 0,
            tunnel_duplicates_total: 0,
            tunnel_out_of_order_total: 0,
            tunnel_fatal_desyncs_total: 0,
            tunnel_resets_total: 0,
            tunnel_fsm_transitions_total: 0,
        }
    }

    /// Populate heartbeat metrics from a HeartbeatStatsSnapshot.
    pub fn with_heartbeat(mut self, stats: &HeartbeatStatsSnapshot) -> Self {
        self.heartbeat_total_requests = stats.total_requests;
        self.heartbeat_continue_count = stats.continue_count;
        self.heartbeat_immediate_reconnect_count = stats.immediate_reconnect_count;
        self.heartbeat_abandon_tunnel_count = stats.abandon_tunnel_count;
        self.heartbeat_delayed_reconnect_count = stats.delayed_reconnect_count;
        self.heartbeat_no_response_count = stats.no_response_count;
        self.heartbeat_fault_rate = stats.fault_rate();
        self
    }

    /// Populate cache metrics from a CacheStatsSnapshot and current entry count.
    pub fn with_cache(mut self, stats: &CacheStatsSnapshot, current_entries: usize) -> Self {
        self.cache_hits_total = stats.hits;
        self.cache_misses_total = stats.misses;
        self.cache_hit_rate = stats.hit_rate();
        self.cache_lookups_total = stats.total_lookups();
        self.cache_evictions_total = stats.evictions;
        self.cache_expirations_total = stats.expirations;
        self.cache_updates_total = stats.updates;
        self.cache_indication_updates = stats.indication_updates;
        self.cache_write_updates = stats.write_updates;
        self.cache_entries_current = current_entries as u64;
        self
    }

    /// Populate error tracker metrics from a TrackerStatsSnapshot.
    pub fn with_error_tracker(mut self, stats: &TrackerStatsSnapshot) -> Self {
        self.error_tracker_successes_total = stats.total_successes;
        self.error_tracker_failures_total = stats.total_failures;
        self.error_tracker_error_rate = stats.error_rate();
        self.error_tracker_events_total = stats.total_events();
        self.error_tracker_consecutive_triggers = stats.consecutive_triggers;
        self.error_tracker_rate_triggers = stats.rate_triggers;
        self
    }

    /// Populate filter chain metrics from a FilterChainStatsSnapshot.
    pub fn with_filter_chain(mut self, stats: &FilterChainStatsSnapshot) -> Self {
        self.filter_chain_frames_sent = stats.frames_sent;
        self.filter_chain_frames_received = stats.frames_received;
        self.filter_chain_frames_dropped = stats.frames_dropped;
        self.filter_chain_frames_queued = stats.frames_queued;
        self.filter_chain_total_delay_us = stats.total_delay_us;
        self.filter_chain_bypass_count = stats.bypass_count;
        self
    }

    /// Populate pace filter metrics from a PaceFilterStatsSnapshot.
    pub fn with_pace_filter(mut self, stats: &PaceFilterStatsSnapshot) -> Self {
        self.pace_immediate_pass = stats.immediate_pass;
        self.pace_delayed_frames = stats.delayed_frames;
        self.pace_dropped_frames = stats.dropped_frames;
        self.pace_total_delay_us = stats.total_delay_us;
        self.pace_busy_to_idle = stats.busy_to_idle;
        self.pace_idle_to_down = stats.idle_to_down;
        self
    }

    /// Populate queue filter metrics from a QueueFilterStatsSnapshot.
    pub fn with_queue_filter(mut self, stats: &QueueFilterStatsSnapshot) -> Self {
        self.queue_direct_pass = stats.direct_pass;
        self.queue_queued_frames = stats.queued_frames;
        self.queue_dropped_full = stats.dropped_full;
        self.queue_evicted_frames = stats.evicted_frames;
        self.queue_drained_frames = stats.drained_frames;
        self.queue_high_priority = stats.high_priority;
        self.queue_normal_priority = stats.normal_priority;
        self.queue_low_priority = stats.low_priority;
        self
    }

    /// Populate retry filter metrics from a RetryFilterStatsSnapshot.
    pub fn with_retry_filter(mut self, stats: &RetryFilterStatsSnapshot) -> Self {
        self.retry_direct_pass = stats.direct_pass;
        self.retry_circuit_open_drops = stats.circuit_open_drops;
        self.retry_probe_frames = stats.probe_frames;
        self.retry_attempts = stats.retry_attempts;
        self.retry_successes = stats.successes;
        self.retry_failures = stats.failures;
        self.retry_state_transitions = stats.state_transitions;
        self.retry_circuit_trips = stats.circuit_trips;
        self.retry_circuit_resets = stats.circuit_resets;
        self
    }

    /// Populate tunnel metrics from aggregated per-connection snapshots.
    pub fn with_tunnel_aggregate(
        mut self,
        seq_stats: &[SequenceStatsSnapshot],
        fsm_stats: &[FsmStatsSnapshot],
    ) -> Self {
        let mut frames_sent = 0u64;
        let mut frames_received = 0u64;
        let mut duplicates = 0u64;
        let mut out_of_order = 0u64;
        let mut fatal_desyncs = 0u64;
        let mut resets = 0u64;

        for s in seq_stats {
            frames_sent += s.frames_sent;
            frames_received += s.frames_received;
            duplicates += s.duplicates_detected;
            out_of_order += s.out_of_order_detected;
            fatal_desyncs += s.fatal_desyncs;
            resets += s.resets;
        }

        self.tunnel_frames_sent_total = frames_sent;
        self.tunnel_frames_received_total = frames_received;
        self.tunnel_duplicates_total = duplicates;
        self.tunnel_out_of_order_total = out_of_order;
        self.tunnel_fatal_desyncs_total = fatal_desyncs;
        self.tunnel_resets_total = resets;

        let mut fsm_transitions = 0u64;
        for f in fsm_stats {
            fsm_transitions += f.transitions;
        }
        self.tunnel_fsm_transitions_total = fsm_transitions;

        self
    }

    /// Render the snapshot in Prometheus text exposition format.
    ///
    /// All metrics use the `knx_` prefix. Counters have `_total` suffix.
    /// Gauges (current-state values) have no suffix.
    ///
    /// Output follows the Prometheus exposition format:
    /// ```text
    /// # HELP knx_server_state Current server state
    /// # TYPE knx_server_state gauge
    /// knx_server_state 2
    /// ```
    pub fn to_prometheus(&self) -> String {
        let mut out = String::with_capacity(4096);

        // Server-level
        prom_gauge(
            &mut out,
            "knx_server_state",
            "Current server state (0=Stopped,1=Starting,2=Running,3=Stopping)",
            self.server_state as f64,
        );
        prom_gauge(
            &mut out,
            "knx_active_connections",
            "Number of active tunnel connections",
            self.active_connections as f64,
        );
        prom_gauge(
            &mut out,
            "knx_max_connections",
            "Maximum connection capacity",
            self.max_connections as f64,
        );

        // Heartbeat
        prom_counter(
            &mut out,
            "knx_heartbeat_requests_total",
            "Total heartbeat requests processed",
            self.heartbeat_total_requests as f64,
        );
        prom_counter(
            &mut out,
            "knx_heartbeat_continue_total",
            "Continue (0x00) heartbeat responses",
            self.heartbeat_continue_count as f64,
        );
        prom_counter(
            &mut out,
            "knx_heartbeat_immediate_reconnect_total",
            "ImmediateReconnect (0x21) heartbeat responses",
            self.heartbeat_immediate_reconnect_count as f64,
        );
        prom_counter(
            &mut out,
            "knx_heartbeat_abandon_tunnel_total",
            "AbandonTunnel (0x27) heartbeat responses",
            self.heartbeat_abandon_tunnel_count as f64,
        );
        prom_counter(
            &mut out,
            "knx_heartbeat_delayed_reconnect_total",
            "DelayedReconnect (0x29) heartbeat responses",
            self.heartbeat_delayed_reconnect_count as f64,
        );
        prom_counter(
            &mut out,
            "knx_heartbeat_no_response_total",
            "NoResponse (timeout simulation) heartbeat instances",
            self.heartbeat_no_response_count as f64,
        );
        prom_gauge(
            &mut out,
            "knx_heartbeat_fault_rate",
            "Heartbeat fault rate (non-Continue / total)",
            self.heartbeat_fault_rate,
        );

        // Group Value Cache
        prom_counter(
            &mut out,
            "knx_cache_hits_total",
            "Total cache hits",
            self.cache_hits_total as f64,
        );
        prom_counter(
            &mut out,
            "knx_cache_misses_total",
            "Total cache misses",
            self.cache_misses_total as f64,
        );
        prom_gauge(
            &mut out,
            "knx_cache_hit_rate",
            "Cache hit rate (0.0-1.0)",
            self.cache_hit_rate,
        );
        prom_counter(
            &mut out,
            "knx_cache_lookups_total",
            "Total cache lookups",
            self.cache_lookups_total as f64,
        );
        prom_counter(
            &mut out,
            "knx_cache_evictions_total",
            "Cache LRU evictions",
            self.cache_evictions_total as f64,
        );
        prom_counter(
            &mut out,
            "knx_cache_expirations_total",
            "Cache TTL expirations",
            self.cache_expirations_total as f64,
        );
        prom_counter(
            &mut out,
            "knx_cache_updates_total",
            "Total cache updates",
            self.cache_updates_total as f64,
        );
        prom_counter(
            &mut out,
            "knx_cache_indication_updates_total",
            "Cache updates from L_Data.ind",
            self.cache_indication_updates as f64,
        );
        prom_counter(
            &mut out,
            "knx_cache_write_updates_total",
            "Cache updates from GroupValueWrite",
            self.cache_write_updates as f64,
        );
        prom_gauge(
            &mut out,
            "knx_cache_entries",
            "Current number of cached entries",
            self.cache_entries_current as f64,
        );

        // Send Error Tracker
        prom_counter(
            &mut out,
            "knx_error_tracker_successes_total",
            "Total successful sends",
            self.error_tracker_successes_total as f64,
        );
        prom_counter(
            &mut out,
            "knx_error_tracker_failures_total",
            "Total failed sends",
            self.error_tracker_failures_total as f64,
        );
        prom_gauge(
            &mut out,
            "knx_error_tracker_error_rate",
            "Overall error rate (0.0-1.0)",
            self.error_tracker_error_rate,
        );
        prom_counter(
            &mut out,
            "knx_error_tracker_events_total",
            "Total tracked events",
            self.error_tracker_events_total as f64,
        );
        prom_counter(
            &mut out,
            "knx_error_tracker_consecutive_triggers_total",
            "Consecutive threshold trigger count",
            self.error_tracker_consecutive_triggers as f64,
        );
        prom_counter(
            &mut out,
            "knx_error_tracker_rate_triggers_total",
            "Rate threshold trigger count",
            self.error_tracker_rate_triggers as f64,
        );

        // Filter Chain
        prom_counter(
            &mut out,
            "knx_filter_chain_frames_sent_total",
            "Frames processed in send direction",
            self.filter_chain_frames_sent as f64,
        );
        prom_counter(
            &mut out,
            "knx_filter_chain_frames_received_total",
            "Frames processed in recv direction",
            self.filter_chain_frames_received as f64,
        );
        prom_counter(
            &mut out,
            "knx_filter_chain_frames_dropped_total",
            "Frames dropped by filter chain",
            self.filter_chain_frames_dropped as f64,
        );
        prom_counter(
            &mut out,
            "knx_filter_chain_frames_queued_total",
            "Frames queued by filter chain",
            self.filter_chain_frames_queued as f64,
        );
        prom_counter(
            &mut out,
            "knx_filter_chain_total_delay_us",
            "Total accumulated filter delay (microseconds)",
            self.filter_chain_total_delay_us as f64,
        );
        prom_counter(
            &mut out,
            "knx_filter_chain_bypass_total",
            "Filter chain bypass count (disabled)",
            self.filter_chain_bypass_count as f64,
        );

        // PaceFilter
        prom_counter(
            &mut out,
            "knx_pace_immediate_pass_total",
            "Frames passed immediately (no delay)",
            self.pace_immediate_pass as f64,
        );
        prom_counter(
            &mut out,
            "knx_pace_delayed_frames_total",
            "Frames delayed for bus timing",
            self.pace_delayed_frames as f64,
        );
        prom_counter(
            &mut out,
            "knx_pace_dropped_frames_total",
            "Frames dropped by pace filter",
            self.pace_dropped_frames as f64,
        );
        prom_counter(
            &mut out,
            "knx_pace_total_delay_us",
            "Total pace filter delay (microseconds)",
            self.pace_total_delay_us as f64,
        );
        prom_counter(
            &mut out,
            "knx_pace_busy_to_idle_total",
            "Busy to Idle state transitions",
            self.pace_busy_to_idle as f64,
        );
        prom_counter(
            &mut out,
            "knx_pace_idle_to_down_total",
            "Idle to Down state transitions",
            self.pace_idle_to_down as f64,
        );

        // QueueFilter
        prom_counter(
            &mut out,
            "knx_queue_direct_pass_total",
            "Frames passed directly (no queuing)",
            self.queue_direct_pass as f64,
        );
        prom_counter(
            &mut out,
            "knx_queue_queued_frames_total",
            "Frames enqueued due to backpressure",
            self.queue_queued_frames as f64,
        );
        prom_counter(
            &mut out,
            "knx_queue_dropped_full_total",
            "Frames dropped (queue full)",
            self.queue_dropped_full as f64,
        );
        prom_counter(
            &mut out,
            "knx_queue_evicted_frames_total",
            "Frames evicted from lower-priority queues",
            self.queue_evicted_frames as f64,
        );
        prom_counter(
            &mut out,
            "knx_queue_drained_frames_total",
            "Frames drained from queue",
            self.queue_drained_frames as f64,
        );
        prom_counter(
            &mut out,
            "knx_queue_high_priority_total",
            "High-priority frames processed",
            self.queue_high_priority as f64,
        );
        prom_counter(
            &mut out,
            "knx_queue_normal_priority_total",
            "Normal-priority frames processed",
            self.queue_normal_priority as f64,
        );
        prom_counter(
            &mut out,
            "knx_queue_low_priority_total",
            "Low-priority frames processed",
            self.queue_low_priority as f64,
        );

        // RetryFilter
        prom_counter(
            &mut out,
            "knx_retry_direct_pass_total",
            "Frames passed directly (circuit closed)",
            self.retry_direct_pass as f64,
        );
        prom_counter(
            &mut out,
            "knx_retry_circuit_open_drops_total",
            "Frames dropped (circuit breaker open)",
            self.retry_circuit_open_drops as f64,
        );
        prom_counter(
            &mut out,
            "knx_retry_probe_frames_total",
            "Probe frames during half-open state",
            self.retry_probe_frames as f64,
        );
        prom_counter(
            &mut out,
            "knx_retry_attempts_total",
            "Total retry attempts",
            self.retry_attempts as f64,
        );
        prom_counter(
            &mut out,
            "knx_retry_successes_total",
            "Successful transmissions (retry filter)",
            self.retry_successes as f64,
        );
        prom_counter(
            &mut out,
            "knx_retry_failures_total",
            "Failed transmissions (retry filter)",
            self.retry_failures as f64,
        );
        prom_counter(
            &mut out,
            "knx_retry_state_transitions_total",
            "Circuit breaker state transitions",
            self.retry_state_transitions as f64,
        );
        prom_counter(
            &mut out,
            "knx_retry_circuit_trips_total",
            "Circuit breaker trips (Closed to Open)",
            self.retry_circuit_trips as f64,
        );
        prom_counter(
            &mut out,
            "knx_retry_circuit_resets_total",
            "Circuit breaker resets (HalfOpen to Closed)",
            self.retry_circuit_resets as f64,
        );

        // Tunnel (aggregated per-connection)
        prom_counter(
            &mut out,
            "knx_tunnel_frames_sent_total",
            "Total frames sent across all connections",
            self.tunnel_frames_sent_total as f64,
        );
        prom_counter(
            &mut out,
            "knx_tunnel_frames_received_total",
            "Total frames received across all connections",
            self.tunnel_frames_received_total as f64,
        );
        prom_counter(
            &mut out,
            "knx_tunnel_duplicates_total",
            "Total duplicate frames detected",
            self.tunnel_duplicates_total as f64,
        );
        prom_counter(
            &mut out,
            "knx_tunnel_out_of_order_total",
            "Total out-of-order frames detected",
            self.tunnel_out_of_order_total as f64,
        );
        prom_counter(
            &mut out,
            "knx_tunnel_fatal_desyncs_total",
            "Total fatal desync events",
            self.tunnel_fatal_desyncs_total as f64,
        );
        prom_counter(
            &mut out,
            "knx_tunnel_resets_total",
            "Total sequence resets",
            self.tunnel_resets_total as f64,
        );
        prom_counter(
            &mut out,
            "knx_tunnel_fsm_transitions_total",
            "Total FSM state transitions",
            self.tunnel_fsm_transitions_total as f64,
        );

        out
    }

    /// Render a human-readable summary of key metrics.
    pub fn summary(&self) -> String {
        let mut out = String::with_capacity(1024);

        out.push_str("=== KNXnet/IP Metrics Summary ===\n");
        out.push_str(&format!(
            "Server state: {}\n",
            match self.server_state {
                0 => "Stopped",
                1 => "Starting",
                2 => "Running",
                3 => "Stopping",
                _ => "Unknown",
            }
        ));
        out.push_str(&format!(
            "Connections: {}/{}\n",
            self.active_connections, self.max_connections
        ));
        out.push_str(&format!("\n--- Heartbeat ---\n"));
        out.push_str(&format!(
            "Requests: {}  Fault rate: {:.1}%\n",
            self.heartbeat_total_requests,
            self.heartbeat_fault_rate * 100.0
        ));
        out.push_str(&format!(
            "  Continue={} ImmRecon={} Abandon={} DelayRecon={} NoResp={}\n",
            self.heartbeat_continue_count,
            self.heartbeat_immediate_reconnect_count,
            self.heartbeat_abandon_tunnel_count,
            self.heartbeat_delayed_reconnect_count,
            self.heartbeat_no_response_count,
        ));
        out.push_str(&format!("\n--- Cache ---\n"));
        out.push_str(&format!(
            "Hit rate: {:.1}%  Lookups: {}  Entries: {}\n",
            self.cache_hit_rate * 100.0,
            self.cache_lookups_total,
            self.cache_entries_current
        ));
        out.push_str(&format!(
            "  Hits={} Misses={} Evictions={} Expirations={}\n",
            self.cache_hits_total,
            self.cache_misses_total,
            self.cache_evictions_total,
            self.cache_expirations_total
        ));
        out.push_str(&format!("\n--- Error Tracker ---\n"));
        out.push_str(&format!(
            "Error rate: {:.1}%  Events: {}\n",
            self.error_tracker_error_rate * 100.0,
            self.error_tracker_events_total
        ));
        out.push_str(&format!(
            "  Successes={} Failures={} ConsecTrigs={} RateTrigs={}\n",
            self.error_tracker_successes_total,
            self.error_tracker_failures_total,
            self.error_tracker_consecutive_triggers,
            self.error_tracker_rate_triggers,
        ));
        out.push_str(&format!("\n--- Filter Chain ---\n"));
        out.push_str(&format!(
            "Sent={} Received={} Dropped={} Queued={} Bypassed={}\n",
            self.filter_chain_frames_sent,
            self.filter_chain_frames_received,
            self.filter_chain_frames_dropped,
            self.filter_chain_frames_queued,
            self.filter_chain_bypass_count,
        ));
        out.push_str(&format!("\n--- Tunnel ---\n"));
        out.push_str(&format!(
            "Sent={} Received={} Dupes={} OoO={} Desyncs={}\n",
            self.tunnel_frames_sent_total,
            self.tunnel_frames_received_total,
            self.tunnel_duplicates_total,
            self.tunnel_out_of_order_total,
            self.tunnel_fatal_desyncs_total,
        ));

        out
    }

    /// Get total metric count (number of fields exposed).
    pub fn metric_count(&self) -> usize {
        // Count of all fields: 3 server + 7 heartbeat + 10 cache + 6 error tracker
        //   + 6 filter chain + 6 pace + 8 queue + 9 retry + 7 tunnel = 62
        62
    }
}

impl fmt::Display for KnxMetricsSnapshot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.summary())
    }
}

// ============================================================================
// KnxMetricsCollector — collects from server components
// ============================================================================

/// Metrics collector that gathers statistics from all server components.
///
/// The collector is lightweight and can be called at any frequency without
/// impacting server performance. All underlying counters use `Ordering::Relaxed`
/// which provides eventual consistency without locking.
pub struct KnxMetricsCollector {
    /// Reference instant for timestamp calculation.
    created_at: Instant,
}

impl KnxMetricsCollector {
    /// Create a new metrics collector.
    pub fn new() -> Self {
        Self {
            created_at: Instant::now(),
        }
    }

    /// Collect a metrics snapshot from individual component snapshots.
    ///
    /// This is the primary collection method. The caller is responsible for
    /// obtaining the individual component snapshots from the server.
    pub fn collect(
        &self,
        server_state: u8,
        active_connections: usize,
        max_connections: usize,
        heartbeat: &HeartbeatStatsSnapshot,
        cache: &CacheStatsSnapshot,
        cache_entries: usize,
        error_tracker: &TrackerStatsSnapshot,
        filter_chain: &FilterChainStatsSnapshot,
        pace: &PaceFilterStatsSnapshot,
        queue: &QueueFilterStatsSnapshot,
        retry: &RetryFilterStatsSnapshot,
        seq_stats: &[SequenceStatsSnapshot],
        fsm_stats: &[FsmStatsSnapshot],
    ) -> KnxMetricsSnapshot {
        KnxMetricsSnapshot::zero()
            .with_heartbeat(heartbeat)
            .with_cache(cache, cache_entries)
            .with_error_tracker(error_tracker)
            .with_filter_chain(filter_chain)
            .with_pace_filter(pace)
            .with_queue_filter(queue)
            .with_retry_filter(retry)
            .with_tunnel_aggregate(seq_stats, fsm_stats)
            .with_server(
                self.created_at.elapsed().as_millis() as u64,
                server_state,
                active_connections,
                max_connections,
            )
    }
}

impl Default for KnxMetricsCollector {
    fn default() -> Self {
        Self::new()
    }
}

impl KnxMetricsSnapshot {
    /// Set server-level fields.
    pub(crate) fn with_server(
        mut self,
        timestamp_ms: u64,
        server_state: u8,
        active_connections: usize,
        max_connections: usize,
    ) -> Self {
        self.timestamp_ms = timestamp_ms;
        self.server_state = server_state;
        self.active_connections = active_connections as u64;
        self.max_connections = max_connections as u64;
        self
    }
}

// ============================================================================
// Per-connection metrics snapshot
// ============================================================================

/// Per-connection metrics snapshot for detailed diagnostics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionMetricsSnapshot {
    /// Channel ID.
    pub channel_id: u8,
    /// Individual address assigned to this connection.
    pub individual_address: String,
    /// Current FSM state.
    pub fsm_state: String,
    /// FSM transitions count.
    pub fsm_transitions: u64,
    /// Sequence tracker statistics.
    pub frames_sent: u64,
    pub frames_received: u64,
    pub duplicates_detected: u64,
    pub out_of_order_detected: u64,
    pub fatal_desyncs: u64,
    pub resets: u64,
    /// Idle duration in milliseconds.
    pub idle_duration_ms: u64,
    /// Whether the connection has timed out.
    pub is_timed_out: bool,
}

// ============================================================================
// Prometheus exposition format helpers
// ============================================================================

/// Emit a Prometheus counter metric (always integer-valued).
fn prom_counter(out: &mut String, name: &str, help: &str, value: f64) {
    out.push_str(&format!(
        "# HELP {} {}\n# TYPE {} counter\n{} {}\n",
        name,
        help,
        name,
        name,
        format_prom_value(value),
    ));
}

/// Emit a Prometheus gauge metric (may be fractional).
fn prom_gauge(out: &mut String, name: &str, help: &str, value: f64) {
    out.push_str(&format!(
        "# HELP {} {}\n# TYPE {} gauge\n{} {}\n",
        name,
        help,
        name,
        name,
        format_prom_value(value),
    ));
}

/// Format a Prometheus metric value.
///
/// Integers are rendered without decimal point for clean output.
/// Fractional values are rendered with enough precision.
fn format_prom_value(value: f64) -> String {
    if value.fract() == 0.0 && value.abs() < 1e15 {
        format!("{}", value as i64)
    } else {
        // Use enough precision for rates (typically 0.0-1.0)
        format!("{:.6}", value)
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_zero_snapshot() {
        let snapshot = KnxMetricsSnapshot::zero();
        assert_eq!(snapshot.server_state, 0);
        assert_eq!(snapshot.active_connections, 0);
        assert_eq!(snapshot.heartbeat_total_requests, 0);
        assert_eq!(snapshot.cache_hits_total, 0);
        assert_eq!(snapshot.error_tracker_successes_total, 0);
        assert_eq!(snapshot.filter_chain_frames_sent, 0);
        assert_eq!(snapshot.tunnel_frames_sent_total, 0);
    }

    #[test]
    fn test_metric_count() {
        let snapshot = KnxMetricsSnapshot::zero();
        assert_eq!(snapshot.metric_count(), 62);
    }

    #[test]
    fn test_with_heartbeat() {
        let heartbeat = HeartbeatStatsSnapshot {
            total_requests: 100,
            continue_count: 80,
            immediate_reconnect_count: 10,
            abandon_tunnel_count: 5,
            delayed_reconnect_count: 3,
            no_response_count: 2,
        };

        let snapshot = KnxMetricsSnapshot::zero().with_heartbeat(&heartbeat);

        assert_eq!(snapshot.heartbeat_total_requests, 100);
        assert_eq!(snapshot.heartbeat_continue_count, 80);
        assert_eq!(snapshot.heartbeat_immediate_reconnect_count, 10);
        assert_eq!(snapshot.heartbeat_abandon_tunnel_count, 5);
        assert_eq!(snapshot.heartbeat_delayed_reconnect_count, 3);
        assert_eq!(snapshot.heartbeat_no_response_count, 2);
        assert!((snapshot.heartbeat_fault_rate - 0.2).abs() < 0.001);
    }

    #[test]
    fn test_with_cache() {
        let cache = CacheStatsSnapshot {
            hits: 800,
            misses: 200,
            evictions: 10,
            expirations: 5,
            updates: 500,
            indication_updates: 300,
            write_updates: 200,
        };

        let snapshot = KnxMetricsSnapshot::zero().with_cache(&cache, 42);

        assert_eq!(snapshot.cache_hits_total, 800);
        assert_eq!(snapshot.cache_misses_total, 200);
        assert!((snapshot.cache_hit_rate - 0.8).abs() < 0.001);
        assert_eq!(snapshot.cache_lookups_total, 1000);
        assert_eq!(snapshot.cache_evictions_total, 10);
        assert_eq!(snapshot.cache_expirations_total, 5);
        assert_eq!(snapshot.cache_updates_total, 500);
        assert_eq!(snapshot.cache_indication_updates, 300);
        assert_eq!(snapshot.cache_write_updates, 200);
        assert_eq!(snapshot.cache_entries_current, 42);
    }

    #[test]
    fn test_with_error_tracker() {
        let tracker = TrackerStatsSnapshot {
            total_successes: 900,
            total_failures: 100,
            consecutive_triggers: 3,
            rate_triggers: 1,
        };

        let snapshot = KnxMetricsSnapshot::zero().with_error_tracker(&tracker);

        assert_eq!(snapshot.error_tracker_successes_total, 900);
        assert_eq!(snapshot.error_tracker_failures_total, 100);
        assert!((snapshot.error_tracker_error_rate - 0.1).abs() < 0.001);
        assert_eq!(snapshot.error_tracker_events_total, 1000);
        assert_eq!(snapshot.error_tracker_consecutive_triggers, 3);
        assert_eq!(snapshot.error_tracker_rate_triggers, 1);
    }

    #[test]
    fn test_with_filter_chain() {
        let filter = FilterChainStatsSnapshot {
            frames_sent: 1000,
            frames_received: 950,
            frames_dropped: 30,
            frames_queued: 20,
            total_delay_us: 500_000,
            bypass_count: 0,
        };

        let snapshot = KnxMetricsSnapshot::zero().with_filter_chain(&filter);

        assert_eq!(snapshot.filter_chain_frames_sent, 1000);
        assert_eq!(snapshot.filter_chain_frames_received, 950);
        assert_eq!(snapshot.filter_chain_frames_dropped, 30);
        assert_eq!(snapshot.filter_chain_frames_queued, 20);
        assert_eq!(snapshot.filter_chain_total_delay_us, 500_000);
        assert_eq!(snapshot.filter_chain_bypass_count, 0);
    }

    #[test]
    fn test_with_pace_filter() {
        let pace = PaceFilterStatsSnapshot {
            immediate_pass: 500,
            delayed_frames: 300,
            dropped_frames: 10,
            total_delay_us: 250_000,
            busy_to_idle: 200,
            idle_to_down: 50,
        };

        let snapshot = KnxMetricsSnapshot::zero().with_pace_filter(&pace);

        assert_eq!(snapshot.pace_immediate_pass, 500);
        assert_eq!(snapshot.pace_delayed_frames, 300);
        assert_eq!(snapshot.pace_dropped_frames, 10);
        assert_eq!(snapshot.pace_total_delay_us, 250_000);
    }

    #[test]
    fn test_with_queue_filter() {
        let queue = QueueFilterStatsSnapshot {
            direct_pass: 800,
            queued_frames: 100,
            dropped_full: 5,
            evicted_frames: 3,
            drained_frames: 95,
            high_priority: 50,
            normal_priority: 700,
            low_priority: 150,
        };

        let snapshot = KnxMetricsSnapshot::zero().with_queue_filter(&queue);

        assert_eq!(snapshot.queue_direct_pass, 800);
        assert_eq!(snapshot.queue_queued_frames, 100);
        assert_eq!(snapshot.queue_dropped_full, 5);
        assert_eq!(snapshot.queue_evicted_frames, 3);
        assert_eq!(snapshot.queue_drained_frames, 95);
        assert_eq!(snapshot.queue_high_priority, 50);
        assert_eq!(snapshot.queue_normal_priority, 700);
        assert_eq!(snapshot.queue_low_priority, 150);
    }

    #[test]
    fn test_with_retry_filter() {
        let retry = RetryFilterStatsSnapshot {
            direct_pass: 900,
            circuit_open_drops: 20,
            probe_frames: 5,
            retry_attempts: 30,
            successes: 910,
            failures: 25,
            state_transitions: 10,
            circuit_trips: 3,
            circuit_resets: 2,
        };

        let snapshot = KnxMetricsSnapshot::zero().with_retry_filter(&retry);

        assert_eq!(snapshot.retry_direct_pass, 900);
        assert_eq!(snapshot.retry_circuit_open_drops, 20);
        assert_eq!(snapshot.retry_probe_frames, 5);
        assert_eq!(snapshot.retry_circuit_trips, 3);
        assert_eq!(snapshot.retry_circuit_resets, 2);
    }

    #[test]
    fn test_with_tunnel_aggregate() {
        let seq_stats = vec![
            SequenceStatsSnapshot {
                frames_sent: 100,
                frames_received: 95,
                duplicates_detected: 3,
                out_of_order_detected: 2,
                fatal_desyncs: 0,
                resets: 1,
            },
            SequenceStatsSnapshot {
                frames_sent: 200,
                frames_received: 190,
                duplicates_detected: 5,
                out_of_order_detected: 1,
                fatal_desyncs: 1,
                resets: 0,
            },
        ];

        let fsm_stats = vec![
            FsmStatsSnapshot { transitions: 50 },
            FsmStatsSnapshot { transitions: 80 },
        ];

        let snapshot = KnxMetricsSnapshot::zero().with_tunnel_aggregate(&seq_stats, &fsm_stats);

        assert_eq!(snapshot.tunnel_frames_sent_total, 300);
        assert_eq!(snapshot.tunnel_frames_received_total, 285);
        assert_eq!(snapshot.tunnel_duplicates_total, 8);
        assert_eq!(snapshot.tunnel_out_of_order_total, 3);
        assert_eq!(snapshot.tunnel_fatal_desyncs_total, 1);
        assert_eq!(snapshot.tunnel_resets_total, 1);
        assert_eq!(snapshot.tunnel_fsm_transitions_total, 130);
    }

    #[test]
    fn test_prometheus_output_format() {
        let snapshot = KnxMetricsSnapshot::zero().with_server(1000, 2, 3, 10);

        let prom = snapshot.to_prometheus();

        // Verify format structure
        assert!(prom.contains("# HELP knx_server_state"));
        assert!(prom.contains("# TYPE knx_server_state gauge"));
        assert!(prom.contains("knx_server_state 2"));

        assert!(prom.contains("# TYPE knx_active_connections gauge"));
        assert!(prom.contains("knx_active_connections 3"));

        assert!(prom.contains("# TYPE knx_heartbeat_requests_total counter"));
        assert!(prom.contains("knx_heartbeat_requests_total 0"));

        assert!(prom.contains("# TYPE knx_cache_hit_rate gauge"));
        assert!(prom.contains("knx_cache_hit_rate"));

        assert!(prom.contains("# TYPE knx_tunnel_frames_sent_total counter"));
    }

    #[test]
    fn test_prometheus_value_formatting() {
        assert_eq!(format_prom_value(0.0), "0");
        assert_eq!(format_prom_value(42.0), "42");
        assert_eq!(format_prom_value(1000.0), "1000");
        assert_eq!(format_prom_value(0.5), "0.500000");
        assert_eq!(format_prom_value(0.123456789), "0.123457");
    }

    #[test]
    fn test_prometheus_all_metrics_present() {
        let snapshot = KnxMetricsSnapshot::zero();
        let prom = snapshot.to_prometheus();

        // Verify all 62 metrics are present
        let help_count = prom.matches("# HELP").count();
        let type_count = prom.matches("# TYPE").count();
        assert_eq!(help_count, 62, "Expected 62 HELP lines, got {}", help_count);
        assert_eq!(type_count, 62, "Expected 62 TYPE lines, got {}", type_count);
    }

    #[test]
    fn test_prometheus_with_real_data() {
        let heartbeat = HeartbeatStatsSnapshot {
            total_requests: 100,
            continue_count: 95,
            immediate_reconnect_count: 3,
            abandon_tunnel_count: 1,
            delayed_reconnect_count: 1,
            no_response_count: 0,
        };

        let snapshot = KnxMetricsSnapshot::zero()
            .with_heartbeat(&heartbeat)
            .with_server(5000, 2, 2, 10);

        let prom = snapshot.to_prometheus();

        assert!(prom.contains("knx_heartbeat_requests_total 100"));
        assert!(prom.contains("knx_heartbeat_continue_total 95"));
        assert!(prom.contains("knx_heartbeat_immediate_reconnect_total 3"));
        assert!(prom.contains("knx_heartbeat_fault_rate 0.050000"));
    }

    #[test]
    fn test_summary_output() {
        let snapshot = KnxMetricsSnapshot::zero().with_server(1000, 2, 3, 10);

        let summary = snapshot.summary();
        assert!(summary.contains("KNXnet/IP Metrics Summary"));
        assert!(summary.contains("Running"));
        assert!(summary.contains("3/10"));
    }

    #[test]
    fn test_display_trait() {
        let snapshot = KnxMetricsSnapshot::zero();
        let display = format!("{}", snapshot);
        assert!(display.contains("KNXnet/IP Metrics Summary"));
    }

    #[test]
    fn test_collector_creation() {
        let collector = KnxMetricsCollector::new();
        let _ = collector.created_at; // Access to verify creation
    }

    #[test]
    fn test_collector_collect() {
        let collector = KnxMetricsCollector::new();

        let heartbeat = HeartbeatStatsSnapshot {
            total_requests: 10,
            continue_count: 10,
            immediate_reconnect_count: 0,
            abandon_tunnel_count: 0,
            delayed_reconnect_count: 0,
            no_response_count: 0,
        };
        let cache = CacheStatsSnapshot {
            hits: 50,
            misses: 10,
            evictions: 0,
            expirations: 0,
            updates: 30,
            indication_updates: 20,
            write_updates: 10,
        };
        let tracker = TrackerStatsSnapshot {
            total_successes: 100,
            total_failures: 5,
            consecutive_triggers: 0,
            rate_triggers: 0,
        };
        let filter = FilterChainStatsSnapshot {
            frames_sent: 200,
            frames_received: 195,
            frames_dropped: 3,
            frames_queued: 2,
            total_delay_us: 100_000,
            bypass_count: 0,
        };
        let pace = PaceFilterStatsSnapshot {
            immediate_pass: 150,
            delayed_frames: 50,
            dropped_frames: 3,
            total_delay_us: 100_000,
            busy_to_idle: 40,
            idle_to_down: 10,
        };
        let queue = QueueFilterStatsSnapshot {
            direct_pass: 190,
            queued_frames: 10,
            dropped_full: 0,
            evicted_frames: 0,
            drained_frames: 10,
            high_priority: 5,
            normal_priority: 180,
            low_priority: 15,
        };
        let retry = RetryFilterStatsSnapshot {
            direct_pass: 195,
            circuit_open_drops: 2,
            probe_frames: 1,
            retry_attempts: 5,
            successes: 195,
            failures: 5,
            state_transitions: 3,
            circuit_trips: 1,
            circuit_resets: 1,
        };

        let snapshot = collector.collect(
            2,
            3,
            10,
            &heartbeat,
            &cache,
            25,
            &tracker,
            &filter,
            &pace,
            &queue,
            &retry,
            &[],
            &[],
        );

        assert_eq!(snapshot.server_state, 2);
        assert_eq!(snapshot.active_connections, 3);
        assert_eq!(snapshot.max_connections, 10);
        assert_eq!(snapshot.heartbeat_total_requests, 10);
        assert_eq!(snapshot.cache_hits_total, 50);
        assert_eq!(snapshot.cache_entries_current, 25);
        assert_eq!(snapshot.error_tracker_successes_total, 100);
        assert_eq!(snapshot.filter_chain_frames_sent, 200);
        assert_eq!(snapshot.pace_immediate_pass, 150);
        assert_eq!(snapshot.queue_direct_pass, 190);
        assert_eq!(snapshot.retry_direct_pass, 195);
        // timestamp_ms may be 0 if sub-millisecond; just verify it was set
        // (not a sentinel like u64::MAX)
        assert!(snapshot.timestamp_ms < 10_000);
    }

    #[test]
    fn test_snapshot_serialization() {
        let snapshot = KnxMetricsSnapshot::zero().with_server(1000, 2, 1, 10);

        let json = serde_json::to_string(&snapshot).unwrap();
        let deserialized: KnxMetricsSnapshot = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.server_state, 2);
        assert_eq!(deserialized.active_connections, 1);
        assert_eq!(deserialized.max_connections, 10);
    }

    #[test]
    fn test_connection_metrics_snapshot() {
        let conn_snap = ConnectionMetricsSnapshot {
            channel_id: 1,
            individual_address: "1.1.101".to_string(),
            fsm_state: "Idle".to_string(),
            fsm_transitions: 15,
            frames_sent: 50,
            frames_received: 48,
            duplicates_detected: 1,
            out_of_order_detected: 1,
            fatal_desyncs: 0,
            resets: 0,
            idle_duration_ms: 500,
            is_timed_out: false,
        };

        assert_eq!(conn_snap.channel_id, 1);
        assert_eq!(conn_snap.frames_sent, 50);
        assert!(!conn_snap.is_timed_out);

        // Ensure serializable
        let json = serde_json::to_string(&conn_snap).unwrap();
        assert!(json.contains("\"channel_id\":1"));
    }

    #[test]
    fn test_collector_default() {
        let collector = KnxMetricsCollector::default();
        // Just verify it compiles and doesn't panic
        let _ = collector.created_at;
    }
}

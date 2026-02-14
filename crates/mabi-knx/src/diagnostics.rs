//! Automatic diagnostic analysis for KNXnet/IP simulator health.
//!
//! This module provides production-grade diagnostic analysis by examining
//! collected metrics and producing actionable recommendations. Each diagnostic
//! rule evaluates a specific aspect of the simulator's operation and generates
//! a [`DiagnosticResult`] with severity, message, and suggested remediation.
//!
//! ## Diagnostic Rules (9 categories)
//!
//! | # | Rule | Evaluates | Severity Threshold |
//! |---|------|-----------|-------------------|
//! | 1 | CacheHitRate | Group value cache effectiveness | < 50% → Warning |
//! | 2 | AckRetryRate | ACK delivery reliability | > 10% retry → Warning |
//! | 3 | SequenceErrors | Sequence validation health | Any fatal desync → Critical |
//! | 4 | HeartbeatFaults | Heartbeat fault injection rate | > 50% → Warning |
//! | 5 | FilterDropRate | Filter chain frame drop rate | > 5% → Warning |
//! | 6 | CircuitBreakerTrips | Circuit breaker stability | Any trip → Warning |
//! | 7 | ErrorTrackerThresholds | Consecutive/rate error triggers | Any trigger → Warning |
//! | 8 | ConnectionCapacity | Connection pool utilization | > 80% → Warning |
//! | 9 | QueueBackpressure | Queue filter backpressure level | > 20% queued → Warning |
//!
//! ## Usage
//!
//! ```rust,ignore
//! use mabi_knx::diagnostics::{KnxDiagnostics, DiagnosticSeverity};
//! use mabi_knx::metrics::KnxMetricsSnapshot;
//!
//! let snapshot = collector.collect(/* ... */);
//! let diagnostics = KnxDiagnostics::analyze(&snapshot);
//!
//! for result in &diagnostics.results {
//!     if result.severity >= DiagnosticSeverity::Warning {
//!         println!("⚠ {}: {}", result.rule, result.message);
//!         println!("  → {}", result.recommendation);
//!     }
//! }
//! ```

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::metrics::KnxMetricsSnapshot;

// ============================================================================
// Diagnostic Severity
// ============================================================================

/// Severity level for diagnostic results.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DiagnosticSeverity {
    /// Everything is operating normally.
    Ok,
    /// Informational note (non-actionable observation).
    Info,
    /// Potential issue that should be investigated.
    Warning,
    /// Serious issue requiring immediate attention.
    Critical,
}

impl DiagnosticSeverity {
    /// Human-readable label.
    pub fn label(&self) -> &'static str {
        match self {
            Self::Ok => "OK",
            Self::Info => "INFO",
            Self::Warning => "WARN",
            Self::Critical => "CRIT",
        }
    }

    /// Whether this severity indicates a problem.
    pub fn is_problem(&self) -> bool {
        matches!(self, Self::Warning | Self::Critical)
    }
}

impl fmt::Display for DiagnosticSeverity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.label())
    }
}

// ============================================================================
// Diagnostic Rule
// ============================================================================

/// Identifies which diagnostic rule produced a result.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticRule {
    /// Cache hit rate analysis.
    CacheHitRate,
    /// ACK retry rate analysis.
    AckRetryRate,
    /// Sequence error analysis.
    SequenceErrors,
    /// Heartbeat fault rate analysis.
    HeartbeatFaults,
    /// Filter chain drop rate analysis.
    FilterDropRate,
    /// Circuit breaker trip analysis.
    CircuitBreakerTrips,
    /// Error tracker threshold analysis.
    ErrorTrackerThresholds,
    /// Connection pool capacity analysis.
    ConnectionCapacity,
    /// Queue backpressure analysis.
    QueueBackpressure,
}

impl DiagnosticRule {
    /// All diagnostic rules.
    pub fn all() -> &'static [DiagnosticRule] {
        &[
            Self::CacheHitRate,
            Self::AckRetryRate,
            Self::SequenceErrors,
            Self::HeartbeatFaults,
            Self::FilterDropRate,
            Self::CircuitBreakerTrips,
            Self::ErrorTrackerThresholds,
            Self::ConnectionCapacity,
            Self::QueueBackpressure,
        ]
    }

    /// Human-readable name.
    pub fn name(&self) -> &'static str {
        match self {
            Self::CacheHitRate => "Cache Hit Rate",
            Self::AckRetryRate => "ACK Retry Rate",
            Self::SequenceErrors => "Sequence Errors",
            Self::HeartbeatFaults => "Heartbeat Faults",
            Self::FilterDropRate => "Filter Drop Rate",
            Self::CircuitBreakerTrips => "Circuit Breaker Trips",
            Self::ErrorTrackerThresholds => "Error Tracker Thresholds",
            Self::ConnectionCapacity => "Connection Capacity",
            Self::QueueBackpressure => "Queue Backpressure",
        }
    }
}

impl fmt::Display for DiagnosticRule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.name())
    }
}

// ============================================================================
// Diagnostic Result
// ============================================================================

/// A single diagnostic result from evaluating a rule.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiagnosticResult {
    /// Which rule produced this result.
    pub rule: DiagnosticRule,
    /// Severity level.
    pub severity: DiagnosticSeverity,
    /// Human-readable diagnostic message describing the finding.
    pub message: String,
    /// Actionable recommendation for remediation.
    pub recommendation: String,
    /// The metric value that triggered this result (for programmatic use).
    pub metric_value: f64,
    /// The threshold that was used for evaluation.
    pub threshold: f64,
}

impl DiagnosticResult {
    /// Create an OK result (no issue detected).
    fn ok(rule: DiagnosticRule, message: impl Into<String>, metric_value: f64) -> Self {
        Self {
            rule,
            severity: DiagnosticSeverity::Ok,
            message: message.into(),
            recommendation: String::new(),
            metric_value,
            threshold: 0.0,
        }
    }

    /// Create an informational result.
    fn info(
        rule: DiagnosticRule,
        message: impl Into<String>,
        recommendation: impl Into<String>,
        metric_value: f64,
    ) -> Self {
        Self {
            rule,
            severity: DiagnosticSeverity::Info,
            message: message.into(),
            recommendation: recommendation.into(),
            metric_value,
            threshold: 0.0,
        }
    }

    /// Create a warning result.
    fn warning(
        rule: DiagnosticRule,
        message: impl Into<String>,
        recommendation: impl Into<String>,
        metric_value: f64,
        threshold: f64,
    ) -> Self {
        Self {
            rule,
            severity: DiagnosticSeverity::Warning,
            message: message.into(),
            recommendation: recommendation.into(),
            metric_value,
            threshold,
        }
    }

    /// Create a critical result.
    fn critical(
        rule: DiagnosticRule,
        message: impl Into<String>,
        recommendation: impl Into<String>,
        metric_value: f64,
        threshold: f64,
    ) -> Self {
        Self {
            rule,
            severity: DiagnosticSeverity::Critical,
            message: message.into(),
            recommendation: recommendation.into(),
            metric_value,
            threshold,
        }
    }
}

impl fmt::Display for DiagnosticResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "[{}] {}: {}",
            self.severity, self.rule, self.message
        )?;
        if !self.recommendation.is_empty() {
            write!(f, " → {}", self.recommendation)?;
        }
        Ok(())
    }
}

// ============================================================================
// Diagnostic Configuration
// ============================================================================

/// Configuration for diagnostic thresholds.
///
/// All thresholds are configurable to adapt to different testing scenarios.
/// For example, a stress test might tolerate higher error rates than a
/// conformance test.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiagnosticConfig {
    /// Cache hit rate warning threshold (below this → Warning).
    /// Default: 0.5 (50%).
    #[serde(default = "default_cache_hit_rate_warning")]
    pub cache_hit_rate_warning: f64,
    /// Cache hit rate critical threshold (below this → Critical).
    /// Default: 0.1 (10%).
    #[serde(default = "default_cache_hit_rate_critical")]
    pub cache_hit_rate_critical: f64,
    /// Minimum cache lookups before evaluating cache hit rate.
    /// Default: 10.
    #[serde(default = "default_min_cache_lookups")]
    pub min_cache_lookups: u64,

    /// ACK retry rate warning threshold (above this → Warning).
    /// Default: 0.1 (10%).
    #[serde(default = "default_ack_retry_rate_warning")]
    pub ack_retry_rate_warning: f64,
    /// ACK retry rate critical threshold (above this → Critical).
    /// Default: 0.3 (30%).
    #[serde(default = "default_ack_retry_rate_critical")]
    pub ack_retry_rate_critical: f64,

    /// Heartbeat fault rate warning threshold (above this → Warning).
    /// Default: 0.5 (50%).
    #[serde(default = "default_heartbeat_fault_rate_warning")]
    pub heartbeat_fault_rate_warning: f64,

    /// Filter drop rate warning threshold (above this → Warning).
    /// Default: 0.05 (5%).
    #[serde(default = "default_filter_drop_rate_warning")]
    pub filter_drop_rate_warning: f64,
    /// Filter drop rate critical threshold (above this → Critical).
    /// Default: 0.20 (20%).
    #[serde(default = "default_filter_drop_rate_critical")]
    pub filter_drop_rate_critical: f64,

    /// Connection capacity warning threshold (above this → Warning).
    /// Default: 0.8 (80%).
    #[serde(default = "default_connection_capacity_warning")]
    pub connection_capacity_warning: f64,
    /// Connection capacity critical threshold (above this → Critical).
    /// Default: 0.95 (95%).
    #[serde(default = "default_connection_capacity_critical")]
    pub connection_capacity_critical: f64,

    /// Queue backpressure warning threshold (above this → Warning).
    /// Ratio of queued frames to total frames.
    /// Default: 0.2 (20%).
    #[serde(default = "default_queue_backpressure_warning")]
    pub queue_backpressure_warning: f64,
    /// Queue backpressure critical threshold (above this → Critical).
    /// Default: 0.5 (50%).
    #[serde(default = "default_queue_backpressure_critical")]
    pub queue_backpressure_critical: f64,

    /// Minimum total frames before evaluating filter/queue metrics.
    /// Default: 20.
    #[serde(default = "default_min_frames_for_evaluation")]
    pub min_frames_for_evaluation: u64,
}

fn default_cache_hit_rate_warning() -> f64 { 0.5 }
fn default_cache_hit_rate_critical() -> f64 { 0.1 }
fn default_min_cache_lookups() -> u64 { 10 }
fn default_ack_retry_rate_warning() -> f64 { 0.1 }
fn default_ack_retry_rate_critical() -> f64 { 0.3 }
fn default_heartbeat_fault_rate_warning() -> f64 { 0.5 }
fn default_filter_drop_rate_warning() -> f64 { 0.05 }
fn default_filter_drop_rate_critical() -> f64 { 0.20 }
fn default_connection_capacity_warning() -> f64 { 0.8 }
fn default_connection_capacity_critical() -> f64 { 0.95 }
fn default_queue_backpressure_warning() -> f64 { 0.2 }
fn default_queue_backpressure_critical() -> f64 { 0.5 }
fn default_min_frames_for_evaluation() -> u64 { 20 }

impl Default for DiagnosticConfig {
    fn default() -> Self {
        Self {
            cache_hit_rate_warning: default_cache_hit_rate_warning(),
            cache_hit_rate_critical: default_cache_hit_rate_critical(),
            min_cache_lookups: default_min_cache_lookups(),
            ack_retry_rate_warning: default_ack_retry_rate_warning(),
            ack_retry_rate_critical: default_ack_retry_rate_critical(),
            heartbeat_fault_rate_warning: default_heartbeat_fault_rate_warning(),
            filter_drop_rate_warning: default_filter_drop_rate_warning(),
            filter_drop_rate_critical: default_filter_drop_rate_critical(),
            connection_capacity_warning: default_connection_capacity_warning(),
            connection_capacity_critical: default_connection_capacity_critical(),
            queue_backpressure_warning: default_queue_backpressure_warning(),
            queue_backpressure_critical: default_queue_backpressure_critical(),
            min_frames_for_evaluation: default_min_frames_for_evaluation(),
        }
    }
}

// ============================================================================
// KnxDiagnostics — the diagnostic engine
// ============================================================================

/// Diagnostic analysis results for a KNXnet/IP server.
///
/// Contains all diagnostic results from evaluating 9 rules against
/// a metrics snapshot. Results are sorted by severity (Critical first).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnxDiagnostics {
    /// All diagnostic results, sorted by severity (highest first).
    pub results: Vec<DiagnosticResult>,
    /// Overall health status (worst severity across all rules).
    pub overall_severity: DiagnosticSeverity,
    /// Number of warnings.
    pub warning_count: usize,
    /// Number of critical issues.
    pub critical_count: usize,
    /// Total diagnostic rules evaluated.
    pub rules_evaluated: usize,
}

impl KnxDiagnostics {
    /// Analyze a metrics snapshot with default diagnostic thresholds.
    pub fn analyze(snapshot: &KnxMetricsSnapshot) -> Self {
        Self::analyze_with_config(snapshot, &DiagnosticConfig::default())
    }

    /// Analyze a metrics snapshot with custom diagnostic thresholds.
    pub fn analyze_with_config(
        snapshot: &KnxMetricsSnapshot,
        config: &DiagnosticConfig,
    ) -> Self {
        let mut results = Vec::with_capacity(9);

        // Rule 1: Cache Hit Rate
        results.push(Self::check_cache_hit_rate(snapshot, config));

        // Rule 2: ACK Retry Rate
        results.push(Self::check_ack_retry_rate(snapshot, config));

        // Rule 3: Sequence Errors
        results.push(Self::check_sequence_errors(snapshot));

        // Rule 4: Heartbeat Faults
        results.push(Self::check_heartbeat_faults(snapshot, config));

        // Rule 5: Filter Drop Rate
        results.push(Self::check_filter_drop_rate(snapshot, config));

        // Rule 6: Circuit Breaker Trips
        results.push(Self::check_circuit_breaker_trips(snapshot));

        // Rule 7: Error Tracker Thresholds
        results.push(Self::check_error_tracker_thresholds(snapshot));

        // Rule 8: Connection Capacity
        results.push(Self::check_connection_capacity(snapshot, config));

        // Rule 9: Queue Backpressure
        results.push(Self::check_queue_backpressure(snapshot, config));

        // Sort by severity (Critical → Warning → Info → Ok)
        results.sort_by(|a, b| b.severity.cmp(&a.severity));

        let warning_count = results.iter()
            .filter(|r| r.severity == DiagnosticSeverity::Warning)
            .count();
        let critical_count = results.iter()
            .filter(|r| r.severity == DiagnosticSeverity::Critical)
            .count();
        let overall_severity = results.iter()
            .map(|r| r.severity)
            .max()
            .unwrap_or(DiagnosticSeverity::Ok);

        Self {
            rules_evaluated: results.len(),
            results,
            overall_severity,
            warning_count,
            critical_count,
        }
    }

    /// Get only the results that indicate problems (Warning or Critical).
    pub fn problems(&self) -> Vec<&DiagnosticResult> {
        self.results.iter()
            .filter(|r| r.severity.is_problem())
            .collect()
    }

    /// Check if all diagnostics are healthy.
    pub fn is_healthy(&self) -> bool {
        self.overall_severity < DiagnosticSeverity::Warning
    }

    /// Render a human-readable diagnostic report.
    pub fn report(&self) -> String {
        let mut out = String::with_capacity(2048);

        out.push_str("=== KNXnet/IP Diagnostic Report ===\n");
        out.push_str(&format!(
            "Overall: {}  ({} rules, {} warnings, {} critical)\n\n",
            self.overall_severity, self.rules_evaluated,
            self.warning_count, self.critical_count,
        ));

        for result in &self.results {
            let icon = match result.severity {
                DiagnosticSeverity::Ok => "[OK]  ",
                DiagnosticSeverity::Info => "[INFO]",
                DiagnosticSeverity::Warning => "[WARN]",
                DiagnosticSeverity::Critical => "[CRIT]",
            };

            out.push_str(&format!("{} {}: {}\n", icon, result.rule, result.message));

            if !result.recommendation.is_empty() {
                out.push_str(&format!("         Recommendation: {}\n", result.recommendation));
            }
        }

        out
    }

    // ========================================================================
    // Individual diagnostic rules
    // ========================================================================

    /// Rule 1: Cache Hit Rate
    ///
    /// Evaluates group value cache effectiveness. A low hit rate indicates
    /// that cached values are not being reused, possibly due to:
    /// - TTL too short
    /// - Cache capacity too small
    /// - Access patterns not cacheable
    fn check_cache_hit_rate(
        snapshot: &KnxMetricsSnapshot,
        config: &DiagnosticConfig,
    ) -> DiagnosticResult {
        let lookups = snapshot.cache_lookups_total;
        let hit_rate = snapshot.cache_hit_rate;

        // Skip if not enough data
        if lookups < config.min_cache_lookups {
            return DiagnosticResult::info(
                DiagnosticRule::CacheHitRate,
                format!("Insufficient data for cache analysis ({} lookups, min {} required)", lookups, config.min_cache_lookups),
                "Accumulate more cache lookups before evaluating",
                hit_rate,
            );
        }

        if hit_rate < config.cache_hit_rate_critical {
            DiagnosticResult::critical(
                DiagnosticRule::CacheHitRate,
                format!("Cache hit rate critically low at {:.1}% ({} hits / {} lookups)", hit_rate * 100.0, snapshot.cache_hits_total, lookups),
                "Increase cache TTL (group_value_cache.ttl_ms) or max_entries. Check if access patterns match cached addresses.",
                hit_rate,
                config.cache_hit_rate_critical,
            )
        } else if hit_rate < config.cache_hit_rate_warning {
            DiagnosticResult::warning(
                DiagnosticRule::CacheHitRate,
                format!("Cache hit rate below target at {:.1}% ({} hits / {} lookups)", hit_rate * 100.0, snapshot.cache_hits_total, lookups),
                "Consider increasing cache TTL or capacity. Review auto_update_on_indication setting.",
                hit_rate,
                config.cache_hit_rate_warning,
            )
        } else {
            DiagnosticResult::ok(
                DiagnosticRule::CacheHitRate,
                format!("Cache hit rate healthy at {:.1}%", hit_rate * 100.0),
                hit_rate,
            )
        }
    }

    /// Rule 2: ACK Retry Rate
    ///
    /// Evaluates the ratio of retry attempts to total frame sends.
    /// High retry rates indicate network issues or client overload.
    fn check_ack_retry_rate(
        snapshot: &KnxMetricsSnapshot,
        config: &DiagnosticConfig,
    ) -> DiagnosticResult {
        let total = snapshot.retry_successes + snapshot.retry_failures;
        if total == 0 {
            return DiagnosticResult::ok(
                DiagnosticRule::AckRetryRate,
                "No retry data available (filter chain may be disabled)".to_string(),
                0.0,
            );
        }

        let retry_rate = snapshot.retry_attempts as f64 / total as f64;

        if retry_rate > config.ack_retry_rate_critical {
            DiagnosticResult::critical(
                DiagnosticRule::AckRetryRate,
                format!("ACK retry rate critically high at {:.1}% ({} retries / {} total)", retry_rate * 100.0, snapshot.retry_attempts, total),
                "Check network connectivity and client ACK processing. Consider increasing ACK timeout (server_ack_timeout_ms) or reducing send rate.",
                retry_rate,
                config.ack_retry_rate_critical,
            )
        } else if retry_rate > config.ack_retry_rate_warning {
            DiagnosticResult::warning(
                DiagnosticRule::AckRetryRate,
                format!("ACK retry rate elevated at {:.1}% ({} retries / {} total)", retry_rate * 100.0, snapshot.retry_attempts, total),
                "Monitor client ACK processing latency. Consider adjusting PaceFilter inter-frame delay.",
                retry_rate,
                config.ack_retry_rate_warning,
            )
        } else {
            DiagnosticResult::ok(
                DiagnosticRule::AckRetryRate,
                format!("ACK retry rate nominal at {:.1}%", retry_rate * 100.0),
                retry_rate,
            )
        }
    }

    /// Rule 3: Sequence Errors
    ///
    /// Evaluates tunnel sequence validation health. Any fatal desync
    /// indicates a serious protocol violation that requires tunnel restart.
    fn check_sequence_errors(snapshot: &KnxMetricsSnapshot) -> DiagnosticResult {
        let desyncs = snapshot.tunnel_fatal_desyncs_total;
        let ooo = snapshot.tunnel_out_of_order_total;
        let dupes = snapshot.tunnel_duplicates_total;

        if desyncs > 0 {
            DiagnosticResult::critical(
                DiagnosticRule::SequenceErrors,
                format!("{} fatal desync(s) detected — tunnel restart(s) occurred", desyncs),
                "Investigate sequence numbering on the client. Fatal desyncs indicate packets arriving far out of order. Check for network routing issues or client bugs.",
                desyncs as f64,
                1.0,
            )
        } else if ooo > 0 {
            DiagnosticResult::warning(
                DiagnosticRule::SequenceErrors,
                format!("{} out-of-order frame(s) detected (0 fatal desyncs)", ooo),
                "Minor out-of-order frames are tolerable but may indicate network jitter. Monitor for escalation to fatal desyncs.",
                ooo as f64,
                1.0,
            )
        } else if dupes > 0 {
            DiagnosticResult::info(
                DiagnosticRule::SequenceErrors,
                format!("{} duplicate frame(s) detected — ACKed without processing", dupes),
                "Duplicate frames are normal during retransmission. No action needed unless count is excessive.",
                dupes as f64,
            )
        } else {
            DiagnosticResult::ok(
                DiagnosticRule::SequenceErrors,
                "No sequence errors detected".to_string(),
                0.0,
            )
        }
    }

    /// Rule 4: Heartbeat Faults
    ///
    /// Evaluates the heartbeat fault injection rate. High fault rates
    /// during testing are expected, but in production they indicate
    /// misconfiguration.
    fn check_heartbeat_faults(
        snapshot: &KnxMetricsSnapshot,
        config: &DiagnosticConfig,
    ) -> DiagnosticResult {
        let total = snapshot.heartbeat_total_requests;
        if total == 0 {
            return DiagnosticResult::ok(
                DiagnosticRule::HeartbeatFaults,
                "No heartbeat requests processed".to_string(),
                0.0,
            );
        }

        let fault_rate = snapshot.heartbeat_fault_rate;

        if fault_rate > config.heartbeat_fault_rate_warning {
            DiagnosticResult::warning(
                DiagnosticRule::HeartbeatFaults,
                format!(
                    "Heartbeat fault rate at {:.1}% (ImmRecon={}, Abandon={}, DelayRecon={}, NoResp={})",
                    fault_rate * 100.0,
                    snapshot.heartbeat_immediate_reconnect_count,
                    snapshot.heartbeat_abandon_tunnel_count,
                    snapshot.heartbeat_delayed_reconnect_count,
                    snapshot.heartbeat_no_response_count,
                ),
                "If this is a stress test, high fault rates may be intentional. For conformance testing, ensure heartbeat_scheduler is configured with appropriate schedule.",
                fault_rate,
                config.heartbeat_fault_rate_warning,
            )
        } else if fault_rate > 0.0 {
            DiagnosticResult::info(
                DiagnosticRule::HeartbeatFaults,
                format!("Heartbeat fault rate at {:.1}%", fault_rate * 100.0),
                "Low fault rate is normal for periodic fault injection testing.",
                fault_rate,
            )
        } else {
            DiagnosticResult::ok(
                DiagnosticRule::HeartbeatFaults,
                format!("All {} heartbeats responded normally", total),
                fault_rate,
            )
        }
    }

    /// Rule 5: Filter Drop Rate
    ///
    /// Evaluates the percentage of frames dropped by the filter chain.
    /// Drops are expected when circuit breaker is open or queues are full,
    /// but high rates indicate systemic issues.
    fn check_filter_drop_rate(
        snapshot: &KnxMetricsSnapshot,
        config: &DiagnosticConfig,
    ) -> DiagnosticResult {
        let total = snapshot.filter_chain_frames_sent;
        if total < config.min_frames_for_evaluation {
            return DiagnosticResult::ok(
                DiagnosticRule::FilterDropRate,
                format!("Insufficient data ({} frames, min {} for evaluation)", total, config.min_frames_for_evaluation),
                0.0,
            );
        }

        let drop_rate = snapshot.filter_chain_frames_dropped as f64 / total as f64;

        if drop_rate > config.filter_drop_rate_critical {
            DiagnosticResult::critical(
                DiagnosticRule::FilterDropRate,
                format!("Filter drop rate critically high at {:.1}% ({} dropped / {} total)", drop_rate * 100.0, snapshot.filter_chain_frames_dropped, total),
                "Check circuit breaker state and queue capacity. High drop rates may indicate the bus timing simulation is too aggressive. Consider relaxing PaceFilter timing or increasing QueueFilter depth.",
                drop_rate,
                config.filter_drop_rate_critical,
            )
        } else if drop_rate > config.filter_drop_rate_warning {
            DiagnosticResult::warning(
                DiagnosticRule::FilterDropRate,
                format!("Filter drop rate elevated at {:.1}% ({} dropped / {} total)", drop_rate * 100.0, snapshot.filter_chain_frames_dropped, total),
                "Monitor drop trends. Consider increasing queue depth (flow_control.queue.max_queue_depth) or adjusting retry thresholds.",
                drop_rate,
                config.filter_drop_rate_warning,
            )
        } else {
            DiagnosticResult::ok(
                DiagnosticRule::FilterDropRate,
                format!("Filter drop rate nominal at {:.1}%", drop_rate * 100.0),
                drop_rate,
            )
        }
    }

    /// Rule 6: Circuit Breaker Trips
    ///
    /// Any circuit breaker trip indicates that the system detected
    /// excessive consecutive failures and temporarily stopped sending.
    fn check_circuit_breaker_trips(snapshot: &KnxMetricsSnapshot) -> DiagnosticResult {
        let trips = snapshot.retry_circuit_trips;
        let resets = snapshot.retry_circuit_resets;

        if trips == 0 {
            return DiagnosticResult::ok(
                DiagnosticRule::CircuitBreakerTrips,
                "No circuit breaker trips".to_string(),
                0.0,
            );
        }

        let still_open = trips > resets;

        if still_open {
            DiagnosticResult::critical(
                DiagnosticRule::CircuitBreakerTrips,
                format!("Circuit breaker is OPEN ({} trips, {} resets — currently open)", trips, resets),
                "The circuit breaker has tripped and not yet recovered. All frames are being dropped. Investigate the root cause of send failures before resetting.",
                trips as f64,
                1.0,
            )
        } else {
            DiagnosticResult::warning(
                DiagnosticRule::CircuitBreakerTrips,
                format!("Circuit breaker tripped {} time(s) (all recovered)", trips),
                "The circuit breaker has tripped and recovered. This indicates transient failure bursts. Review error patterns to determine if retry thresholds are appropriate.",
                trips as f64,
                1.0,
            )
        }
    }

    /// Rule 7: Error Tracker Thresholds
    ///
    /// Evaluates whether any error tracking thresholds have been triggered.
    fn check_error_tracker_thresholds(snapshot: &KnxMetricsSnapshot) -> DiagnosticResult {
        let consecutive = snapshot.error_tracker_consecutive_triggers;
        let rate = snapshot.error_tracker_rate_triggers;

        if consecutive == 0 && rate == 0 {
            let error_rate = snapshot.error_tracker_error_rate;
            if error_rate > 0.0 {
                return DiagnosticResult::info(
                    DiagnosticRule::ErrorTrackerThresholds,
                    format!("Error rate at {:.1}% — below thresholds", error_rate * 100.0),
                    "Errors are occurring but within tolerance. Monitor trends.",
                    error_rate,
                );
            }
            return DiagnosticResult::ok(
                DiagnosticRule::ErrorTrackerThresholds,
                "No error thresholds triggered".to_string(),
                0.0,
            );
        }

        if consecutive > 0 && rate > 0 {
            DiagnosticResult::critical(
                DiagnosticRule::ErrorTrackerThresholds,
                format!("Both error thresholds triggered: {} consecutive, {} rate", consecutive, rate),
                "Multiple error thresholds exceeded. Tunnel restarts have been recommended. Check send pipeline reliability and network connectivity.",
                (consecutive + rate) as f64,
                1.0,
            )
        } else if consecutive > 0 {
            DiagnosticResult::warning(
                DiagnosticRule::ErrorTrackerThresholds,
                format!("Consecutive error threshold triggered {} time(s)", consecutive),
                "Consecutive failures indicate burst errors. Check if the client is dropping connections or if there are network blackholes.",
                consecutive as f64,
                1.0,
            )
        } else {
            DiagnosticResult::warning(
                DiagnosticRule::ErrorTrackerThresholds,
                format!("Error rate threshold triggered {} time(s)", rate),
                "Sustained error rate exceeds threshold. Review the error_tracker sliding window configuration (window_ms, rate_threshold).",
                rate as f64,
                1.0,
            )
        }
    }

    /// Rule 8: Connection Capacity
    ///
    /// Evaluates connection pool utilization against configured capacity.
    fn check_connection_capacity(
        snapshot: &KnxMetricsSnapshot,
        config: &DiagnosticConfig,
    ) -> DiagnosticResult {
        if snapshot.max_connections == 0 {
            return DiagnosticResult::ok(
                DiagnosticRule::ConnectionCapacity,
                "No connection capacity configured".to_string(),
                0.0,
            );
        }

        let utilization = snapshot.active_connections as f64 / snapshot.max_connections as f64;

        if utilization > config.connection_capacity_critical {
            DiagnosticResult::critical(
                DiagnosticRule::ConnectionCapacity,
                format!("Connection pool at {:.1}% capacity ({}/{})", utilization * 100.0, snapshot.active_connections, snapshot.max_connections),
                "Near capacity — new connections will be rejected. Increase max_connections or disconnect idle clients.",
                utilization,
                config.connection_capacity_critical,
            )
        } else if utilization > config.connection_capacity_warning {
            DiagnosticResult::warning(
                DiagnosticRule::ConnectionCapacity,
                format!("Connection pool at {:.1}% capacity ({}/{})", utilization * 100.0, snapshot.active_connections, snapshot.max_connections),
                "Connection pool is filling up. Consider increasing max_connections if more clients are expected.",
                utilization,
                config.connection_capacity_warning,
            )
        } else {
            DiagnosticResult::ok(
                DiagnosticRule::ConnectionCapacity,
                format!("Connection pool at {:.1}% capacity ({}/{})", utilization * 100.0, snapshot.active_connections, snapshot.max_connections),
                utilization,
            )
        }
    }

    /// Rule 9: Queue Backpressure
    ///
    /// Evaluates queue utilization — high queuing rates indicate that
    /// the server is generating frames faster than clients can ACK them.
    fn check_queue_backpressure(
        snapshot: &KnxMetricsSnapshot,
        config: &DiagnosticConfig,
    ) -> DiagnosticResult {
        let total = snapshot.filter_chain_frames_sent;
        if total < config.min_frames_for_evaluation {
            return DiagnosticResult::ok(
                DiagnosticRule::QueueBackpressure,
                format!("Insufficient data ({} frames)", total),
                0.0,
            );
        }

        let queue_rate = snapshot.filter_chain_frames_queued as f64 / total as f64;

        if queue_rate > config.queue_backpressure_critical {
            DiagnosticResult::critical(
                DiagnosticRule::QueueBackpressure,
                format!("Queue backpressure critically high at {:.1}% ({} queued / {} total)", queue_rate * 100.0, snapshot.filter_chain_frames_queued, total),
                "Server is generating frames much faster than clients can process them. Increase PaceFilter delay or reduce broadcasting rate.",
                queue_rate,
                config.queue_backpressure_critical,
            )
        } else if queue_rate > config.queue_backpressure_warning {
            DiagnosticResult::warning(
                DiagnosticRule::QueueBackpressure,
                format!("Queue backpressure elevated at {:.1}% ({} queued / {} total)", queue_rate * 100.0, snapshot.filter_chain_frames_queued, total),
                "Moderate queuing indicates backpressure. Consider increasing bus timing delay or queue depth.",
                queue_rate,
                config.queue_backpressure_warning,
            )
        } else {
            DiagnosticResult::ok(
                DiagnosticRule::QueueBackpressure,
                format!("Queue backpressure nominal at {:.1}%", queue_rate * 100.0),
                queue_rate,
            )
        }
    }
}

impl fmt::Display for KnxDiagnostics {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.report())
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn make_snapshot() -> KnxMetricsSnapshot {
        KnxMetricsSnapshot::zero()
            .with_server(1000, 2, 3, 10)
    }

    #[test]
    fn test_analyze_zero_snapshot() {
        let snapshot = KnxMetricsSnapshot::zero();
        let diag = KnxDiagnostics::analyze(&snapshot);

        assert_eq!(diag.rules_evaluated, 9);
        assert!(diag.is_healthy());
        assert_eq!(diag.warning_count, 0);
        assert_eq!(diag.critical_count, 0);
    }

    #[test]
    fn test_analyze_healthy_snapshot() {
        let mut snapshot = make_snapshot();

        // Good cache hit rate
        snapshot.cache_hits_total = 90;
        snapshot.cache_misses_total = 10;
        snapshot.cache_hit_rate = 0.9;
        snapshot.cache_lookups_total = 100;

        let diag = KnxDiagnostics::analyze(&snapshot);
        assert!(diag.is_healthy());
    }

    #[test]
    fn test_cache_hit_rate_warning() {
        let mut snapshot = make_snapshot();
        snapshot.cache_hits_total = 30;
        snapshot.cache_misses_total = 70;
        snapshot.cache_hit_rate = 0.3;
        snapshot.cache_lookups_total = 100;

        let diag = KnxDiagnostics::analyze(&snapshot);
        let cache_result = diag.results.iter()
            .find(|r| r.rule == DiagnosticRule::CacheHitRate)
            .unwrap();

        assert_eq!(cache_result.severity, DiagnosticSeverity::Warning);
        assert!(cache_result.message.contains("30.0%"));
        assert!(!cache_result.recommendation.is_empty());
    }

    #[test]
    fn test_cache_hit_rate_critical() {
        let mut snapshot = make_snapshot();
        snapshot.cache_hits_total = 5;
        snapshot.cache_misses_total = 95;
        snapshot.cache_hit_rate = 0.05;
        snapshot.cache_lookups_total = 100;

        let diag = KnxDiagnostics::analyze(&snapshot);
        let cache_result = diag.results.iter()
            .find(|r| r.rule == DiagnosticRule::CacheHitRate)
            .unwrap();

        assert_eq!(cache_result.severity, DiagnosticSeverity::Critical);
    }

    #[test]
    fn test_cache_hit_rate_insufficient_data() {
        let mut snapshot = make_snapshot();
        snapshot.cache_lookups_total = 5;
        snapshot.cache_hit_rate = 0.0;

        let diag = KnxDiagnostics::analyze(&snapshot);
        let cache_result = diag.results.iter()
            .find(|r| r.rule == DiagnosticRule::CacheHitRate)
            .unwrap();

        assert_eq!(cache_result.severity, DiagnosticSeverity::Info);
    }

    #[test]
    fn test_sequence_errors_fatal_desync() {
        let mut snapshot = make_snapshot();
        snapshot.tunnel_fatal_desyncs_total = 2;

        let diag = KnxDiagnostics::analyze(&snapshot);
        let seq_result = diag.results.iter()
            .find(|r| r.rule == DiagnosticRule::SequenceErrors)
            .unwrap();

        assert_eq!(seq_result.severity, DiagnosticSeverity::Critical);
        assert!(seq_result.message.contains("2 fatal desync"));
    }

    #[test]
    fn test_sequence_errors_out_of_order() {
        let mut snapshot = make_snapshot();
        snapshot.tunnel_out_of_order_total = 5;

        let diag = KnxDiagnostics::analyze(&snapshot);
        let seq_result = diag.results.iter()
            .find(|r| r.rule == DiagnosticRule::SequenceErrors)
            .unwrap();

        assert_eq!(seq_result.severity, DiagnosticSeverity::Warning);
    }

    #[test]
    fn test_sequence_errors_duplicates_only() {
        let mut snapshot = make_snapshot();
        snapshot.tunnel_duplicates_total = 3;

        let diag = KnxDiagnostics::analyze(&snapshot);
        let seq_result = diag.results.iter()
            .find(|r| r.rule == DiagnosticRule::SequenceErrors)
            .unwrap();

        assert_eq!(seq_result.severity, DiagnosticSeverity::Info);
    }

    #[test]
    fn test_heartbeat_faults_high() {
        let mut snapshot = make_snapshot();
        snapshot.heartbeat_total_requests = 100;
        snapshot.heartbeat_continue_count = 40;
        snapshot.heartbeat_immediate_reconnect_count = 30;
        snapshot.heartbeat_abandon_tunnel_count = 20;
        snapshot.heartbeat_delayed_reconnect_count = 10;
        snapshot.heartbeat_fault_rate = 0.6;

        let diag = KnxDiagnostics::analyze(&snapshot);
        let hb_result = diag.results.iter()
            .find(|r| r.rule == DiagnosticRule::HeartbeatFaults)
            .unwrap();

        assert_eq!(hb_result.severity, DiagnosticSeverity::Warning);
    }

    #[test]
    fn test_heartbeat_faults_low() {
        let mut snapshot = make_snapshot();
        snapshot.heartbeat_total_requests = 100;
        snapshot.heartbeat_continue_count = 95;
        snapshot.heartbeat_immediate_reconnect_count = 5;
        snapshot.heartbeat_fault_rate = 0.05;

        let diag = KnxDiagnostics::analyze(&snapshot);
        let hb_result = diag.results.iter()
            .find(|r| r.rule == DiagnosticRule::HeartbeatFaults)
            .unwrap();

        assert_eq!(hb_result.severity, DiagnosticSeverity::Info);
    }

    #[test]
    fn test_filter_drop_rate_warning() {
        let mut snapshot = make_snapshot();
        snapshot.filter_chain_frames_sent = 100;
        snapshot.filter_chain_frames_dropped = 8;

        let diag = KnxDiagnostics::analyze(&snapshot);
        let filter_result = diag.results.iter()
            .find(|r| r.rule == DiagnosticRule::FilterDropRate)
            .unwrap();

        assert_eq!(filter_result.severity, DiagnosticSeverity::Warning);
    }

    #[test]
    fn test_filter_drop_rate_critical() {
        let mut snapshot = make_snapshot();
        snapshot.filter_chain_frames_sent = 100;
        snapshot.filter_chain_frames_dropped = 25;

        let diag = KnxDiagnostics::analyze(&snapshot);
        let filter_result = diag.results.iter()
            .find(|r| r.rule == DiagnosticRule::FilterDropRate)
            .unwrap();

        assert_eq!(filter_result.severity, DiagnosticSeverity::Critical);
    }

    #[test]
    fn test_circuit_breaker_trips_open() {
        let mut snapshot = make_snapshot();
        snapshot.retry_circuit_trips = 3;
        snapshot.retry_circuit_resets = 2; // 3 trips, 2 resets → still open

        let diag = KnxDiagnostics::analyze(&snapshot);
        let cb_result = diag.results.iter()
            .find(|r| r.rule == DiagnosticRule::CircuitBreakerTrips)
            .unwrap();

        assert_eq!(cb_result.severity, DiagnosticSeverity::Critical);
        assert!(cb_result.message.contains("OPEN"));
    }

    #[test]
    fn test_circuit_breaker_trips_recovered() {
        let mut snapshot = make_snapshot();
        snapshot.retry_circuit_trips = 2;
        snapshot.retry_circuit_resets = 2; // All recovered

        let diag = KnxDiagnostics::analyze(&snapshot);
        let cb_result = diag.results.iter()
            .find(|r| r.rule == DiagnosticRule::CircuitBreakerTrips)
            .unwrap();

        assert_eq!(cb_result.severity, DiagnosticSeverity::Warning);
    }

    #[test]
    fn test_error_tracker_both_thresholds() {
        let mut snapshot = make_snapshot();
        snapshot.error_tracker_consecutive_triggers = 3;
        snapshot.error_tracker_rate_triggers = 2;

        let diag = KnxDiagnostics::analyze(&snapshot);
        let et_result = diag.results.iter()
            .find(|r| r.rule == DiagnosticRule::ErrorTrackerThresholds)
            .unwrap();

        assert_eq!(et_result.severity, DiagnosticSeverity::Critical);
    }

    #[test]
    fn test_error_tracker_consecutive_only() {
        let mut snapshot = make_snapshot();
        snapshot.error_tracker_consecutive_triggers = 2;

        let diag = KnxDiagnostics::analyze(&snapshot);
        let et_result = diag.results.iter()
            .find(|r| r.rule == DiagnosticRule::ErrorTrackerThresholds)
            .unwrap();

        assert_eq!(et_result.severity, DiagnosticSeverity::Warning);
    }

    #[test]
    fn test_error_tracker_rate_only() {
        let mut snapshot = make_snapshot();
        snapshot.error_tracker_rate_triggers = 1;

        let diag = KnxDiagnostics::analyze(&snapshot);
        let et_result = diag.results.iter()
            .find(|r| r.rule == DiagnosticRule::ErrorTrackerThresholds)
            .unwrap();

        assert_eq!(et_result.severity, DiagnosticSeverity::Warning);
    }

    #[test]
    fn test_connection_capacity_warning() {
        let mut snapshot = make_snapshot();
        snapshot.active_connections = 9;
        snapshot.max_connections = 10;

        let diag = KnxDiagnostics::analyze(&snapshot);
        let cap_result = diag.results.iter()
            .find(|r| r.rule == DiagnosticRule::ConnectionCapacity)
            .unwrap();

        assert_eq!(cap_result.severity, DiagnosticSeverity::Warning);
    }

    #[test]
    fn test_connection_capacity_critical() {
        let mut snapshot = make_snapshot();
        snapshot.active_connections = 10;
        snapshot.max_connections = 10;

        let diag = KnxDiagnostics::analyze(&snapshot);
        let cap_result = diag.results.iter()
            .find(|r| r.rule == DiagnosticRule::ConnectionCapacity)
            .unwrap();

        assert_eq!(cap_result.severity, DiagnosticSeverity::Critical);
    }

    #[test]
    fn test_queue_backpressure_warning() {
        let mut snapshot = make_snapshot();
        snapshot.filter_chain_frames_sent = 100;
        snapshot.filter_chain_frames_queued = 25;

        let diag = KnxDiagnostics::analyze(&snapshot);
        let qb_result = diag.results.iter()
            .find(|r| r.rule == DiagnosticRule::QueueBackpressure)
            .unwrap();

        assert_eq!(qb_result.severity, DiagnosticSeverity::Warning);
    }

    #[test]
    fn test_queue_backpressure_critical() {
        let mut snapshot = make_snapshot();
        snapshot.filter_chain_frames_sent = 100;
        snapshot.filter_chain_frames_queued = 55;

        let diag = KnxDiagnostics::analyze(&snapshot);
        let qb_result = diag.results.iter()
            .find(|r| r.rule == DiagnosticRule::QueueBackpressure)
            .unwrap();

        assert_eq!(qb_result.severity, DiagnosticSeverity::Critical);
    }

    #[test]
    fn test_problems_method() {
        let mut snapshot = make_snapshot();
        snapshot.tunnel_fatal_desyncs_total = 1;
        snapshot.error_tracker_consecutive_triggers = 1;

        let diag = KnxDiagnostics::analyze(&snapshot);
        let problems = diag.problems();

        // At least 2 problems: fatal desync (Critical) + error threshold (Warning)
        assert!(problems.len() >= 2);
        for p in &problems {
            assert!(p.severity.is_problem());
        }
    }

    #[test]
    fn test_sorted_by_severity() {
        let mut snapshot = make_snapshot();
        snapshot.tunnel_fatal_desyncs_total = 1; // Critical
        snapshot.error_tracker_rate_triggers = 1; // Warning
        snapshot.cache_lookups_total = 100;
        snapshot.cache_hit_rate = 0.3; // Warning

        let diag = KnxDiagnostics::analyze(&snapshot);

        // First result should be Critical
        if !diag.results.is_empty() {
            let first_problem = diag.results.iter()
                .find(|r| r.severity.is_problem())
                .unwrap();
            assert_eq!(first_problem.severity, DiagnosticSeverity::Critical);
        }
    }

    #[test]
    fn test_overall_severity() {
        let mut snapshot = make_snapshot();
        snapshot.tunnel_fatal_desyncs_total = 1;

        let diag = KnxDiagnostics::analyze(&snapshot);
        assert_eq!(diag.overall_severity, DiagnosticSeverity::Critical);
        assert!(!diag.is_healthy());
    }

    #[test]
    fn test_report_output() {
        let snapshot = make_snapshot();
        let diag = KnxDiagnostics::analyze(&snapshot);
        let report = diag.report();

        assert!(report.contains("KNXnet/IP Diagnostic Report"));
        assert!(report.contains("9 rules"));
    }

    #[test]
    fn test_display_trait() {
        let snapshot = make_snapshot();
        let diag = KnxDiagnostics::analyze(&snapshot);
        let display = format!("{}", diag);
        assert!(display.contains("KNXnet/IP Diagnostic Report"));
    }

    #[test]
    fn test_diagnostic_result_display() {
        let result = DiagnosticResult::warning(
            DiagnosticRule::CacheHitRate,
            "Low hit rate",
            "Increase TTL",
            0.3,
            0.5,
        );

        let display = format!("{}", result);
        assert!(display.contains("[WARN]"));
        assert!(display.contains("Cache Hit Rate"));
        assert!(display.contains("Low hit rate"));
        assert!(display.contains("Increase TTL"));
    }

    #[test]
    fn test_diagnostic_severity_ordering() {
        assert!(DiagnosticSeverity::Critical > DiagnosticSeverity::Warning);
        assert!(DiagnosticSeverity::Warning > DiagnosticSeverity::Info);
        assert!(DiagnosticSeverity::Info > DiagnosticSeverity::Ok);
    }

    #[test]
    fn test_diagnostic_rule_all() {
        let all = DiagnosticRule::all();
        assert_eq!(all.len(), 9);
    }

    #[test]
    fn test_custom_config() {
        let config = DiagnosticConfig {
            cache_hit_rate_warning: 0.9, // Very strict
            ..Default::default()
        };

        let mut snapshot = make_snapshot();
        snapshot.cache_hits_total = 80;
        snapshot.cache_misses_total = 20;
        snapshot.cache_hit_rate = 0.8;
        snapshot.cache_lookups_total = 100;

        let diag = KnxDiagnostics::analyze_with_config(&snapshot, &config);
        let cache_result = diag.results.iter()
            .find(|r| r.rule == DiagnosticRule::CacheHitRate)
            .unwrap();

        // 80% < 90% custom threshold → Warning
        assert_eq!(cache_result.severity, DiagnosticSeverity::Warning);
    }

    #[test]
    fn test_config_defaults() {
        let config = DiagnosticConfig::default();
        assert_eq!(config.cache_hit_rate_warning, 0.5);
        assert_eq!(config.cache_hit_rate_critical, 0.1);
        assert_eq!(config.min_cache_lookups, 10);
        assert_eq!(config.ack_retry_rate_warning, 0.1);
        assert_eq!(config.heartbeat_fault_rate_warning, 0.5);
        assert_eq!(config.filter_drop_rate_warning, 0.05);
        assert_eq!(config.connection_capacity_warning, 0.8);
        assert_eq!(config.queue_backpressure_warning, 0.2);
    }

    #[test]
    fn test_ack_retry_rate_no_data() {
        let snapshot = make_snapshot();

        let diag = KnxDiagnostics::analyze(&snapshot);
        let ack_result = diag.results.iter()
            .find(|r| r.rule == DiagnosticRule::AckRetryRate)
            .unwrap();

        assert_eq!(ack_result.severity, DiagnosticSeverity::Ok);
    }

    #[test]
    fn test_ack_retry_rate_warning() {
        let mut snapshot = make_snapshot();
        snapshot.retry_successes = 80;
        snapshot.retry_failures = 20;
        snapshot.retry_attempts = 15; // 15% retry rate

        let diag = KnxDiagnostics::analyze(&snapshot);
        let ack_result = diag.results.iter()
            .find(|r| r.rule == DiagnosticRule::AckRetryRate)
            .unwrap();

        assert_eq!(ack_result.severity, DiagnosticSeverity::Warning);
    }

    #[test]
    fn test_ack_retry_rate_critical() {
        let mut snapshot = make_snapshot();
        snapshot.retry_successes = 60;
        snapshot.retry_failures = 40;
        snapshot.retry_attempts = 35; // 35% retry rate

        let diag = KnxDiagnostics::analyze(&snapshot);
        let ack_result = diag.results.iter()
            .find(|r| r.rule == DiagnosticRule::AckRetryRate)
            .unwrap();

        assert_eq!(ack_result.severity, DiagnosticSeverity::Critical);
    }

    #[test]
    fn test_error_tracker_with_error_rate_below_threshold() {
        let mut snapshot = make_snapshot();
        snapshot.error_tracker_error_rate = 0.05;

        let diag = KnxDiagnostics::analyze(&snapshot);
        let et_result = diag.results.iter()
            .find(|r| r.rule == DiagnosticRule::ErrorTrackerThresholds)
            .unwrap();

        assert_eq!(et_result.severity, DiagnosticSeverity::Info);
    }

    #[test]
    fn test_serialization() {
        let snapshot = make_snapshot();
        let diag = KnxDiagnostics::analyze(&snapshot);

        let json = serde_json::to_string(&diag).unwrap();
        let deserialized: KnxDiagnostics = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.rules_evaluated, 9);
        assert_eq!(deserialized.results.len(), 9);
    }

    #[test]
    fn test_multiple_issues_combined() {
        let mut snapshot = make_snapshot();

        // Create multiple issues
        snapshot.tunnel_fatal_desyncs_total = 3;    // Critical
        snapshot.retry_circuit_trips = 2;            // Warning (all recovered)
        snapshot.retry_circuit_resets = 2;
        snapshot.cache_lookups_total = 100;
        snapshot.cache_hits_total = 20;
        snapshot.cache_misses_total = 80;
        snapshot.cache_hit_rate = 0.2;               // Warning
        snapshot.error_tracker_consecutive_triggers = 1; // Warning

        let diag = KnxDiagnostics::analyze(&snapshot);

        assert!(!diag.is_healthy());
        assert!(diag.critical_count >= 1);
        assert!(diag.warning_count >= 2);

        // Verify sorted: Critical first
        let first = &diag.results[0];
        assert_eq!(first.severity, DiagnosticSeverity::Critical);
    }
}

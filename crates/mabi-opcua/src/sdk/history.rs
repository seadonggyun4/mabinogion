//! OPC UA historical data access implementation.
//!
//! Provides storage and retrieval of historical data values with support
//! for raw and aggregated history reading.

use chrono::{DateTime, Duration, Utc};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::types::{data_value::DataValueBuilder, DataValue, NodeId, StatusCode, Variant};

/// Aggregate type for processed history.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AggregateType {
    /// No aggregation (raw data).
    None,
    /// Interpolated value at timestamps.
    Interpolative,
    /// Average value over interval.
    Average,
    /// Time-weighted average.
    TimeAverage,
    /// Total/sum over interval.
    Total,
    /// Minimum value in interval.
    Minimum,
    /// Maximum value in interval.
    Maximum,
    /// Minimum with timestamp.
    MinimumActualTime,
    /// Maximum with timestamp.
    MaximumActualTime,
    /// Range (max - min).
    Range,
    /// Count of values in interval.
    Count,
    /// Standard deviation.
    StandardDeviation,
    /// Variance.
    Variance,
    /// Start value of interval.
    Start,
    /// End value of interval.
    End,
    /// Delta (end - start).
    Delta,
    /// Duration in state (for discrete values).
    DurationInState,
    /// Number of transitions.
    NumberOfTransitions,
    /// Percent good data.
    PercentGood,
    /// Percent bad data.
    PercentBad,
    /// Worst quality in interval.
    WorstQuality,
    /// Annotations count.
    AnnotationCount,
}

impl Default for AggregateType {
    fn default() -> Self {
        Self::None
    }
}

/// Configuration for history store.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryStoreConfig {
    /// Maximum number of values to store per node.
    pub max_values_per_node: usize,
    /// Maximum age of historical data (in seconds).
    pub max_age_seconds: u64,
    /// Default read batch size.
    pub default_batch_size: usize,
    /// Maximum read batch size.
    pub max_batch_size: usize,
    /// Enable automatic cleanup.
    pub auto_cleanup: bool,
    /// Cleanup interval in seconds.
    pub cleanup_interval_seconds: u64,
    /// Enable data compression.
    pub enable_compression: bool,
    /// Deadband for compression (percentage).
    pub compression_deadband: f64,
}

impl Default for HistoryStoreConfig {
    fn default() -> Self {
        Self {
            max_values_per_node: 100_000,
            max_age_seconds: 86400 * 30, // 30 days
            default_batch_size: 1000,
            max_batch_size: 10_000,
            auto_cleanup: true,
            cleanup_interval_seconds: 3600, // 1 hour
            enable_compression: false,
            compression_deadband: 0.0,
        }
    }
}

impl HistoryStoreConfig {
    /// Create a new configuration with specified max values per node.
    pub fn with_max_values(mut self, max_values: usize) -> Self {
        self.max_values_per_node = max_values;
        self
    }

    /// Set maximum age in days.
    pub fn with_max_age_days(mut self, days: u64) -> Self {
        self.max_age_seconds = days * 86400;
        self
    }

    /// Enable compression with specified deadband.
    pub fn with_compression(mut self, deadband: f64) -> Self {
        self.enable_compression = true;
        self.compression_deadband = deadband;
        self
    }
}

/// A historical data point.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoricalDataPoint {
    /// Timestamp of the value.
    pub timestamp: DateTime<Utc>,
    /// The data value.
    pub value: DataValue,
    /// Whether this is a modified value.
    pub is_modified: bool,
    /// Modification info (user, time, etc.).
    pub modification_info: Option<ModificationInfo>,
}

impl HistoricalDataPoint {
    /// Create a new historical data point.
    pub fn new(timestamp: DateTime<Utc>, value: DataValue) -> Self {
        Self {
            timestamp,
            value,
            is_modified: false,
            modification_info: None,
        }
    }

    /// Create from a DataValue using its source timestamp.
    pub fn from_data_value(value: DataValue) -> Self {
        let timestamp = value.source_timestamp().unwrap_or_else(Utc::now);
        Self::new(timestamp, value)
    }
}

/// Information about a modification.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModificationInfo {
    /// User who made the modification.
    pub user_name: String,
    /// Time of modification.
    pub modification_time: DateTime<Utc>,
    /// Type of modification.
    pub modification_type: ModificationType,
}

/// Type of modification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ModificationType {
    /// Value was inserted.
    Insert,
    /// Value was replaced.
    Replace,
    /// Value was deleted.
    Delete,
    /// Value was annotated.
    Annotation,
}

/// Parameters for reading raw history.
#[derive(Debug, Clone)]
pub struct ReadRawModifiedDetails {
    /// Start time for the query.
    pub start_time: DateTime<Utc>,
    /// End time for the query.
    pub end_time: DateTime<Utc>,
    /// Maximum number of values to return.
    pub num_values_per_node: u32,
    /// Return bounds (start/end values).
    pub return_bounds: bool,
    /// Read modified values only.
    pub is_read_modified: bool,
}

impl ReadRawModifiedDetails {
    /// Create new raw read parameters.
    pub fn new(start_time: DateTime<Utc>, end_time: DateTime<Utc>) -> Self {
        Self {
            start_time,
            end_time,
            num_values_per_node: 0, // 0 means no limit
            return_bounds: false,
            is_read_modified: false,
        }
    }

    /// Set maximum number of values.
    pub fn with_max_values(mut self, max_values: u32) -> Self {
        self.num_values_per_node = max_values;
        self
    }

    /// Include boundary values.
    pub fn with_bounds(mut self) -> Self {
        self.return_bounds = true;
        self
    }
}

/// Parameters for reading processed/aggregated history.
#[derive(Debug, Clone)]
pub struct ReadProcessedDetails {
    /// Start time for the query.
    pub start_time: DateTime<Utc>,
    /// End time for the query.
    pub end_time: DateTime<Utc>,
    /// Processing interval in milliseconds.
    pub processing_interval_ms: f64,
    /// Aggregate type to use.
    pub aggregate_type: AggregateType,
    /// Aggregate configuration (e.g., percent data good required).
    pub aggregate_config: AggregateConfig,
}

impl ReadProcessedDetails {
    /// Create new processed read parameters.
    pub fn new(
        start_time: DateTime<Utc>,
        end_time: DateTime<Utc>,
        interval_ms: f64,
        aggregate_type: AggregateType,
    ) -> Self {
        Self {
            start_time,
            end_time,
            processing_interval_ms: interval_ms,
            aggregate_type,
            aggregate_config: AggregateConfig::default(),
        }
    }
}

/// Configuration for aggregate calculations.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AggregateConfig {
    /// Use server capabilities for defaults.
    pub use_server_capabilities_defaults: bool,
    /// Treat uncertain as bad.
    pub treat_uncertain_as_bad: bool,
    /// Percent data bad threshold.
    pub percent_data_bad: u8,
    /// Percent data good threshold.
    pub percent_data_good: u8,
    /// Use sloped extrapolation.
    pub use_sloped_extrapolation: bool,
}

impl Default for AggregateConfig {
    fn default() -> Self {
        Self {
            use_server_capabilities_defaults: true,
            treat_uncertain_as_bad: true,
            percent_data_bad: 100,
            percent_data_good: 100,
            use_sloped_extrapolation: false,
        }
    }
}

/// Result of a history read operation.
#[derive(Debug, Clone)]
pub struct HistoryReadResult {
    /// Status of the operation.
    pub status: StatusCode,
    /// Data values returned.
    pub data_values: Vec<HistoricalDataPoint>,
    /// Continuation point for pagination.
    pub continuation_point: Option<ContinuationPoint>,
}

impl HistoryReadResult {
    /// Create a successful result.
    pub fn success(data_values: Vec<HistoricalDataPoint>) -> Self {
        Self {
            status: StatusCode::GOOD,
            data_values,
            continuation_point: None,
        }
    }

    /// Create an error result.
    pub fn error(status: StatusCode) -> Self {
        Self {
            status,
            data_values: Vec::new(),
            continuation_point: None,
        }
    }

    /// Add a continuation point.
    pub fn with_continuation(mut self, point: ContinuationPoint) -> Self {
        self.continuation_point = Some(point);
        self
    }
}

/// Continuation point for paginated reads.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContinuationPoint {
    /// Unique identifier.
    pub id: u64,
    /// Node being read.
    pub node_id: NodeId,
    /// Next timestamp to read from.
    pub next_timestamp: DateTime<Utc>,
    /// End timestamp.
    pub end_time: DateTime<Utc>,
    /// Remaining count.
    pub remaining_count: u32,
    /// Creation time for expiry.
    pub created_at: DateTime<Utc>,
}

/// Node history data storage.
struct NodeHistory {
    /// Historical values sorted by timestamp.
    values: BTreeMap<DateTime<Utc>, HistoricalDataPoint>,
    /// Maximum values to store.
    max_values: usize,
    /// Last value (for compression).
    last_value: Option<f64>,
}

impl NodeHistory {
    fn new(max_values: usize) -> Self {
        Self {
            values: BTreeMap::new(),
            max_values,
            last_value: None,
        }
    }

    fn insert(&mut self, point: HistoricalDataPoint) {
        // Enforce maximum values
        while self.values.len() >= self.max_values {
            // Remove oldest
            if let Some(oldest) = self.values.keys().next().cloned() {
                self.values.remove(&oldest);
            }
        }

        // Update last value for compression
        if let Some(value) = point.value.value() {
            self.last_value = value.as_f64();
        }

        self.values.insert(point.timestamp, point);
    }

    fn read_range(
        &self,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
        max_values: Option<u32>,
    ) -> Vec<HistoricalDataPoint> {
        let range = self.values.range(start..=end);
        let iter: Box<dyn Iterator<Item = _>> = if let Some(max) = max_values {
            Box::new(range.take(max as usize))
        } else {
            Box::new(range)
        };
        iter.map(|(_, v)| v.clone()).collect()
    }

    fn cleanup(&mut self, max_age: Duration) {
        let cutoff = Utc::now() - max_age;
        self.values.retain(|ts, _| *ts >= cutoff);
    }

    fn len(&self) -> usize {
        self.values.len()
    }
}

/// Historical data store.
pub struct HistoryStore {
    config: HistoryStoreConfig,
    /// Per-node history storage.
    node_histories: RwLock<HashMap<NodeId, NodeHistory>>,
    /// Active continuation points.
    continuation_points: RwLock<HashMap<u64, ContinuationPoint>>,
    /// Next continuation point ID.
    next_continuation_id: AtomicU64,
    /// Statistics.
    stats: HistoryStoreStats,
}

/// Statistics for history store.
#[derive(Debug, Default)]
pub struct HistoryStoreStats {
    /// Total values stored.
    pub total_values: AtomicU64,
    /// Total reads performed.
    pub total_reads: AtomicU64,
    /// Total writes performed.
    pub total_writes: AtomicU64,
    /// Values cleaned up.
    pub values_cleaned: AtomicU64,
    /// Active nodes with history.
    pub active_nodes: AtomicU64,
}

impl HistoryStoreStats {
    /// Get current statistics as a snapshot.
    pub fn snapshot(&self) -> HistoryStoreStatsSnapshot {
        HistoryStoreStatsSnapshot {
            total_values: self.total_values.load(Ordering::Relaxed),
            total_reads: self.total_reads.load(Ordering::Relaxed),
            total_writes: self.total_writes.load(Ordering::Relaxed),
            values_cleaned: self.values_cleaned.load(Ordering::Relaxed),
            active_nodes: self.active_nodes.load(Ordering::Relaxed),
        }
    }
}

/// Snapshot of history store statistics.
#[derive(Debug, Clone)]
pub struct HistoryStoreStatsSnapshot {
    pub total_values: u64,
    pub total_reads: u64,
    pub total_writes: u64,
    pub values_cleaned: u64,
    pub active_nodes: u64,
}

impl HistoryStore {
    /// Create a new history store.
    pub fn new(config: HistoryStoreConfig) -> Self {
        Self {
            config,
            node_histories: RwLock::new(HashMap::new()),
            continuation_points: RwLock::new(HashMap::new()),
            next_continuation_id: AtomicU64::new(1),
            stats: HistoryStoreStats::default(),
        }
    }

    /// Create with default configuration.
    pub fn with_defaults() -> Self {
        Self::new(HistoryStoreConfig::default())
    }

    /// Record a historical value for a node.
    pub fn record_value(&self, node_id: &NodeId, value: DataValue) {
        let point = HistoricalDataPoint::from_data_value(value);
        self.record_point(node_id, point);
    }

    /// Record a historical data point.
    pub fn record_point(&self, node_id: &NodeId, point: HistoricalDataPoint) {
        // Check compression
        if self.config.enable_compression {
            if let Some(value) = point.value.value() {
                if let Some(new_f64) = value.as_f64() {
                    let histories = self.node_histories.read();
                    if let Some(history) = histories.get(node_id) {
                        if let Some(last) = history.last_value {
                            let change = ((new_f64 - last) / last).abs() * 100.0;
                            if change < self.config.compression_deadband {
                                return; // Skip this value (compression)
                            }
                        }
                    }
                }
            }
        }

        let mut histories = self.node_histories.write();
        let is_new = !histories.contains_key(node_id);
        let history = histories
            .entry(node_id.clone())
            .or_insert_with(|| NodeHistory::new(self.config.max_values_per_node));

        history.insert(point);
        self.stats.total_writes.fetch_add(1, Ordering::Relaxed);
        self.stats.total_values.fetch_add(1, Ordering::Relaxed);

        if is_new {
            self.stats.active_nodes.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Read raw historical values.
    pub fn read_raw(
        &self,
        node_id: &NodeId,
        details: &ReadRawModifiedDetails,
    ) -> HistoryReadResult {
        self.stats.total_reads.fetch_add(1, Ordering::Relaxed);

        let histories = self.node_histories.read();
        let Some(history) = histories.get(node_id) else {
            return HistoryReadResult::error(StatusCode::BAD_NODE_ID_UNKNOWN);
        };

        let max_values = if details.num_values_per_node == 0 {
            Some(self.config.max_batch_size as u32)
        } else {
            Some(
                details
                    .num_values_per_node
                    .min(self.config.max_batch_size as u32),
            )
        };

        let mut data_values = history.read_range(details.start_time, details.end_time, max_values);

        // Filter for modified values if requested
        if details.is_read_modified {
            data_values.retain(|v| v.is_modified);
        }

        // Check if continuation point is needed
        let result = if let Some(max) = max_values {
            if data_values.len() == max as usize {
                // More data may be available
                if let Some(last) = data_values.last() {
                    let next_ts = last.timestamp + Duration::milliseconds(1);
                    let cp = ContinuationPoint {
                        id: self.next_continuation_id.fetch_add(1, Ordering::Relaxed),
                        node_id: node_id.clone(),
                        next_timestamp: next_ts,
                        end_time: details.end_time,
                        remaining_count: 0, // Unknown
                        created_at: Utc::now(),
                    };

                    self.continuation_points.write().insert(cp.id, cp.clone());
                    HistoryReadResult::success(data_values).with_continuation(cp)
                } else {
                    HistoryReadResult::success(data_values)
                }
            } else {
                HistoryReadResult::success(data_values)
            }
        } else {
            HistoryReadResult::success(data_values)
        };

        result
    }

    /// Continue a previous read operation.
    pub fn continue_read(&self, continuation_point_id: u64) -> HistoryReadResult {
        let cp = {
            let points = self.continuation_points.read();
            points.get(&continuation_point_id).cloned()
        };

        let Some(cp) = cp else {
            return HistoryReadResult::error(StatusCode::BAD_CONTINUATION_POINT_INVALID);
        };

        // Check expiry (5 minutes)
        if Utc::now() - cp.created_at > Duration::minutes(5) {
            self.continuation_points
                .write()
                .remove(&continuation_point_id);
            return HistoryReadResult::error(StatusCode::BAD_CONTINUATION_POINT_INVALID);
        }

        let details = ReadRawModifiedDetails {
            start_time: cp.next_timestamp,
            end_time: cp.end_time,
            num_values_per_node: self.config.default_batch_size as u32,
            return_bounds: false,
            is_read_modified: false,
        };

        // Remove old continuation point
        self.continuation_points
            .write()
            .remove(&continuation_point_id);

        self.read_raw(&cp.node_id, &details)
    }

    /// Read processed/aggregated historical values.
    pub fn read_processed(
        &self,
        node_id: &NodeId,
        details: &ReadProcessedDetails,
    ) -> HistoryReadResult {
        self.stats.total_reads.fetch_add(1, Ordering::Relaxed);

        let histories = self.node_histories.read();
        let Some(history) = histories.get(node_id) else {
            return HistoryReadResult::error(StatusCode::BAD_NODE_ID_UNKNOWN);
        };

        // Get all raw values in range
        let raw_values = history.read_range(details.start_time, details.end_time, None);

        if raw_values.is_empty() {
            return HistoryReadResult::success(Vec::new());
        }

        // Calculate aggregate intervals
        let interval = Duration::milliseconds(details.processing_interval_ms as i64);
        let mut aggregated_values = Vec::new();
        let mut current_start = details.start_time;

        while current_start < details.end_time {
            let current_end = (current_start + interval).min(details.end_time);

            // Get values in this interval
            let interval_values: Vec<_> = raw_values
                .iter()
                .filter(|v| v.timestamp >= current_start && v.timestamp < current_end)
                .collect();

            // Calculate aggregate
            if let Some(aggregate_value) =
                self.calculate_aggregate(&interval_values, details.aggregate_type)
            {
                let data_value = DataValueBuilder::new()
                    .value(aggregate_value)
                    .status(StatusCode::GOOD)
                    .source_timestamp(current_start)
                    .server_timestamp(Utc::now())
                    .build();

                aggregated_values.push(HistoricalDataPoint::new(current_start, data_value));
            }

            current_start = current_end;
        }

        HistoryReadResult::success(aggregated_values)
    }

    /// Calculate an aggregate value.
    fn calculate_aggregate(
        &self,
        values: &[&HistoricalDataPoint],
        aggregate_type: AggregateType,
    ) -> Option<Variant> {
        if values.is_empty() {
            return None;
        }

        // Extract numeric values
        let numeric_values: Vec<f64> = values
            .iter()
            .filter_map(|v| v.value.value())
            .filter_map(|v| v.as_f64())
            .collect();

        if numeric_values.is_empty() && !matches!(aggregate_type, AggregateType::Count) {
            return None;
        }

        match aggregate_type {
            AggregateType::None => values.first().map(|v| v.value.value().cloned()).flatten(),

            AggregateType::Average | AggregateType::TimeAverage => {
                if numeric_values.is_empty() {
                    return None;
                }
                let sum: f64 = numeric_values.iter().sum();
                Some(Variant::Double(sum / numeric_values.len() as f64))
            }

            AggregateType::Total => {
                let sum: f64 = numeric_values.iter().sum();
                Some(Variant::Double(sum))
            }

            AggregateType::Minimum | AggregateType::MinimumActualTime => numeric_values
                .iter()
                .copied()
                .min_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
                .map(Variant::Double),

            AggregateType::Maximum | AggregateType::MaximumActualTime => numeric_values
                .iter()
                .copied()
                .max_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
                .map(Variant::Double),

            AggregateType::Range => {
                let min = numeric_values
                    .iter()
                    .copied()
                    .min_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
                let max = numeric_values
                    .iter()
                    .copied()
                    .max_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
                match (min, max) {
                    (Some(min), Some(max)) => Some(Variant::Double(max - min)),
                    _ => None,
                }
            }

            AggregateType::Count => Some(Variant::UInt32(values.len() as u32)),

            AggregateType::StandardDeviation => {
                if numeric_values.len() < 2 {
                    return None;
                }
                let mean = numeric_values.iter().sum::<f64>() / numeric_values.len() as f64;
                let variance = numeric_values
                    .iter()
                    .map(|v| (v - mean).powi(2))
                    .sum::<f64>()
                    / (numeric_values.len() - 1) as f64;
                Some(Variant::Double(variance.sqrt()))
            }

            AggregateType::Variance => {
                if numeric_values.len() < 2 {
                    return None;
                }
                let mean = numeric_values.iter().sum::<f64>() / numeric_values.len() as f64;
                let variance = numeric_values
                    .iter()
                    .map(|v| (v - mean).powi(2))
                    .sum::<f64>()
                    / (numeric_values.len() - 1) as f64;
                Some(Variant::Double(variance))
            }

            AggregateType::Start => values.first().map(|v| v.value.value().cloned()).flatten(),

            AggregateType::End => values.last().map(|v| v.value.value().cloned()).flatten(),

            AggregateType::Delta => {
                let start = values
                    .first()
                    .and_then(|v| v.value.value())
                    .and_then(|v| v.as_f64());
                let end = values
                    .last()
                    .and_then(|v| v.value.value())
                    .and_then(|v| v.as_f64());
                match (start, end) {
                    (Some(s), Some(e)) => Some(Variant::Double(e - s)),
                    _ => None,
                }
            }

            AggregateType::PercentGood => {
                let good_count = values.iter().filter(|v| v.value.status().is_good()).count();
                Some(Variant::Double(
                    (good_count as f64 / values.len() as f64) * 100.0,
                ))
            }

            AggregateType::PercentBad => {
                let bad_count = values.iter().filter(|v| v.value.status().is_bad()).count();
                Some(Variant::Double(
                    (bad_count as f64 / values.len() as f64) * 100.0,
                ))
            }

            _ => None, // Other aggregates not yet implemented
        }
    }

    /// Clean up old historical data.
    pub fn cleanup(&self) {
        let max_age = Duration::seconds(self.config.max_age_seconds as i64);
        let mut histories = self.node_histories.write();
        let mut cleaned = 0u64;

        for (_, history) in histories.iter_mut() {
            let before = history.len();
            history.cleanup(max_age);
            let after = history.len();
            cleaned += (before - after) as u64;
        }

        // Remove empty histories
        histories.retain(|_, h| !h.values.is_empty());

        self.stats
            .values_cleaned
            .fetch_add(cleaned, Ordering::Relaxed);
        self.stats
            .active_nodes
            .store(histories.len() as u64, Ordering::Relaxed);
    }

    /// Get statistics.
    pub fn stats(&self) -> HistoryStoreStatsSnapshot {
        self.stats.snapshot()
    }

    /// Get the number of values stored for a node.
    pub fn value_count(&self, node_id: &NodeId) -> usize {
        self.node_histories
            .read()
            .get(node_id)
            .map(|h| h.len())
            .unwrap_or(0)
    }

    /// Clear all history for a node.
    pub fn clear_node_history(&self, node_id: &NodeId) {
        let mut histories = self.node_histories.write();
        if let Some(history) = histories.remove(node_id) {
            self.stats
                .total_values
                .fetch_sub(history.len() as u64, Ordering::Relaxed);
            self.stats.active_nodes.fetch_sub(1, Ordering::Relaxed);
        }
    }

    /// Clear all history.
    pub fn clear_all(&self) {
        let mut histories = self.node_histories.write();
        histories.clear();
        self.stats.total_values.store(0, Ordering::Relaxed);
        self.stats.active_nodes.store(0, Ordering::Relaxed);
    }
}

/// Builder for history stores.
pub struct HistoryStoreBuilder {
    config: HistoryStoreConfig,
}

impl HistoryStoreBuilder {
    /// Create a new builder.
    pub fn new() -> Self {
        Self {
            config: HistoryStoreConfig::default(),
        }
    }

    /// Set maximum values per node.
    pub fn max_values_per_node(mut self, max: usize) -> Self {
        self.config.max_values_per_node = max;
        self
    }

    /// Set maximum age in seconds.
    pub fn max_age_seconds(mut self, seconds: u64) -> Self {
        self.config.max_age_seconds = seconds;
        self
    }

    /// Set batch sizes.
    pub fn batch_sizes(mut self, default: usize, max: usize) -> Self {
        self.config.default_batch_size = default;
        self.config.max_batch_size = max;
        self
    }

    /// Enable compression.
    pub fn with_compression(mut self, deadband: f64) -> Self {
        self.config.enable_compression = true;
        self.config.compression_deadband = deadband;
        self
    }

    /// Build the history store.
    pub fn build(self) -> HistoryStore {
        HistoryStore::new(self.config)
    }
}

impl Default for HistoryStoreBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_record_and_read() {
        let store = HistoryStore::with_defaults();
        let node_id = NodeId::numeric(2, 1001);

        // Record some values
        for i in 0..10 {
            let value = DataValueBuilder::new()
                .value(Variant::Double(i as f64 * 10.0))
                .source_timestamp(Utc::now())
                .build();
            store.record_value(&node_id, value);
        }

        // Read raw history
        let details = ReadRawModifiedDetails::new(
            Utc::now() - Duration::hours(1),
            Utc::now() + Duration::hours(1),
        );

        let result = store.read_raw(&node_id, &details);
        assert!(result.status.is_good());
        assert_eq!(result.data_values.len(), 10);
    }

    #[test]
    fn test_aggregate_average() {
        let store = HistoryStore::with_defaults();
        let node_id = NodeId::numeric(2, 1002);

        let base_time = Utc::now() - Duration::hours(1);

        // Record values: 10, 20, 30, 40, 50
        for i in 0..5 {
            let value = DataValueBuilder::new()
                .value(Variant::Double((i + 1) as f64 * 10.0))
                .source_timestamp(base_time + Duration::minutes(i * 10))
                .build();
            store.record_value(&node_id, value);
        }

        // Read processed history with average
        let details = ReadProcessedDetails::new(
            base_time,
            base_time + Duration::hours(1),
            3600000.0, // 1 hour interval
            AggregateType::Average,
        );

        let result = store.read_processed(&node_id, &details);
        assert!(result.status.is_good());
        assert!(!result.data_values.is_empty());

        if let Some(first) = result.data_values.first() {
            if let Some(Variant::Double(avg)) = first.value.value() {
                assert!((avg - 30.0).abs() < 0.001); // Average of 10,20,30,40,50
            }
        }
    }

    #[test]
    fn test_max_values_limit() {
        let config = HistoryStoreConfig::default().with_max_values(5);
        let store = HistoryStore::new(config);
        let node_id = NodeId::numeric(2, 1003);

        // Record 10 values
        for i in 0..10 {
            let value = DataValueBuilder::new()
                .value(Variant::Double(i as f64))
                .source_timestamp(Utc::now() + Duration::milliseconds(i))
                .build();
            store.record_value(&node_id, value);
        }

        // Should only have 5 values
        assert_eq!(store.value_count(&node_id), 5);
    }

    #[test]
    fn test_cleanup() {
        let mut config = HistoryStoreConfig::default();
        config.max_age_seconds = 1; // 1 second
        let store = HistoryStore::new(config);
        let node_id = NodeId::numeric(2, 1004);

        // Record old value
        let old_value = DataValueBuilder::new()
            .value(Variant::Double(100.0))
            .source_timestamp(Utc::now() - Duration::seconds(10))
            .build();
        store.record_point(
            &node_id,
            HistoricalDataPoint::new(Utc::now() - Duration::seconds(10), old_value),
        );

        // Record new value
        let new_value = DataValueBuilder::new()
            .value(Variant::Double(200.0))
            .source_timestamp(Utc::now())
            .build();
        store.record_value(&node_id, new_value);

        assert_eq!(store.value_count(&node_id), 2);

        // Cleanup
        store.cleanup();

        // Only new value should remain
        assert_eq!(store.value_count(&node_id), 1);
    }
}

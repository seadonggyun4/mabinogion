//! Configuration update utilities and helpers.

use std::time::{Duration, SystemTime};

use serde::{Deserialize, Serialize};

use super::{ConfigUpdate, RuntimeState};

/// Builder for creating configuration updates.
#[derive(Default)]
pub struct UpdateBuilder {
    updates: Vec<ConfigUpdate>,
}

impl UpdateBuilder {
    /// Create a new update builder.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set maximum connections.
    pub fn max_connections(mut self, max: usize) -> Self {
        self.updates.push(ConfigUpdate::MaxConnections(max));
        self
    }

    /// Set idle timeout.
    pub fn idle_timeout(mut self, duration: Duration) -> Self {
        self.updates.push(ConfigUpdate::IdleTimeout(duration));
        self
    }

    /// Set request timeout.
    pub fn request_timeout(mut self, duration: Duration) -> Self {
        self.updates.push(ConfigUpdate::RequestTimeout(duration));
        self
    }

    /// Enable/disable a unit.
    pub fn unit_enabled(mut self, unit_id: u8, enabled: bool) -> Self {
        self.updates
            .push(ConfigUpdate::UnitEnabled { unit_id, enabled });
        self
    }

    /// Enable TCP nodelay.
    pub fn tcp_nodelay(mut self, enabled: bool) -> Self {
        self.updates.push(ConfigUpdate::TcpNoDelay(enabled));
        self
    }

    /// Set keepalive interval.
    pub fn keepalive(mut self, interval: Option<Duration>) -> Self {
        self.updates.push(ConfigUpdate::KeepaliveInterval(interval));
        self
    }

    /// Enable/disable metrics.
    pub fn metrics_enabled(mut self, enabled: bool) -> Self {
        self.updates.push(ConfigUpdate::MetricsEnabled(enabled));
        self
    }

    /// Enable/disable debug logging.
    pub fn debug_logging(mut self, enabled: bool) -> Self {
        self.updates.push(ConfigUpdate::DebugLogging(enabled));
        self
    }

    /// Set a register value.
    pub fn set_register(mut self, unit_id: u8, address: u16, value: u16) -> Self {
        self.updates.push(ConfigUpdate::SetRegister {
            unit_id,
            address,
            value,
        });
        self
    }

    /// Set multiple registers.
    pub fn set_registers(mut self, unit_id: u8, start_address: u16, values: Vec<u16>) -> Self {
        self.updates.push(ConfigUpdate::SetRegisters {
            unit_id,
            start_address,
            values,
        });
        self
    }

    /// Set a coil value.
    pub fn set_coil(mut self, unit_id: u8, address: u16, value: bool) -> Self {
        self.updates.push(ConfigUpdate::SetCoil {
            unit_id,
            address,
            value,
        });
        self
    }

    /// Add a custom setting.
    pub fn custom(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.updates.push(ConfigUpdate::Custom {
            key: key.into(),
            value: value.into(),
        });
        self
    }

    /// Add any update.
    pub fn update(mut self, update: ConfigUpdate) -> Self {
        self.updates.push(update);
        self
    }

    /// Build the list of updates.
    pub fn build(self) -> Vec<ConfigUpdate> {
        self.updates
    }

    /// Get the number of updates.
    pub fn len(&self) -> usize {
        self.updates.len()
    }

    /// Check if empty.
    pub fn is_empty(&self) -> bool {
        self.updates.is_empty()
    }
}

/// Record of a configuration update for auditing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateRecord {
    /// Unique ID for this update.
    pub id: u64,

    /// The update that was applied.
    pub update: ConfigUpdate,

    /// When the update was applied.
    pub timestamp: SystemTime,

    /// Whether the update was successful.
    pub success: bool,

    /// Error message if the update failed.
    pub error: Option<String>,

    /// Who initiated the update (if known).
    pub source: Option<String>,
}

impl UpdateRecord {
    /// Create a successful update record.
    pub fn success(id: u64, update: ConfigUpdate) -> Self {
        Self {
            id,
            update,
            timestamp: SystemTime::now(),
            success: true,
            error: None,
            source: None,
        }
    }

    /// Create a failed update record.
    pub fn failure(id: u64, update: ConfigUpdate, error: String) -> Self {
        Self {
            id,
            update,
            timestamp: SystemTime::now(),
            success: false,
            error: Some(error),
            source: None,
        }
    }

    /// Set the source of the update.
    pub fn with_source(mut self, source: impl Into<String>) -> Self {
        self.source = Some(source.into());
        self
    }
}

/// Audit log for configuration updates.
pub struct UpdateAuditLog {
    records: Vec<UpdateRecord>,
    max_records: usize,
    next_id: u64,
}

impl UpdateAuditLog {
    /// Create a new audit log.
    pub fn new(max_records: usize) -> Self {
        Self {
            records: Vec::with_capacity(max_records.min(1000)),
            max_records,
            next_id: 1,
        }
    }

    /// Record a successful update.
    pub fn record_success(&mut self, update: ConfigUpdate) -> &UpdateRecord {
        let id = self.next_id;
        self.next_id += 1;

        self.add_record(UpdateRecord::success(id, update))
    }

    /// Record a failed update.
    pub fn record_failure(&mut self, update: ConfigUpdate, error: String) -> &UpdateRecord {
        let id = self.next_id;
        self.next_id += 1;

        self.add_record(UpdateRecord::failure(id, update, error))
    }

    fn add_record(&mut self, record: UpdateRecord) -> &UpdateRecord {
        // Remove oldest if at capacity
        while self.records.len() >= self.max_records {
            self.records.remove(0);
        }

        self.records.push(record);
        self.records.last().unwrap()
    }

    /// Get all records.
    pub fn records(&self) -> &[UpdateRecord] {
        &self.records
    }

    /// Get recent records (last N).
    pub fn recent(&self, n: usize) -> &[UpdateRecord] {
        let start = self.records.len().saturating_sub(n);
        &self.records[start..]
    }

    /// Get failed updates only.
    pub fn failures(&self) -> Vec<&UpdateRecord> {
        self.records.iter().filter(|r| !r.success).collect()
    }

    /// Clear all records.
    pub fn clear(&mut self) {
        self.records.clear();
    }

    /// Get total update count.
    pub fn total_count(&self) -> u64 {
        self.next_id - 1
    }

    /// Get success rate.
    pub fn success_rate(&self) -> f64 {
        if self.records.is_empty() {
            return 1.0;
        }
        let successes = self.records.iter().filter(|r| r.success).count();
        successes as f64 / self.records.len() as f64
    }
}

impl Default for UpdateAuditLog {
    fn default() -> Self {
        Self::new(1000)
    }
}

/// Diff between two runtime states.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateDiff {
    /// Fields that changed.
    pub changes: Vec<FieldChange>,
}

/// A single field change.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FieldChange {
    /// Field name.
    pub field: String,
    /// Old value (as string).
    pub old: String,
    /// New value (as string).
    pub new: String,
}

impl StateDiff {
    /// Calculate diff between two states.
    pub fn diff(old: &RuntimeState, new: &RuntimeState) -> Self {
        let mut changes = Vec::new();

        if old.max_connections != new.max_connections {
            changes.push(FieldChange {
                field: "max_connections".into(),
                old: old.max_connections.to_string(),
                new: new.max_connections.to_string(),
            });
        }

        if old.idle_timeout != new.idle_timeout {
            changes.push(FieldChange {
                field: "idle_timeout".into(),
                old: format!("{:?}", old.idle_timeout),
                new: format!("{:?}", new.idle_timeout),
            });
        }

        if old.request_timeout != new.request_timeout {
            changes.push(FieldChange {
                field: "request_timeout".into(),
                old: format!("{:?}", old.request_timeout),
                new: format!("{:?}", new.request_timeout),
            });
        }

        if old.tcp_nodelay != new.tcp_nodelay {
            changes.push(FieldChange {
                field: "tcp_nodelay".into(),
                old: old.tcp_nodelay.to_string(),
                new: new.tcp_nodelay.to_string(),
            });
        }

        if old.metrics_enabled != new.metrics_enabled {
            changes.push(FieldChange {
                field: "metrics_enabled".into(),
                old: old.metrics_enabled.to_string(),
                new: new.metrics_enabled.to_string(),
            });
        }

        if old.debug_logging != new.debug_logging {
            changes.push(FieldChange {
                field: "debug_logging".into(),
                old: old.debug_logging.to_string(),
                new: new.debug_logging.to_string(),
            });
        }

        if old.enabled_units != new.enabled_units {
            changes.push(FieldChange {
                field: "enabled_units".into(),
                old: format!("{:?}", old.enabled_units),
                new: format!("{:?}", new.enabled_units),
            });
        }

        Self { changes }
    }

    /// Check if there are any changes.
    pub fn has_changes(&self) -> bool {
        !self.changes.is_empty()
    }

    /// Get number of changes.
    pub fn change_count(&self) -> usize {
        self.changes.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_update_builder() {
        let updates = UpdateBuilder::new()
            .max_connections(200)
            .idle_timeout(Duration::from_secs(600))
            .metrics_enabled(false)
            .build();

        assert_eq!(updates.len(), 3);
    }

    #[test]
    fn test_update_builder_empty() {
        let builder = UpdateBuilder::new();
        assert!(builder.is_empty());
    }

    #[test]
    fn test_update_builder_registers() {
        let updates = UpdateBuilder::new()
            .set_register(1, 100, 1234)
            .set_registers(1, 200, vec![1, 2, 3])
            .set_coil(1, 50, true)
            .build();

        assert_eq!(updates.len(), 3);
    }

    #[test]
    fn test_update_record() {
        let record =
            UpdateRecord::success(1, ConfigUpdate::MaxConnections(100)).with_source("test_user");

        assert!(record.success);
        assert_eq!(record.source, Some("test_user".to_string()));
    }

    #[test]
    fn test_update_record_failure() {
        let record =
            UpdateRecord::failure(2, ConfigUpdate::MaxConnections(0), "Invalid value".into());

        assert!(!record.success);
        assert!(record.error.is_some());
    }

    #[test]
    fn test_audit_log() {
        let mut log = UpdateAuditLog::new(10);

        log.record_success(ConfigUpdate::MaxConnections(100));
        log.record_success(ConfigUpdate::MaxConnections(200));
        log.record_failure(ConfigUpdate::MaxConnections(0), "Invalid".into());

        assert_eq!(log.records().len(), 3);
        assert_eq!(log.failures().len(), 1);
        assert_eq!(log.total_count(), 3);
    }

    #[test]
    fn test_audit_log_capacity() {
        let mut log = UpdateAuditLog::new(5);

        for i in 0..10 {
            log.record_success(ConfigUpdate::MaxConnections(i));
        }

        assert_eq!(log.records().len(), 5);
        assert_eq!(log.total_count(), 10);
    }

    #[test]
    fn test_audit_log_success_rate() {
        let mut log = UpdateAuditLog::new(10);

        log.record_success(ConfigUpdate::MaxConnections(100));
        log.record_success(ConfigUpdate::MaxConnections(100));
        log.record_failure(ConfigUpdate::MaxConnections(0), "Error".into());

        let rate = log.success_rate();
        assert!((rate - 0.6666).abs() < 0.01);
    }

    #[test]
    fn test_state_diff() {
        let old = RuntimeState::default();
        let mut new = RuntimeState::default();
        new.max_connections = 200;
        new.metrics_enabled = false;

        let diff = StateDiff::diff(&old, &new);

        assert!(diff.has_changes());
        assert_eq!(diff.change_count(), 2);
    }

    #[test]
    fn test_state_diff_no_changes() {
        let state = RuntimeState::default();
        let diff = StateDiff::diff(&state, &state);

        assert!(!diff.has_changes());
    }
}

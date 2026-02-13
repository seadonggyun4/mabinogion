//! OPC UA monitored item implementation.
//!
//! Monitored items track changes to node attributes and generate notifications.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::types::{NodeId, AttributeId, DataValue, StatusCode, Variant};

/// Data change trigger type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DataChangeTrigger {
    /// Trigger on status change only.
    Status,
    /// Trigger on status or value change.
    StatusValue,
    /// Trigger on status, value, or timestamp change.
    StatusValueTimestamp,
}

impl Default for DataChangeTrigger {
    fn default() -> Self {
        Self::StatusValue
    }
}

/// Deadband type for filtering value changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DeadbandType {
    /// No deadband filtering.
    None,
    /// Absolute deadband (change must exceed this value).
    Absolute,
    /// Percent deadband (change must exceed this percentage of the range).
    Percent,
}

impl Default for DeadbandType {
    fn default() -> Self {
        Self::None
    }
}

/// Data change filter configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataChangeFilter {
    /// Trigger type.
    pub trigger: DataChangeTrigger,
    /// Deadband type.
    pub deadband_type: DeadbandType,
    /// Deadband value.
    pub deadband_value: f64,
}

impl Default for DataChangeFilter {
    fn default() -> Self {
        Self {
            trigger: DataChangeTrigger::StatusValue,
            deadband_type: DeadbandType::None,
            deadband_value: 0.0,
        }
    }
}

impl DataChangeFilter {
    /// Create a new data change filter.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set absolute deadband.
    pub fn with_absolute_deadband(mut self, value: f64) -> Self {
        self.deadband_type = DeadbandType::Absolute;
        self.deadband_value = value;
        self
    }

    /// Set percent deadband.
    pub fn with_percent_deadband(mut self, percent: f64) -> Self {
        self.deadband_type = DeadbandType::Percent;
        self.deadband_value = percent;
        self
    }

    /// Set trigger type.
    pub fn with_trigger(mut self, trigger: DataChangeTrigger) -> Self {
        self.trigger = trigger;
        self
    }
}

/// The kind of monitoring: data change or event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MonitoredItemKind {
    /// Monitor data changes on a variable attribute.
    DataChange,
    /// Monitor events on an object with EventNotifier.
    Event,
}

impl Default for MonitoredItemKind {
    fn default() -> Self {
        Self::DataChange
    }
}

/// Monitored item configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MonitoredItemConfig {
    /// Node ID to monitor.
    pub node_id: NodeId,
    /// Attribute ID to monitor.
    pub attribute_id: AttributeId,
    /// Sampling interval in milliseconds.
    pub sampling_interval_ms: f64,
    /// Queue size for notifications.
    pub queue_size: u32,
    /// Whether to discard oldest notifications when queue is full.
    pub discard_oldest: bool,
    /// Data change filter (optional).
    pub filter: Option<DataChangeFilter>,
    /// The kind of monitoring (data change vs event).
    #[serde(default)]
    pub kind: MonitoredItemKind,
    /// Event filter (only used when kind == Event).
    /// Serialization is skipped since EventFilter doesn't derive Serialize.
    #[serde(skip)]
    pub event_filter: Option<super::event::EventFilter>,
}

impl Default for MonitoredItemConfig {
    fn default() -> Self {
        Self {
            node_id: NodeId::null(),
            attribute_id: AttributeId::Value,
            sampling_interval_ms: 1000.0,
            queue_size: 10,
            discard_oldest: true,
            filter: None,
            kind: MonitoredItemKind::DataChange,
            event_filter: None,
        }
    }
}

impl MonitoredItemConfig {
    /// Create a new config for monitoring a node's value.
    pub fn for_value(node_id: NodeId) -> Self {
        Self {
            node_id,
            attribute_id: AttributeId::Value,
            ..Default::default()
        }
    }

    /// Create a new config for monitoring events on a node.
    pub fn for_event(node_id: NodeId, event_filter: super::event::EventFilter) -> Self {
        Self {
            node_id,
            attribute_id: AttributeId::EventNotifier,
            kind: MonitoredItemKind::Event,
            event_filter: Some(event_filter),
            ..Default::default()
        }
    }

    /// Set sampling interval.
    pub fn with_sampling_interval(mut self, interval_ms: f64) -> Self {
        self.sampling_interval_ms = interval_ms;
        self
    }

    /// Set queue size.
    pub fn with_queue_size(mut self, size: u32) -> Self {
        self.queue_size = size;
        self
    }

    /// Set data change filter.
    pub fn with_filter(mut self, filter: DataChangeFilter) -> Self {
        self.filter = Some(filter);
        self
    }
}

/// Monitored item notification.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MonitoredItemNotification {
    /// Client handle (assigned by client).
    pub client_handle: u32,
    /// Monitored item ID.
    pub monitored_item_id: u32,
    /// The data value.
    pub value: DataValue,
}

/// Monitoring mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MonitoringMode {
    /// Monitoring is disabled.
    Disabled,
    /// Sampling only (no reporting).
    Sampling,
    /// Reporting enabled.
    Reporting,
}

impl Default for MonitoringMode {
    fn default() -> Self {
        Self::Reporting
    }
}

/// A monitored item instance.
#[derive(Debug)]
pub struct MonitoredItem {
    /// Monitored item ID.
    id: u32,
    /// Configuration.
    config: MonitoredItemConfig,
    /// Client handle.
    client_handle: u32,
    /// Monitoring mode.
    monitoring_mode: MonitoringMode,
    /// Last sampled value.
    last_value: Option<DataValue>,
    /// Notification queue.
    notification_queue: Vec<MonitoredItemNotification>,
    /// Last sample time.
    last_sample_time: DateTime<Utc>,
    /// Created time.
    created_at: DateTime<Utc>,
}

impl MonitoredItem {
    /// Create a new monitored item.
    pub fn new(id: u32, config: MonitoredItemConfig) -> Self {
        Self {
            id,
            config,
            client_handle: id, // Default to same as ID
            monitoring_mode: MonitoringMode::Reporting,
            last_value: None,
            notification_queue: Vec::new(),
            last_sample_time: Utc::now(),
            created_at: Utc::now(),
        }
    }

    /// Get the monitored item ID.
    pub fn id(&self) -> u32 {
        self.id
    }

    /// Get the configuration.
    pub fn config(&self) -> &MonitoredItemConfig {
        &self.config
    }

    /// Get the client handle.
    pub fn client_handle(&self) -> u32 {
        self.client_handle
    }

    /// Set the client handle.
    pub fn set_client_handle(&mut self, handle: u32) {
        self.client_handle = handle;
    }

    /// Get the monitoring mode.
    pub fn monitoring_mode(&self) -> MonitoringMode {
        self.monitoring_mode
    }

    /// Set the monitoring mode.
    pub fn set_monitoring_mode(&mut self, mode: MonitoringMode) {
        self.monitoring_mode = mode;
    }

    /// Modify the monitored item.
    pub fn modify(
        &mut self,
        sampling_interval_ms: f64,
        queue_size: u32,
        discard_oldest: bool,
        filter: Option<DataChangeFilter>,
    ) {
        self.config.sampling_interval_ms = sampling_interval_ms;
        self.config.queue_size = queue_size;
        self.config.discard_oldest = discard_oldest;
        self.config.filter = filter;
    }

    /// Handle a value change.
    ///
    /// Returns a notification if the change passes the filter.
    pub fn on_value_change(&mut self, new_value: DataValue) -> Option<MonitoredItemNotification> {
        if self.monitoring_mode == MonitoringMode::Disabled {
            return None;
        }

        // Check if change passes filter
        if !self.should_notify(&new_value) {
            return None;
        }

        // Update last value
        self.last_value = Some(new_value.clone());
        self.last_sample_time = Utc::now();

        // If sampling only, don't report
        if self.monitoring_mode == MonitoringMode::Sampling {
            return None;
        }

        // Create notification
        let notification = MonitoredItemNotification {
            client_handle: self.client_handle,
            monitored_item_id: self.id,
            value: new_value,
        };

        // Queue or return notification
        if self.notification_queue.len() >= self.config.queue_size as usize {
            if self.config.discard_oldest {
                self.notification_queue.remove(0);
            } else {
                // Discard newest - don't add
                return None;
            }
        }

        self.notification_queue.push(notification.clone());
        Some(notification)
    }

    /// Check if a value change should trigger a notification.
    fn should_notify(&self, new_value: &DataValue) -> bool {
        let Some(ref last_value) = self.last_value else {
            // No previous value, always notify
            return true;
        };

        let Some(ref filter) = self.config.filter else {
            // No filter, check for actual value change
            return match (last_value.value(), new_value.value()) {
                (Some(last), Some(new)) => last != new,
                (None, None) => false,
                _ => true, // One has value, other doesn't
            };
        };

        // Check trigger type
        match filter.trigger {
            DataChangeTrigger::Status => {
                // Only notify on status change
                return last_value.status() != new_value.status();
            }
            DataChangeTrigger::StatusValue => {
                // Notify on status or value change
                if last_value.status() != new_value.status() {
                    return true;
                }
            }
            DataChangeTrigger::StatusValueTimestamp => {
                // Notify on any change including timestamp
                if last_value.status() != new_value.status() {
                    return true;
                }
                if last_value.source_timestamp() != new_value.source_timestamp() {
                    return true;
                }
            }
        }

        // Apply deadband filter for numeric values
        if filter.deadband_type != DeadbandType::None {
            if let (Some(last_var), Some(new_var)) = (last_value.value(), new_value.value()) {
                if let (Some(last_f), Some(new_f)) = (last_var.as_f64(), new_var.as_f64()) {
                    let change = (new_f - last_f).abs();

                    match filter.deadband_type {
                        DeadbandType::Absolute => {
                            if change < filter.deadband_value {
                                return false;
                            }
                        }
                        DeadbandType::Percent => {
                            // For percent deadband, would need range info
                            // Simplified: use as absolute for now
                            if change < filter.deadband_value {
                                return false;
                            }
                        }
                        DeadbandType::None => {}
                    }
                }
            }
        }

        // Check for actual value change
        match (last_value.value(), new_value.value()) {
            (Some(last), Some(new)) => last != new,
            (None, Some(_)) | (Some(_), None) => true,
            (None, None) => false,
        }
    }

    /// Get and clear pending notifications.
    pub fn take_notifications(&mut self) -> Vec<MonitoredItemNotification> {
        std::mem::take(&mut self.notification_queue)
    }

    /// Get the number of queued notifications.
    pub fn queue_length(&self) -> usize {
        self.notification_queue.len()
    }

    /// Get the last sampled value.
    pub fn last_value(&self) -> Option<&DataValue> {
        self.last_value.as_ref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_monitored_item_creation() {
        let config = MonitoredItemConfig::for_value(NodeId::numeric(2, 1001))
            .with_sampling_interval(500.0)
            .with_queue_size(5);

        let item = MonitoredItem::new(1, config);

        assert_eq!(item.id(), 1);
        assert_eq!(item.config().sampling_interval_ms, 500.0);
        assert_eq!(item.config().queue_size, 5);
    }

    #[test]
    fn test_value_change_notification() {
        let config = MonitoredItemConfig::for_value(NodeId::numeric(2, 1001));
        let mut item = MonitoredItem::new(1, config);

        // First value
        let notification = item.on_value_change(DataValue::new(Variant::double(25.0)));
        assert!(notification.is_some());

        // Same value - should still notify without filter
        let notification = item.on_value_change(DataValue::new(Variant::double(25.0)));
        assert!(notification.is_none()); // Values are equal

        // Different value
        let notification = item.on_value_change(DataValue::new(Variant::double(26.0)));
        assert!(notification.is_some());
    }

    #[test]
    fn test_deadband_filter() {
        let config = MonitoredItemConfig::for_value(NodeId::numeric(2, 1001))
            .with_filter(DataChangeFilter::new().with_absolute_deadband(1.0));

        let mut item = MonitoredItem::new(1, config);

        // Initial value
        item.on_value_change(DataValue::new(Variant::double(25.0)));

        // Small change - should not notify
        let notification = item.on_value_change(DataValue::new(Variant::double(25.5)));
        assert!(notification.is_none());

        // Large change - should notify
        let notification = item.on_value_change(DataValue::new(Variant::double(27.0)));
        assert!(notification.is_some());
    }

    #[test]
    fn test_queue_overflow_discard_oldest() {
        let config = MonitoredItemConfig::for_value(NodeId::numeric(2, 1001))
            .with_queue_size(3);

        let mut item = MonitoredItem::new(1, config);

        // Add 5 notifications
        for i in 0..5 {
            item.on_value_change(DataValue::new(Variant::double(i as f64)));
        }

        // Should only have 3
        assert_eq!(item.queue_length(), 3);
    }

    #[test]
    fn test_monitoring_mode() {
        let config = MonitoredItemConfig::for_value(NodeId::numeric(2, 1001));
        let mut item = MonitoredItem::new(1, config);

        // Disable monitoring
        item.set_monitoring_mode(MonitoringMode::Disabled);
        let notification = item.on_value_change(DataValue::new(Variant::double(25.0)));
        assert!(notification.is_none());

        // Enable sampling only
        item.set_monitoring_mode(MonitoringMode::Sampling);
        let notification = item.on_value_change(DataValue::new(Variant::double(26.0)));
        assert!(notification.is_none());
        assert!(item.last_value().is_some()); // Value is still sampled

        // Enable reporting
        item.set_monitoring_mode(MonitoringMode::Reporting);
        let notification = item.on_value_change(DataValue::new(Variant::double(27.0)));
        assert!(notification.is_some());
    }
}

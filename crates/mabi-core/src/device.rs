//! Device trait and related types.

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::error::Result;
use crate::protocol::Protocol;
use crate::tags::Tags;
use crate::types::{DataPoint, DataPointDef};
use crate::value::Value;

/// Device state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum DeviceState {
    /// Device is not initialized.
    #[default]
    Uninitialized,
    /// Device is initializing.
    Initializing,
    /// Device is online and ready.
    Online,
    /// Device is offline.
    Offline,
    /// Device has an error.
    Error,
    /// Device is shutting down.
    ShuttingDown,
}

impl DeviceState {
    /// Check if device is operational.
    pub fn is_operational(&self) -> bool {
        matches!(self, Self::Online)
    }

    /// Check if device can accept requests.
    pub fn can_accept_requests(&self) -> bool {
        matches!(self, Self::Online | Self::Initializing)
    }
}

/// Device information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceInfo {
    /// Unique device ID.
    pub id: String,
    /// Human-readable name.
    pub name: String,
    /// Description.
    pub description: String,
    /// Protocol.
    pub protocol: Protocol,
    /// Device state.
    pub state: DeviceState,
    /// Number of data points.
    pub point_count: usize,
    /// Creation time.
    pub created_at: DateTime<Utc>,
    /// Last update time.
    pub updated_at: DateTime<Utc>,
    /// Custom metadata.
    #[serde(default)]
    pub metadata: HashMap<String, String>,
    /// Device tags for organization and filtering.
    #[serde(default, skip_serializing_if = "Tags::is_empty")]
    pub tags: Tags,
}

impl DeviceInfo {
    /// Create new device info.
    pub fn new(id: impl Into<String>, name: impl Into<String>, protocol: Protocol) -> Self {
        let now = Utc::now();
        Self {
            id: id.into(),
            name: name.into(),
            description: String::new(),
            protocol,
            state: DeviceState::Uninitialized,
            point_count: 0,
            created_at: now,
            updated_at: now,
            metadata: HashMap::new(),
            tags: Tags::new(),
        }
    }

    /// Set description.
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = description.into();
        self
    }

    /// Add metadata.
    pub fn with_metadata(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.metadata.insert(key.into(), value.into());
        self
    }

    /// Set tags.
    pub fn with_tags(mut self, tags: Tags) -> Self {
        self.tags = tags;
        self
    }

    /// Add a single tag.
    pub fn with_tag(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.tags.insert(key.into(), value.into());
        self
    }

    /// Add a label.
    pub fn with_label(mut self, label: impl Into<String>) -> Self {
        self.tags.add_label(label.into());
        self
    }
}

/// Core device trait that all protocol devices must implement.
#[async_trait]
pub trait Device: Send + Sync {
    /// Get device information.
    fn info(&self) -> &DeviceInfo;

    /// Get device ID.
    fn id(&self) -> &str {
        &self.info().id
    }

    /// Get device name.
    fn name(&self) -> &str {
        &self.info().name
    }

    /// Get protocol.
    fn protocol(&self) -> Protocol {
        self.info().protocol
    }

    /// Get device state.
    fn state(&self) -> DeviceState {
        self.info().state
    }

    /// Initialize the device.
    async fn initialize(&mut self) -> Result<()>;

    /// Start the device.
    async fn start(&mut self) -> Result<()>;

    /// Stop the device.
    async fn stop(&mut self) -> Result<()>;

    /// Process one tick (for simulation updates).
    async fn tick(&mut self) -> Result<()>;

    /// Get all data point definitions.
    fn point_definitions(&self) -> Vec<&DataPointDef>;

    /// Get a data point definition by ID.
    fn point_definition(&self, point_id: &str) -> Option<&DataPointDef>;

    /// Read a data point value.
    async fn read(&self, point_id: &str) -> Result<DataPoint>;

    /// Read multiple data point values.
    async fn read_multiple(&self, point_ids: &[&str]) -> Result<Vec<DataPoint>> {
        let mut results = Vec::with_capacity(point_ids.len());
        for point_id in point_ids {
            results.push(self.read(point_id).await?);
        }
        Ok(results)
    }

    /// Read all data points.
    async fn read_all(&self) -> Result<Vec<DataPoint>> {
        let point_ids: Vec<&str> = self
            .point_definitions()
            .iter()
            .map(|d| d.id.as_str())
            .collect();
        self.read_multiple(&point_ids).await
    }

    /// Write a data point value.
    async fn write(&mut self, point_id: &str, value: Value) -> Result<()>;

    /// Write multiple data point values.
    async fn write_multiple(&mut self, values: &[(&str, Value)]) -> Result<()> {
        for (point_id, value) in values {
            self.write(point_id, value.clone()).await?;
        }
        Ok(())
    }

    /// Subscribe to value changes (returns a receiver).
    fn subscribe(&self) -> Option<tokio::sync::broadcast::Receiver<DataPoint>> {
        None
    }

    /// Get device statistics.
    fn statistics(&self) -> DeviceStatistics {
        DeviceStatistics::default()
    }
}

/// Device statistics.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DeviceStatistics {
    /// Total read operations.
    pub reads_total: u64,
    /// Total write operations.
    pub writes_total: u64,
    /// Read errors.
    pub read_errors: u64,
    /// Write errors.
    pub write_errors: u64,
    /// Total ticks processed.
    pub ticks_total: u64,
    /// Average tick duration in microseconds.
    pub avg_tick_duration_us: u64,
    /// Last error message.
    pub last_error: Option<String>,
    /// Uptime in seconds.
    pub uptime_secs: u64,
}

impl DeviceStatistics {
    /// Record a successful read.
    pub fn record_read(&mut self) {
        self.reads_total += 1;
    }

    /// Record a read error.
    pub fn record_read_error(&mut self, error: &str) {
        self.read_errors += 1;
        self.last_error = Some(error.to_string());
    }

    /// Record a successful write.
    pub fn record_write(&mut self) {
        self.writes_total += 1;
    }

    /// Record a write error.
    pub fn record_write_error(&mut self, error: &str) {
        self.write_errors += 1;
        self.last_error = Some(error.to_string());
    }

    /// Record a tick.
    pub fn record_tick(&mut self, duration_us: u64) {
        self.ticks_total += 1;
        // Moving average
        self.avg_tick_duration_us =
            (self.avg_tick_duration_us * (self.ticks_total - 1) + duration_us) / self.ticks_total;
    }
}

/// Type alias for a boxed device.
pub type BoxedDevice = Box<dyn Device>;

/// Type alias for an Arc'd device.
pub type ArcDevice = Arc<dyn Device>;

/// Device handle for thread-safe access.
/// Uses tokio::sync::RwLock for async-friendly locking.
#[derive(Clone)]
pub struct DeviceHandle {
    inner: Arc<tokio::sync::RwLock<BoxedDevice>>,
    /// Cached device info for synchronous access.
    cached_info: Arc<parking_lot::RwLock<DeviceInfo>>,
}

impl DeviceHandle {
    /// Create a new device handle.
    pub fn new(device: BoxedDevice) -> Self {
        let info = device.info().clone();
        Self {
            inner: Arc::new(tokio::sync::RwLock::new(device)),
            cached_info: Arc::new(parking_lot::RwLock::new(info)),
        }
    }

    /// Get device info (synchronous, returns cached info).
    pub fn info(&self) -> DeviceInfo {
        self.cached_info.read().clone()
    }

    /// Get device ID (synchronous).
    pub fn id(&self) -> String {
        self.cached_info.read().id.clone()
    }

    /// Get device state (synchronous, returns cached state).
    pub fn state(&self) -> DeviceState {
        self.cached_info.read().state
    }

    /// Refresh cached info from the device.
    pub async fn refresh_info(&self) {
        let info = self.inner.read().await.info().clone();
        *self.cached_info.write() = info;
    }

    /// Initialize the device.
    pub async fn initialize(&self) -> Result<()> {
        let result = self.inner.write().await.initialize().await;
        self.refresh_info().await;
        result
    }

    /// Start the device.
    pub async fn start(&self) -> Result<()> {
        let result = self.inner.write().await.start().await;
        self.refresh_info().await;
        result
    }

    /// Stop the device.
    pub async fn stop(&self) -> Result<()> {
        let result = self.inner.write().await.stop().await;
        self.refresh_info().await;
        result
    }

    /// Process one tick.
    pub async fn tick(&self) -> Result<()> {
        self.inner.write().await.tick().await
    }

    /// Read a data point.
    pub async fn read(&self, point_id: &str) -> Result<DataPoint> {
        self.inner.read().await.read(point_id).await
    }

    /// Write a data point.
    pub async fn write(&self, point_id: &str, value: Value) -> Result<()> {
        self.inner.write().await.write(point_id, value).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_device_state() {
        assert!(!DeviceState::Uninitialized.is_operational());
        assert!(DeviceState::Online.is_operational());
        assert!(DeviceState::Online.can_accept_requests());
    }

    #[test]
    fn test_device_info() {
        let info = DeviceInfo::new("dev-001", "Test Device", Protocol::ModbusTcp)
            .with_description("A test device")
            .with_metadata("location", "Building A");

        assert_eq!(info.id, "dev-001");
        assert_eq!(info.protocol, Protocol::ModbusTcp);
        assert_eq!(
            info.metadata.get("location"),
            Some(&"Building A".to_string())
        );
    }

    #[test]
    fn test_device_info_with_tags() {
        let info = DeviceInfo::new("dev-002", "Tagged Device", Protocol::BacnetIp)
            .with_tag("zone", "hvac")
            .with_tag("floor", "3")
            .with_label("critical")
            .with_label("monitored");

        assert_eq!(info.tags.get("zone"), Some("hvac"));
        assert_eq!(info.tags.get("floor"), Some("3"));
        assert!(info.tags.has_label("critical"));
        assert!(info.tags.has_label("monitored"));
    }

    #[test]
    fn test_device_statistics() {
        let mut stats = DeviceStatistics::default();
        stats.record_read();
        stats.record_tick(100);
        stats.record_tick(200);

        assert_eq!(stats.reads_total, 1);
        assert_eq!(stats.ticks_total, 2);
        assert_eq!(stats.avg_tick_duration_us, 150);
    }
}

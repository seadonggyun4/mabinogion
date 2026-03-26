//! TrendLog object implementation per ASHRAE 135, Clause 12.25.
//!
//! A TrendLog object monitors the value of a specified property of a
//! referenced object and records samples in a circular log buffer.
//!
//! ## Key Features
//!
//! - **Circular buffer**: Ring-buffer with configurable capacity
//! - **Interval logging**: Samples at a configurable interval (0 = COV-based)
//! - **Time-stamped entries**: Each record includes a BACnet DateTime stamp
//! - **Enable/disable**: Logging can be started/stopped dynamically
//! - **Stop-when-full**: Optionally halts logging when the buffer fills
//! - **Sequence numbers**: Monotonically increasing for ReadRange queries
//! - **Thread-safe**: Uses `parking_lot::RwLock` for concurrent access

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};

use super::property::{
    BACnetDate, BACnetTime, BACnetValue, EventState, PropertyError, PropertyId, PropertyStore,
    Reliability, StatusFlags,
};
use super::traits::BACnetObject;
use super::types::{ObjectId, ObjectType};

// ============================================================================
// Log Record
// ============================================================================

/// Status of a log record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LogStatus {
    /// Value was logged successfully.
    LogDisabled = 0,
    /// Buffer purged.
    BufferPurged = 1,
    /// Normal log entry.
    Normal = 2,
}

/// A single entry in the trend log buffer.
///
/// Per ASHRAE 135, Clause 12.25.28, each LogRecord contains:
/// - Timestamp (date + time)
/// - Log datum (the value or a status)
/// - Status flags at the time of sampling
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogRecord {
    /// Timestamp of the record.
    pub timestamp: LogTimestamp,
    /// The logged datum.
    pub datum: LogDatum,
    /// Status flags at the time of logging.
    pub status_flags: u8,
    /// Sequence number (monotonically increasing, wraps at u64::MAX).
    pub sequence_number: u64,
}

/// Timestamp for a log record.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct LogTimestamp {
    pub date: BACnetDate,
    pub time: BACnetTime,
}

impl Default for LogTimestamp {
    fn default() -> Self {
        Self {
            date: BACnetDate::default(),
            time: BACnetTime::default(),
        }
    }
}

impl LogTimestamp {
    /// Create a timestamp from date and time.
    pub fn new(date: BACnetDate, time: BACnetTime) -> Self {
        Self { date, time }
    }

    /// Create a "now" timestamp from the system clock.
    pub fn now() -> Self {
        let now = chrono_free_now();
        Self {
            date: now.0,
            time: now.1,
        }
    }

    /// Convert to a comparable u64 for ordering (YYMMDDHHMMSScc).
    /// This is an approximation for ordering purposes, not a real epoch.
    pub fn to_sortable(&self) -> u64 {
        let d = &self.date;
        let t = &self.time;
        // Pack: year(8) month(4) day(5) hour(5) minute(6) second(6) hundredths(7) = 41 bits
        ((d.year as u64) << 33)
            | ((d.month as u64) << 29)
            | ((d.day as u64) << 24)
            | ((t.hour as u64) << 19)
            | ((t.minute as u64) << 13)
            | ((t.second as u64) << 7)
            | (t.hundredths as u64)
    }
}

/// Get current date/time without chrono dependency.
/// Uses std::time::SystemTime to derive year/month/day/hour/minute/second.
fn chrono_free_now() -> (BACnetDate, BACnetTime) {
    use std::time::{SystemTime, UNIX_EPOCH};

    let duration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let secs = duration.as_secs();

    // Simple civil time calculation (no timezone, UTC only)
    let days = secs / 86400;
    let time_of_day = secs % 86400;
    let hour = (time_of_day / 3600) as u8;
    let minute = ((time_of_day % 3600) / 60) as u8;
    let second = (time_of_day % 60) as u8;
    let hundredths = (duration.subsec_millis() / 10) as u8;

    // Days since 1970-01-01 to (year, month, day) — civil calendar algorithm
    let (year, month, day) = days_to_civil(days as i64);

    let bacnet_year = if (1900..=2155).contains(&year) {
        (year - 1900) as u8
    } else {
        255
    };

    let date = BACnetDate {
        year: bacnet_year,
        month: month as u8,
        day: day as u8,
        day_of_week: 255, // unspecified
    };
    let time = BACnetTime {
        hour,
        minute,
        second,
        hundredths,
    };
    (date, time)
}

/// Convert days since 1970-01-01 to (year, month, day).
/// Algorithm from Howard Hinnant's `chrono`-compatible date library.
fn days_to_civil(days: i64) -> (i32, i32, i32) {
    let z = days + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u32;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i32 + era as i32 * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m as i32, d as i32)
}

/// The logged value or status indicator.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum LogDatum {
    /// A real (float) value.
    RealValue(f32),
    /// An enumerated value.
    EnumeratedValue(u32),
    /// An unsigned integer value.
    UnsignedValue(u32),
    /// A signed integer value.
    SignedValue(i32),
    /// A boolean value.
    BooleanValue(bool),
    /// A bitstring value.
    BitstringValue(Vec<bool>),
    /// A null value (e.g., property couldn't be read).
    NullValue,
    /// A failure status indicator.
    Failure { error_class: u16, error_code: u16 },
    /// Time change indicator (internal clock adjustment).
    TimeChange(f32),
}

impl LogDatum {
    /// Create from a BACnetValue.
    pub fn from_bacnet_value(value: &BACnetValue) -> Self {
        match value {
            BACnetValue::Real(v) => Self::RealValue(*v),
            BACnetValue::Double(v) => Self::RealValue(*v as f32),
            BACnetValue::Unsigned(v) => Self::UnsignedValue(*v),
            BACnetValue::Unsigned64(v) => Self::UnsignedValue(*v as u32),
            BACnetValue::Signed(v) => Self::SignedValue(*v),
            BACnetValue::Signed64(v) => Self::SignedValue(*v as i32),
            BACnetValue::Enumerated(v) => Self::EnumeratedValue(*v),
            BACnetValue::Boolean(v) => Self::BooleanValue(*v),
            BACnetValue::BitString(v) => Self::BitstringValue(v.clone()),
            BACnetValue::Null => Self::NullValue,
            _ => Self::NullValue,
        }
    }

    /// Convert to a BACnetValue for property reads.
    pub fn to_bacnet_value(&self) -> BACnetValue {
        match self {
            Self::RealValue(v) => BACnetValue::Real(*v),
            Self::EnumeratedValue(v) => BACnetValue::Enumerated(*v),
            Self::UnsignedValue(v) => BACnetValue::Unsigned(*v),
            Self::SignedValue(v) => BACnetValue::Signed(*v),
            Self::BooleanValue(v) => BACnetValue::Boolean(*v),
            Self::BitstringValue(v) => BACnetValue::BitString(v.clone()),
            Self::NullValue => BACnetValue::Null,
            Self::Failure { .. } => BACnetValue::Null,
            Self::TimeChange(v) => BACnetValue::Real(*v),
        }
    }
}

// ============================================================================
// Device-Object-Property Reference
// ============================================================================

/// Reference to a device/object/property being logged.
///
/// Per ASHRAE 135, Clause 12.25.6 (Log_Device_Object_Property).
#[derive(Debug, Clone)]
pub struct DeviceObjectPropertyReference {
    /// Object being monitored.
    pub object_id: ObjectId,
    /// Property being logged.
    pub property_id: PropertyId,
    /// Optional array index.
    pub array_index: Option<u32>,
    /// Optional device identifier (if remote; None = local).
    pub device_id: Option<ObjectId>,
}

// ============================================================================
// TrendLog Object
// ============================================================================

/// TrendLog object — ASHRAE 135, Clause 12.25.
///
/// Maintains a circular buffer of timestamped log records sampled from a
/// referenced object property at a configurable interval.
pub struct TrendLog {
    id: ObjectId,
    name: String,
    description: String,
    properties: PropertyStore,

    /// Enable/disable logging.
    enabled: AtomicBool,
    /// Stop logging when the buffer is full (vs. circular overwrite).
    stop_when_full: AtomicBool,

    /// The object/property being monitored.
    log_device_object_property: RwLock<Option<DeviceObjectPropertyReference>>,
    /// Logging interval in seconds (0 = COV-triggered).
    log_interval: RwLock<u32>,

    /// Circular log buffer (ring buffer).
    buffer: RwLock<VecDeque<LogRecord>>,
    /// Maximum buffer capacity.
    buffer_size: usize,

    /// Total records ever logged (monotonic counter for sequence numbers).
    total_record_count: AtomicU64,
    /// Notification threshold (0 = disabled).
    notification_threshold: RwLock<u32>,
    /// Records logged since last notification.
    records_since_notification: AtomicU64,
}

impl TrendLog {
    /// Create a new TrendLog object.
    pub fn new(instance: u32, name: impl Into<String>, buffer_size: usize) -> Self {
        let id = ObjectId::new(ObjectType::TrendLog, instance);
        let name = name.into();
        let properties = PropertyStore::new();

        properties.set(
            PropertyId::EventState,
            BACnetValue::Enumerated(EventState::Normal as u32),
        );
        properties.set(
            PropertyId::Reliability,
            BACnetValue::Enumerated(Reliability::NoFaultDetected as u32),
        );

        Self {
            id,
            name,
            description: String::new(),
            properties,
            enabled: AtomicBool::new(false),
            stop_when_full: AtomicBool::new(false),
            log_device_object_property: RwLock::new(None),
            log_interval: RwLock::new(0),
            buffer: RwLock::new(VecDeque::with_capacity(buffer_size)),
            buffer_size,
            total_record_count: AtomicU64::new(0),
            notification_threshold: RwLock::new(0),
            records_since_notification: AtomicU64::new(0),
        }
    }

    /// Builder: set description.
    pub fn with_description(mut self, desc: impl Into<String>) -> Self {
        self.description = desc.into();
        self
    }

    /// Builder: set logging interval in seconds.
    pub fn with_log_interval(self, seconds: u32) -> Self {
        *self.log_interval.write() = seconds;
        self
    }

    /// Builder: set the monitored object/property reference.
    pub fn with_log_device_object_property(self, reference: DeviceObjectPropertyReference) -> Self {
        *self.log_device_object_property.write() = Some(reference);
        self
    }

    /// Builder: set enabled state.
    pub fn with_enabled(self, enabled: bool) -> Self {
        self.enabled.store(enabled, Ordering::Release);
        self
    }

    /// Builder: set stop-when-full behavior.
    pub fn with_stop_when_full(self, stop: bool) -> Self {
        self.stop_when_full.store(stop, Ordering::Release);
        self
    }

    /// Builder: set notification threshold.
    pub fn with_notification_threshold(self, threshold: u32) -> Self {
        *self.notification_threshold.write() = threshold;
        self
    }

    // ── Public API ──

    /// Check if logging is enabled.
    pub fn is_enabled(&self) -> bool {
        self.enabled.load(Ordering::Acquire)
    }

    /// Enable or disable logging.
    pub fn set_enabled(&self, enabled: bool) {
        self.enabled.store(enabled, Ordering::Release);
    }

    /// Get the current buffer size (number of records).
    pub fn record_count(&self) -> usize {
        self.buffer.read().len()
    }

    /// Get total records ever logged.
    pub fn total_record_count(&self) -> u64 {
        self.total_record_count.load(Ordering::Relaxed)
    }

    /// Get the buffer capacity.
    pub fn buffer_size(&self) -> usize {
        self.buffer_size
    }

    /// Get the logging interval.
    pub fn log_interval(&self) -> u32 {
        *self.log_interval.read()
    }

    /// Add a log record.
    ///
    /// If the buffer is full and `stop_when_full` is set, this is a no-op.
    /// Otherwise, the oldest record is evicted (circular buffer).
    pub fn add_record(&self, datum: LogDatum) {
        self.add_record_with_timestamp(datum, LogTimestamp::now());
    }

    /// Add a log record with a specific timestamp (useful for testing).
    pub fn add_record_with_timestamp(&self, datum: LogDatum, timestamp: LogTimestamp) {
        if !self.is_enabled() {
            return;
        }

        let mut buffer = self.buffer.write();

        if buffer.len() >= self.buffer_size {
            if self.stop_when_full.load(Ordering::Acquire) {
                return; // Buffer full and stop_when_full = true
            }
            buffer.pop_front(); // Evict oldest
        }

        let seq = self.total_record_count.fetch_add(1, Ordering::Relaxed);
        buffer.push_back(LogRecord {
            timestamp,
            datum,
            status_flags: 0,
            sequence_number: seq,
        });

        self.records_since_notification
            .fetch_add(1, Ordering::Relaxed);
    }

    /// Add a record with explicit status flags.
    pub fn add_record_with_status(
        &self,
        datum: LogDatum,
        timestamp: LogTimestamp,
        status_flags: u8,
    ) {
        if !self.is_enabled() {
            return;
        }

        let mut buffer = self.buffer.write();

        if buffer.len() >= self.buffer_size {
            if self.stop_when_full.load(Ordering::Acquire) {
                return;
            }
            buffer.pop_front();
        }

        let seq = self.total_record_count.fetch_add(1, Ordering::Relaxed);
        buffer.push_back(LogRecord {
            timestamp,
            datum,
            status_flags,
            sequence_number: seq,
        });

        self.records_since_notification
            .fetch_add(1, Ordering::Relaxed);
    }

    /// Check whether the notification threshold has been reached.
    pub fn notification_threshold_reached(&self) -> bool {
        let threshold = *self.notification_threshold.read();
        if threshold == 0 {
            return false;
        }
        self.records_since_notification.load(Ordering::Relaxed) >= threshold as u64
    }

    /// Reset the records-since-notification counter.
    pub fn reset_notification_counter(&self) {
        self.records_since_notification.store(0, Ordering::Relaxed);
    }

    /// Purge the log buffer.
    pub fn purge(&self) {
        let mut buffer = self.buffer.write();
        buffer.clear();
        self.records_since_notification.store(0, Ordering::Relaxed);
    }

    // ── ReadRange support ──

    /// Read a range of records by position (1-based index).
    ///
    /// Returns records from `start_index` (1-based), up to `count` records.
    /// Negative count reads backwards from start_index.
    pub fn read_range_by_position(
        &self,
        start_index: i32,
        count: i32,
    ) -> (Vec<LogRecord>, u32, bool) {
        let buffer = self.buffer.read();
        let buf_len = buffer.len();

        if buf_len == 0 || count == 0 {
            return (Vec::new(), 0, false);
        }

        let (start, take, reverse) = if count > 0 {
            // Forward: start_index is 1-based
            let s = ((start_index.max(1) - 1) as usize).min(buf_len);
            let t = (count as usize).min(buf_len - s);
            (s, t, false)
        } else {
            // Backward: read |count| records ending at start_index
            let end = (start_index.max(1) as usize).min(buf_len);
            let t = ((-count) as usize).min(end);
            (end - t, t, true)
        };

        let records: Vec<LogRecord> = buffer.iter().skip(start).take(take).cloned().collect();
        let more = (start + take) < buf_len;

        if reverse {
            let mut reversed = records;
            reversed.reverse();
            let len = reversed.len() as u32;
            (reversed, len, more)
        } else {
            let len = records.len() as u32;
            (records, len, more)
        }
    }

    /// Read a range of records by sequence number.
    ///
    /// Returns records with sequence_number >= `start_seq`, up to `count`.
    /// Negative count reads backwards from start_seq.
    pub fn read_range_by_sequence(
        &self,
        start_seq: u64,
        count: i32,
    ) -> (Vec<LogRecord>, u32, bool) {
        let buffer = self.buffer.read();

        if buffer.is_empty() || count == 0 {
            return (Vec::new(), 0, false);
        }

        if count > 0 {
            let records: Vec<LogRecord> = buffer
                .iter()
                .filter(|r| r.sequence_number >= start_seq)
                .take(count as usize)
                .cloned()
                .collect();
            let total_matching = buffer
                .iter()
                .filter(|r| r.sequence_number >= start_seq)
                .count();
            let more = total_matching > records.len();
            let len = records.len() as u32;
            (records, len, more)
        } else {
            // Backwards: records with seq <= start_seq, last |count| of them
            let matching: Vec<LogRecord> = buffer
                .iter()
                .filter(|r| r.sequence_number <= start_seq)
                .cloned()
                .collect();
            let take = ((-count) as usize).min(matching.len());
            let start = matching.len() - take;
            let records: Vec<LogRecord> = matching[start..].to_vec();
            let more = start > 0;
            let len = records.len() as u32;
            (records, len, more)
        }
    }

    /// Read a range of records by timestamp.
    ///
    /// Returns records with timestamp >= `start_time`, up to `count`.
    /// Negative count reads backwards from start_time.
    pub fn read_range_by_time(
        &self,
        start_time: &LogTimestamp,
        count: i32,
    ) -> (Vec<LogRecord>, u32, bool) {
        let buffer = self.buffer.read();
        let start_sortable = start_time.to_sortable();

        if buffer.is_empty() || count == 0 {
            return (Vec::new(), 0, false);
        }

        if count > 0 {
            let records: Vec<LogRecord> = buffer
                .iter()
                .filter(|r| r.timestamp.to_sortable() >= start_sortable)
                .take(count as usize)
                .cloned()
                .collect();
            let total_matching = buffer
                .iter()
                .filter(|r| r.timestamp.to_sortable() >= start_sortable)
                .count();
            let more = total_matching > records.len();
            let len = records.len() as u32;
            (records, len, more)
        } else {
            let matching: Vec<LogRecord> = buffer
                .iter()
                .filter(|r| r.timestamp.to_sortable() <= start_sortable)
                .cloned()
                .collect();
            let take = ((-count) as usize).min(matching.len());
            let start = matching.len() - take;
            let records: Vec<LogRecord> = matching[start..].to_vec();
            let more = start > 0;
            let len = records.len() as u32;
            (records, len, more)
        }
    }

    /// Read all records (for LogBuffer property read).
    pub fn read_all_records(&self) -> Vec<LogRecord> {
        self.buffer.read().iter().cloned().collect()
    }

    /// Get the first and last sequence numbers in the buffer.
    pub fn sequence_range(&self) -> Option<(u64, u64)> {
        let buffer = self.buffer.read();
        if buffer.is_empty() {
            return None;
        }
        let first = buffer.front().unwrap().sequence_number;
        let last = buffer.back().unwrap().sequence_number;
        Some((first, last))
    }
}

// ============================================================================
// BACnetObject Implementation
// ============================================================================

impl BACnetObject for TrendLog {
    fn object_identifier(&self) -> ObjectId {
        self.id
    }

    fn object_name(&self) -> &str {
        &self.name
    }

    fn description(&self) -> Option<&str> {
        if self.description.is_empty() {
            None
        } else {
            Some(&self.description)
        }
    }

    fn read_property(&self, property_id: PropertyId) -> Result<BACnetValue, PropertyError> {
        match property_id {
            PropertyId::ObjectIdentifier => Ok(BACnetValue::ObjectIdentifier(self.id)),
            PropertyId::ObjectName => Ok(BACnetValue::CharacterString(self.name.clone())),
            PropertyId::ObjectType => Ok(BACnetValue::Enumerated(ObjectType::TrendLog as u32)),
            PropertyId::Description => Ok(BACnetValue::CharacterString(self.description.clone())),
            PropertyId::Enable => Ok(BACnetValue::Boolean(self.is_enabled())),
            PropertyId::StopWhenFull => Ok(BACnetValue::Boolean(
                self.stop_when_full.load(Ordering::Acquire),
            )),
            PropertyId::BufferSize => Ok(BACnetValue::Unsigned(self.buffer_size as u32)),
            PropertyId::RecordCount => Ok(BACnetValue::Unsigned(self.record_count() as u32)),
            PropertyId::TotalRecordCount => {
                Ok(BACnetValue::Unsigned(self.total_record_count() as u32))
            }
            PropertyId::LogInterval => Ok(BACnetValue::Unsigned(*self.log_interval.read())),
            PropertyId::StatusFlags => Ok(BACnetValue::BitString(self.status_flags().to_bits())),
            PropertyId::NotificationThreshold => {
                Ok(BACnetValue::Unsigned(*self.notification_threshold.read()))
            }
            PropertyId::RecordsSinceNotification => Ok(BACnetValue::Unsigned(
                self.records_since_notification.load(Ordering::Relaxed) as u32,
            )),
            PropertyId::LogBuffer => {
                // Return as an Array of structured values.
                // Each element encodes: timestamp, datum, status_flags.
                let records = self.read_all_records();
                let values: Vec<BACnetValue> = records
                    .iter()
                    .map(|r| {
                        // Encode each record as a List of [timestamp_date, timestamp_time, value, status_flags]
                        BACnetValue::List(vec![
                            BACnetValue::Date(r.timestamp.date),
                            BACnetValue::Time(r.timestamp.time),
                            r.datum.to_bacnet_value(),
                            BACnetValue::Unsigned(r.status_flags as u32),
                        ])
                    })
                    .collect();
                Ok(BACnetValue::Array(values))
            }
            PropertyId::LogDeviceObjectProperty => {
                let reference = self.log_device_object_property.read();
                match reference.as_ref() {
                    Some(r) => Ok(BACnetValue::ObjectIdentifier(r.object_id)),
                    None => Ok(BACnetValue::Null),
                }
            }
            _ => self
                .properties
                .get(property_id)
                .ok_or(PropertyError::NotFound(property_id)),
        }
    }

    fn write_property(
        &self,
        property_id: PropertyId,
        value: BACnetValue,
    ) -> Result<(), PropertyError> {
        match property_id {
            PropertyId::ObjectIdentifier | PropertyId::ObjectType | PropertyId::ObjectName => {
                Err(PropertyError::ReadOnly(property_id))
            }
            PropertyId::Enable => {
                let v = value
                    .as_bool()
                    .ok_or(PropertyError::InvalidDataType(property_id))?;
                self.enabled.store(v, Ordering::Release);
                Ok(())
            }
            PropertyId::StopWhenFull => {
                let v = value
                    .as_bool()
                    .ok_or(PropertyError::InvalidDataType(property_id))?;
                self.stop_when_full.store(v, Ordering::Release);
                Ok(())
            }
            PropertyId::LogInterval => {
                let v = value
                    .as_unsigned()
                    .ok_or(PropertyError::InvalidDataType(property_id))?;
                *self.log_interval.write() = v;
                Ok(())
            }
            PropertyId::NotificationThreshold => {
                let v = value
                    .as_unsigned()
                    .ok_or(PropertyError::InvalidDataType(property_id))?;
                *self.notification_threshold.write() = v;
                Ok(())
            }
            PropertyId::RecordCount => {
                // Writing 0 to RecordCount purges the buffer (ASHRAE 135, Clause 12.25.37)
                let v = value
                    .as_unsigned()
                    .ok_or(PropertyError::InvalidDataType(property_id))?;
                if v == 0 {
                    self.purge();
                    Ok(())
                } else {
                    Err(PropertyError::ValueOutOfRange(property_id))
                }
            }
            PropertyId::Description => {
                // Description is writable on most objects
                self.properties.set(property_id, value);
                Ok(())
            }
            _ => Err(PropertyError::WriteAccessDenied(property_id)),
        }
    }

    fn list_properties(&self) -> Vec<PropertyId> {
        vec![
            PropertyId::ObjectIdentifier,
            PropertyId::ObjectName,
            PropertyId::ObjectType,
            PropertyId::Description,
            PropertyId::Enable,
            PropertyId::StopWhenFull,
            PropertyId::BufferSize,
            PropertyId::LogBuffer,
            PropertyId::RecordCount,
            PropertyId::TotalRecordCount,
            PropertyId::LogInterval,
            PropertyId::LogDeviceObjectProperty,
            PropertyId::StatusFlags,
            PropertyId::EventState,
            PropertyId::Reliability,
            PropertyId::NotificationThreshold,
            PropertyId::RecordsSinceNotification,
        ]
    }

    fn status_flags(&self) -> StatusFlags {
        StatusFlags {
            in_alarm: false,
            fault: false,
            overridden: false,
            out_of_service: false,
        }
    }

    fn present_value(&self) -> Result<BACnetValue, PropertyError> {
        // TrendLog doesn't have a traditional PresentValue.
        // Return the record count as a proxy.
        Ok(BACnetValue::Unsigned(self.record_count() as u32))
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn make_timestamp(hour: u8, minute: u8, second: u8) -> LogTimestamp {
        LogTimestamp {
            date: BACnetDate {
                year: 126, // 2026
                month: 2,
                day: 14,
                day_of_week: 255,
            },
            time: BACnetTime {
                hour,
                minute,
                second,
                hundredths: 0,
            },
        }
    }

    #[test]
    fn test_trend_log_creation() {
        let tl = TrendLog::new(1, "TL_1", 100);
        assert_eq!(
            tl.object_identifier(),
            ObjectId::new(ObjectType::TrendLog, 1)
        );
        assert_eq!(tl.object_name(), "TL_1");
        assert_eq!(tl.buffer_size(), 100);
        assert_eq!(tl.record_count(), 0);
        assert!(!tl.is_enabled());
    }

    #[test]
    fn test_trend_log_builder() {
        let tl = TrendLog::new(2, "TL_2", 50)
            .with_description("Test trend log")
            .with_log_interval(60)
            .with_enabled(true)
            .with_stop_when_full(true)
            .with_notification_threshold(10);

        assert_eq!(tl.description(), Some("Test trend log"));
        assert_eq!(tl.log_interval(), 60);
        assert!(tl.is_enabled());
        assert_eq!(
            tl.read_property(PropertyId::StopWhenFull).unwrap(),
            BACnetValue::Boolean(true)
        );
        assert_eq!(
            tl.read_property(PropertyId::NotificationThreshold).unwrap(),
            BACnetValue::Unsigned(10)
        );
    }

    #[test]
    fn test_add_records() {
        let tl = TrendLog::new(1, "TL", 100).with_enabled(true);

        tl.add_record_with_timestamp(LogDatum::RealValue(72.5), make_timestamp(10, 0, 0));
        tl.add_record_with_timestamp(LogDatum::RealValue(73.0), make_timestamp(10, 1, 0));
        tl.add_record_with_timestamp(LogDatum::RealValue(73.5), make_timestamp(10, 2, 0));

        assert_eq!(tl.record_count(), 3);
        assert_eq!(tl.total_record_count(), 3);
    }

    #[test]
    fn test_circular_buffer_eviction() {
        let tl = TrendLog::new(1, "TL", 3).with_enabled(true);

        for i in 0..5 {
            tl.add_record_with_timestamp(LogDatum::RealValue(i as f32), make_timestamp(10, i, 0));
        }

        // Buffer size is 3, so only last 3 records should remain
        assert_eq!(tl.record_count(), 3);
        assert_eq!(tl.total_record_count(), 5);

        let records = tl.read_all_records();
        // Records 2, 3, 4 (0-indexed sequence numbers)
        assert_eq!(records[0].sequence_number, 2);
        assert_eq!(records[2].sequence_number, 4);
    }

    #[test]
    fn test_stop_when_full() {
        let tl = TrendLog::new(1, "TL", 3)
            .with_enabled(true)
            .with_stop_when_full(true);

        for i in 0..5 {
            tl.add_record_with_timestamp(LogDatum::RealValue(i as f32), make_timestamp(10, i, 0));
        }

        // Should only have the first 3 records
        assert_eq!(tl.record_count(), 3);
        assert_eq!(tl.total_record_count(), 3);

        let records = tl.read_all_records();
        assert_eq!(records[0].sequence_number, 0);
        assert_eq!(records[2].sequence_number, 2);
    }

    #[test]
    fn test_disabled_no_logging() {
        let tl = TrendLog::new(1, "TL", 100); // Disabled by default

        tl.add_record_with_timestamp(LogDatum::RealValue(1.0), make_timestamp(10, 0, 0));

        assert_eq!(tl.record_count(), 0);
    }

    #[test]
    fn test_purge() {
        let tl = TrendLog::new(1, "TL", 100).with_enabled(true);

        for i in 0..10 {
            tl.add_record_with_timestamp(LogDatum::RealValue(i as f32), make_timestamp(10, i, 0));
        }

        assert_eq!(tl.record_count(), 10);
        tl.purge();
        assert_eq!(tl.record_count(), 0);
    }

    #[test]
    fn test_write_record_count_zero_purges() {
        let tl = TrendLog::new(1, "TL", 100).with_enabled(true);

        for i in 0..5 {
            tl.add_record_with_timestamp(LogDatum::RealValue(i as f32), make_timestamp(10, i, 0));
        }

        assert_eq!(tl.record_count(), 5);
        tl.write_property(PropertyId::RecordCount, BACnetValue::Unsigned(0))
            .unwrap();
        assert_eq!(tl.record_count(), 0);
    }

    #[test]
    fn test_read_range_by_position_forward() {
        let tl = TrendLog::new(1, "TL", 100).with_enabled(true);

        for i in 0..10u8 {
            tl.add_record_with_timestamp(LogDatum::RealValue(i as f32), make_timestamp(10, i, 0));
        }

        // Read 3 records starting at position 4 (1-based)
        let (records, count, more) = tl.read_range_by_position(4, 3);
        assert_eq!(count, 3);
        assert!(more); // More records exist after position 6
        assert_eq!(records[0].sequence_number, 3);
        assert_eq!(records[2].sequence_number, 5);
    }

    #[test]
    fn test_read_range_by_position_backward() {
        let tl = TrendLog::new(1, "TL", 100).with_enabled(true);

        for i in 0..10u8 {
            tl.add_record_with_timestamp(LogDatum::RealValue(i as f32), make_timestamp(10, i, 0));
        }

        // Read 3 records backwards ending at position 5 (1-based)
        let (records, count, more) = tl.read_range_by_position(5, -3);
        assert_eq!(count, 3);
        assert!(more); // More records before
                       // Reversed: seq 4, 3, 2
        assert_eq!(records[0].sequence_number, 4);
        assert_eq!(records[2].sequence_number, 2);
    }

    #[test]
    fn test_read_range_by_sequence() {
        let tl = TrendLog::new(1, "TL", 100).with_enabled(true);

        for i in 0..10u8 {
            tl.add_record_with_timestamp(LogDatum::RealValue(i as f32), make_timestamp(10, i, 0));
        }

        // Read 3 records starting at sequence 5
        let (records, count, more) = tl.read_range_by_sequence(5, 3);
        assert_eq!(count, 3);
        assert!(more);
        assert_eq!(records[0].sequence_number, 5);
        assert_eq!(records[2].sequence_number, 7);
    }

    #[test]
    fn test_read_range_by_time() {
        let tl = TrendLog::new(1, "TL", 100).with_enabled(true);

        for i in 0..10u8 {
            tl.add_record_with_timestamp(LogDatum::RealValue(i as f32), make_timestamp(10, i, 0));
        }

        // Read 3 records starting at 10:05:00
        let start = make_timestamp(10, 5, 0);
        let (records, count, more) = tl.read_range_by_time(&start, 3);
        assert_eq!(count, 3);
        assert!(more);
        assert_eq!(records[0].sequence_number, 5);
        assert_eq!(records[2].sequence_number, 7);
    }

    #[test]
    fn test_notification_threshold() {
        let tl = TrendLog::new(1, "TL", 100)
            .with_enabled(true)
            .with_notification_threshold(5);

        for i in 0..4 {
            tl.add_record_with_timestamp(LogDatum::RealValue(i as f32), make_timestamp(10, i, 0));
        }
        assert!(!tl.notification_threshold_reached());

        tl.add_record_with_timestamp(LogDatum::RealValue(4.0), make_timestamp(10, 4, 0));
        assert!(tl.notification_threshold_reached());

        tl.reset_notification_counter();
        assert!(!tl.notification_threshold_reached());
    }

    #[test]
    fn test_sequence_range() {
        let tl = TrendLog::new(1, "TL", 5).with_enabled(true);

        assert!(tl.sequence_range().is_none());

        for i in 0..7 {
            tl.add_record_with_timestamp(LogDatum::RealValue(i as f32), make_timestamp(10, i, 0));
        }

        // Buffer holds 5 records (seq 2..6)
        let (first, last) = tl.sequence_range().unwrap();
        assert_eq!(first, 2);
        assert_eq!(last, 6);
    }

    #[test]
    fn test_log_datum_from_bacnet_value() {
        let datum = LogDatum::from_bacnet_value(&BACnetValue::Real(42.0));
        assert!(matches!(datum, LogDatum::RealValue(v) if (v - 42.0).abs() < 0.001));

        let datum = LogDatum::from_bacnet_value(&BACnetValue::Boolean(true));
        assert!(matches!(datum, LogDatum::BooleanValue(true)));

        let datum = LogDatum::from_bacnet_value(&BACnetValue::Unsigned(100));
        assert!(matches!(datum, LogDatum::UnsignedValue(100)));
    }

    #[test]
    fn test_property_reads() {
        let tl = TrendLog::new(1, "TL_Test", 200)
            .with_enabled(true)
            .with_log_interval(60);

        assert_eq!(
            tl.read_property(PropertyId::ObjectName).unwrap(),
            BACnetValue::CharacterString("TL_Test".into()),
        );
        assert_eq!(
            tl.read_property(PropertyId::ObjectType).unwrap(),
            BACnetValue::Enumerated(ObjectType::TrendLog as u32),
        );
        assert_eq!(
            tl.read_property(PropertyId::Enable).unwrap(),
            BACnetValue::Boolean(true),
        );
        assert_eq!(
            tl.read_property(PropertyId::BufferSize).unwrap(),
            BACnetValue::Unsigned(200),
        );
        assert_eq!(
            tl.read_property(PropertyId::LogInterval).unwrap(),
            BACnetValue::Unsigned(60),
        );
    }

    #[test]
    fn test_property_writes() {
        let tl = TrendLog::new(1, "TL", 100);

        tl.write_property(PropertyId::Enable, BACnetValue::Boolean(true))
            .unwrap();
        assert!(tl.is_enabled());

        tl.write_property(PropertyId::LogInterval, BACnetValue::Unsigned(30))
            .unwrap();
        assert_eq!(tl.log_interval(), 30);

        // Read-only properties should fail
        assert!(tl
            .write_property(PropertyId::ObjectIdentifier, BACnetValue::Unsigned(0))
            .is_err());
        assert!(tl
            .write_property(PropertyId::BufferSize, BACnetValue::Unsigned(0))
            .is_err());
    }

    #[test]
    fn test_list_properties() {
        let tl = TrendLog::new(1, "TL", 100);
        let props = tl.list_properties();

        assert!(props.contains(&PropertyId::ObjectIdentifier));
        assert!(props.contains(&PropertyId::LogBuffer));
        assert!(props.contains(&PropertyId::Enable));
        assert!(props.contains(&PropertyId::StopWhenFull));
        assert!(props.contains(&PropertyId::BufferSize));
        assert!(props.contains(&PropertyId::RecordCount));
        assert!(props.contains(&PropertyId::TotalRecordCount));
    }

    #[test]
    fn test_timestamp_sortable() {
        let t1 = make_timestamp(10, 0, 0);
        let t2 = make_timestamp(10, 30, 0);
        let t3 = make_timestamp(11, 0, 0);

        assert!(t1.to_sortable() < t2.to_sortable());
        assert!(t2.to_sortable() < t3.to_sortable());
    }
}

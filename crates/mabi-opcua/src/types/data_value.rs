//! OPC UA DataValue implementation.
//!
//! DataValue wraps a Variant with additional metadata including status,
//! source timestamp, and server timestamp.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fmt;

use crate::types::{StatusCode, Variant};

/// OPC UA Data Value.
///
/// A DataValue contains a value along with associated metadata:
/// - Status code indicating the quality of the value
/// - Source timestamp (when the value was obtained from the source)
/// - Server timestamp (when the server received/processed the value)
///
/// # Examples
///
/// ```
/// use mabi_opcua::types::{DataValue, Variant, StatusCode};
///
/// // Create a data value with just a value
/// let dv = DataValue::new(Variant::double(3.14));
/// assert!(dv.status().is_good());
///
/// // Create a data value with status
/// let dv = DataValue::with_status(Variant::int32(42), StatusCode::UNCERTAIN_INITIAL_VALUE);
/// assert!(dv.status().is_uncertain());
/// ```
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DataValue {
    /// The value.
    value: Option<Variant>,
    /// Status code indicating the quality of the value.
    status: StatusCode,
    /// Time when the value was obtained from the source.
    source_timestamp: Option<DateTime<Utc>>,
    /// Picoseconds portion of the source timestamp.
    source_picoseconds: u16,
    /// Time when the server received/processed the value.
    server_timestamp: Option<DateTime<Utc>>,
    /// Picoseconds portion of the server timestamp.
    server_picoseconds: u16,
}

impl DataValue {
    /// Create a new DataValue with a value.
    pub fn new(value: Variant) -> Self {
        let now = Utc::now();
        Self {
            value: Some(value),
            status: StatusCode::GOOD,
            source_timestamp: Some(now),
            source_picoseconds: 0,
            server_timestamp: Some(now),
            server_picoseconds: 0,
        }
    }

    /// Create a DataValue with a specific status.
    pub fn with_status(value: Variant, status: StatusCode) -> Self {
        let now = Utc::now();
        Self {
            value: Some(value),
            status,
            source_timestamp: Some(now),
            source_picoseconds: 0,
            server_timestamp: Some(now),
            server_picoseconds: 0,
        }
    }

    /// Create a null/empty DataValue.
    pub fn null() -> Self {
        Self {
            value: None,
            status: StatusCode::GOOD,
            source_timestamp: None,
            source_picoseconds: 0,
            server_timestamp: None,
            server_picoseconds: 0,
        }
    }

    /// Create a DataValue indicating a bad status.
    pub fn bad(status: StatusCode) -> Self {
        Self {
            value: None,
            status,
            source_timestamp: None,
            source_picoseconds: 0,
            server_timestamp: Some(Utc::now()),
            server_picoseconds: 0,
        }
    }

    /// Get the value.
    pub fn value(&self) -> Option<&Variant> {
        self.value.as_ref()
    }

    /// Get mutable access to the value.
    pub fn value_mut(&mut self) -> Option<&mut Variant> {
        self.value.as_mut()
    }

    /// Take the value, leaving None in its place.
    pub fn take_value(&mut self) -> Option<Variant> {
        self.value.take()
    }

    /// Set the value.
    pub fn set_value(&mut self, value: Variant) {
        self.value = Some(value);
    }

    /// Get the status code.
    pub fn status(&self) -> StatusCode {
        self.status
    }

    /// Set the status code.
    pub fn set_status(&mut self, status: StatusCode) {
        self.status = status;
    }

    /// Get the source timestamp.
    pub fn source_timestamp(&self) -> Option<DateTime<Utc>> {
        self.source_timestamp
    }

    /// Set the source timestamp.
    pub fn set_source_timestamp(&mut self, timestamp: DateTime<Utc>) {
        self.source_timestamp = Some(timestamp);
    }

    /// Get the source picoseconds.
    pub fn source_picoseconds(&self) -> u16 {
        self.source_picoseconds
    }

    /// Set the source picoseconds.
    pub fn set_source_picoseconds(&mut self, picoseconds: u16) {
        self.source_picoseconds = picoseconds;
    }

    /// Get the server timestamp.
    pub fn server_timestamp(&self) -> Option<DateTime<Utc>> {
        self.server_timestamp
    }

    /// Set the server timestamp.
    pub fn set_server_timestamp(&mut self, timestamp: DateTime<Utc>) {
        self.server_timestamp = Some(timestamp);
    }

    /// Get the server picoseconds.
    pub fn server_picoseconds(&self) -> u16 {
        self.server_picoseconds
    }

    /// Set the server picoseconds.
    pub fn set_server_picoseconds(&mut self, picoseconds: u16) {
        self.server_picoseconds = picoseconds;
    }

    /// Check if the status is good.
    pub fn is_good(&self) -> bool {
        self.status.is_good()
    }

    /// Check if the status is bad.
    pub fn is_bad(&self) -> bool {
        self.status.is_bad()
    }

    /// Check if the status is uncertain.
    pub fn is_uncertain(&self) -> bool {
        self.status.is_uncertain()
    }

    /// Check if there is a value.
    pub fn has_value(&self) -> bool {
        self.value.is_some()
    }

    /// Update timestamps to current time.
    pub fn update_timestamps(&mut self) {
        let now = Utc::now();
        self.source_timestamp = Some(now);
        self.server_timestamp = Some(now);
    }

    /// Update only the server timestamp to current time.
    pub fn update_server_timestamp(&mut self) {
        self.server_timestamp = Some(Utc::now());
    }

    /// Builder-style: set source timestamp.
    pub fn with_source_timestamp(mut self, timestamp: DateTime<Utc>) -> Self {
        self.source_timestamp = Some(timestamp);
        self
    }

    /// Builder-style: set server timestamp.
    pub fn with_server_timestamp(mut self, timestamp: DateTime<Utc>) -> Self {
        self.server_timestamp = Some(timestamp);
        self
    }

    /// Builder-style: set both timestamps.
    pub fn with_timestamps(mut self, source: DateTime<Utc>, server: DateTime<Utc>) -> Self {
        self.source_timestamp = Some(source);
        self.server_timestamp = Some(server);
        self
    }
}

impl Default for DataValue {
    fn default() -> Self {
        Self::null()
    }
}

impl fmt::Display for DataValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.value {
            Some(v) => write!(f, "{} [{}]", v, self.status),
            None => write!(f, "<null> [{}]", self.status),
        }
    }
}

impl From<Variant> for DataValue {
    fn from(value: Variant) -> Self {
        Self::new(value)
    }
}

/// Builder for creating DataValue instances with custom configuration.
#[derive(Debug, Default)]
pub struct DataValueBuilder {
    value: Option<Variant>,
    status: Option<StatusCode>,
    source_timestamp: Option<DateTime<Utc>>,
    source_picoseconds: u16,
    server_timestamp: Option<DateTime<Utc>>,
    server_picoseconds: u16,
}

impl DataValueBuilder {
    /// Create a new builder.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the value.
    pub fn value(mut self, value: Variant) -> Self {
        self.value = Some(value);
        self
    }

    /// Set the status.
    pub fn status(mut self, status: StatusCode) -> Self {
        self.status = Some(status);
        self
    }

    /// Set the source timestamp.
    pub fn source_timestamp(mut self, timestamp: DateTime<Utc>) -> Self {
        self.source_timestamp = Some(timestamp);
        self
    }

    /// Set the source picoseconds.
    pub fn source_picoseconds(mut self, picoseconds: u16) -> Self {
        self.source_picoseconds = picoseconds;
        self
    }

    /// Set the server timestamp.
    pub fn server_timestamp(mut self, timestamp: DateTime<Utc>) -> Self {
        self.server_timestamp = Some(timestamp);
        self
    }

    /// Set the server picoseconds.
    pub fn server_picoseconds(mut self, picoseconds: u16) -> Self {
        self.server_picoseconds = picoseconds;
        self
    }

    /// Build the DataValue.
    pub fn build(self) -> DataValue {
        DataValue {
            value: self.value,
            status: self.status.unwrap_or(StatusCode::GOOD),
            source_timestamp: self.source_timestamp,
            source_picoseconds: self.source_picoseconds,
            server_timestamp: self.server_timestamp,
            server_picoseconds: self.server_picoseconds,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_data_value_new() {
        let dv = DataValue::new(Variant::double(3.14));
        assert!(dv.is_good());
        assert!(dv.has_value());
        assert!(dv.source_timestamp().is_some());
        assert!(dv.server_timestamp().is_some());
    }

    #[test]
    fn test_data_value_with_status() {
        let dv = DataValue::with_status(Variant::int32(42), StatusCode::UNCERTAIN_INITIAL_VALUE);
        assert!(dv.is_uncertain());
        assert!(!dv.is_good());
        assert!(!dv.is_bad());
    }

    #[test]
    fn test_data_value_bad() {
        let dv = DataValue::bad(StatusCode::BAD_NODE_ID_UNKNOWN);
        assert!(dv.is_bad());
        assert!(!dv.has_value());
    }

    #[test]
    fn test_data_value_builder() {
        let now = Utc::now();
        let dv = DataValueBuilder::new()
            .value(Variant::string("test"))
            .status(StatusCode::GOOD)
            .source_timestamp(now)
            .server_timestamp(now)
            .build();

        assert!(dv.is_good());
        assert_eq!(dv.value().unwrap().as_str(), Some("test"));
    }

    #[test]
    fn test_data_value_display() {
        let dv = DataValue::new(Variant::int32(42));
        let display = dv.to_string();
        assert!(display.contains("42"));
        assert!(display.contains("Good"));
    }
}

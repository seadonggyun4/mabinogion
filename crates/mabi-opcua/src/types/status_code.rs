//! OPC UA Status Code implementation.
//!
//! Status codes are 32-bit values that indicate the result of an operation.
//! The structure follows the OPC UA specification.

use serde::{Deserialize, Serialize};
use std::fmt;

/// OPC UA Status Code.
///
/// A status code is a 32-bit value composed of:
/// - Bits 31-30: Severity (00=Good, 01=Uncertain, 10=Bad)
/// - Bits 29-16: Sub-code
/// - Bits 15-0: Info bits
///
/// # Examples
///
/// ```
/// use mabi_opcua::types::StatusCode;
///
/// let status = StatusCode::GOOD;
/// assert!(status.is_good());
///
/// let status = StatusCode::BAD_NODE_ID_UNKNOWN;
/// assert!(status.is_bad());
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[repr(transparent)]
pub struct StatusCode(u32);

impl StatusCode {
    // ==========================================================================
    // Good Status Codes
    // ==========================================================================

    /// The operation was successful.
    pub const GOOD: Self = Self(0x00000000);
    /// The data source is a local value.
    pub const GOOD_LOCAL_OVERRIDE: Self = Self(0x00960000);
    /// The operation completed but resulted in clamped values.
    pub const GOOD_CLAMPED: Self = Self(0x00300000);
    /// No data exists for the requested time range.
    pub const GOOD_NO_DATA: Self = Self(0x00A50000);
    /// More data is available for the requested bounds.
    pub const GOOD_MORE_DATA: Self = Self(0x00A60000);
    /// The subscription was transferred.
    pub const GOOD_SUBSCRIPTION_TRANSFERRED: Self = Self(0x002D0000);
    /// Refresh in progress.
    pub const GOOD_REFRESH_IN_PROGRESS: Self = Self(0x006A0000);
    /// Refresh finished.
    pub const GOOD_REFRESH_FINISHED: Self = Self(0x006B0000);

    // ==========================================================================
    // Uncertain Status Codes
    // ==========================================================================

    /// The value is uncertain.
    pub const UNCERTAIN: Self = Self(0x40000000);
    /// The data value is from a different source.
    pub const UNCERTAIN_LAST_USABLE_VALUE: Self = Self(0x408F0000);
    /// Sensor failure.
    pub const UNCERTAIN_SENSOR_NOT_ACCURATE: Self = Self(0x40930000);
    /// Sub-normal operation.
    pub const UNCERTAIN_SUB_NORMAL: Self = Self(0x40A40000);
    /// Initial value.
    pub const UNCERTAIN_INITIAL_VALUE: Self = Self(0x40920000);

    // ==========================================================================
    // Bad Status Codes
    // ==========================================================================

    /// An unexpected error occurred.
    pub const BAD_UNEXPECTED_ERROR: Self = Self(0x80010000);
    /// An internal error occurred.
    pub const BAD_INTERNAL_ERROR: Self = Self(0x80020000);
    /// Not enough memory.
    pub const BAD_OUT_OF_MEMORY: Self = Self(0x80030000);
    /// Invalid argument.
    pub const BAD_INVALID_ARGUMENT: Self = Self(0x80AB0000);
    /// Resource not available.
    pub const BAD_RESOURCE_UNAVAILABLE: Self = Self(0x80040000);
    /// Communication error.
    pub const BAD_COMMUNICATION_ERROR: Self = Self(0x80050000);
    /// Encoding error.
    pub const BAD_ENCODING_ERROR: Self = Self(0x80060000);
    /// Decoding error.
    pub const BAD_DECODING_ERROR: Self = Self(0x80070000);
    /// Timeout occurred.
    pub const BAD_TIMEOUT: Self = Self(0x800A0000);
    /// Service not supported.
    pub const BAD_SERVICE_UNSUPPORTED: Self = Self(0x800B0000);
    /// Operation was shut down.
    pub const BAD_SHUTDOWN: Self = Self(0x800C0000);
    /// Server not connected.
    pub const BAD_SERVER_NOT_CONNECTED: Self = Self(0x800D0000);
    /// Server halted.
    pub const BAD_SERVER_HALTED: Self = Self(0x800E0000);
    /// Nothing to do.
    pub const BAD_NOTHING_TO_DO: Self = Self(0x800F0000);
    /// Too many operations.
    pub const BAD_TOO_MANY_OPERATIONS: Self = Self(0x80100000);
    /// Data type mismatch.
    pub const BAD_DATA_TYPE_MISMATCH: Self = Self(0x80490000);
    /// Node ID unknown.
    pub const BAD_NODE_ID_UNKNOWN: Self = Self(0x80340000);
    /// Node ID invalid.
    pub const BAD_NODE_ID_INVALID: Self = Self(0x80330000);
    /// Attribute ID invalid.
    pub const BAD_ATTRIBUTE_ID_INVALID: Self = Self(0x80350000);
    /// Index range invalid.
    pub const BAD_INDEX_RANGE_INVALID: Self = Self(0x80360000);
    /// Index range out of bounds.
    pub const BAD_INDEX_RANGE_NO_DATA: Self = Self(0x80370000);
    /// Data encoding invalid.
    pub const BAD_DATA_ENCODING_INVALID: Self = Self(0x80380000);
    /// Data encoding unsupported.
    pub const BAD_DATA_ENCODING_UNSUPPORTED: Self = Self(0x80390000);
    /// Not readable.
    pub const BAD_NOT_READABLE: Self = Self(0x803A0000);
    /// Not writable.
    pub const BAD_NOT_WRITABLE: Self = Self(0x803B0000);
    /// Out of range.
    pub const BAD_OUT_OF_RANGE: Self = Self(0x803C0000);
    /// Not supported.
    pub const BAD_NOT_SUPPORTED: Self = Self(0x803D0000);
    /// Not found.
    pub const BAD_NOT_FOUND: Self = Self(0x803E0000);
    /// Object deleted.
    pub const BAD_OBJECT_DELETED: Self = Self(0x803F0000);
    /// Not implemented.
    pub const BAD_NOT_IMPLEMENTED: Self = Self(0x80400000);
    /// Monitoring mode invalid.
    pub const BAD_MONITORING_MODE_INVALID: Self = Self(0x80410000);
    /// Monitored item ID invalid.
    pub const BAD_MONITORED_ITEM_ID_INVALID: Self = Self(0x80420000);
    /// Monitored item filter invalid.
    pub const BAD_MONITORED_ITEM_FILTER_INVALID: Self = Self(0x80430000);
    /// Monitored item filter unsupported.
    pub const BAD_MONITORED_ITEM_FILTER_UNSUPPORTED: Self = Self(0x80440000);
    /// Filter not allowed.
    pub const BAD_FILTER_NOT_ALLOWED: Self = Self(0x80450000);
    /// Structure missing.
    pub const BAD_STRUCTURE_MISSING: Self = Self(0x80460000);
    /// Event filter invalid.
    pub const BAD_EVENT_FILTER_INVALID: Self = Self(0x80470000);
    /// Content filter invalid.
    pub const BAD_CONTENT_FILTER_INVALID: Self = Self(0x80480000);
    /// Subscription ID invalid.
    pub const BAD_SUBSCRIPTION_ID_INVALID: Self = Self(0x80280000);
    /// Sequence number unknown.
    pub const BAD_SEQUENCE_NUMBER_UNKNOWN: Self = Self(0x80290000);
    /// Message not available.
    pub const BAD_MESSAGE_NOT_AVAILABLE: Self = Self(0x802A0000);
    /// Insufficient client profile.
    pub const BAD_INSUFFICIENT_CLIENT_PROFILE: Self = Self(0x802B0000);
    /// Session ID invalid.
    pub const BAD_SESSION_ID_INVALID: Self = Self(0x80250000);
    /// Session closed.
    pub const BAD_SESSION_CLOSED: Self = Self(0x80260000);
    /// Session not activated.
    pub const BAD_SESSION_NOT_ACTIVATED: Self = Self(0x80270000);
    /// Secure channel ID invalid.
    pub const BAD_SECURE_CHANNEL_ID_INVALID: Self = Self(0x80220000);
    /// Secure channel closed.
    pub const BAD_SECURE_CHANNEL_CLOSED: Self = Self(0x80860000);
    /// Request timeout.
    pub const BAD_REQUEST_TIMEOUT: Self = Self(0x80A10000);
    /// Security checks failed.
    pub const BAD_SECURITY_CHECKS_FAILED: Self = Self(0x80130000);
    /// User access denied.
    pub const BAD_USER_ACCESS_DENIED: Self = Self(0x801F0000);
    /// Identity token invalid.
    pub const BAD_IDENTITY_TOKEN_INVALID: Self = Self(0x80200000);
    /// Identity token rejected.
    pub const BAD_IDENTITY_TOKEN_REJECTED: Self = Self(0x80210000);
    /// No match.
    pub const BAD_NO_MATCH: Self = Self(0x806F0000);
    /// Browse direction invalid.
    pub const BAD_BROWSE_DIRECTION_INVALID: Self = Self(0x804D0000);
    /// Node not browsable.
    pub const BAD_NODE_NOT_BROWSABLE: Self = Self(0x804E0000);
    /// Reference type ID invalid.
    pub const BAD_REFERENCE_TYPE_ID_INVALID: Self = Self(0x804C0000);
    /// Continuation point invalid.
    pub const BAD_CONTINUATION_POINT_INVALID: Self = Self(0x804F0000);
    /// No continuation points.
    pub const BAD_NO_CONTINUATION_POINTS: Self = Self(0x80500000);
    /// Too many subscriptions.
    pub const BAD_TOO_MANY_SUBSCRIPTIONS: Self = Self(0x80770000);
    /// Too many monitored items.
    pub const BAD_TOO_MANY_MONITORED_ITEMS: Self = Self(0x80780000);
    /// Write not supported.
    pub const BAD_WRITE_NOT_SUPPORTED: Self = Self(0x80730000);
    /// History operation invalid.
    pub const BAD_HISTORY_OPERATION_INVALID: Self = Self(0x80710000);
    /// History operation unsupported.
    pub const BAD_HISTORY_OPERATION_UNSUPPORTED: Self = Self(0x80720000);

    // ==========================================================================
    // Methods
    // ==========================================================================

    /// Create a status code from a raw value.
    pub const fn from_raw(value: u32) -> Self {
        Self(value)
    }

    /// Get the raw value.
    pub const fn raw(&self) -> u32 {
        self.0
    }

    /// Check if the status is good.
    pub const fn is_good(&self) -> bool {
        self.severity() == 0
    }

    /// Check if the status is uncertain.
    pub const fn is_uncertain(&self) -> bool {
        self.severity() == 1
    }

    /// Check if the status is bad.
    pub const fn is_bad(&self) -> bool {
        self.severity() >= 2
    }

    /// Get the severity (0=Good, 1=Uncertain, 2=Bad).
    pub const fn severity(&self) -> u8 {
        ((self.0 >> 30) & 0x03) as u8
    }

    /// Get the sub-code.
    pub const fn sub_code(&self) -> u16 {
        ((self.0 >> 16) & 0x3FFF) as u16
    }

    /// Get the info bits.
    pub const fn info_bits(&self) -> u16 {
        (self.0 & 0xFFFF) as u16
    }

    /// Get a description of this status code.
    pub fn description(&self) -> &'static str {
        match *self {
            Self::GOOD => "Good",
            Self::GOOD_LOCAL_OVERRIDE => "Good (local override)",
            Self::GOOD_CLAMPED => "Good (clamped)",
            Self::GOOD_NO_DATA => "Good (no data)",
            Self::GOOD_MORE_DATA => "Good (more data available)",
            Self::UNCERTAIN => "Uncertain",
            Self::UNCERTAIN_LAST_USABLE_VALUE => "Uncertain (last usable value)",
            Self::UNCERTAIN_SENSOR_NOT_ACCURATE => "Uncertain (sensor not accurate)",
            Self::UNCERTAIN_INITIAL_VALUE => "Uncertain (initial value)",
            Self::BAD_UNEXPECTED_ERROR => "Bad (unexpected error)",
            Self::BAD_INTERNAL_ERROR => "Bad (internal error)",
            Self::BAD_OUT_OF_MEMORY => "Bad (out of memory)",
            Self::BAD_INVALID_ARGUMENT => "Bad (invalid argument)",
            Self::BAD_TIMEOUT => "Bad (timeout)",
            Self::BAD_NODE_ID_UNKNOWN => "Bad (node ID unknown)",
            Self::BAD_NODE_ID_INVALID => "Bad (node ID invalid)",
            Self::BAD_ATTRIBUTE_ID_INVALID => "Bad (attribute ID invalid)",
            Self::BAD_NOT_READABLE => "Bad (not readable)",
            Self::BAD_NOT_WRITABLE => "Bad (not writable)",
            Self::BAD_NOT_FOUND => "Bad (not found)",
            Self::BAD_NOT_SUPPORTED => "Bad (not supported)",
            Self::BAD_NOT_IMPLEMENTED => "Bad (not implemented)",
            Self::BAD_SUBSCRIPTION_ID_INVALID => "Bad (subscription ID invalid)",
            Self::BAD_SESSION_ID_INVALID => "Bad (session ID invalid)",
            Self::BAD_SESSION_CLOSED => "Bad (session closed)",
            Self::BAD_TOO_MANY_SUBSCRIPTIONS => "Bad (too many subscriptions)",
            Self::BAD_TOO_MANY_MONITORED_ITEMS => "Bad (too many monitored items)",
            _ => "Unknown status code",
        }
    }
}

impl Default for StatusCode {
    fn default() -> Self {
        Self::GOOD
    }
}

impl fmt::Display for StatusCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} (0x{:08X})", self.description(), self.0)
    }
}

impl From<u32> for StatusCode {
    fn from(value: u32) -> Self {
        Self(value)
    }
}

impl From<StatusCode> for u32 {
    fn from(status: StatusCode) -> Self {
        status.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_good_status() {
        assert!(StatusCode::GOOD.is_good());
        assert!(!StatusCode::GOOD.is_uncertain());
        assert!(!StatusCode::GOOD.is_bad());
        assert_eq!(StatusCode::GOOD.severity(), 0);
    }

    #[test]
    fn test_uncertain_status() {
        assert!(!StatusCode::UNCERTAIN.is_good());
        assert!(StatusCode::UNCERTAIN.is_uncertain());
        assert!(!StatusCode::UNCERTAIN.is_bad());
        assert_eq!(StatusCode::UNCERTAIN.severity(), 1);
    }

    #[test]
    fn test_bad_status() {
        assert!(!StatusCode::BAD_UNEXPECTED_ERROR.is_good());
        assert!(!StatusCode::BAD_UNEXPECTED_ERROR.is_uncertain());
        assert!(StatusCode::BAD_UNEXPECTED_ERROR.is_bad());
        assert_eq!(StatusCode::BAD_UNEXPECTED_ERROR.severity(), 2);
    }

    #[test]
    fn test_status_code_display() {
        let status = StatusCode::GOOD;
        assert!(status.to_string().contains("Good"));

        let status = StatusCode::BAD_NODE_ID_UNKNOWN;
        assert!(status.to_string().contains("node ID unknown"));
    }
}

//! Fault context for carrying request/response information.
//!
//! The [`FaultContext`] provides all the information needed to determine
//! whether a fault should be applied and how it should modify the operation.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use mabi_core::{DataPoint, Protocol, Quality, Value};
use serde::{Deserialize, Serialize};

// =============================================================================
// Fault Context
// =============================================================================

/// Context for fault injection decisions and modifications.
///
/// The context carries all information about the current operation,
/// allowing faults to make decisions and modify behavior.
#[derive(Debug, Clone)]
pub struct FaultContext {
    /// Unique request ID.
    pub request_id: String,

    /// Target information.
    pub target: TargetInfo,

    /// Request phase.
    pub phase: RequestPhase,

    /// Operation type.
    pub operation: OperationType,

    /// Protocol being used.
    pub protocol: Protocol,

    /// Request timestamp.
    pub timestamp: DateTime<Utc>,

    /// Request start time (for duration tracking).
    pub started_at: Instant,

    /// Request data (for modification).
    pub request_data: Option<RequestData>,

    /// Response data (available in after_operation).
    pub response_data: Option<ResponseData>,

    /// Arbitrary metadata.
    pub metadata: HashMap<String, String>,

    /// Faults that have been applied.
    pub applied_faults: Vec<AppliedFault>,

    /// Whether to skip remaining faults.
    pub skip_remaining: bool,

    /// Accumulated delay (in milliseconds).
    pub accumulated_delay_ms: u64,
}

impl FaultContext {
    /// Create a new fault context.
    pub fn new(target: TargetInfo, operation: OperationType, protocol: Protocol) -> Self {
        Self {
            request_id: uuid::Uuid::new_v4().to_string(),
            target,
            phase: RequestPhase::Before,
            operation,
            protocol,
            timestamp: Utc::now(),
            started_at: Instant::now(),
            request_data: None,
            response_data: None,
            metadata: HashMap::new(),
            applied_faults: Vec::new(),
            skip_remaining: false,
            accumulated_delay_ms: 0,
        }
    }

    /// Create a builder for constructing contexts.
    pub fn builder() -> FaultContextBuilder {
        FaultContextBuilder::default()
    }

    /// Get elapsed time since request start.
    pub fn elapsed(&self) -> Duration {
        self.started_at.elapsed()
    }

    /// Get elapsed milliseconds.
    pub fn elapsed_ms(&self) -> u64 {
        self.elapsed().as_millis() as u64
    }

    /// Set metadata value.
    pub fn set_metadata(&mut self, key: impl Into<String>, value: impl Into<String>) {
        self.metadata.insert(key.into(), value.into());
    }

    /// Get metadata value.
    pub fn get_metadata(&self, key: &str) -> Option<&str> {
        self.metadata.get(key).map(|s| s.as_str())
    }

    /// Record that a fault was applied.
    pub fn record_applied_fault(&mut self, fault_id: &str, behavior: &str) {
        self.applied_faults.push(AppliedFault {
            fault_id: fault_id.to_string(),
            behavior: behavior.to_string(),
            timestamp: Utc::now(),
        });
    }

    /// Add delay to accumulated delay.
    pub fn add_delay(&mut self, delay_ms: u64) {
        self.accumulated_delay_ms += delay_ms;
    }

    /// Check if any fault was applied.
    pub fn was_affected(&self) -> bool {
        !self.applied_faults.is_empty()
    }

    /// Transition to the after phase.
    pub fn transition_to_after(&mut self) {
        self.phase = RequestPhase::After;
    }

    /// Set response data.
    pub fn set_response(&mut self, response: ResponseData) {
        self.response_data = Some(response);
    }

    /// Check if this is a read operation.
    pub fn is_read(&self) -> bool {
        matches!(
            self.operation,
            OperationType::Read { .. } | OperationType::ReadMultiple { .. }
        )
    }

    /// Check if this is a write operation.
    pub fn is_write(&self) -> bool {
        matches!(
            self.operation,
            OperationType::Write { .. } | OperationType::WriteMultiple { .. }
        )
    }

    /// Get point IDs involved in this operation.
    pub fn point_ids(&self) -> Vec<&str> {
        match &self.operation {
            OperationType::Read { point_id } => vec![point_id.as_str()],
            OperationType::Write { point_id, .. } => vec![point_id.as_str()],
            OperationType::ReadMultiple { point_ids } => {
                point_ids.iter().map(|s| s.as_str()).collect()
            }
            OperationType::WriteMultiple { values } => {
                values.iter().map(|(id, _)| id.as_str()).collect()
            }
            OperationType::Initialize
            | OperationType::Start
            | OperationType::Stop
            | OperationType::Tick => vec![],
            OperationType::Custom { .. } => vec![],
        }
    }
}

// =============================================================================
// Fault Context Builder
// =============================================================================

/// Builder for creating fault contexts.
#[derive(Debug, Default)]
pub struct FaultContextBuilder {
    target: Option<TargetInfo>,
    operation: Option<OperationType>,
    protocol: Option<Protocol>,
    request_data: Option<RequestData>,
    metadata: HashMap<String, String>,
}

impl FaultContextBuilder {
    /// Set target information.
    pub fn target(mut self, target: TargetInfo) -> Self {
        self.target = Some(target);
        self
    }

    /// Set target by device ID.
    pub fn device_id(mut self, device_id: impl Into<String>) -> Self {
        self.target = Some(TargetInfo::device(device_id));
        self
    }

    /// Set operation type.
    pub fn operation(mut self, operation: OperationType) -> Self {
        self.operation = Some(operation);
        self
    }

    /// Set read operation.
    pub fn read(mut self, point_id: impl Into<String>) -> Self {
        self.operation = Some(OperationType::Read {
            point_id: point_id.into(),
        });
        self
    }

    /// Set write operation.
    pub fn write(mut self, point_id: impl Into<String>, value: Value) -> Self {
        self.operation = Some(OperationType::Write {
            point_id: point_id.into(),
            value,
        });
        self
    }

    /// Set protocol.
    pub fn protocol(mut self, protocol: Protocol) -> Self {
        self.protocol = Some(protocol);
        self
    }

    /// Set request data.
    pub fn request_data(mut self, data: RequestData) -> Self {
        self.request_data = Some(data);
        self
    }

    /// Add metadata.
    pub fn metadata(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.metadata.insert(key.into(), value.into());
        self
    }

    /// Build the context.
    pub fn build(self) -> FaultContext {
        let target = self.target.unwrap_or_default();
        let operation = self.operation.unwrap_or(OperationType::Custom {
            name: "unknown".to_string(),
        });
        let protocol = self.protocol.unwrap_or(Protocol::ModbusTcp);

        let mut ctx = FaultContext::new(target, operation, protocol);
        ctx.request_data = self.request_data;
        ctx.metadata = self.metadata;
        ctx
    }
}

// =============================================================================
// Target Info
// =============================================================================

/// Information about the target of an operation.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TargetInfo {
    /// Device ID.
    pub device_id: String,

    /// Device name (optional).
    pub device_name: Option<String>,

    /// Connection address.
    pub address: Option<String>,

    /// Port number.
    pub port: Option<u16>,

    /// Unit ID (for Modbus).
    pub unit_id: Option<u8>,

    /// Additional target attributes.
    #[serde(default)]
    pub attributes: HashMap<String, String>,
}

impl TargetInfo {
    /// Create target info for a device.
    pub fn device(device_id: impl Into<String>) -> Self {
        Self {
            device_id: device_id.into(),
            ..Default::default()
        }
    }

    /// Set device name.
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.device_name = Some(name.into());
        self
    }

    /// Set address.
    pub fn with_address(mut self, address: impl Into<String>) -> Self {
        self.address = Some(address.into());
        self
    }

    /// Set port.
    pub fn with_port(mut self, port: u16) -> Self {
        self.port = Some(port);
        self
    }

    /// Set unit ID.
    pub fn with_unit_id(mut self, unit_id: u8) -> Self {
        self.unit_id = Some(unit_id);
        self
    }

    /// Add attribute.
    pub fn with_attribute(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.attributes.insert(key.into(), value.into());
        self
    }

    /// Get identifier for matching.
    pub fn identifier(&self) -> &str {
        &self.device_id
    }
}

// =============================================================================
// Request Phase
// =============================================================================

/// Phase of request processing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum RequestPhase {
    /// Before the operation is executed.
    #[default]
    Before,

    /// After the operation has completed.
    After,
}

// =============================================================================
// Operation Type
// =============================================================================

/// Type of operation being performed.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum OperationType {
    /// Read a single data point.
    Read { point_id: String },

    /// Write a single data point.
    Write { point_id: String, value: Value },

    /// Read multiple data points.
    ReadMultiple { point_ids: Vec<String> },

    /// Write multiple data points.
    WriteMultiple { values: Vec<(String, Value)> },

    /// Device initialization.
    Initialize,

    /// Device start.
    Start,

    /// Device stop.
    Stop,

    /// Device tick.
    Tick,

    /// Custom operation.
    Custom { name: String },
}

impl Default for OperationType {
    fn default() -> Self {
        Self::Custom {
            name: "unknown".to_string(),
        }
    }
}

// =============================================================================
// Request/Response Data
// =============================================================================

/// Request data that can be modified by faults.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestData {
    /// Raw bytes (for protocol-level faults).
    pub raw: Option<Vec<u8>>,

    /// Parsed values.
    pub values: HashMap<String, Value>,

    /// Request-specific parameters.
    pub params: HashMap<String, String>,
}

impl RequestData {
    /// Create new request data.
    pub fn new() -> Self {
        Self {
            raw: None,
            values: HashMap::new(),
            params: HashMap::new(),
        }
    }

    /// Create from raw bytes.
    pub fn from_raw(raw: Vec<u8>) -> Self {
        Self {
            raw: Some(raw),
            values: HashMap::new(),
            params: HashMap::new(),
        }
    }

    /// Set a value.
    pub fn with_value(mut self, key: impl Into<String>, value: Value) -> Self {
        self.values.insert(key.into(), value);
        self
    }

    /// Set a parameter.
    pub fn with_param(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.params.insert(key.into(), value.into());
        self
    }
}

impl Default for RequestData {
    fn default() -> Self {
        Self::new()
    }
}

/// Response data that can be modified by faults.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResponseData {
    /// Raw bytes (for protocol-level faults).
    pub raw: Option<Vec<u8>>,

    /// Data points returned.
    pub data_points: Vec<DataPoint>,

    /// Error information (if any).
    pub error: Option<ResponseError>,

    /// Response-specific parameters.
    pub params: HashMap<String, String>,
}

impl ResponseData {
    /// Create new response data.
    pub fn new() -> Self {
        Self {
            raw: None,
            data_points: Vec::new(),
            error: None,
            params: HashMap::new(),
        }
    }

    /// Create from data points.
    pub fn from_points(points: Vec<DataPoint>) -> Self {
        Self {
            raw: None,
            data_points: points,
            error: None,
            params: HashMap::new(),
        }
    }

    /// Create error response.
    pub fn error(code: i32, message: impl Into<String>) -> Self {
        Self {
            raw: None,
            data_points: Vec::new(),
            error: Some(ResponseError {
                code,
                message: message.into(),
            }),
            params: HashMap::new(),
        }
    }

    /// Check if response has error.
    pub fn is_error(&self) -> bool {
        self.error.is_some()
    }

    /// Corrupt data point values.
    pub fn corrupt_values(&mut self) {
        for point in &mut self.data_points {
            point.quality = Quality::BAD;
        }
    }
}

impl Default for ResponseData {
    fn default() -> Self {
        Self::new()
    }
}

/// Response error information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResponseError {
    /// Error code.
    pub code: i32,

    /// Error message.
    pub message: String,
}

// =============================================================================
// Applied Fault
// =============================================================================

/// Record of a fault that was applied.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppliedFault {
    /// Fault ID.
    pub fault_id: String,

    /// Behavior that was applied.
    pub behavior: String,

    /// When the fault was applied.
    pub timestamp: DateTime<Utc>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fault_context_creation() {
        let target = TargetInfo::device("device-001")
            .with_name("Test Device")
            .with_port(502);

        let ctx = FaultContext::new(
            target,
            OperationType::Read {
                point_id: "temp".to_string(),
            },
            Protocol::ModbusTcp,
        );

        assert_eq!(ctx.target.device_id, "device-001");
        assert!(ctx.is_read());
        assert!(!ctx.is_write());
        assert_eq!(ctx.point_ids(), vec!["temp"]);
    }

    #[test]
    fn test_context_builder() {
        let ctx = FaultContext::builder()
            .device_id("device-002")
            .read("pressure")
            .protocol(Protocol::OpcUa)
            .metadata("source", "test")
            .build();

        assert_eq!(ctx.target.device_id, "device-002");
        assert_eq!(ctx.protocol, Protocol::OpcUa);
        assert_eq!(ctx.get_metadata("source"), Some("test"));
    }

    #[test]
    fn test_target_info() {
        let target = TargetInfo::device("modbus-001")
            .with_address("192.168.1.100")
            .with_port(502)
            .with_unit_id(1)
            .with_attribute("location", "Building A");

        assert_eq!(target.identifier(), "modbus-001");
        assert_eq!(target.port, Some(502));
        assert_eq!(target.unit_id, Some(1));
    }

    #[test]
    fn test_applied_faults() {
        let mut ctx = FaultContext::builder()
            .device_id("test")
            .operation(OperationType::Initialize)
            .build();

        assert!(!ctx.was_affected());

        ctx.record_applied_fault("latency-001", "delay");
        assert!(ctx.was_affected());
        assert_eq!(ctx.applied_faults.len(), 1);
    }
}

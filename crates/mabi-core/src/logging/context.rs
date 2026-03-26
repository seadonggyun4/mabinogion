//! Structured context propagation for distributed tracing.
//!
//! This module provides utilities for propagating context information
//! across async boundaries and between services. It supports:
//!
//! - Request/correlation IDs for tracing requests across components
//! - Device context for device-specific operations
//! - Protocol context for protocol-specific logging
//! - Custom context fields for application-specific needs
//!
//! # Example
//!
//! ```rust,ignore
//! use mabi_core::logging::context::{TraceContext, RequestContext};
//!
//! // Create a trace context for a request
//! let ctx = TraceContext::new()
//!     .with_request_id("req-12345")
//!     .with_device_id("device-001")
//!     .with_protocol("modbus");
//!
//! // Use in a span
//! let span = ctx.create_span("handle_request");
//! let _guard = span.enter();
//!
//! // Or use the request_span! macro
//! request_span!(ctx, "handle_request", {
//!     // Your code here
//! });
//! ```

use std::collections::HashMap;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tracing::{span, Level, Span};
use uuid::Uuid;

/// Trace context for distributed tracing.
///
/// This struct carries context information that should be propagated
/// across async boundaries and potentially across service boundaries.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceContext {
    /// Unique request/correlation ID.
    pub request_id: String,

    /// Trace ID (for distributed tracing systems).
    #[serde(default)]
    pub trace_id: Option<String>,

    /// Span ID (for distributed tracing systems).
    #[serde(default)]
    pub span_id: Option<String>,

    /// Parent span ID.
    #[serde(default)]
    pub parent_span_id: Option<String>,

    /// Device ID (if applicable).
    #[serde(default)]
    pub device_id: Option<String>,

    /// Protocol name (if applicable).
    #[serde(default)]
    pub protocol: Option<String>,

    /// Operation name.
    #[serde(default)]
    pub operation: Option<String>,

    /// Custom fields.
    #[serde(default)]
    pub fields: HashMap<String, String>,

    /// Timestamp when context was created.
    #[serde(default = "default_timestamp")]
    pub created_at: u64,
}

fn default_timestamp() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

impl Default for TraceContext {
    fn default() -> Self {
        Self::new()
    }
}

impl TraceContext {
    /// Create a new trace context with a generated request ID.
    pub fn new() -> Self {
        Self {
            request_id: Uuid::new_v4().to_string(),
            trace_id: None,
            span_id: None,
            parent_span_id: None,
            device_id: None,
            protocol: None,
            operation: None,
            fields: HashMap::new(),
            created_at: default_timestamp(),
        }
    }

    /// Create a trace context with a specific request ID.
    pub fn with_request_id(request_id: impl Into<String>) -> Self {
        Self {
            request_id: request_id.into(),
            ..Self::new()
        }
    }

    /// Create a child context (new span under same trace).
    pub fn child(&self) -> Self {
        Self {
            request_id: self.request_id.clone(),
            trace_id: self.trace_id.clone(),
            span_id: Some(Uuid::new_v4().to_string()),
            parent_span_id: self.span_id.clone(),
            device_id: self.device_id.clone(),
            protocol: self.protocol.clone(),
            operation: None,
            fields: self.fields.clone(),
            created_at: default_timestamp(),
        }
    }

    /// Set the device ID.
    pub fn with_device_id(mut self, device_id: impl Into<String>) -> Self {
        self.device_id = Some(device_id.into());
        self
    }

    /// Set the protocol.
    pub fn with_protocol(mut self, protocol: impl Into<String>) -> Self {
        self.protocol = Some(protocol.into());
        self
    }

    /// Set the operation name.
    pub fn with_operation(mut self, operation: impl Into<String>) -> Self {
        self.operation = Some(operation.into());
        self
    }

    /// Set the trace ID (for integration with distributed tracing).
    pub fn with_trace_id(mut self, trace_id: impl Into<String>) -> Self {
        self.trace_id = Some(trace_id.into());
        self
    }

    /// Add a custom field.
    pub fn with_field(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.fields.insert(key.into(), value.into());
        self
    }

    /// Add multiple custom fields.
    pub fn with_fields(mut self, fields: impl IntoIterator<Item = (String, String)>) -> Self {
        self.fields.extend(fields);
        self
    }

    /// Create a tracing span with this context.
    pub fn create_span(&self, name: &'static str) -> Span {
        let span = span!(
            Level::INFO,
            "request",
            request_id = %self.request_id,
            operation = name,
        );

        // Add optional fields
        if let Some(ref device_id) = self.device_id {
            span.record("device_id", device_id.as_str());
        }
        if let Some(ref protocol) = self.protocol {
            span.record("protocol", protocol.as_str());
        }
        if let Some(ref trace_id) = self.trace_id {
            span.record("trace_id", trace_id.as_str());
        }

        span
    }

    /// Create a debug-level span (for less critical operations).
    pub fn create_debug_span(&self, name: &'static str) -> Span {
        span!(
            Level::DEBUG,
            "operation",
            request_id = %self.request_id,
            operation = name,
            device_id = self.device_id.as_deref().unwrap_or(""),
        )
    }

    /// Get the age of this context in milliseconds.
    pub fn age_ms(&self) -> u64 {
        default_timestamp().saturating_sub(self.created_at)
    }

    /// Check if this context is older than the given milliseconds.
    pub fn is_older_than_ms(&self, ms: u64) -> bool {
        self.age_ms() > ms
    }

    /// Convert to a map for logging or serialization.
    pub fn to_map(&self) -> HashMap<String, String> {
        let mut map = HashMap::new();
        map.insert("request_id".to_string(), self.request_id.clone());

        if let Some(ref trace_id) = self.trace_id {
            map.insert("trace_id".to_string(), trace_id.clone());
        }
        if let Some(ref device_id) = self.device_id {
            map.insert("device_id".to_string(), device_id.clone());
        }
        if let Some(ref protocol) = self.protocol {
            map.insert("protocol".to_string(), protocol.clone());
        }
        if let Some(ref operation) = self.operation {
            map.insert("operation".to_string(), operation.clone());
        }

        map.extend(self.fields.clone());
        map
    }

    /// Parse from HTTP headers (for distributed tracing).
    pub fn from_headers(headers: &HashMap<String, String>) -> Self {
        let mut ctx = Self::new();

        if let Some(request_id) = headers
            .get("x-request-id")
            .or(headers.get("x-correlation-id"))
        {
            ctx.request_id = request_id.clone();
        }
        if let Some(trace_id) = headers.get("x-trace-id").or(headers.get("traceparent")) {
            ctx.trace_id = Some(trace_id.clone());
        }
        if let Some(span_id) = headers.get("x-span-id") {
            ctx.span_id = Some(span_id.clone());
        }
        if let Some(device_id) = headers.get("x-device-id") {
            ctx.device_id = Some(device_id.clone());
        }

        ctx
    }

    /// Convert to HTTP headers (for distributed tracing).
    pub fn to_headers(&self) -> HashMap<String, String> {
        let mut headers = HashMap::new();

        headers.insert("x-request-id".to_string(), self.request_id.clone());

        if let Some(ref trace_id) = self.trace_id {
            headers.insert("x-trace-id".to_string(), trace_id.clone());
        }
        if let Some(ref span_id) = self.span_id {
            headers.insert("x-span-id".to_string(), span_id.clone());
        }
        if let Some(ref device_id) = self.device_id {
            headers.insert("x-device-id".to_string(), device_id.clone());
        }

        headers
    }
}

/// Request context for protocol operations.
///
/// This is a specialized context for protocol-level requests.
#[derive(Debug, Clone)]
pub struct RequestContext {
    /// Base trace context.
    pub trace: TraceContext,

    /// Request start time.
    pub start_time: std::time::Instant,

    /// Request timeout (if set).
    pub timeout: Option<std::time::Duration>,

    /// Whether this request should be logged at debug level.
    pub debug_request: bool,
}

impl RequestContext {
    /// Create a new request context.
    pub fn new() -> Self {
        Self {
            trace: TraceContext::new(),
            start_time: std::time::Instant::now(),
            timeout: None,
            debug_request: false,
        }
    }

    /// Create with an existing trace context.
    pub fn with_trace(trace: TraceContext) -> Self {
        Self {
            trace,
            start_time: std::time::Instant::now(),
            timeout: None,
            debug_request: false,
        }
    }

    /// Set the device ID.
    pub fn device(mut self, device_id: impl Into<String>) -> Self {
        self.trace = self.trace.with_device_id(device_id);
        self
    }

    /// Set the protocol.
    pub fn protocol(mut self, protocol: impl Into<String>) -> Self {
        self.trace = self.trace.with_protocol(protocol);
        self
    }

    /// Set the operation.
    pub fn operation(mut self, operation: impl Into<String>) -> Self {
        self.trace = self.trace.with_operation(operation);
        self
    }

    /// Set a timeout.
    pub fn with_timeout(mut self, timeout: std::time::Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }

    /// Mark as a debug request (logged at debug level).
    pub fn debug(mut self) -> Self {
        self.debug_request = true;
        self
    }

    /// Get elapsed time since request started.
    pub fn elapsed(&self) -> std::time::Duration {
        self.start_time.elapsed()
    }

    /// Check if the request has timed out.
    pub fn is_timed_out(&self) -> bool {
        self.timeout.map(|t| self.elapsed() > t).unwrap_or(false)
    }

    /// Get remaining time before timeout.
    pub fn remaining_timeout(&self) -> Option<std::time::Duration> {
        self.timeout.and_then(|t| t.checked_sub(self.elapsed()))
    }

    /// Get the request ID.
    pub fn request_id(&self) -> &str {
        &self.trace.request_id
    }

    /// Create a span for this request.
    pub fn span(&self, name: &'static str) -> Span {
        if self.debug_request {
            self.trace.create_debug_span(name)
        } else {
            self.trace.create_span(name)
        }
    }
}

impl Default for RequestContext {
    fn default() -> Self {
        Self::new()
    }
}

/// Shared trace context for passing across threads.
pub type SharedTraceContext = Arc<TraceContext>;

/// Create a shared trace context.
pub fn shared_context(ctx: TraceContext) -> SharedTraceContext {
    Arc::new(ctx)
}

/// Device context for device-specific operations.
#[derive(Debug, Clone)]
pub struct DeviceContext {
    /// Device ID.
    pub device_id: String,

    /// Protocol.
    pub protocol: String,

    /// Base trace context.
    pub trace: TraceContext,
}

impl DeviceContext {
    /// Create a new device context.
    pub fn new(device_id: impl Into<String>, protocol: impl Into<String>) -> Self {
        let device_id = device_id.into();
        let protocol = protocol.into();

        Self {
            device_id: device_id.clone(),
            protocol: protocol.clone(),
            trace: TraceContext::new()
                .with_device_id(device_id)
                .with_protocol(protocol),
        }
    }

    /// Create with an existing trace context.
    pub fn with_trace(
        device_id: impl Into<String>,
        protocol: impl Into<String>,
        trace: TraceContext,
    ) -> Self {
        let device_id = device_id.into();
        let protocol = protocol.into();

        Self {
            device_id: device_id.clone(),
            protocol: protocol.clone(),
            trace: trace.with_device_id(device_id).with_protocol(protocol),
        }
    }

    /// Create a span for a device operation.
    pub fn span(&self, operation: &'static str) -> Span {
        span!(
            Level::DEBUG,
            "device_operation",
            device_id = %self.device_id,
            protocol = %self.protocol,
            operation = operation,
            request_id = %self.trace.request_id,
        )
    }

    /// Get the request ID.
    pub fn request_id(&self) -> &str {
        &self.trace.request_id
    }

    /// Create a child context for a sub-operation.
    pub fn child(&self) -> Self {
        Self {
            device_id: self.device_id.clone(),
            protocol: self.protocol.clone(),
            trace: self.trace.child(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_trace_context_creation() {
        let ctx = TraceContext::new();
        assert!(!ctx.request_id.is_empty());
        assert!(ctx.device_id.is_none());
        assert!(ctx.protocol.is_none());
    }

    #[test]
    fn test_trace_context_builder() {
        let ctx = TraceContext::new()
            .with_device_id("device-001")
            .with_protocol("modbus")
            .with_operation("read")
            .with_field("unit_id", "1");

        assert_eq!(ctx.device_id, Some("device-001".to_string()));
        assert_eq!(ctx.protocol, Some("modbus".to_string()));
        assert_eq!(ctx.operation, Some("read".to_string()));
        assert_eq!(ctx.fields.get("unit_id"), Some(&"1".to_string()));
    }

    #[test]
    fn test_trace_context_child() {
        let parent = TraceContext::new()
            .with_device_id("device-001")
            .with_trace_id("trace-123");

        let child = parent.child();

        assert_eq!(child.request_id, parent.request_id);
        assert_eq!(child.trace_id, parent.trace_id);
        assert_eq!(child.device_id, parent.device_id);
        assert_eq!(child.parent_span_id, parent.span_id);
    }

    #[test]
    fn test_trace_context_to_map() {
        let ctx = TraceContext::new()
            .with_device_id("device-001")
            .with_protocol("modbus");

        let map = ctx.to_map();
        assert!(map.contains_key("request_id"));
        assert_eq!(map.get("device_id"), Some(&"device-001".to_string()));
        assert_eq!(map.get("protocol"), Some(&"modbus".to_string()));
    }

    #[test]
    fn test_trace_context_headers() {
        let ctx = TraceContext::new()
            .with_device_id("device-001")
            .with_trace_id("trace-123");

        let headers = ctx.to_headers();
        assert!(headers.contains_key("x-request-id"));
        assert_eq!(headers.get("x-trace-id"), Some(&"trace-123".to_string()));
        assert_eq!(headers.get("x-device-id"), Some(&"device-001".to_string()));

        // Round-trip
        let parsed = TraceContext::from_headers(&headers);
        assert_eq!(parsed.request_id, ctx.request_id);
        assert_eq!(parsed.trace_id, ctx.trace_id);
        assert_eq!(parsed.device_id, ctx.device_id);
    }

    #[test]
    fn test_request_context() {
        let ctx = RequestContext::new()
            .device("device-001")
            .protocol("modbus")
            .operation("read")
            .with_timeout(std::time::Duration::from_secs(5));

        assert!(!ctx.request_id().is_empty());
        assert!(!ctx.is_timed_out());
        assert!(ctx.remaining_timeout().is_some());
    }

    #[test]
    fn test_device_context() {
        let ctx = DeviceContext::new("device-001", "modbus");

        assert_eq!(ctx.device_id, "device-001");
        assert_eq!(ctx.protocol, "modbus");
        assert!(!ctx.request_id().is_empty());

        let child = ctx.child();
        assert_eq!(child.request_id(), ctx.request_id());
    }

    #[test]
    fn test_trace_context_age() {
        let ctx = TraceContext::new();
        std::thread::sleep(std::time::Duration::from_millis(10));

        assert!(ctx.age_ms() >= 10);
        assert!(ctx.is_older_than_ms(5));
        assert!(!ctx.is_older_than_ms(1000));
    }

    #[test]
    fn test_shared_context() {
        let ctx = TraceContext::new().with_device_id("device-001");
        let shared = shared_context(ctx);

        assert_eq!(shared.device_id, Some("device-001".to_string()));
    }
}

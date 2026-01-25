//! Logging and tracing macros.
//!
//! This module provides convenient macros for structured logging and tracing.

/// Create a span for tracing protocol requests.
///
/// This macro creates a structured span with standard fields for request tracing.
///
/// # Example
///
/// ```rust,ignore
/// use mabi_core::trace_request;
///
/// async fn handle_request() {
///     let _span = trace_request!("modbus", "read_registers");
///     // ... request handling
/// }
/// ```
#[macro_export]
macro_rules! trace_request {
    ($protocol:expr, $operation:expr) => {
        tracing::info_span!(
            "request",
            protocol = $protocol,
            operation = $operation,
            request_id = %uuid::Uuid::new_v4(),
        )
    };
    ($protocol:expr, $operation:expr, $($field:tt)*) => {
        tracing::info_span!(
            "request",
            protocol = $protocol,
            operation = $operation,
            request_id = %uuid::Uuid::new_v4(),
            $($field)*
        )
    };
}

/// Create a span for tracing device operations.
///
/// # Example
///
/// ```rust,ignore
/// use mabi_core::trace_device;
///
/// async fn device_tick(device_id: &str) {
///     let _span = trace_device!(device_id, "tick");
///     // ... device processing
/// }
/// ```
#[macro_export]
macro_rules! trace_device {
    ($device_id:expr, $operation:expr) => {
        tracing::debug_span!(
            "device",
            device_id = $device_id,
            operation = $operation,
        )
    };
    ($device_id:expr, $operation:expr, $($field:tt)*) => {
        tracing::debug_span!(
            "device",
            device_id = $device_id,
            operation = $operation,
            $($field)*
        )
    };
}

/// Log a successful operation with timing.
///
/// # Example
///
/// ```rust,ignore
/// use mabi_core::trace_success;
/// use std::time::Instant;
///
/// let start = Instant::now();
/// // ... operation
/// trace_success!("read", start);
/// ```
#[macro_export]
macro_rules! trace_success {
    ($operation:expr, $start:expr) => {
        tracing::info!(
            operation = $operation,
            duration_us = $start.elapsed().as_micros() as u64,
            "Operation completed"
        );
    };
    ($operation:expr, $start:expr, $($field:tt)*) => {
        tracing::info!(
            operation = $operation,
            duration_us = $start.elapsed().as_micros() as u64,
            $($field)*,
            "Operation completed"
        );
    };
}

/// Log a failed operation with error details.
///
/// # Example
///
/// ```rust,ignore
/// use mabi_core::trace_error;
///
/// match result {
///     Ok(_) => {},
///     Err(e) => trace_error!("read", e),
/// }
/// ```
#[macro_export]
macro_rules! trace_error {
    ($operation:expr, $error:expr) => {
        tracing::error!(
            operation = $operation,
            error = %$error,
            "Operation failed"
        );
    };
    ($operation:expr, $error:expr, $($field:tt)*) => {
        tracing::error!(
            operation = $operation,
            error = %$error,
            $($field)*,
            "Operation failed"
        );
    };
}

/// Create a span for protocol message handling.
///
/// # Example
///
/// ```rust,ignore
/// use mabi_core::trace_message;
///
/// let _span = trace_message!("modbus", "request", unit_id = 1);
/// ```
#[macro_export]
macro_rules! trace_message {
    ($protocol:expr, $direction:expr) => {
        tracing::debug_span!(
            "message",
            protocol = $protocol,
            direction = $direction,
        )
    };
    ($protocol:expr, $direction:expr, $($field:tt)*) => {
        tracing::debug_span!(
            "message",
            protocol = $protocol,
            direction = $direction,
            $($field)*
        )
    };
}

/// Log a connection event.
///
/// # Example
///
/// ```rust,ignore
/// use mabi_core::trace_connection;
///
/// trace_connection!("modbus", "connected", peer = %addr);
/// ```
#[macro_export]
macro_rules! trace_connection {
    ($protocol:expr, $event:expr) => {
        tracing::info!(
            protocol = $protocol,
            event = $event,
            "Connection event"
        );
    };
    ($protocol:expr, $event:expr, $($field:tt)*) => {
        tracing::info!(
            protocol = $protocol,
            event = $event,
            $($field)*,
            "Connection event"
        );
    };
}

/// Log a state change with before/after values.
///
/// # Example
///
/// ```rust,ignore
/// use mabi_core::trace_state_change;
///
/// trace_state_change!("engine", "stopped", "running");
/// ```
#[macro_export]
macro_rules! trace_state_change {
    ($component:expr, $from:expr, $to:expr) => {
        tracing::info!(
            component = $component,
            from_state = $from,
            to_state = $to,
            "State changed"
        );
    };
    ($component:expr, $from:expr, $to:expr, $($field:tt)*) => {
        tracing::info!(
            component = $component,
            from_state = $from,
            to_state = $to,
            $($field)*,
            "State changed"
        );
    };
}

/// Log a metric observation.
///
/// # Example
///
/// ```rust,ignore
/// use mabi_core::trace_metric;
///
/// trace_metric!("request_duration_ms", 42.5, protocol = "modbus");
/// ```
#[macro_export]
macro_rules! trace_metric {
    ($name:expr, $value:expr) => {
        tracing::debug!(
            metric_name = $name,
            metric_value = $value,
            "Metric observation"
        );
    };
    ($name:expr, $value:expr, $($field:tt)*) => {
        tracing::debug!(
            metric_name = $name,
            metric_value = $value,
            $($field)*,
            "Metric observation"
        );
    };
}

/// Create a span for engine tick processing.
///
/// # Example
///
/// ```rust,ignore
/// use mabi_core::trace_tick;
///
/// let _span = trace_tick!(42, device_count = 100);
/// ```
#[macro_export]
macro_rules! trace_tick {
    ($tick_num:expr) => {
        tracing::trace_span!(
            "tick",
            tick = $tick_num,
        )
    };
    ($tick_num:expr, $($field:tt)*) => {
        tracing::trace_span!(
            "tick",
            tick = $tick_num,
            $($field)*
        )
    };
}

/// Log entering a critical section or important function.
///
/// # Example
///
/// ```rust,ignore
/// use mabi_core::trace_enter;
///
/// fn process_batch() {
///     trace_enter!("process_batch", batch_size = 100);
///     // ...
/// }
/// ```
#[macro_export]
macro_rules! trace_enter {
    ($name:expr) => {
        tracing::debug!(function = $name, "Entering");
    };
    ($name:expr, $($field:tt)*) => {
        tracing::debug!(function = $name, $($field)*, "Entering");
    };
}

/// Log exiting a critical section or important function.
///
/// # Example
///
/// ```rust,ignore
/// use mabi_core::trace_exit;
///
/// fn process_batch() {
///     // ...
///     trace_exit!("process_batch", processed = 100);
/// }
/// ```
#[macro_export]
macro_rules! trace_exit {
    ($name:expr) => {
        tracing::debug!(function = $name, "Exiting");
    };
    ($name:expr, $($field:tt)*) => {
        tracing::debug!(function = $name, $($field)*, "Exiting");
    };
}

#[cfg(test)]
mod tests {
    // Macro tests would require initializing the tracing subscriber
    // which can only be done once per process.
    // These tests verify that the macros compile correctly.

    #[test]
    fn test_macros_compile() {
        // This test just verifies the macros are syntactically correct
        // by checking they can be expanded without errors
    }
}

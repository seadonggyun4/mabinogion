//! Middleware for transparent fault injection.

use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::context::{FaultContext, FaultContextBuilder, OperationType, TargetInfo, ResponseData};
use crate::engine::ChaosEngine;
use crate::error::ChaosResult;
use crate::fault::FaultBehavior;
use mabi_core::{DataPoint, Protocol, Value};

// =============================================================================
// Middleware Configuration
// =============================================================================

/// Configuration for chaos middleware.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MiddlewareConfig {
    /// Whether middleware is enabled.
    #[serde(default = "default_true")]
    pub enabled: bool,

    /// Whether to inject faults on reads.
    #[serde(default = "default_true")]
    pub inject_on_read: bool,

    /// Whether to inject faults on writes.
    #[serde(default = "default_true")]
    pub inject_on_write: bool,

    /// Whether to inject faults on lifecycle operations.
    #[serde(default)]
    pub inject_on_lifecycle: bool,

    /// Whether to log all fault applications.
    #[serde(default)]
    pub verbose_logging: bool,
}

fn default_true() -> bool {
    true
}

impl Default for MiddlewareConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            inject_on_read: true,
            inject_on_write: true,
            inject_on_lifecycle: false,
            verbose_logging: false,
        }
    }
}

// =============================================================================
// Middleware Result
// =============================================================================

/// Result of middleware processing.
#[derive(Debug, Clone)]
pub enum MiddlewareResult<T> {
    /// Operation should proceed normally with potential modifications.
    Proceed(T),

    /// Operation should be skipped (no response).
    Skip,

    /// Operation should return an error.
    Error { code: i32, message: String },

    /// Operation was delayed.
    Delayed { delay_ms: u64, result: T },
}

impl<T> MiddlewareResult<T> {
    /// Check if operation should proceed.
    pub fn should_proceed(&self) -> bool {
        matches!(self, Self::Proceed(_) | Self::Delayed { .. })
    }

    /// Get the inner value if present.
    pub fn into_inner(self) -> Option<T> {
        match self {
            Self::Proceed(v) => Some(v),
            Self::Delayed { result, .. } => Some(result),
            _ => None,
        }
    }

    /// Map the inner value.
    pub fn map<U, F: FnOnce(T) -> U>(self, f: F) -> MiddlewareResult<U> {
        match self {
            Self::Proceed(v) => MiddlewareResult::Proceed(f(v)),
            Self::Delayed { delay_ms, result } => MiddlewareResult::Delayed {
                delay_ms,
                result: f(result),
            },
            Self::Skip => MiddlewareResult::Skip,
            Self::Error { code, message } => MiddlewareResult::Error { code, message },
        }
    }
}

// =============================================================================
// Chaos Middleware
// =============================================================================

/// Middleware for transparent fault injection.
///
/// Wraps device operations to apply chaos engineering faults.
///
/// # Example
///
/// ```rust,ignore
/// use mabi_chaos::middleware::ChaosMiddleware;
///
/// let engine = ChaosEngine::builder()
///     .add_fault("latency", latency_fault)
///     .build();
///
/// let middleware = ChaosMiddleware::new(engine);
///
/// // Wrap a read operation
/// let result = middleware.wrap_read("device-001", Protocol::ModbusTcp, "temperature").await;
///
/// match result {
///     MiddlewareResult::Proceed(ctx) => {
///         // Perform actual read, then process response
///         let response = device.read("temperature").await?;
///         let final_result = middleware.process_response(ctx, response).await;
///     }
///     MiddlewareResult::Skip => {
///         // Don't respond
///     }
///     MiddlewareResult::Error { code, message } => {
///         // Return error
///     }
/// }
/// ```
#[derive(Debug, Clone)]
pub struct ChaosMiddleware {
    engine: Arc<ChaosEngine>,
    config: MiddlewareConfig,
}

impl ChaosMiddleware {
    /// Create a new middleware with default config.
    pub fn new(engine: ChaosEngine) -> Self {
        Self {
            engine: Arc::new(engine),
            config: MiddlewareConfig::default(),
        }
    }

    /// Create with specific config.
    pub fn with_config(engine: ChaosEngine, config: MiddlewareConfig) -> Self {
        Self {
            engine: Arc::new(engine),
            config,
        }
    }

    /// Create from existing Arc engine.
    pub fn from_arc(engine: Arc<ChaosEngine>) -> Self {
        Self {
            engine,
            config: MiddlewareConfig::default(),
        }
    }

    /// Get the engine.
    pub fn engine(&self) -> &ChaosEngine {
        &self.engine
    }

    /// Check if middleware is enabled.
    pub fn is_enabled(&self) -> bool {
        self.config.enabled && self.engine.is_running()
    }

    /// Disable the middleware.
    pub fn disable(&mut self) {
        self.config.enabled = false;
    }

    /// Enable the middleware.
    pub fn enable(&mut self) {
        self.config.enabled = true;
    }

    /// Wrap a read operation.
    pub async fn wrap_read(
        &self,
        device_id: &str,
        protocol: Protocol,
        point_id: &str,
    ) -> ChaosResult<MiddlewareResult<FaultContext>> {
        if !self.is_enabled() || !self.config.inject_on_read {
            let ctx = FaultContext::builder()
                .device_id(device_id)
                .read(point_id)
                .protocol(protocol)
                .build();
            return Ok(MiddlewareResult::Proceed(ctx));
        }

        let mut ctx = FaultContext::builder()
            .device_id(device_id)
            .read(point_id)
            .protocol(protocol)
            .build();

        let behavior = self.engine.process(&mut ctx).await?;

        self.behavior_to_result(behavior, ctx)
    }

    /// Wrap a write operation.
    pub async fn wrap_write(
        &self,
        device_id: &str,
        protocol: Protocol,
        point_id: &str,
        value: Value,
    ) -> ChaosResult<MiddlewareResult<FaultContext>> {
        if !self.is_enabled() || !self.config.inject_on_write {
            let ctx = FaultContext::builder()
                .device_id(device_id)
                .write(point_id, value)
                .protocol(protocol)
                .build();
            return Ok(MiddlewareResult::Proceed(ctx));
        }

        let mut ctx = FaultContext::builder()
            .device_id(device_id)
            .write(point_id, value)
            .protocol(protocol)
            .build();

        let behavior = self.engine.process(&mut ctx).await?;

        self.behavior_to_result(behavior, ctx)
    }

    /// Wrap a multi-read operation.
    pub async fn wrap_read_multiple(
        &self,
        device_id: &str,
        protocol: Protocol,
        point_ids: Vec<String>,
    ) -> ChaosResult<MiddlewareResult<FaultContext>> {
        if !self.is_enabled() || !self.config.inject_on_read {
            let ctx = FaultContext::builder()
                .device_id(device_id)
                .operation(OperationType::ReadMultiple { point_ids })
                .protocol(protocol)
                .build();
            return Ok(MiddlewareResult::Proceed(ctx));
        }

        let mut ctx = FaultContext::builder()
            .device_id(device_id)
            .operation(OperationType::ReadMultiple { point_ids })
            .protocol(protocol)
            .build();

        let behavior = self.engine.process(&mut ctx).await?;

        self.behavior_to_result(behavior, ctx)
    }

    /// Wrap a lifecycle operation.
    pub async fn wrap_lifecycle(
        &self,
        device_id: &str,
        protocol: Protocol,
        operation: OperationType,
    ) -> ChaosResult<MiddlewareResult<FaultContext>> {
        if !self.is_enabled() || !self.config.inject_on_lifecycle {
            let ctx = FaultContext::builder()
                .device_id(device_id)
                .operation(operation)
                .protocol(protocol)
                .build();
            return Ok(MiddlewareResult::Proceed(ctx));
        }

        let mut ctx = FaultContext::builder()
            .device_id(device_id)
            .operation(operation)
            .protocol(protocol)
            .build();

        let behavior = self.engine.process(&mut ctx).await?;

        self.behavior_to_result(behavior, ctx)
    }

    /// Process response after the actual operation.
    pub async fn process_response(
        &self,
        mut ctx: FaultContext,
        points: Vec<DataPoint>,
    ) -> ChaosResult<MiddlewareResult<Vec<DataPoint>>> {
        if !self.is_enabled() {
            return Ok(MiddlewareResult::Proceed(points));
        }

        ctx.set_response(ResponseData::from_points(points.clone()));
        self.engine.process_after(&mut ctx).await?;

        // Extract potentially modified points
        let result_points = ctx
            .response_data
            .map(|r| r.data_points)
            .unwrap_or(points);

        if ctx.accumulated_delay_ms > 0 {
            Ok(MiddlewareResult::Delayed {
                delay_ms: ctx.accumulated_delay_ms,
                result: result_points,
            })
        } else {
            Ok(MiddlewareResult::Proceed(result_points))
        }
    }

    /// Convert fault behavior to middleware result.
    fn behavior_to_result(
        &self,
        behavior: FaultBehavior,
        ctx: FaultContext,
    ) -> ChaosResult<MiddlewareResult<FaultContext>> {
        match behavior {
            FaultBehavior::Continue | FaultBehavior::Modify => {
                Ok(MiddlewareResult::Proceed(ctx))
            }

            FaultBehavior::Skip => Ok(MiddlewareResult::Skip),

            FaultBehavior::Abort { error } => Ok(MiddlewareResult::Error {
                code: -1,
                message: error,
            }),

            FaultBehavior::ReturnError { error_code, message } => {
                Ok(MiddlewareResult::Error {
                    code: error_code,
                    message,
                })
            }

            FaultBehavior::Delay { duration_ms } => Ok(MiddlewareResult::Delayed {
                delay_ms: duration_ms,
                result: ctx,
            }),

            FaultBehavior::Retry { .. } => {
                // For middleware, retry is treated as proceed
                Ok(MiddlewareResult::Proceed(ctx))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::network::NetworkLatencyFault;

    #[tokio::test]
    async fn test_middleware_disabled() {
        let engine = ChaosEngine::new();
        let middleware = ChaosMiddleware::new(engine);

        // Engine not started, should pass through
        let result = middleware
            .wrap_read("device-001", Protocol::ModbusTcp, "temp")
            .await
            .unwrap();

        assert!(result.should_proceed());
    }

    #[tokio::test]
    async fn test_middleware_with_fault() {
        let fault = NetworkLatencyFault::builder()
            .id("test")
            .base_ms(10)
            .build();

        let engine = ChaosEngine::builder()
            .add_fault("test", fault)
            .build();

        engine.start().await.unwrap();
        engine.enable_globally("test").await.unwrap();

        let middleware = ChaosMiddleware::new(engine);

        let result = middleware
            .wrap_read("device-001", Protocol::ModbusTcp, "temp")
            .await
            .unwrap();

        // Should have been delayed
        match result {
            MiddlewareResult::Proceed(ctx) | MiddlewareResult::Delayed { result: ctx, .. } => {
                assert!(ctx.was_affected());
            }
            _ => panic!("Expected Proceed or Delayed"),
        }
    }

    #[test]
    fn test_middleware_result_map() {
        let result: MiddlewareResult<i32> = MiddlewareResult::Proceed(42);
        let mapped = result.map(|x| x * 2);

        match mapped {
            MiddlewareResult::Proceed(v) => assert_eq!(v, 84),
            _ => panic!("Expected Proceed"),
        }
    }
}

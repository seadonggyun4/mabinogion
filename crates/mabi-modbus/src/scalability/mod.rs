//! # Scalability Module for High-Volume Processing
//!
//! This module provides infrastructure for handling large-scale Modbus simulations:
//!
//! - **10,000+ concurrent connections** via sharded connection pooling
//! - **100,000+ TPS** via batch request processing
//! - **Backpressure management** via resource limiting
//! - **Performance profiling** for optimization and benchmarking
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────────────────┐
//! │                         Scalability Module                                   │
//! │  ┌────────────────────┐  ┌────────────────────┐  ┌────────────────────────┐ │
//! │  │ ShardedConnPool    │  │ BatchProcessor     │  │ ResourceLimiter        │ │
//! │  │ - 64/128 shards    │  │ - Request batching │  │ - Memory limits        │ │
//! │  │ - Lock-free ops    │  │ - Parallel exec    │  │ - Connection limits    │ │
//! │  │ - O(1) lookup      │  │ - Coalescing       │  │ - Rate limiting        │ │
//! │  └────────────────────┘  └────────────────────┘  └────────────────────────┘ │
//! │                              │                                               │
//! │  ┌────────────────────┐      │                   ┌────────────────────────┐ │
//! │  │ PerformanceProfiler│◄─────┘                   │ ScalabilityConfig      │ │
//! │  │ - Latency tracking │                          │ - All tuning params    │ │
//! │  │ - TPS measurement  │                          │ - Preset profiles      │ │
//! │  │ - Resource usage   │                          │ - Dynamic adjustment   │ │
//! │  └────────────────────┘                          └────────────────────────┘ │
//! └─────────────────────────────────────────────────────────────────────────────┘
//! ```
//!
//! ## Usage
//!
//! ```rust,ignore
//! use mabi_modbus::scalability::{
//!     ScalabilityConfig, ShardedConnectionPool, BatchProcessor,
//!     ResourceLimiter, PerformanceProfiler,
//! };
//!
//! // Use preset for 10K connections
//! let config = ScalabilityConfig::for_connections(10_000);
//!
//! // Create sharded connection pool
//! let pool = ShardedConnectionPool::new(config.connection_pool);
//!
//! // Create batch processor for high TPS
//! let processor = BatchProcessor::new(config.batch);
//!
//! // Create resource limiter for backpressure
//! let limiter = ResourceLimiter::new(config.resources);
//! ```
//!
//! ## Performance Targets
//!
//! | Metric              | Target       | Critical     |
//! |---------------------|--------------|--------------|
//! | Concurrent Conns    | 10,000+      | 50,000+      |
//! | TPS (throughput)    | 100,000+     | 500,000+     |
//! | p99 Latency         | < 10ms       | < 50ms       |
//! | Memory (10K dev)    | < 2GB        | < 8GB        |

pub mod config;
pub mod connection_pool;
pub mod batch_processor;
pub mod resource_limiter;
pub mod profiler;

// Re-exports
pub use config::{
    ScalabilityConfig, ConnectionPoolConfig, BatchProcessorConfig,
    ResourceLimiterConfig, ProfilerConfig, ScalabilityPreset,
    BackpressureStrategyConfig, ConfigError,
};
pub use connection_pool::{
    ShardedConnectionPool, ShardedConnectionInfo, ConnectionShard,
    ConnectionHandle, PoolStatistics,
};
pub use batch_processor::{
    BatchProcessor, BatchRequest, BatchResponse, BatchConfig,
    ProcessingStrategy, BatchHandler, BatchError, BatchStatistics,
    RequestPriority, RequestId,
};
pub use config::CoalescingConfig;
pub use resource_limiter::{
    ResourceLimiter, ResourceType, LimitResult, BackpressureStrategy,
    ResourceSnapshot, LimiterStatistics,
};
pub use profiler::{
    PerformanceProfiler, ProfileSnapshot, LatencyHistogram, HistogramSnapshot,
    ThroughputCounter, ResourceUsage, ProfileReport, PerformanceSummary,
};

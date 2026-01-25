//! Testing utilities and performance validation framework.
//!
//! This module provides comprehensive testing infrastructure for:
//! - Large-scale performance validation (10K+ connections, 100K+ TPS)
//! - Memory profiling and leak detection
//! - Scalability benchmarking
//! - Stress testing helpers
//!
//! # Example
//!
//! ```rust,ignore
//! use mabi_modbus::testing::{
//!     PerformanceValidator, ScaleTest, MemoryProfiler, TestReport
//! };
//!
//! #[tokio::test]
//! async fn test_large_scale() {
//!     let validator = PerformanceValidator::new()
//!         .target_connections(10_000)
//!         .target_tps(100_000)
//!         .duration(Duration::from_secs(60));
//!
//!     let report = validator.run().await.unwrap();
//!     assert!(report.passed());
//! }
//! ```

pub mod performance;
pub mod memory;
pub mod load_generator;
pub mod report;

pub use performance::{
    PerformanceValidator, PerformanceConfig, PerformanceTarget, ValidationResult,
};
pub use memory::{MemoryProfiler, MemorySnapshot, MemoryReport, AllocationTracker};
pub use load_generator::{LoadGenerator, LoadConfig, LoadPattern, ConnectionSimulator};
pub use report::{TestReport, TestMetrics, TestSummary};

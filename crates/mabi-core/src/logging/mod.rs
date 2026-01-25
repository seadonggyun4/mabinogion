//! Logging and tracing infrastructure.
//!
//! This module provides a unified logging and tracing setup for the simulator.
//! It supports multiple output formats (JSON, Pretty) and destinations (stdout, file).
//!
//! ## Features
//!
//! - **Multiple output formats**: JSON, Pretty, Compact, Full
//! - **Multiple output targets**: stdout, stderr, file with rotation
//! - **Log rotation**: Time-based and size-based rotation
//! - **Dynamic log level**: Change log level at runtime
//! - **Structured context**: Request tracing with correlation IDs
//! - **OpenTelemetry ready**: Integration layer for distributed tracing
//!
//! # Example
//!
//! ```rust,no_run
//! use mabi_core::logging::{init_logging, LogConfig, LogFormat, LogLevel};
//!
//! // Initialize with default settings
//! init_logging(&LogConfig::default()).expect("Failed to initialize logging");
//!
//! // Or with custom configuration
//! let config = LogConfig::builder()
//!     .level(LogLevel::Debug)
//!     .format(LogFormat::Json)
//!     .build();
//! init_logging(&config).expect("Failed to initialize logging");
//! ```
//!
//! # File Logging with Rotation
//!
//! ```rust,ignore
//! use mabi_core::logging::{LogConfig, LogTarget, RotationConfig, RotationStrategy};
//!
//! let config = LogConfig::builder()
//!     .target(LogTarget::File {
//!         directory: "/var/log/trap-simulator".into(),
//!         filename_prefix: "simulator".into(),
//!         rotation: RotationConfig {
//!             strategy: RotationStrategy::Daily,
//!             max_files: Some(30),
//!         },
//!     })
//!     .build();
//! ```

mod config;
mod context;
mod dynamic;
mod init;
mod macros;
mod rotation;

pub use config::*;
pub use context::*;
pub use dynamic::*;
pub use init::*;
pub use rotation::*;

// Re-export macros at module level
pub use crate::{trace_device, trace_error, trace_request, trace_success};

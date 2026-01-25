//! # mabi-cli
//!
//! CLI framework for the OT Protocol Simulator.
//!
//! This crate provides a highly extensible CLI framework with:
//! - Trait-based command abstraction
//! - Protocol-agnostic command execution
//! - Rich terminal output with progress indicators
//! - Configurable output formats (table, JSON, YAML)

pub mod commands;
pub mod context;
pub mod error;
pub mod output;
pub mod prelude;
pub mod runner;

pub use context::{CliContext, CliContextBuilder};
pub use error::{CliError, CliResult};
pub use output::{OutputFormat, OutputWriter};
pub use runner::CommandRunner;

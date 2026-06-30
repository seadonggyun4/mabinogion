//! # mabi-cli
//!
//! Installed command surface for the Mabinogion protocol resilience engine.
//!
//! This crate provides local operator commands and runner-facing contracts for:
//! - Protocol/session execution through `mabi serve`
//! - Installed binary smoke checks through `mabi doctor`
//! - Runtime, schema, and version inspection
//! - Scenario/config validation with machine-readable envelopes
//! - Stable Imugi and Trials integration metadata

pub mod commands;
pub mod context;
pub mod error;
pub mod output;
pub mod prelude;
pub mod runner;
pub mod runner_contract;
pub mod runtime_registry;
pub mod validation;

pub use context::{CliContext, CliContextBuilder};
pub use error::{CliError, CliResult};
pub use output::{OutputFormat, OutputWriter};
pub use runner::CommandRunner;
pub use runner_contract::{CliOutputEnvelope, LOCAL_RUNNER_CONTRACT_VERSION};

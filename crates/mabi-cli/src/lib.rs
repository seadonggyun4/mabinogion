#![allow(clippy::ptr_arg, clippy::should_implement_trait)] // Preserve existing CLI helper APIs.

//! # mabi-cli
//!
//! Installed command surface for the Mabinogion protocol resilience engine.
//!
//! This crate provides local operator commands and runner-facing contracts for:
//! - Protocol/session execution through `mabi serve`
//! - Installed binary smoke checks through `mabi doctor`
//! - Runtime, schema, and version inspection
//! - Scenario/config validation with machine-readable envelopes
//! - Stable Trials and external consumers integration metadata

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

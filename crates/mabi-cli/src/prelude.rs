//! Prelude module for convenient imports.
//!
//! This module re-exports commonly used types for CLI development.

pub use crate::commands::{
    BacnetCommand, DoctorCommand, DoctorProtocol, KnxCommand, ListCommand, ModbusCommand,
    OpcuaCommand, ProtocolCommand, RunCommand, ValidateCommand,
};
pub use crate::context::{CliContext, CliContextBuilder};
pub use crate::error::{CliError, CliResult, CliResultExt};
pub use crate::output::{OutputFormat, OutputWriter, StatusType, TableBuilder};
pub use crate::runner::{
    Command, CommandHook, CommandOutput, CommandRunner, LoggingHook, MetricsHook,
};

// Re-export from core
pub use mabi_core::prelude::*;

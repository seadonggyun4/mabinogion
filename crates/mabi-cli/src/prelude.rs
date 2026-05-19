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
pub use crate::runner_contract::{
    is_machine_format, write_failure, write_success, CliErrorPayload, CliExitCategory,
    CliOutputEnvelope, LOCAL_RUNNER_CONTRACT_VERSION,
};

// Re-export from core
pub use mabi_core::prelude::*;

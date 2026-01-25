//! Modbus function code handlers.
//!
//! This module provides a modular, extensible architecture for handling
//! Modbus function codes. Each handler implements the `FunctionHandler` trait
//! and can be registered with the `HandlerRegistry`.
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────┐
//! │                    HandlerRegistry                       │
//! │  ┌──────────────────────────────────────────────────┐   │
//! │  │ FC01 → ReadCoilsHandler                          │   │
//! │  │ FC02 → ReadDiscreteInputsHandler                 │   │
//! │  │ FC03 → ReadHoldingRegistersHandler               │   │
//! │  │ FC04 → ReadInputRegistersHandler                 │   │
//! │  │ FC05 → WriteSingleCoilHandler                    │   │
//! │  │ FC06 → WriteSingleRegisterHandler                │   │
//! │  │ FC0F → WriteMultipleCoilsHandler                 │   │
//! │  │ FC10 → WriteMultipleRegistersHandler             │   │
//! │  │ FC17 → ReadWriteMultipleRegistersHandler         │   │
//! │  │ ... → CustomHandler (extensible)                 │   │
//! │  └──────────────────────────────────────────────────┘   │
//! └─────────────────────────────────────────────────────────┘
//! ```
//!
//! # Example
//!
//! ```rust,ignore
//! use mabi_modbus::handler::{HandlerRegistry, FunctionHandler, HandlerContext};
//!
//! // Create registry with default handlers
//! let registry = HandlerRegistry::with_defaults();
//!
//! // Or create custom registry
//! let mut registry = HandlerRegistry::new();
//! registry.register(Box::new(MyCustomHandler));
//!
//! // Dispatch request
//! let response = registry.dispatch(function_code, &pdu, &context)?;
//! ```

mod exception;
mod read;
mod registry;
mod traits;
mod write;

pub use exception::{build_exception_pdu, ExceptionCode, ExceptionResponse};
pub use read::{
    ReadCoilsHandler, ReadDiscreteInputsHandler, ReadHoldingRegistersHandler,
    ReadInputRegistersHandler,
};
pub use registry::HandlerRegistry;
pub use traits::{FunctionHandler, FunctionHandlerExt, HandlerContext, HandlerResult};
pub use write::{
    ReadWriteMultipleRegistersHandler, WriteMultipleCoilsHandler, WriteMultipleRegistersHandler,
    WriteSingleCoilHandler, WriteSingleRegisterHandler,
};

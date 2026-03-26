//! High-performance register storage for large-scale Modbus simulation.
//!
//! This module provides a sparse, memory-efficient register storage system
//! designed for simulating 1,000,000+ data points across 10,000+ devices.
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────┐
//! │                      SparseRegisterStore                        │
//! │  ┌─────────────────────────────────────────────────────────┐   │
//! │  │                  RegisterStoreConfig                     │   │
//! │  │  • Address ranges per type                               │   │
//! │  │  • Initialization mode (Zero/Random/Pattern/Lazy)        │   │
//! │  │  • Default values                                        │   │
//! │  └─────────────────────────────────────────────────────────┘   │
//! │                                                                 │
//! │  ┌─────────────────────────────────────────────────────────┐   │
//! │  │              Sparse Storage (DashMap)                    │   │
//! │  │  Coils: DashMap<u16, bool>                               │   │
//! │  │  Discrete Inputs: DashMap<u16, bool>                     │   │
//! │  │  Holding Registers: DashMap<u16, u16>                    │   │
//! │  │  Input Registers: DashMap<u16, u16>                      │   │
//! │  └─────────────────────────────────────────────────────────┘   │
//! │                                                                 │
//! │  ┌─────────────────────────────────────────────────────────┐   │
//! │  │                   Callback System                        │   │
//! │  │  ReadCallback: on_read(type, addr, value)                │   │
//! │  │  WriteCallback: on_write(type, addr, old, new)           │   │
//! │  └─────────────────────────────────────────────────────────┘   │
//! └─────────────────────────────────────────────────────────────────┘
//! ```
//!
//! ## Features
//!
//! - **Sparse Storage**: Only accessed registers are stored in memory
//! - **High Concurrency**: DashMap provides lock-free concurrent access
//! - **Observable**: Callback system for scenario engine integration
//! - **Configurable**: Flexible initialization modes and address ranges
//! - **Memory Efficient**: Estimated 3-24 bytes per accessed register
//!
//! ## Example
//!
//! ```rust
//! use mabi_modbus::registers::{
//!     SparseRegisterStore, RegisterStoreConfig, InitializationMode,
//! };
//!
//! // Create with default config
//! let store = SparseRegisterStore::new(RegisterStoreConfig::default());
//!
//! // Write and read registers
//! store.write_holding_register(0, 12345).unwrap();
//! let values = store.read_holding_registers(0, 1).unwrap();
//! assert_eq!(values[0], 12345);
//!
//! // Check memory usage
//! println!("Memory usage: {} bytes", store.memory_usage());
//! ```

mod callback;
mod config;
mod sparse_store;
mod types;
mod value;

// Re-exports
pub use callback::{
    CallbackManager, CallbackPriority, ReadCallback, ReadCallbackFn, WriteCallback, WriteCallbackFn,
};
pub use config::{
    AddressRange, DefaultValue, InitializationMode, RegisterRangeConfig, RegisterStoreConfig,
};
pub use sparse_store::SparseRegisterStore;
pub use types::RegisterType;
pub use value::RegisterValue;

#[cfg(test)]
mod tests;

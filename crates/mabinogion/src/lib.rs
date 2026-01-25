//! # Mabinogion
//!
//! Industrial protocol simulator for stress testing, edge case validation,
//! and large-scale data processing verification.
//!
//! ## Overview
//!
//! Mabinogion provides virtual device simulators for industrial protocols:
//!
//! - **Modbus TCP/RTU**: PLC and industrial device simulation
//! - **OPC UA**: Industrial automation server simulation
//! - **BACnet/IP**: Building automation simulation
//! - **KNXnet/IP**: Home and building automation simulation
//!
//! ## Features
//!
//! - **Large-scale simulation**: 10,000+ virtual devices, 1,000,000+ data points
//! - **Protocol accuracy**: Faithful reproduction of real industrial environments
//! - **Chaos engineering**: Network failures, device errors, latency simulation
//! - **Reproducible testing**: Consistent test results with scenario-based testing
//!
//! ## Quick Start
//!
//! ```rust,no_run
//! use mabinogion::prelude::*;
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     // Example: Create a Modbus TCP server
//!     // let server = ModbusTcpServer::builder()
//!     //     .port(502)
//!     //     .devices(100)
//!     //     .build()?;
//!     // server.start().await?;
//!     Ok(())
//! }
//! ```
//!
//! ## Feature Flags
//!
//! - `modbus` - Enable Modbus TCP/RTU simulator (default)
//! - `opcua` - Enable OPC UA server simulator (default)
//! - `bacnet` - Enable BACnet/IP simulator (default)
//! - `knx` - Enable KNXnet/IP simulator (default)
//! - `scenario` - Enable scenario engine (default)
//! - `chaos` - Enable chaos engineering module (default)
//! - `full` - Enable all features

#![doc(html_root_url = "https://docs.rs/mabinogion/0.1.0")]
#![warn(missing_docs)]
#![warn(rust_2018_idioms)]

// Re-export core
pub use mabi_core as core;

// Re-export protocol simulators
#[cfg(feature = "modbus")]
pub use mabi_modbus as modbus;

#[cfg(feature = "opcua")]
pub use mabi_opcua as opcua;

#[cfg(feature = "bacnet")]
pub use mabi_bacnet as bacnet;

#[cfg(feature = "knx")]
pub use mabi_knx as knx;

// Re-export engines
#[cfg(feature = "scenario")]
pub use mabi_scenario as scenario;

#[cfg(feature = "chaos")]
pub use mabi_chaos as chaos;

/// Prelude module for convenient imports
pub mod prelude {
    //! Commonly used types and traits.
    //!
    //! ```rust
    //! use mabinogion::prelude::*;
    //! ```

    // Core types (always available)
    pub use mabi_core::prelude::*;

    // BACnet prelude
    #[cfg(feature = "bacnet")]
    pub use mabi_bacnet::prelude::*;

    // KNX prelude
    #[cfg(feature = "knx")]
    pub use mabi_knx::prelude::*;

    // Scenario prelude
    #[cfg(feature = "scenario")]
    pub use mabi_scenario::prelude::*;

    // Chaos prelude (selected types to avoid conflicts)
    #[cfg(feature = "chaos")]
    pub use mabi_chaos::prelude::{
        ChaosEngine, ChaosEngineBuilder, ChaosMiddleware, FaultContext,
        FaultRegistry,
    };
}

/// Version information
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Package name
pub const NAME: &str = env!("CARGO_PKG_NAME");

//! Multi-Unit Management for Modbus Simulation.
//!
//! This module provides the `MultiUnitManager` for managing multiple Modbus
//! slave units, each with its own register store and configuration.
//!
//! # Architecture
//!
//! ```text
//! ┌────────────────────────────────────────────────────────────────────────────┐
//! │                         MultiUnitManager                                    │
//! │  ┌──────────────────────────────────────────────────────────────────────┐  │
//! │  │                      UnitManagerConfig                                │  │
//! │  │  • Default word order   • Broadcast mode   • Max units               │  │
//! │  └──────────────────────────────────────────────────────────────────────┘  │
//! │                                                                             │
//! │  ┌──────────────────────────────────────────────────────────────────────┐  │
//! │  │                    Unit Store (DashMap<u8, UnitInfo>)                 │  │
//! │  │  Unit 1: { registers: SparseStore, config: UnitConfig, converter }   │  │
//! │  │  Unit 2: { registers: SparseStore, config: UnitConfig, converter }   │  │
//! │  │  Unit N: { registers: SparseStore, config: UnitConfig, converter }   │  │
//! │  └──────────────────────────────────────────────────────────────────────┘  │
//! │                                                                             │
//! │  ┌──────────────────────────────────────────────────────────────────────┐  │
//! │  │                      Broadcast Handler (Unit 0)                       │  │
//! │  │  • Write to all units   • No response expected   • Configurable      │  │
//! │  └──────────────────────────────────────────────────────────────────────┘  │
//! └────────────────────────────────────────────────────────────────────────────┘
//! ```
//!
//! # Example
//!
//! ```rust
//! use mabi_modbus::unit::{MultiUnitManager, UnitConfig, UnitManagerConfig};
//! use mabi_modbus::types::WordOrder;
//!
//! // Create manager with default configuration
//! let mut manager = MultiUnitManager::new(UnitManagerConfig::default());
//!
//! // Add units
//! manager.add_unit(1, UnitConfig::new("Pump Controller"));
//! manager.add_unit(2, UnitConfig::new("Temperature Sensor"));
//! manager.add_unit(3, UnitConfig::with_word_order("VFD Drive", WordOrder::BigEndianWordSwap));
//!
//! // Get unit's register store
//! if let Some(unit) = manager.get_unit(1) {
//!     unit.registers().write_holding_register(0, 1000).unwrap();
//! }
//!
//! // Broadcast to all units (Unit ID 0)
//! manager.broadcast_write_holding_register(0, 0xFF00);
//! ```

mod config;
mod manager;

pub use crate::context::BroadcastPolicy;
pub use config::{BroadcastMode, UnitConfig, UnitManagerConfig};
pub use manager::{MultiUnitManager, UnitInfo};

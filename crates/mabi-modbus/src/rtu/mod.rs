//! Modbus RTU simulation module.
//!
//! This module provides Modbus RTU (Remote Terminal Unit) protocol simulation,
//! including:
//!
//! - **RTU framing**: CRC-16 calculation and frame encoding/decoding
//! - **Virtual serial ports**: PTY-based serial port simulation (Unix)
//! - **Timing simulation**: Baud rate and inter-character delays
//! - **Transport abstraction**: Unified interface for serial and other transports
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │                     ModbusRtuServer                         │
//! │  ┌──────────────────┐  ┌────────────────┐                   │
//! │  │ TransportManager │──│ HandlerRegistry│                   │
//! │  └────────┬─────────┘  └────────────────┘                   │
//! │           │                                                  │
//! │  ┌────────┴─────────────────────────────────┐               │
//! │  │           Transport Trait                 │               │
//! │  │  VirtualSerial  │  TcpBridge  │  Custom  │               │
//! │  └──────────────────────────────────────────┘               │
//! │                           │                                  │
//! │           ┌───────────────┴───────────────────┐             │
//! │           │          RtuCodec                  │             │
//! │           │  Frame Detection │ CRC Validation │             │
//! │           └────────────────────────────────────┘             │
//! └─────────────────────────────────────────────────────────────┘
//! ```
//!
//! # RTU Frame Format
//!
//! ```text
//! ┌──────────┬───────────────┬─────────────┬──────────┐
//! │ Unit ID  │ Function Code │ Data        │ CRC-16   │
//! │ (1 byte) │ (1 byte)      │ (N bytes)   │ (2 bytes)│
//! └──────────┴───────────────┴─────────────┴──────────┘
//! ```
//!
//! # Timing Requirements
//!
//! According to Modbus specification:
//! - **Inter-frame gap**: 3.5 character times (silence between frames)
//! - **Inter-character gap**: 1.5 character times max (within a frame)
//!
//! For example, at 9600 baud (11 bits per character):
//! - Character time ≈ 1.145 ms
//! - Inter-frame gap ≈ 4 ms
//! - Max inter-character gap ≈ 1.7 ms
//!
//! # Example
//!
//! ```rust,no_run
//! use mabi_modbus::rtu::{RtuServerConfig, ModbusRtuServer};
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     // Configure RTU server
//!     let server_config = RtuServerConfig::default()
//!         .with_unit_ids(vec![1, 2, 3])
//!         .with_broadcast(true);
//!
//!     let server = ModbusRtuServer::new(server_config);
//!     // server.run().await?; // Would run the server
//!     Ok(())
//! }
//! ```

pub mod codec;
pub mod frame;
pub mod serial;
pub mod server;
pub mod transport;

// Re-exports
pub use codec::{RtuCodec, RtuTiming, StreamingRtuCodec};
pub use frame::{RtuFrame, RtuFrameError, calculate_crc, verify_crc, pack_bits, unpack_bits};
pub use serial::{VirtualSerialConfig, VirtualSerial, SerialConfig, Parity, StopBits, DataBits};
pub use server::{ModbusRtuServer, RtuServerConfig, RtuServerEvent, RtuServerState, RtuServerStats};
pub use transport::{RtuTransport, TransportConfig, TransportType, ChannelTransport, TransportMetrics};

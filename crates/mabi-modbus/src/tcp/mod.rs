//! Modbus TCP server components.
//!
//! This module provides a modular, high-performance Modbus TCP server implementation
//! with the following features:
//!
//! - Connection pooling with limits and tracking
//! - MBAP (Modbus Application Protocol) frame codec
//! - Metrics collection for monitoring
//! - Graceful shutdown support
//! - Extensible handler architecture
//!
//! # Architecture
//!
//! ```text
//! ┌────────────────────────────────────────────────────────────┐
//! │                    ModbusTcpServer                          │
//! │  ┌──────────────┐  ┌──────────────┐  ┌──────────────────┐  │
//! │  │ TcpListener  │──│ConnectionPool│──│  HandlerRegistry │  │
//! │  └──────────────┘  └──────────────┘  └──────────────────┘  │
//! │         │                 │                    │            │
//! │         ▼                 ▼                    ▼            │
//! │  ┌──────────────────────────────────────────────────────┐  │
//! │  │                  Connection Handlers                  │  │
//! │  │  ┌────────┐  ┌────────┐  ┌────────┐  ┌────────┐     │  │
//! │  │  │Conn #1 │  │Conn #2 │  │Conn #3 │  │  ...   │     │  │
//! │  │  └────────┘  └────────┘  └────────┘  └────────┘     │  │
//! │  │       │            │            │            │        │  │
//! │  │       ▼            ▼            ▼            ▼        │  │
//! │  │  ┌──────────────────────────────────────────────┐   │  │
//! │  │  │              MbapCodec (framing)              │   │  │
//! │  │  └──────────────────────────────────────────────┘   │  │
//! │  └──────────────────────────────────────────────────────┘  │
//! │                                                             │
//! │  ┌──────────────────────────────────────────────────────┐  │
//! │  │                    ServerMetrics                      │  │
//! │  │  connections_total | requests_total | errors_total   │  │
//! │  │  request_duration  | active_connections              │  │
//! │  └──────────────────────────────────────────────────────┘  │
//! └────────────────────────────────────────────────────────────┘
//! ```

mod codec;
mod connection;
mod metrics;
mod server;

pub use codec::{MbapCodec, MbapFrame, MbapHeader};
pub use connection::{ConnectionInfo, ConnectionPool, ConnectionState};
pub use metrics::ServerMetrics;
pub use server::{ModbusTcpServerV2, PerformancePreset, ServerConfigV2};

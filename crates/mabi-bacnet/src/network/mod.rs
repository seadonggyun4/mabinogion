//! BACnet/IP network layer implementation.
//!
//! This module provides the complete BACnet/IP network stack:
//! - UDP socket management for BACnet/IP communication
//! - BVLC (BACnet Virtual Link Control) protocol
//! - NPDU (Network Protocol Data Unit) handling
//! - BBMD (BACnet Broadcast Management Device) support
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │                    Network Layer                            │
//! │                                                             │
//! │  ┌───────────┐  ┌───────────┐  ┌───────────┐  ┌──────────┐ │
//! │  │    UDP    │  │   BVLC    │  │   NPDU    │  │   BBMD   │ │
//! │  │  Socket   │──│  Protocol │──│  Handler  │──│  Manager │ │
//! │  └───────────┘  └───────────┘  └───────────┘  └──────────┘ │
//! └─────────────────────────────────────────────────────────────┘
//! ```

pub mod bbmd;
pub mod bvlc;
pub mod npdu;
pub mod udp;

pub use bbmd::{
    Bbmd, BbmdConfig, BbmdError,
    BroadcastDistributionTable, BdtEntry,
    ForeignDeviceTable, FdtEntry,
};
pub use bvlc::{BvlcError, BvlcFunction, BvlcMessage, BvlcResultCode, BVLC_TYPE};
pub use npdu::{BACnetAddress, Npdu, NpduControl, NpduError, Priority, NPDU_VERSION};
pub use udp::{BACnetNetwork, IncomingPacket, NetworkConfig, NetworkHandle};

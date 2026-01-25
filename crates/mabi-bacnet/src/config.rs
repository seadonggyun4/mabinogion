//! BACnet configuration types.

use serde::{Deserialize, Serialize};
use std::net::SocketAddr;

/// BACnet/IP server configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BacnetServerConfig {
    /// Bind address.
    #[serde(default = "default_bind_address")]
    pub bind_address: SocketAddr,

    /// Device instance number.
    #[serde(default = "default_device_instance")]
    pub device_instance: u32,

    /// Device name.
    #[serde(default = "default_device_name")]
    pub device_name: String,

    /// Vendor ID.
    #[serde(default = "default_vendor_id")]
    pub vendor_id: u16,

    /// Enable BBMD.
    #[serde(default)]
    pub enable_bbmd: bool,
}

fn default_bind_address() -> SocketAddr {
    "0.0.0.0:47808".parse().unwrap()
}

fn default_device_instance() -> u32 {
    1234
}

fn default_device_name() -> String {
    "TRAP Simulator BACnet Device".to_string()
}

fn default_vendor_id() -> u16 {
    999 // Example vendor ID
}

impl Default for BacnetServerConfig {
    fn default() -> Self {
        Self {
            bind_address: default_bind_address(),
            device_instance: default_device_instance(),
            device_name: default_device_name(),
            vendor_id: default_vendor_id(),
            enable_bbmd: false,
        }
    }
}

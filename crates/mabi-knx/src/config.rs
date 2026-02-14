//! KNX configuration types.
//!
//! This module provides configuration structures for KNXnet/IP server and devices.

use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::address::IndividualAddress;
use crate::error_tracker::SendErrorTrackerConfig;
use crate::filter::FilterChainConfig;
use crate::group_cache::GroupValueCacheConfig;
use crate::heartbeat::HeartbeatSchedulerConfig;

// ============================================================================
// Server Configuration
// ============================================================================

/// KNXnet/IP server configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnxServerConfig {
    /// UDP bind address for KNXnet/IP.
    #[serde(default = "default_bind_addr")]
    pub bind_addr: SocketAddr,

    /// Multicast address for KNX routing.
    #[serde(default = "default_multicast_addr")]
    pub multicast_addr: SocketAddr,

    /// Individual address of this KNX device.
    #[serde(default = "default_individual_address")]
    pub individual_address: IndividualAddress,

    /// Device name for discovery.
    #[serde(default = "default_device_name")]
    pub device_name: String,

    /// Device serial number.
    #[serde(default = "default_serial_number")]
    pub serial_number: [u8; 6],

    /// MAC address (for HPAI).
    #[serde(default = "default_mac_address")]
    pub mac_address: [u8; 6],

    /// Maximum number of simultaneous tunnel connections.
    #[serde(default = "default_max_connections")]
    pub max_connections: usize,

    /// Connection heartbeat interval in seconds.
    #[serde(default = "default_heartbeat_interval_secs")]
    pub heartbeat_interval_secs: u64,

    /// Connection timeout in seconds.
    #[serde(default = "default_connection_timeout_secs")]
    pub connection_timeout_secs: u64,

    /// Enable routing mode.
    #[serde(default)]
    pub routing_enabled: bool,

    /// Enable tunneling mode.
    #[serde(default = "default_true")]
    pub tunneling_enabled: bool,

    /// Enable device management mode.
    #[serde(default)]
    pub device_management_enabled: bool,

    /// Tunnel behavior configuration for protocol simulation fidelity.
    #[serde(default)]
    pub tunnel_behavior: TunnelBehaviorConfig,
}

fn default_bind_addr() -> SocketAddr {
    SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 3671))
}

fn default_multicast_addr() -> SocketAddr {
    // KNX standard multicast address: 224.0.23.12:3671
    SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::new(224, 0, 23, 12), 3671))
}

fn default_individual_address() -> IndividualAddress {
    IndividualAddress::new(1, 1, 1)
}

fn default_device_name() -> String {
    "OTSim KNX Simulator".to_string()
}

fn default_serial_number() -> [u8; 6] {
    [0x00, 0x00, 0x00, 0x00, 0x00, 0x01]
}

fn default_mac_address() -> [u8; 6] {
    [0x00, 0x00, 0x00, 0x00, 0x00, 0x01]
}

fn default_max_connections() -> usize {
    256
}

fn default_heartbeat_interval_secs() -> u64 {
    60
}

fn default_connection_timeout_secs() -> u64 {
    120
}

fn default_true() -> bool {
    true
}

impl Default for KnxServerConfig {
    fn default() -> Self {
        Self {
            bind_addr: default_bind_addr(),
            multicast_addr: default_multicast_addr(),
            individual_address: default_individual_address(),
            device_name: default_device_name(),
            serial_number: default_serial_number(),
            mac_address: default_mac_address(),
            max_connections: default_max_connections(),
            heartbeat_interval_secs: default_heartbeat_interval_secs(),
            connection_timeout_secs: default_connection_timeout_secs(),
            routing_enabled: false,
            tunneling_enabled: true,
            device_management_enabled: false,
            tunnel_behavior: TunnelBehaviorConfig::default(),
        }
    }
}

impl KnxServerConfig {
    /// Create a new server config with the specified bind address.
    pub fn with_bind_addr(mut self, addr: SocketAddr) -> Self {
        self.bind_addr = addr;
        self
    }

    /// Set the individual address.
    pub fn with_individual_address(mut self, addr: IndividualAddress) -> Self {
        self.individual_address = addr;
        self
    }

    /// Set the device name.
    pub fn with_device_name(mut self, name: impl Into<String>) -> Self {
        self.device_name = name.into();
        self
    }

    /// Set maximum connections.
    pub fn with_max_connections(mut self, max: usize) -> Self {
        self.max_connections = max;
        self
    }

    /// Enable routing mode.
    pub fn with_routing(mut self, enabled: bool) -> Self {
        self.routing_enabled = enabled;
        self
    }

    /// Get heartbeat interval as Duration.
    pub fn heartbeat_interval(&self) -> Duration {
        Duration::from_secs(self.heartbeat_interval_secs)
    }

    /// Get connection timeout as Duration.
    pub fn connection_timeout(&self) -> Duration {
        Duration::from_secs(self.connection_timeout_secs)
    }

    /// Validate configuration.
    pub fn validate(&self) -> Result<(), String> {
        if self.max_connections == 0 {
            return Err("max_connections must be greater than 0".to_string());
        }
        if self.heartbeat_interval_secs == 0 {
            return Err("heartbeat_interval_secs must be greater than 0".to_string());
        }
        if self.device_name.is_empty() {
            return Err("device_name cannot be empty".to_string());
        }
        if self.device_name.len() > 30 {
            return Err("device_name cannot exceed 30 characters".to_string());
        }
        Ok(())
    }
}

// ============================================================================
// Device Configuration
// ============================================================================

/// KNX device configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnxDeviceConfig {
    /// Device ID.
    pub id: String,

    /// Device name.
    pub name: String,

    /// Device description.
    #[serde(default)]
    pub description: String,

    /// Individual address.
    pub individual_address: IndividualAddress,

    /// Group objects configuration.
    #[serde(default)]
    pub group_objects: Vec<GroupObjectConfig>,

    /// Simulation tick interval in milliseconds.
    #[serde(default = "default_tick_interval_ms")]
    pub tick_interval_ms: u64,
}

fn default_tick_interval_ms() -> u64 {
    100
}

impl Default for KnxDeviceConfig {
    fn default() -> Self {
        Self {
            id: "knx-device-1".to_string(),
            name: "KNX Device".to_string(),
            description: String::new(),
            individual_address: IndividualAddress::new(1, 1, 1),
            group_objects: Vec::new(),
            tick_interval_ms: default_tick_interval_ms(),
        }
    }
}

impl KnxDeviceConfig {
    /// Create a new device config.
    pub fn new(id: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            ..Default::default()
        }
    }

    /// Set the individual address.
    pub fn with_individual_address(mut self, addr: IndividualAddress) -> Self {
        self.individual_address = addr;
        self
    }

    /// Add a group object.
    pub fn with_group_object(mut self, obj: GroupObjectConfig) -> Self {
        self.group_objects.push(obj);
        self
    }

    /// Get tick interval as Duration.
    pub fn tick_interval(&self) -> Duration {
        Duration::from_millis(self.tick_interval_ms)
    }
}

// ============================================================================
// Group Object Configuration
// ============================================================================

/// Group object configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroupObjectConfig {
    /// Group address (e.g., "1/2/3").
    pub address: String,

    /// Object name.
    pub name: String,

    /// Datapoint type (e.g., "DPT1.001", "DPT9.001").
    pub dpt: String,

    /// Object flags.
    #[serde(default)]
    pub flags: GroupObjectFlagsConfig,

    /// Initial value (JSON).
    #[serde(default)]
    pub initial_value: Option<serde_json::Value>,
}

/// Group object flags configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroupObjectFlagsConfig {
    /// Communication enabled.
    #[serde(default = "default_true")]
    pub communication: bool,

    /// Read enabled.
    #[serde(default = "default_true")]
    pub read: bool,

    /// Write enabled.
    #[serde(default = "default_true")]
    pub write: bool,

    /// Transmit on change.
    #[serde(default = "default_true")]
    pub transmit: bool,

    /// Update on receive.
    #[serde(default = "default_true")]
    pub update: bool,
}

impl Default for GroupObjectFlagsConfig {
    fn default() -> Self {
        Self {
            communication: true,
            read: true,
            write: true,
            transmit: true,
            update: true,
        }
    }
}

impl GroupObjectFlagsConfig {
    /// Create read-only flags.
    pub fn read_only() -> Self {
        Self {
            communication: true,
            read: true,
            write: false,
            transmit: true,
            update: false,
        }
    }

    /// Create write-only flags.
    pub fn write_only() -> Self {
        Self {
            communication: true,
            read: false,
            write: true,
            transmit: false,
            update: true,
        }
    }
}

// ============================================================================
// Connection Configuration
// ============================================================================

/// Tunnel connection configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TunnelConfig {
    /// KNX layer type.
    #[serde(default)]
    pub layer: KnxLayerConfig,

    /// Request timeout in milliseconds.
    #[serde(default = "default_request_timeout_ms")]
    pub request_timeout_ms: u64,

    /// Maximum retry count.
    #[serde(default = "default_max_retries")]
    pub max_retries: u8,

    /// ACK timeout in milliseconds (knxd default: 1000ms).
    #[serde(default = "default_ack_timeout_ms")]
    pub ack_timeout_ms: u64,

    /// L_Data.con confirmation timeout in milliseconds (default: 3000ms).
    #[serde(default = "default_confirmation_timeout_ms")]
    pub confirmation_timeout_ms: u64,

    /// Consecutive send error threshold for tunnel restart (knxd: 5).
    #[serde(default = "default_send_error_threshold")]
    pub send_error_threshold: u32,

    /// Fatal desync threshold — sequence distance triggering tunnel restart (knxd: 5).
    #[serde(default = "default_fatal_desync_threshold")]
    pub fatal_desync_threshold: u8,
}

fn default_request_timeout_ms() -> u64 {
    1000
}

fn default_max_retries() -> u8 {
    3
}

fn default_ack_timeout_ms() -> u64 {
    1000
}

fn default_confirmation_timeout_ms() -> u64 {
    3000
}

fn default_send_error_threshold() -> u32 {
    5
}

fn default_fatal_desync_threshold() -> u8 {
    5
}

impl Default for TunnelConfig {
    fn default() -> Self {
        Self {
            layer: KnxLayerConfig::default(),
            request_timeout_ms: default_request_timeout_ms(),
            max_retries: default_max_retries(),
            ack_timeout_ms: default_ack_timeout_ms(),
            confirmation_timeout_ms: default_confirmation_timeout_ms(),
            send_error_threshold: default_send_error_threshold(),
            fatal_desync_threshold: default_fatal_desync_threshold(),
        }
    }
}

impl TunnelConfig {
    /// Get request timeout as Duration.
    pub fn request_timeout(&self) -> Duration {
        Duration::from_millis(self.request_timeout_ms)
    }

    /// Get ACK timeout as Duration.
    pub fn ack_timeout(&self) -> Duration {
        Duration::from_millis(self.ack_timeout_ms)
    }

    /// Get confirmation timeout as Duration.
    pub fn confirmation_timeout(&self) -> Duration {
        Duration::from_millis(self.confirmation_timeout_ms)
    }
}

/// Server-side tunnel behavior configuration.
///
/// Controls how the simulator behaves from the gateway perspective,
/// enabling realistic protocol testing for trap-knx clients.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TunnelBehaviorConfig {
    /// Enable L_Data.con confirmation flow (MC=0x2E).
    /// When true, the server sends L_Data.con after processing L_Data.req.
    #[serde(default = "default_true")]
    pub ldata_con_enabled: bool,

    /// Base probability of L_Data.con success (0.0 - 1.0).
    /// 1.0 = always succeed, 0.0 = always NACK.
    #[serde(default = "default_confirmation_success_rate")]
    pub confirmation_success_rate: f64,

    /// Simulated bus delivery delay in milliseconds before sending L_Data.con.
    #[serde(default = "default_bus_delivery_delay_ms")]
    pub bus_delivery_delay_ms: u64,

    /// Enable sequence validation with duplicate/out-of-order detection.
    #[serde(default = "default_true")]
    pub sequence_validation_enabled: bool,

    /// Enable ACK timeout and retry tracking for server→client frames.
    #[serde(default = "default_true")]
    pub ack_tracking_enabled: bool,

    /// Heartbeat response status override.
    /// None = use normal behavior, Some(status) = always respond with this status.
    #[serde(default)]
    pub heartbeat_status_override: Option<u8>,

    /// Per-connection ACK timeout in milliseconds for server→client frames.
    #[serde(default = "default_server_ack_timeout_ms")]
    pub server_ack_timeout_ms: u64,

    /// Maximum retries for server→client frame delivery.
    #[serde(default = "default_max_retries")]
    pub server_max_retries: u8,

    /// Enable Bus Monitor frame generation (MC=0x2B).
    /// When true, L_Busmon.ind frames with Additional Info TLV are sent
    /// for every bus frame to connections in BusMonitor mode.
    #[serde(default)]
    pub bus_monitor_enabled: bool,

    /// Enable L_Data.ind broadcast to other tunnel connections.
    /// When true, group value writes are forwarded to all other connected tunnels.
    #[serde(default = "default_true")]
    pub ldata_ind_broadcast_enabled: bool,

    /// Enable cEMI property service handling (M_PropRead, M_PropWrite).
    /// When true, the server processes property read/write requests and
    /// sends appropriate confirmations.
    #[serde(default = "default_true")]
    pub property_service_enabled: bool,

    /// Enable M_Reset handling.
    /// When true, the server responds to M_Reset.req with M_Reset.ind.
    #[serde(default = "default_true")]
    pub reset_service_enabled: bool,

    /// Flow control filter chain configuration.
    /// Enables bus timing simulation, priority queuing, and circuit breaker
    /// for realistic KNX bus behavior testing.
    #[serde(default)]
    pub flow_control: FilterChainConfig,

    /// Heartbeat scheduler configuration.
    /// When enabled, the scheduler determines the heartbeat response action
    /// for each connection state request, supporting 5 action types.
    #[serde(default)]
    pub heartbeat_scheduler: HeartbeatSchedulerConfig,

    /// Group value cache configuration.
    /// Caches group values with TTL/LRU eviction and auto-updates on
    /// L_Data.ind indications for consistent multi-client simulation.
    #[serde(default)]
    pub group_value_cache: GroupValueCacheConfig,

    /// Send error tracker configuration.
    /// Monitors consecutive and sliding window error rates for each channel,
    /// triggering tunnel restart when thresholds are exceeded.
    #[serde(default)]
    pub send_error_tracker: SendErrorTrackerConfig,
}

fn default_confirmation_success_rate() -> f64 {
    1.0
}

fn default_bus_delivery_delay_ms() -> u64 {
    0
}

fn default_server_ack_timeout_ms() -> u64 {
    1000
}

impl Default for TunnelBehaviorConfig {
    fn default() -> Self {
        Self {
            ldata_con_enabled: true,
            confirmation_success_rate: default_confirmation_success_rate(),
            bus_delivery_delay_ms: default_bus_delivery_delay_ms(),
            sequence_validation_enabled: true,
            ack_tracking_enabled: true,
            heartbeat_status_override: None,
            server_ack_timeout_ms: default_server_ack_timeout_ms(),
            server_max_retries: default_max_retries(),
            bus_monitor_enabled: false,
            ldata_ind_broadcast_enabled: true,
            property_service_enabled: true,
            reset_service_enabled: true,
            flow_control: FilterChainConfig::default(),
            heartbeat_scheduler: HeartbeatSchedulerConfig::default(),
            group_value_cache: GroupValueCacheConfig::default(),
            send_error_tracker: SendErrorTrackerConfig::default(),
        }
    }
}

/// KNX layer configuration.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum KnxLayerConfig {
    /// Link layer (default for tunneling).
    #[default]
    LinkLayer,
    /// Raw layer.
    Raw,
    /// Bus monitor layer.
    BusMonitor,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_server_config_default() {
        let config = KnxServerConfig::default();
        assert_eq!(config.bind_addr.port(), 3671);
        assert_eq!(config.max_connections, 256);
        assert!(config.tunneling_enabled);
    }

    #[test]
    fn test_server_config_builder() {
        let config = KnxServerConfig::default()
            .with_bind_addr("192.168.1.100:3671".parse().unwrap())
            .with_max_connections(100)
            .with_device_name("Test Server");

        assert_eq!(config.device_name, "Test Server");
        assert_eq!(config.max_connections, 100);
    }

    #[test]
    fn test_server_config_validation() {
        let config = KnxServerConfig::default();
        assert!(config.validate().is_ok());

        let invalid = KnxServerConfig {
            max_connections: 0,
            ..Default::default()
        };
        assert!(invalid.validate().is_err());
    }

    #[test]
    fn test_device_config() {
        let config = KnxDeviceConfig::new("dev-1", "Living Room Controller")
            .with_individual_address(IndividualAddress::new(1, 2, 3));

        assert_eq!(config.id, "dev-1");
        assert_eq!(config.individual_address.to_string(), "1.2.3");
    }

    #[test]
    fn test_group_object_flags() {
        let ro = GroupObjectFlagsConfig::read_only();
        assert!(ro.read);
        assert!(!ro.write);

        let wo = GroupObjectFlagsConfig::write_only();
        assert!(!wo.read);
        assert!(wo.write);
    }
}

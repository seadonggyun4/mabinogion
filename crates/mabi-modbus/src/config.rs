//! Modbus configuration types.

use std::net::SocketAddr;
use std::time::Duration;

use mabi_core::tags::Tags;
use serde::{Deserialize, Serialize};

/// Modbus TCP server configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModbusServerConfig {
    /// Bind address.
    #[serde(default = "default_bind_address")]
    pub bind_address: SocketAddr,

    /// Maximum concurrent connections.
    #[serde(default = "default_max_connections")]
    pub max_connections: usize,

    /// Connection timeout.
    #[serde(default = "default_timeout_secs")]
    pub timeout_secs: u64,

    /// Enable TCP keep-alive.
    #[serde(default = "default_true")]
    pub keep_alive: bool,

    /// TCP nodelay (disable Nagle algorithm).
    #[serde(default = "default_true")]
    pub tcp_nodelay: bool,

    /// Maximum requests per second per connection (0 = unlimited).
    #[serde(default)]
    pub rate_limit: u32,
}

fn default_bind_address() -> SocketAddr {
    "0.0.0.0:502".parse().unwrap()
}

fn default_max_connections() -> usize {
    1000
}

fn default_timeout_secs() -> u64 {
    30
}

fn default_true() -> bool {
    true
}

impl Default for ModbusServerConfig {
    fn default() -> Self {
        Self {
            bind_address: default_bind_address(),
            max_connections: default_max_connections(),
            timeout_secs: default_timeout_secs(),
            keep_alive: true,
            tcp_nodelay: true,
            rate_limit: 0,
        }
    }
}

impl ModbusServerConfig {
    /// Create a new config with the specified bind address.
    pub fn with_bind_address(mut self, addr: SocketAddr) -> Self {
        self.bind_address = addr;
        self
    }

    /// Set maximum connections.
    pub fn with_max_connections(mut self, max: usize) -> Self {
        self.max_connections = max;
        self
    }

    /// Get timeout as Duration.
    pub fn timeout(&self) -> Duration {
        Duration::from_secs(self.timeout_secs)
    }
}

/// Modbus device configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModbusDeviceConfig {
    /// Unit ID (1-247).
    pub unit_id: u8,

    /// Device name.
    pub name: String,

    /// Number of coils.
    #[serde(default = "default_coils")]
    pub coils: u16,

    /// Number of discrete inputs.
    #[serde(default = "default_discrete_inputs")]
    pub discrete_inputs: u16,

    /// Number of holding registers.
    #[serde(default = "default_holding_registers")]
    pub holding_registers: u16,

    /// Number of input registers.
    #[serde(default = "default_input_registers")]
    pub input_registers: u16,

    /// Response delay in milliseconds (for simulation).
    #[serde(default)]
    pub response_delay_ms: u64,

    /// Device tags for organization and filtering.
    #[serde(default, skip_serializing_if = "Tags::is_empty")]
    pub tags: Tags,
}

fn default_coils() -> u16 {
    10000
}

fn default_discrete_inputs() -> u16 {
    10000
}

fn default_holding_registers() -> u16 {
    10000
}

fn default_input_registers() -> u16 {
    10000
}

impl Default for ModbusDeviceConfig {
    fn default() -> Self {
        Self {
            unit_id: 1,
            name: "Modbus Device".to_string(),
            coils: default_coils(),
            discrete_inputs: default_discrete_inputs(),
            holding_registers: default_holding_registers(),
            input_registers: default_input_registers(),
            response_delay_ms: 0,
            tags: Tags::new(),
        }
    }
}

impl ModbusDeviceConfig {
    /// Create a new device config with the specified unit ID.
    pub fn new(unit_id: u8, name: impl Into<String>) -> Self {
        Self {
            unit_id,
            name: name.into(),
            ..Default::default()
        }
    }

    /// Set response delay.
    pub fn with_response_delay(mut self, delay_ms: u64) -> Self {
        self.response_delay_ms = delay_ms;
        self
    }

    /// Set tags.
    pub fn with_tags(mut self, tags: Tags) -> Self {
        self.tags = tags;
        self
    }

    /// Add a single tag.
    pub fn with_tag(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.tags.insert(key.into(), value.into());
        self
    }

    /// Add a label.
    pub fn with_label(mut self, label: impl Into<String>) -> Self {
        self.tags.add_label(label.into());
        self
    }

    /// Validate unit ID (1-247).
    pub fn validate(&self) -> Result<(), String> {
        if self.unit_id == 0 || self.unit_id > 247 {
            return Err(format!("Invalid unit ID: {} (must be 1-247)", self.unit_id));
        }
        Ok(())
    }
}

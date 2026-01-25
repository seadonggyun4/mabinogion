//! Protocol capabilities and feature detection.
//!
//! This module provides a way to define and query protocol-specific
//! capabilities, enabling extensible feature detection and runtime
//! capability negotiation.
//!
//! # Example
//!
//! ```rust,ignore
//! use mabi_core::capabilities::{ProtocolCapabilities, Capability};
//!
//! // Check if a device supports subscriptions
//! if device.capabilities().supports(Capability::Subscription) {
//!     device.subscribe(point_id).await?;
//! }
//!
//! // Get all supported capabilities
//! for cap in device.capabilities().list() {
//!     println!("Supports: {:?}", cap);
//! }
//! ```

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use crate::protocol::Protocol;

/// Standard capabilities that protocols may support.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Capability {
    // Data Operations
    /// Single point read operations.
    Read,
    /// Single point write operations.
    Write,
    /// Batch read operations (multiple points at once).
    BatchRead,
    /// Batch write operations (multiple points at once).
    BatchWrite,

    // Subscription/Notification
    /// Push-based subscriptions for value changes.
    Subscription,
    /// Change of Value (COV) notifications.
    ChangeOfValue,
    /// Deadband filtering for subscriptions.
    Deadband,

    // Historical Data
    /// Historical data read support.
    HistoryRead,
    /// Historical data write support.
    HistoryWrite,
    /// Trend logging.
    TrendLog,

    // Discovery
    /// Device discovery (broadcast/multicast).
    Discovery,
    /// Browse/enumerate data points.
    Browse,

    // Security
    /// Authentication support.
    Authentication,
    /// Encryption support.
    Encryption,
    /// Certificate-based security.
    Certificates,

    // Advanced Features
    /// Alarms and events.
    Alarms,
    /// Scheduling support.
    Scheduling,
    /// Time synchronization.
    TimeSync,
    /// Diagnostics and self-test.
    Diagnostics,

    // Data Types
    /// Array/sequence data types.
    ArrayTypes,
    /// Structure/complex data types.
    StructTypes,
    /// Bit string operations.
    BitStrings,

    // Protocol-Specific
    /// Modbus function codes 1-4 (basic read).
    ModbusBasicRead,
    /// Modbus function codes 5-6, 15-16 (basic write).
    ModbusBasicWrite,
    /// OPC UA method calls.
    OpcUaMethods,
    /// BACnet services.
    BacnetServices,
    /// KNX group communication.
    KnxGroupComm,
}

impl Capability {
    /// Get a human-readable description of this capability.
    pub fn description(&self) -> &'static str {
        match self {
            Self::Read => "Read single data points",
            Self::Write => "Write single data points",
            Self::BatchRead => "Read multiple data points at once",
            Self::BatchWrite => "Write multiple data points at once",
            Self::Subscription => "Subscribe to value changes",
            Self::ChangeOfValue => "Change of Value notifications",
            Self::Deadband => "Deadband filtering for subscriptions",
            Self::HistoryRead => "Read historical data",
            Self::HistoryWrite => "Write historical data",
            Self::TrendLog => "Trend logging support",
            Self::Discovery => "Device discovery",
            Self::Browse => "Browse/enumerate data points",
            Self::Authentication => "Authentication support",
            Self::Encryption => "Encryption support",
            Self::Certificates => "Certificate-based security",
            Self::Alarms => "Alarms and events",
            Self::Scheduling => "Scheduling support",
            Self::TimeSync => "Time synchronization",
            Self::Diagnostics => "Diagnostics and self-test",
            Self::ArrayTypes => "Array/sequence data types",
            Self::StructTypes => "Structure/complex data types",
            Self::BitStrings => "Bit string operations",
            Self::ModbusBasicRead => "Modbus basic read functions",
            Self::ModbusBasicWrite => "Modbus basic write functions",
            Self::OpcUaMethods => "OPC UA method calls",
            Self::BacnetServices => "BACnet services",
            Self::KnxGroupComm => "KNX group communication",
        }
    }

    /// Check if this is a core capability (expected in most protocols).
    pub fn is_core(&self) -> bool {
        matches!(self, Self::Read | Self::Write | Self::Browse)
    }
}

/// A set of capabilities supported by a device or protocol.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CapabilitySet {
    capabilities: HashSet<Capability>,
}

impl CapabilitySet {
    /// Create an empty capability set.
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a capability set with the given capabilities.
    pub fn with_capabilities(caps: impl IntoIterator<Item = Capability>) -> Self {
        Self {
            capabilities: caps.into_iter().collect(),
        }
    }

    /// Add a capability.
    pub fn add(&mut self, cap: Capability) {
        self.capabilities.insert(cap);
    }

    /// Remove a capability.
    pub fn remove(&mut self, cap: Capability) {
        self.capabilities.remove(&cap);
    }

    /// Check if a capability is supported.
    pub fn supports(&self, cap: Capability) -> bool {
        self.capabilities.contains(&cap)
    }

    /// Check if all given capabilities are supported.
    pub fn supports_all(&self, caps: &[Capability]) -> bool {
        caps.iter().all(|c| self.supports(*c))
    }

    /// Check if any of the given capabilities are supported.
    pub fn supports_any(&self, caps: &[Capability]) -> bool {
        caps.iter().any(|c| self.supports(*c))
    }

    /// List all supported capabilities.
    pub fn list(&self) -> impl Iterator<Item = Capability> + '_ {
        self.capabilities.iter().copied()
    }

    /// Get the number of supported capabilities.
    pub fn len(&self) -> usize {
        self.capabilities.len()
    }

    /// Check if empty.
    pub fn is_empty(&self) -> bool {
        self.capabilities.is_empty()
    }

    /// Merge with another capability set.
    pub fn merge(&mut self, other: &CapabilitySet) {
        self.capabilities.extend(&other.capabilities);
    }

    /// Create the intersection of two capability sets.
    pub fn intersection(&self, other: &CapabilitySet) -> CapabilitySet {
        CapabilitySet {
            capabilities: self
                .capabilities
                .intersection(&other.capabilities)
                .copied()
                .collect(),
        }
    }
}

impl From<Vec<Capability>> for CapabilitySet {
    fn from(caps: Vec<Capability>) -> Self {
        Self::with_capabilities(caps)
    }
}

impl FromIterator<Capability> for CapabilitySet {
    fn from_iter<T: IntoIterator<Item = Capability>>(iter: T) -> Self {
        Self::with_capabilities(iter)
    }
}

/// Trait for types that have protocol capabilities.
pub trait ProtocolCapabilities {
    /// Get the capability set for this protocol/device.
    fn capabilities(&self) -> &CapabilitySet;

    /// Check if a specific capability is supported.
    fn supports(&self, cap: Capability) -> bool {
        self.capabilities().supports(cap)
    }

    /// Check if all the given capabilities are supported.
    fn supports_all(&self, caps: &[Capability]) -> bool {
        self.capabilities().supports_all(caps)
    }
}

/// Get the default capabilities for a protocol.
pub fn default_capabilities(protocol: Protocol) -> CapabilitySet {
    match protocol {
        Protocol::ModbusTcp | Protocol::ModbusRtu => CapabilitySet::with_capabilities([
            Capability::Read,
            Capability::Write,
            Capability::BatchRead,
            Capability::BatchWrite,
            Capability::Browse,
            Capability::ModbusBasicRead,
            Capability::ModbusBasicWrite,
            Capability::BitStrings,
        ]),

        Protocol::OpcUa => CapabilitySet::with_capabilities([
            Capability::Read,
            Capability::Write,
            Capability::BatchRead,
            Capability::BatchWrite,
            Capability::Browse,
            Capability::Subscription,
            Capability::Deadband,
            Capability::HistoryRead,
            Capability::HistoryWrite,
            Capability::Discovery,
            Capability::Authentication,
            Capability::Encryption,
            Capability::Certificates,
            Capability::Alarms,
            Capability::OpcUaMethods,
            Capability::ArrayTypes,
            Capability::StructTypes,
        ]),

        Protocol::BacnetIp => CapabilitySet::with_capabilities([
            Capability::Read,
            Capability::Write,
            Capability::BatchRead,
            Capability::BatchWrite,
            Capability::Browse,
            Capability::ChangeOfValue,
            Capability::Discovery,
            Capability::TrendLog,
            Capability::Alarms,
            Capability::Scheduling,
            Capability::TimeSync,
            Capability::BacnetServices,
        ]),

        Protocol::KnxIp => CapabilitySet::with_capabilities([
            Capability::Read,
            Capability::Write,
            Capability::Browse,
            Capability::Discovery,
            Capability::KnxGroupComm,
        ]),
    }
}

/// Builder for creating custom capability sets.
#[derive(Debug, Default)]
pub struct CapabilitySetBuilder {
    set: CapabilitySet,
}

impl CapabilitySetBuilder {
    /// Create a new builder.
    pub fn new() -> Self {
        Self::default()
    }

    /// Start with default capabilities for a protocol.
    pub fn from_protocol(protocol: Protocol) -> Self {
        Self {
            set: default_capabilities(protocol),
        }
    }

    /// Add a capability.
    pub fn add(mut self, cap: Capability) -> Self {
        self.set.add(cap);
        self
    }

    /// Add multiple capabilities.
    pub fn add_all(mut self, caps: impl IntoIterator<Item = Capability>) -> Self {
        for cap in caps {
            self.set.add(cap);
        }
        self
    }

    /// Remove a capability.
    pub fn remove(mut self, cap: Capability) -> Self {
        self.set.remove(cap);
        self
    }

    /// Add core capabilities (Read, Write, Browse).
    pub fn with_core(self) -> Self {
        self.add(Capability::Read)
            .add(Capability::Write)
            .add(Capability::Browse)
    }

    /// Add subscription capabilities.
    pub fn with_subscriptions(self) -> Self {
        self.add(Capability::Subscription)
            .add(Capability::ChangeOfValue)
            .add(Capability::Deadband)
    }

    /// Add history capabilities.
    pub fn with_history(self) -> Self {
        self.add(Capability::HistoryRead)
            .add(Capability::HistoryWrite)
            .add(Capability::TrendLog)
    }

    /// Add security capabilities.
    pub fn with_security(self) -> Self {
        self.add(Capability::Authentication)
            .add(Capability::Encryption)
            .add(Capability::Certificates)
    }

    /// Build the capability set.
    pub fn build(self) -> CapabilitySet {
        self.set
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_capability_set() {
        let mut set = CapabilitySet::new();
        set.add(Capability::Read);
        set.add(Capability::Write);

        assert!(set.supports(Capability::Read));
        assert!(set.supports(Capability::Write));
        assert!(!set.supports(Capability::Subscription));
        assert_eq!(set.len(), 2);
    }

    #[test]
    fn test_supports_all_any() {
        let set = CapabilitySet::with_capabilities([
            Capability::Read,
            Capability::Write,
            Capability::Browse,
        ]);

        assert!(set.supports_all(&[Capability::Read, Capability::Write]));
        assert!(!set.supports_all(&[Capability::Read, Capability::Subscription]));

        assert!(set.supports_any(&[Capability::Read, Capability::Subscription]));
        assert!(!set.supports_any(&[Capability::Subscription, Capability::HistoryRead]));
    }

    #[test]
    fn test_default_capabilities() {
        let modbus_caps = default_capabilities(Protocol::ModbusTcp);
        assert!(modbus_caps.supports(Capability::Read));
        assert!(modbus_caps.supports(Capability::ModbusBasicRead));
        assert!(!modbus_caps.supports(Capability::Subscription));

        let opcua_caps = default_capabilities(Protocol::OpcUa);
        assert!(opcua_caps.supports(Capability::Subscription));
        assert!(opcua_caps.supports(Capability::HistoryRead));
        assert!(opcua_caps.supports(Capability::OpcUaMethods));
    }

    #[test]
    fn test_capability_builder() {
        let set = CapabilitySetBuilder::new()
            .with_core()
            .with_subscriptions()
            .add(Capability::Discovery)
            .build();

        assert!(set.supports(Capability::Read));
        assert!(set.supports(Capability::Write));
        assert!(set.supports(Capability::Browse));
        assert!(set.supports(Capability::Subscription));
        assert!(set.supports(Capability::Discovery));
    }

    #[test]
    fn test_capability_builder_from_protocol() {
        let set = CapabilitySetBuilder::from_protocol(Protocol::ModbusTcp)
            .remove(Capability::Write) // Remove write capability
            .add(Capability::Subscription) // Add custom capability
            .build();

        assert!(set.supports(Capability::Read));
        assert!(!set.supports(Capability::Write));
        assert!(set.supports(Capability::Subscription));
    }

    #[test]
    fn test_intersection() {
        let set1 = CapabilitySet::with_capabilities([
            Capability::Read,
            Capability::Write,
            Capability::Browse,
        ]);

        let set2 = CapabilitySet::with_capabilities([
            Capability::Read,
            Capability::Browse,
            Capability::Subscription,
        ]);

        let intersection = set1.intersection(&set2);
        assert!(intersection.supports(Capability::Read));
        assert!(intersection.supports(Capability::Browse));
        assert!(!intersection.supports(Capability::Write));
        assert!(!intersection.supports(Capability::Subscription));
    }

    #[test]
    fn test_capability_description() {
        assert_eq!(Capability::Read.description(), "Read single data points");
        assert!(Capability::Read.is_core());
        assert!(!Capability::Subscription.is_core());
    }
}

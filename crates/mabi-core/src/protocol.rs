//! Protocol definitions.

use serde::{Deserialize, Serialize};
use strum::{Display, EnumIter, EnumString};

/// Supported protocols.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Display, EnumIter, EnumString,
)]
#[strum(serialize_all = "lowercase")]
#[derive(Default)]
pub enum Protocol {
    /// Modbus TCP protocol.
    #[strum(serialize = "modbus_tcp", serialize = "modbus-tcp")]
    #[default]
    ModbusTcp,

    /// Modbus RTU protocol.
    #[strum(serialize = "modbus_rtu", serialize = "modbus-rtu")]
    ModbusRtu,

    /// OPC UA protocol.
    #[strum(serialize = "opcua", serialize = "opc-ua")]
    OpcUa,

    /// BACnet/IP protocol.
    #[strum(serialize = "bacnet", serialize = "bacnet-ip")]
    BacnetIp,

    /// KNXnet/IP protocol.
    #[strum(serialize = "knx", serialize = "knxnet-ip")]
    KnxIp,
}

impl Protocol {
    /// Get default port for this protocol.
    pub fn default_port(&self) -> u16 {
        match self {
            Protocol::ModbusTcp => 502,
            Protocol::ModbusRtu => 0, // Serial
            Protocol::OpcUa => 4840,
            Protocol::BacnetIp => 47808,
            Protocol::KnxIp => 3671,
        }
    }

    /// Check if protocol uses TCP.
    pub fn is_tcp(&self) -> bool {
        matches!(self, Protocol::ModbusTcp | Protocol::OpcUa)
    }

    /// Check if protocol uses UDP.
    pub fn is_udp(&self) -> bool {
        matches!(self, Protocol::BacnetIp | Protocol::KnxIp)
    }

    /// Check if protocol uses serial.
    pub fn is_serial(&self) -> bool {
        matches!(self, Protocol::ModbusRtu)
    }

    /// Get protocol display name.
    pub fn display_name(&self) -> &'static str {
        match self {
            Protocol::ModbusTcp => "Modbus TCP",
            Protocol::ModbusRtu => "Modbus RTU",
            Protocol::OpcUa => "OPC UA",
            Protocol::BacnetIp => "BACnet/IP",
            Protocol::KnxIp => "KNXnet/IP",
        }
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    #[test]
    fn test_protocol_from_str() {
        assert_eq!(
            Protocol::from_str("modbus_tcp").unwrap(),
            Protocol::ModbusTcp
        );
        assert_eq!(Protocol::from_str("opcua").unwrap(), Protocol::OpcUa);
        assert_eq!(Protocol::from_str("bacnet").unwrap(), Protocol::BacnetIp);
        assert_eq!(Protocol::from_str("knx").unwrap(), Protocol::KnxIp);
    }

    #[test]
    fn test_protocol_default_port() {
        assert_eq!(Protocol::ModbusTcp.default_port(), 502);
        assert_eq!(Protocol::OpcUa.default_port(), 4840);
        assert_eq!(Protocol::BacnetIp.default_port(), 47808);
        assert_eq!(Protocol::KnxIp.default_port(), 3671);
    }
}

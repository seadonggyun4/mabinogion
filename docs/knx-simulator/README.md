# KNX Simulator (mabi-knx)

A KNXnet/IP protocol simulator implementing the KNX IP communication standard for building automation testing and development.

## Table of Contents

- [Overview](#overview)
- [Architecture](#architecture)
- [Addressing System](#addressing-system)
- [Datapoint Types (DPT)](#datapoint-types-dpt)
- [KNXnet/IP Protocol Implementation](#knxnetip-protocol-implementation)
- [Configuration](#configuration)
- [API Reference](#api-reference)
- [Integration](#integration)
- [Error Handling](#error-handling)

---

## Overview

The `mabi-knx` crate provides a KNXnet/IP server simulator that implements the tunneling protocol for building automation testing. It supports group communication, multiple datapoint types, and integrates with the Mabinogion framework for scenario-based testing and chaos engineering.

### Module Structure

```
mabi-knx/
├── src/
│   ├── lib.rs           # Public API exports
│   ├── error.rs         # Error types
│   ├── config.rs        # Configuration structures
│   ├── address.rs       # Individual and group addressing
│   ├── cemi.rs          # Common EMI frame handling
│   ├── server.rs        # KNXnet/IP server implementation
│   ├── device.rs        # KNX device abstraction
│   ├── factory.rs       # Device factory for core integration
│   ├── tunnel.rs        # Tunneling connection management
│   ├── frame/           # KNXnet/IP frame parsing
│   │   ├── mod.rs
│   │   ├── header.rs    # Protocol header
│   │   └── hpai.rs      # Host Protocol Address Information
│   └── dpt/             # Datapoint type system
│       ├── mod.rs
│       ├── codec.rs     # DPT encoding/decoding trait
│       ├── registry.rs  # Dynamic codec registry
│       ├── types.rs     # Standard DPT implementations
│       └── values.rs    # DPT value enumeration
```

---

## Architecture

### Core Components

| Component | Description |
|-----------|-------------|
| `KnxServer` | UDP server handling KNXnet/IP protocol communication |
| `KnxDevice` | Device abstraction implementing the core `Device` trait |
| `GroupObjectTable` | Storage and management of group objects |
| `DptRegistry` | Dynamic registry for datapoint type codecs |
| `ConnectionManager` | Tunneling connection lifecycle management |

### Communication Flow

```
Client Request
      │
      ▼
┌─────────────────┐
│   KnxServer     │  ◄── UDP Socket (port 3671)
│   (UDP Layer)   │
└────────┬────────┘
         │
         ▼
┌─────────────────┐
│ ConnectionMgr   │  ◄── Tunnel connection handling
│ (Session Layer) │
└────────┬────────┘
         │
         ▼
┌─────────────────┐
│   CemiFrame     │  ◄── cEMI frame parsing
│ (Transport)     │
└────────┬────────┘
         │
         ▼
┌─────────────────┐
│ GroupObjectTbl  │  ◄── Group object operations
│ (Application)   │
└─────────────────┘
```

---

## Addressing System

### Individual Address

Physical addresses identifying devices on the KNX bus.

**Format**: `Area.Line.Device`

| Field | Bits | Range | Description |
|-------|------|-------|-------------|
| Area | 4 | 0-15 | Topological area |
| Line | 4 | 0-15 | Line within area |
| Device | 8 | 0-255 | Device on line |

```rust
use mabi_knx::IndividualAddress;

// Construction
let addr = IndividualAddress::new(1, 2, 3);
let addr = IndividualAddress::try_new(1, 2, 3)?;

// Parsing
let addr: IndividualAddress = "1.2.3".parse()?;

// Properties
assert_eq!(addr.area(), 1);
assert_eq!(addr.line(), 2);
assert_eq!(addr.device(), 3);

// Special addresses
assert!(IndividualAddress::new(0, 0, 0).is_broadcast());
```

### Group Address

Functional addresses for group communication.

**Three-Level Format**: `Main/Middle/Sub`

| Field | Bits | Range | Description |
|-------|------|-------|-------------|
| Main | 5 | 0-31 | Main group |
| Middle | 3 | 0-7 | Middle group |
| Sub | 8 | 0-255 | Sub group |

**Two-Level Format**: `Main/Sub`

| Field | Bits | Range | Description |
|-------|------|-------|-------------|
| Main | 5 | 0-31 | Main group |
| Sub | 11 | 0-2047 | Sub address |

```rust
use mabi_knx::GroupAddress;

// Three-level addressing
let addr = GroupAddress::three_level(1, 2, 3);
let addr: GroupAddress = "1/2/3".parse()?;

// Two-level addressing
let addr = GroupAddress::two_level(1, 2048);

// Raw 16-bit value
let addr = GroupAddress::from_raw(0x0A0B);

// Format conversion
let (main, middle, sub) = addr.as_three_level();
let (main, sub) = addr.as_two_level();
```

### Group Address Range

Range specification for batch operations.

```rust
use mabi_knx::GroupAddressRange;

let range = GroupAddressRange::new(
    GroupAddress::three_level(1, 0, 0),
    GroupAddress::three_level(1, 7, 255),
);

assert!(range.contains(&GroupAddress::three_level(1, 2, 100)));
```

---

## Datapoint Types (DPT)

The DPT system provides type-safe encoding and decoding of KNX data values.

### DPT Identifier

```rust
use mabi_knx::DptId;

let id = DptId::new(9, 1);  // DPT 9.001 (Temperature)
let id: DptId = "DPT9.001".parse()?;
let id: DptId = "9.001".parse()?;
```

### Supported Datapoint Types

| DPT | Name | Size | Rust Type | Description |
|-----|------|------|-----------|-------------|
| 1.001 | Switch | 1-bit | `bool` | On/Off switch |
| 1.002 | Bool | 1-bit | `bool` | True/False |
| 1.008 | Up/Down | 1-bit | `bool` | Direction control |
| 2.001 | Switch Control | 2-bit | `PriorityControl` | Control with priority |
| 3.007 | Dimming Control | 4-bit | `DimmingControl` | Dimmer control |
| 3.008 | Blinds Control | 4-bit | `BlindsControl` | Blind/shutter control |
| 5.001 | Scaling | 1 byte | `u8` | 0-100% percentage |
| 5.003 | Angle | 1 byte | `u8` | 0-360 degrees |
| 5.010 | Counter Pulses | 1 byte | `u8` | Unsigned counter |
| 6.001 | Percent (Signed) | 1 byte | `i8` | -128% to +127% |
| 7.001 | Pulses | 2 bytes | `u16` | Unsigned 16-bit counter |
| 8.001 | Pulses Difference | 2 bytes | `i16` | Signed 16-bit counter |
| 9.001 | Temperature | 2 bytes | `f32` | 16-bit float (°C) |
| 9.004 | Lux | 2 bytes | `f32` | Illuminance (lux) |
| 9.007 | Humidity | 2 bytes | `f32` | Relative humidity (%) |
| 12.001 | Counter Value | 4 bytes | `u32` | Unsigned 32-bit counter |
| 13.001 | Counter (Signed) | 4 bytes | `i32` | Signed 32-bit counter |
| 14.* | Float | 4 bytes | `f32` | IEEE 754 float |
| 16.001 | ASCII String | 14 bytes | `String` | Text (max 14 chars) |
| 17.001 | Scene Number | 1 byte | `Scene` | Scene 0-63 |
| 18.001 | Scene Control | 1 byte | `Scene` | Scene with learn flag |
| 20.102 | HVAC Mode | 1 byte | `HvacMode` | HVAC control mode |
| 232.600 | Color RGB | 3 bytes | `ColorRgb` | RGB color value |

### DPT Value Type

```rust
use mabi_knx::DptValue;

// Boolean values
let value = DptValue::Bool(true);

// Numeric values
let value = DptValue::U8(100);
let value = DptValue::F16(21.5);

// Complex values
let value = DptValue::ColorRgb { r: 255, g: 128, b: 0 };
let value = DptValue::Scene { number: 5, learn: false };
let value = DptValue::DimmingControl { direction: true, step_code: 3 };

// HVAC modes
let value = DptValue::HvacMode(HvacMode::Comfort);
```

### DPT Registry

The registry manages codec instances for encoding and decoding.

```rust
use mabi_knx::{DptRegistry, DptId};

let registry = DptRegistry::new();  // Pre-populated with standard DPTs

// Lookup codec
let codec = registry.get(&DptId::new(9, 1))?;

// Encode value
let bytes = codec.encode(&DptValue::F16(22.5))?;

// Decode bytes
let value = codec.decode(&bytes)?;

// List available DPTs
for id in registry.list_ids() {
    println!("{}", id);
}
```

### Custom DPT Codec

```rust
use mabi_knx::{DptCodec, DptId, DptValue, KnxResult};

struct MyCustomDpt;

impl DptCodec for MyCustomDpt {
    fn id(&self) -> DptId { DptId::new(100, 1) }
    fn name(&self) -> &'static str { "Custom DPT" }
    fn size(&self) -> usize { 2 }

    fn encode(&self, value: &DptValue) -> KnxResult<Vec<u8>> {
        // Encoding logic
    }

    fn decode(&self, data: &[u8]) -> KnxResult<DptValue> {
        // Decoding logic
    }

    fn default_value(&self) -> DptValue { DptValue::U16(0) }
}

// Register custom codec
registry.register(MyCustomDpt);
```

---

## KNXnet/IP Protocol Implementation

### Service Types

**Core Services (0x02xx)**

| Service | Code | Description |
|---------|------|-------------|
| SearchRequest | 0x0201 | Device discovery request |
| SearchResponse | 0x0202 | Device discovery response |
| DescriptionRequest | 0x0203 | Request device description |
| DescriptionResponse | 0x0204 | Device description response |
| ConnectRequest | 0x0205 | Establish tunnel connection |
| ConnectResponse | 0x0206 | Connection response |
| ConnectionStateRequest | 0x0207 | Heartbeat request |
| ConnectionStateResponse | 0x0208 | Heartbeat response |
| DisconnectRequest | 0x0209 | Close connection |
| DisconnectResponse | 0x020A | Disconnect confirmation |

**Tunneling Services (0x04xx)**

| Service | Code | Description |
|---------|------|-------------|
| TunnellingRequest | 0x0420 | Send data through tunnel |
| TunnellingAck | 0x0421 | Acknowledge data receipt |

**Routing Services (0x05xx)**

| Service | Code | Description |
|---------|------|-------------|
| RoutingIndication | 0x0530 | Multicast group data |
| RoutingLostMessage | 0x0531 | Lost message notification |
| RoutingBusy | 0x0532 | Router busy indication |

### cEMI Message Codes

| Code | Value | Description |
|------|-------|-------------|
| L_Data.req | 0x11 | Data request |
| L_Data.con | 0x2E | Data confirmation |
| L_Data.ind | 0x29 | Data indication |
| L_Busmon.ind | 0x2B | Bus monitor indication |
| M_Reset.req | 0xF1 | Reset request |

### cEMI Frame Structure (Wire Format)

```
Offset: 0    1    2    3    4-5    6-7    8      9+
Field:  MC   AI   C1   C2   Src    Dst    NPDU   [NPDU Data...]
                                          Len
```

- **MC**: Message Code (0x11=L_Data.req, 0x29=L_Data.ind)
- **AI**: Additional Info Length (typically 0x00)
- **C1**: Control byte 1 (frame type, priority, repeat)
- **C2**: Control byte 2 (bit7: 0=Individual, 1=Group addressing, bits 2-0: hop count)
- **NPDU Len**: NPDU byte count (includes TPCI/APCI)
- **NPDU Data**: `npdu_len` bytes, first byte is TPCI/APCI

### NPDU / APCI Encoding

The APCI is encoded in the first NPDU byte:

| APCI | Byte1 Value | Byte2 | Description |
|------|------------|-------|-------------|
| GroupValueRead | 0x00 | - | npdu_len=1 |
| GroupValueResponse | 0x40 \| data | data... | npdu_len=1 (small) or 1+N |
| GroupValueWrite | 0x80 \| data | data... | npdu_len=1 (small) or 1+N |

**Small data (npdu_len=1)**: 6-bit data is packed in the lower 6 bits of the first byte
**Full data (npdu_len≥2)**: First byte = APCI, subsequent bytes = data

> **Note**: `npdu_len` is a count that includes the TPCI/APCI byte.
> Exactly `npdu_len` bytes follow after the npdu_len field.

### APCI Commands

| Command | Wire Value | Description |
|---------|-----------|-------------|
| GroupValueRead | 0x00 | Request group value |
| GroupValueResponse | 0x40 | Respond with group value |
| GroupValueWrite | 0x80 | Write group value |
| IndividualAddressWrite | - | Write physical address |
| IndividualAddressRead | - | Read physical address |
| DeviceDescriptorRead | - | Request device descriptor |
| Restart | - | Device restart command |

### Priority Levels

| Priority | Value | Description |
|----------|-------|-------------|
| System | 0 | Highest priority |
| Normal | 1 | Default priority |
| Urgent | 2 | High priority |
| Low | 3 | Lowest priority |

---

## Configuration

### Server Configuration

```rust
use mabi_knx::{KnxServerConfig, IndividualAddress};
use std::net::SocketAddr;

let config = KnxServerConfig {
    bind_addr: "0.0.0.0:3671".parse()?,
    multicast_addr: "224.0.23.12:3671".parse()?,
    individual_address: IndividualAddress::new(1, 1, 0),
    device_name: "Mabinogion KNX Server".to_string(),
    serial_number: [0x00; 6],
    mac_address: [0x00; 6],
    max_connections: 256,
    heartbeat_interval_secs: 60,
    connection_timeout_secs: 120,
    routing_enabled: false,
    tunneling_enabled: true,
    device_management_enabled: false,
};
```

**Configuration Parameters**

| Parameter | Type | Default | Description |
|-----------|------|---------|-------------|
| `bind_addr` | `SocketAddr` | 0.0.0.0:3671 | UDP bind address |
| `multicast_addr` | `SocketAddr` | 224.0.23.12:3671 | Multicast address |
| `individual_address` | `IndividualAddress` | 1.1.0 | Server physical address |
| `device_name` | `String` | - | Device name (max 30 chars) |
| `max_connections` | `usize` | 256 | Maximum concurrent tunnel connections |
| `heartbeat_interval_secs` | `u64` | 60 | Heartbeat interval |
| `connection_timeout_secs` | `u64` | 120 | Connection timeout |
| `tunneling_enabled` | `bool` | true | Enable tunneling mode |
| `routing_enabled` | `bool` | false | Enable routing mode |

### Device Configuration

```rust
use mabi_knx::KnxDeviceConfig;

let config = KnxDeviceConfig {
    id: "thermostat-01".to_string(),
    name: "Room Thermostat".to_string(),
    description: "Living room temperature control".to_string(),
    individual_address: IndividualAddress::new(1, 2, 1),
    group_objects: vec![
        GroupObjectConfig {
            address: "1/0/1".to_string(),
            name: "Temperature".to_string(),
            dpt: "DPT9.001".to_string(),
            flags: GroupObjectFlagsConfig::read_only(),
            initial_value: Some(serde_json::json!(21.5)),
        },
        GroupObjectConfig {
            address: "1/0/2".to_string(),
            name: "Setpoint".to_string(),
            dpt: "DPT9.001".to_string(),
            flags: GroupObjectFlagsConfig::read_write(),
            initial_value: Some(serde_json::json!(22.0)),
        },
    ],
    tick_interval_ms: 100,
};
```

### Group Object Flags

```rust
use mabi_knx::GroupObjectFlagsConfig;

// Preset configurations
let flags = GroupObjectFlagsConfig::read_only();
let flags = GroupObjectFlagsConfig::write_only();
let flags = GroupObjectFlagsConfig::read_write();

// Custom configuration
let flags = GroupObjectFlagsConfig {
    communication: true,
    read: true,
    write: false,
    transmit: true,
    update: true,
};
```

---

## API Reference

### KnxServer

```rust
use mabi_knx::{KnxServer, KnxServerConfig, GroupObjectTable, DptId};

// Create server
let config = KnxServerConfig { ... };
let server = KnxServer::new(config);

// Configure group object table (optional)
let group_table = Arc::new(GroupObjectTable::new());
group_table.create(GroupAddress::three_level(1, 0, 0), "Switch", &DptId::new(1, 1))?;
group_table.create(GroupAddress::three_level(1, 0, 1), "Temp", &DptId::new(9, 1))?;
let server = Arc::new(server.with_group_objects(group_table));

// Start server
server.start().await?;

// Subscribe to events
let mut rx = server.subscribe_events();
while let Ok(event) = rx.recv().await {
    match event {
        ServerEvent::ClientConnected { channel_id, address } => { /* ... */ }
        ServerEvent::GroupValueWrite { address, value, source } => { /* ... */ }
        ServerEvent::GroupValueRead { address, source } => {
            // Server automatically sends GroupValueResponse
        }
        _ => {}
    }
}

// Stop server
server.stop().await?;
```

### GroupValueRead/Response Processing Flow

When the server receives a `GroupValueRead` request:

1. Immediately send TunnellingACK
2. Look up the current value for the address in the group object table
3. Build a response cEMI frame using `CemiFrame::group_value_response()`
4. Wrap it in a TunnellingRequest and send to the client

```
Client                         Server
  │── TunnellingRequest ──>     │
  │   (GroupValueRead)          │
  │                             │
  │<── TunnellingACK ──────     │  (immediate ACK)
  │                             │
  │<── TunnellingRequest ──     │  (GroupValueResponse with data)
  │   (GroupValueResponse)      │
```

### KnxDevice

```rust
use mabi_knx::{KnxDevice, KnxDeviceBuilder, GroupAddress, DptValue, DptId};

// Builder pattern
let device = KnxDeviceBuilder::new("device-01", "Test Device")
    .individual_address(IndividualAddress::new(1, 2, 3))
    .description("A test device")
    .group_object(
        GroupAddress::three_level(1, 0, 1),
        "Switch",
        DptId::new(1, 1),
    )
    .group_object_with_value(
        GroupAddress::three_level(1, 0, 2),
        "Temperature",
        DptId::new(9, 1),
        DptValue::F16(20.0),
    )
    .build()?;

// Read/write group values
let value = device.read_group(&GroupAddress::three_level(1, 0, 1))?;
device.write_group(&GroupAddress::three_level(1, 0, 2), &DptValue::F16(22.5))?;

// List data points
for point in device.list_data_points() {
    println!("{}: {}", point.id, point.name);
}
```

### KnxDeviceFactory

```rust
use mabi_knx::KnxDeviceFactory;
use mabi_core::{DeviceFactory, DeviceConfig};

let factory = KnxDeviceFactory::new();
let device = factory.create(config)?;
```

---

## Integration

### Core Framework Integration

The `KnxDevice` implements the `mabi_core::Device` trait:

```rust
#[async_trait]
impl Device for KnxDevice {
    fn id(&self) -> String;
    fn protocol(&self) -> Protocol;
    fn state(&self) -> DeviceState;

    async fn initialize(&self) -> CoreResult<()>;
    async fn shutdown(&self) -> CoreResult<()>;

    async fn read_point(&self, id: &str) -> CoreResult<Value>;
    async fn write_point(&self, id: &str, value: &Value) -> CoreResult<()>;

    fn list_data_points(&self) -> Vec<DataPointDef>;
    fn subscribe_changes(&self) -> broadcast::Receiver<DataPoint>;
    fn get_statistics(&self) -> DeviceStatistics;
}
```

### Factory Registration

```rust
use mabi_knx::register_knx_factory;
use mabi_core::FactoryRegistry;

let registry = FactoryRegistry::new();
register_knx_factory(&registry)?;

// Factory is now available for Protocol::KnxIp
```

### Server Events

```rust
pub enum ServerEvent {
    Started { address: SocketAddr },
    Stopped,
    ClientConnected { channel_id: u8, address: SocketAddr },
    ClientDisconnected { channel_id: u8 },
    GroupValueWrite { address: GroupAddress, value: Vec<u8>, source: IndividualAddress },
    GroupValueRead { address: GroupAddress, source: IndividualAddress },
    Error { message: String },
}
```

---

## Error Handling

### Error Types

```rust
pub enum KnxError {
    // Address errors
    InvalidGroupAddress(String),
    InvalidIndividualAddress(String),
    AddressOutOfRange { address: String, valid_range: String },

    // DPT errors
    InvalidDpt(String),
    DptEncoding { dpt: String, reason: String },
    DptDecoding { dpt: String, reason: String },
    DptValueOutOfRange { value: String, valid_range: String },

    // Frame errors
    FrameTooShort { expected: usize, actual: usize },
    InvalidHeader(String),
    InvalidProtocolVersion { expected: u8, actual: u8 },
    UnknownServiceType(u16),

    // Connection errors
    ConnectionFailed { address: SocketAddr, reason: String },
    ConnectionTimeout { timeout_ms: u64 },
    ConnectionClosed(String),
    NoMoreConnections { max: usize },

    // Group object errors
    GroupObjectNotFound(String),
    GroupObjectWriteNotAllowed(String),
    GroupObjectReadNotAllowed(String),

    // Server errors
    ServerNotRunning,
    ServerAlreadyRunning,
    BindError { address: SocketAddr, reason: String },
}
```

### Error Classification

```rust
impl KnxError {
    /// Returns true if the error is recoverable
    pub fn is_recoverable(&self) -> bool;

    /// Returns true if this is a protocol-level error
    pub fn is_protocol_error(&self) -> bool;

    /// Returns true if this is a configuration error
    pub fn is_config_error(&self) -> bool;
}
```

### Recoverable Errors

The following errors are classified as recoverable:
- `ConnectionTimeout`
- `TunnelTimeout`
- `SequenceError`
- `ConnectionClosed`

---

## Protocol Constants

| Constant | Value | Description |
|----------|-------|-------------|
| `KNXNETIP_VERSION` | 0x10 | Protocol version 1.0 |
| `DEFAULT_PORT` | 3671 | Standard KNXnet/IP port |
| `DEFAULT_MULTICAST_ADDR` | 224.0.23.12 | KNX multicast address |
| `HEADER_SIZE` | 6 | KNXnet/IP header size |

---

## References

- KNX Association: [www.knx.org](https://www.knx.org)
- KNXnet/IP Specification: KNX System Specification, Volume 3 (Communication)
- cEMI Specification: KNX System Specification, Volume 3, Part 6

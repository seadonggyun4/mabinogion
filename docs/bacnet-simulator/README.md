# BACnet/IP Simulator

The `mabi-bacnet` crate provides a BACnet/IP server implementation for simulating Building Automation and Control networks. This document describes the architecture, supported features, and usage patterns of the simulator.

## Table of Contents

- [Architecture](#architecture)
- [Supported Object Types](#supported-object-types)
- [Supported Services](#supported-services)
- [Network Layer](#network-layer)
- [Configuration](#configuration)
- [Usage](#usage)
- [API Reference](#api-reference)

## Architecture

The simulator follows a layered architecture conforming to the BACnet protocol stack:

```
┌─────────────────────────────────────────────────────────────┐
│                     BACnet Server                           │
│       (Server, Device Management, Event Handling)          │
└─────────────────────────────────────────────────────────────┘
                             │
          ┌──────────────────┼──────────────────┐
          ▼                  ▼                  ▼
┌─────────────────┐ ┌─────────────────┐ ┌─────────────────┐
│  Service Layer  │ │  Object Model   │ │      BBMD       │
│ (Handler Reg.)  │ │ (Registry)      │ │ (Cross-subnet)  │
└─────────────────┘ └─────────────────┘ └─────────────────┘
          │                  │
          ▼                  ▼
┌─────────────────┐ ┌─────────────────┐
│   APDU Layer    │ │ Property Store  │
│ (Segmentation)  │ │ (DashMap-based) │
└─────────────────┘ └─────────────────┘
          │
          ▼
┌─────────────────────────────────────────────────────────────┐
│                    Network Layer                            │
│             (UDP, BVLC, NPDU handling)                     │
└─────────────────────────────────────────────────────────────┘
```

### Core Components

| Component | Description |
|-----------|-------------|
| `BACnetServer` | Main server coordinating packet processing, service dispatch, and COV management |
| `ObjectRegistry` | Thread-safe registry for BACnet objects using `DashMap` |
| `ServiceRegistry` | Dispatcher for confirmed and unconfirmed service handlers |
| `CovManager` | Manages Change of Value subscriptions and notifications |
| `PropertyStore` | Concurrent property storage for individual objects |

## Supported Object Types

The simulator implements the following BACnet object types:

| Object Type | Type ID | Description | Writable | COV Support |
|-------------|---------|-------------|----------|-------------|
| Analog Input (AI) | 0 | Read-only analog sensor | No | Yes |
| Analog Output (AO) | 1 | Analog control output with priority array | Yes | Yes |
| Analog Value (AV) | 2 | Writable analog data point | Yes | Yes |
| Binary Input (BI) | 3 | Read-only binary sensor | No | Yes |
| Binary Output (BO) | 4 | Binary control output with priority array | Yes | Yes |
| Binary Value (BV) | 5 | Writable binary data point | Yes | Yes |
| Multi-State Input (MSI) | 13 | Read-only multi-state sensor | No | Yes |
| Multi-State Output (MSO) | 14 | Multi-state control output with priority array | Yes | Yes |
| Multi-State Value (MSV) | 19 | Writable multi-state data point | Yes | Yes |

### Priority Array

Output objects (AO, BO, MSO) implement a 16-level priority array as defined in BACnet. The priority array allows multiple control sources to write to the same object, with the highest priority (lowest number) taking precedence.

### Object Properties

Each object type supports the following property categories:

**Common Properties:**
- `ObjectIdentifier`, `ObjectName`, `ObjectType`, `Description`
- `PresentValue`, `StatusFlags`, `EventState`, `Reliability`
- `OutOfService`

**Analog-specific:**
- `Units`, `MinPresValue`, `MaxPresValue`, `Resolution`, `CovIncrement`

**Binary-specific:**
- `Polarity`, `ActiveText`, `InactiveText`

**Multi-State-specific:**
- `NumberOfStates`, `StateText`

**Output-specific:**
- `PriorityArray`, `RelinquishDefault`

## Supported Services

### Confirmed Services

| Service | Choice | Description |
|---------|--------|-------------|
| ReadProperty | 12 | Read a single property from an object |
| ReadPropertyMultiple | 14 | Batch read properties across multiple objects |
| WriteProperty | 15 | Write a single property to an object |
| WritePropertyMultiple | 16 | Batch write properties to multiple objects |
| SubscribeCOV | 5 | Subscribe to Change of Value notifications |

### Unconfirmed Services

| Service | Choice | Description |
|---------|--------|-------------|
| Who-Is | 8 | Device discovery with optional instance range filtering |
| I-Am | 0 | Device identification response |
| UnconfirmedCOVNotification | 2 | COV notification without acknowledgment |

### Service Handler Architecture

The simulator uses a trait-based handler pattern:

```rust
pub trait ConfirmedServiceHandler: Send + Sync {
    fn service(&self) -> ConfirmedService;
    fn handle(&self, data: &[u8], ctx: &ServiceContext) -> ServiceResult;
}

pub trait UnconfirmedServiceHandler: Send + Sync {
    fn service(&self) -> UnconfirmedService;
    fn handle(&self, data: &[u8], ctx: &ServiceContext) -> ServiceResult;
}
```

Custom service handlers can be registered with the `ServiceRegistry`.

## Network Layer

### BVLC (BACnet Virtual Link Control)

Supported BVLC message types:

| Function | Code | Description |
|----------|------|-------------|
| Result | 0x00 | BVLC result/acknowledgment |
| OriginalUnicastNPDU | 0x0A | Unicast message to a single device |
| OriginalBroadcastNPDU | 0x0B | Broadcast message to all devices |
| ForwardedNPDU | 0x04 | Message forwarded by BBMD |
| RegisterForeignDevice | 0x05 | Foreign device registration |

### NPDU (Network Protocol Data Unit)

The NPDU layer handles:
- Network layer message routing
- Priority levels: `LifeSafety`, `CriticalEquipment`, `Urgent`, `Normal`
- Source and destination network addressing
- Reply expectation flags

### BBMD (BACnet Broadcast Management Device)

The simulator includes BBMD support for cross-subnet communication:

- **Broadcast Distribution Table (BDT):** Routes broadcasts to peer BBMDs
- **Foreign Device Table (FDT):** Tracks registered foreign devices with TTL
- Automatic entry expiration and cleanup

### APDU Encoding

`ApduEncoder`는 BACnet APDU 메시지의 구조화된 인코딩을 담당합니다. Application Tag 기반의 타입 안전 인코딩을 제공하며, Error PDU 등 복합 PDU 구성을 위한 전용 메서드를 포함합니다.

#### Error PDU (ASHRAE 135, Clause 21.8)

Error PDU는 Confirmed Service 요청 실패 시 반환되며, `error-class`와 `error-code`를 **Enumerated** (Application Tag 9)로 인코딩합니다.

```rust
let mut encoder = ApduEncoder::new();
encoder.encode_error_pdu(invoke_id, service_choice, error_class, error_code);
```

```text
| 0x50 | Invoke ID | Service Choice | Error Class  | Error Code   |
|  1B  |    1B     |      1B        | Tag9 + Value | Tag9 + Value |
```

### APDU Segmentation

For messages exceeding the maximum APDU length, the simulator supports:

- Segmented transmission and reception
- Configurable maximum segments (2, 4, 8, 16, 32, 64, or more)
- Window-based flow control
- Segment timeout handling (default: 10 seconds)

## Configuration

### Server Configuration

```rust
pub struct ServerConfig {
    pub bind_addr: SocketAddr,        // Default: 0.0.0.0:47808
    pub broadcast_addr: SocketAddr,   // Default: 255.255.255.255:47808
    pub device_instance: u32,         // Default: 1234
    pub device_name: String,          // Default: "BACnet Simulator"
    pub vendor_id: u16,               // Default: 0
    pub model_name: String,           // Default: "OTSIM"
    pub max_apdu_length: u16,         // Default: 1476
    pub max_cov_subscriptions: usize, // Default: 1000
    pub cov_check_interval: Duration, // Default: 1 second
    pub shutdown_timeout: Duration,   // Default: 30 seconds
}
```

### CLI Configuration

```bash
# Start BACnet/IP server
mabi bacnet --port 47808 --instance 1234
```

### YAML Configuration

```yaml
bind_address: "0.0.0.0:47808"
device_instance: 1234
device_name: "BACnet Simulator"
vendor_id: 999
enable_bbmd: false
```

## Usage

### Basic Server Setup

```rust
use mabi_bacnet::prelude::*;
use std::sync::Arc;

// Create object registry
let mut registry = ObjectRegistry::new();

// Create and configure analog input
let ai = AnalogInput::new(1, "Zone Temperature");
ai.set_value(72.5);
registry.register(Arc::new(ai));

// Create and configure binary output
let bo = BinaryOutput::new(1, "Fan Control");
bo.set_value(false);
registry.register(Arc::new(bo));

// Create server configuration
let config = ServerConfig::new(1234)
    .with_device_name("HVAC Controller")
    .with_vendor_id(999);

// Create and run server
let server = BACnetServer::new(config, registry);
server.run().await?;
```

### Bulk Object Creation

CLI 및 대량 시뮬레이션 시나리오에서는 `ObjectTypeDescriptor` 기반의 데이터 주도 생성을 사용합니다. 인스턴스 번호는 ASHRAE 135 표준에 따라 **0부터** 시작하며, 이름은 `{prefix}_{instance}` 패턴을 따릅니다.

```rust
use mabi_bacnet::prelude::*;

let registry = ObjectRegistry::new();

// 기본 4개 타입 (AI, AO, BI, BO) 디스크립터 사용
let descriptors = default_object_descriptors();
registry.populate_standard_objects(&descriptors, 50);
// → AI_0..AI_49, AO_0..AO_49, BI_0..BI_49, BO_0..BO_49 (총 200개)

// 커스텀 디스크립터로 특정 타입만 생성 가능
let custom = vec![
    ObjectTypeDescriptor {
        prefix: "AI",
        create: |instance, name| Arc::new(AnalogInput::new(instance, name)),
    },
];
registry.populate_standard_objects(&custom, 100);
```

### Custom Service Handler

```rust
use mabi_bacnet::prelude::*;

struct CustomHandler;

impl ConfirmedServiceHandler for CustomHandler {
    fn service(&self) -> ConfirmedService {
        ConfirmedService::ReadProperty
    }

    fn handle(&self, data: &[u8], ctx: &ServiceContext) -> ServiceResult {
        // Custom handling logic
        ServiceResult::ComplexAck(vec![/* response data */])
    }
}

// Register custom handler
let mut services = ServiceRegistry::new();
services.register_confirmed(Arc::new(CustomHandler));

let server = BACnetServer::new(config, registry)
    .with_services(services);
```

### Event Subscription

```rust
let server = BACnetServer::new(config, registry);
let mut events = server.subscribe();

tokio::spawn(async move {
    while let Ok(event) = events.recv().await {
        match event {
            ServerEvent::Started { address } => {
                println!("Server started on {}", address);
            }
            ServerEvent::DeviceDiscovered { device_instance, address } => {
                println!("Discovered device {} at {}", device_instance, address);
            }
            ServerEvent::Stopped => {
                println!("Server stopped");
                break;
            }
            ServerEvent::Error { message } => {
                eprintln!("Error: {}", message);
            }
        }
    }
});

server.run().await?;
```

## API Reference

### Core Traits

#### `BACnetObject`

Base trait for all BACnet objects:

```rust
pub trait BACnetObject: Send + Sync {
    fn object_id(&self) -> ObjectId;
    fn object_type(&self) -> ObjectType;
    fn object_name(&self) -> String;
    fn read_property(&self, property_id: PropertyId) -> Result<BACnetValue, PropertyError>;
    fn property_list(&self) -> Vec<PropertyId>;
}
```

#### `WritableObject`

Trait for objects that support write operations:

```rust
pub trait WritableObject: BACnetObject {
    fn write_property(
        &self,
        property_id: PropertyId,
        value: BACnetValue,
        priority: Option<u8>,
    ) -> Result<(), PropertyError>;
}
```

#### `CovSupport`

Trait for objects that support COV subscriptions:

```rust
pub trait CovSupport: BACnetObject {
    fn cov_increment(&self) -> Option<f32>;
    fn has_changed(&self, last_value: &BACnetValue) -> bool;
}
```

### Value Types

The `BACnetValue` enum represents all supported BACnet data types:

- `Null`, `Boolean`, `Unsigned`, `Signed`, `Real`, `Double`
- `OctetString`, `CharacterString`, `BitString`
- `Enumerated`, `Date`, `Time`, `ObjectIdentifier`
- `Constructed`, `ContextTagged`, `PropertyReference`, `Array`

### Error Handling

The simulator uses the `BacnetError` enum for error handling:

```rust
pub enum BacnetError {
    Io(std::io::Error),
    Protocol(String),
    Object(String),
    Property(PropertyError),
    Service(String),
    Network(String),
}
```

### Metrics

The server collects operational metrics accessible via `ServerMetrics`:

- Request counts (total, confirmed, unconfirmed)
- Error counts
- Bytes sent/received
- Service-specific counters (ReadProperty, WriteProperty, Who-Is, etc.)
- COV subscription and notification counts
- Latency statistics

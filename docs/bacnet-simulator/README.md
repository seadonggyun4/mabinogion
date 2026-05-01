# BACnet/IP Simulator

The `mabi-bacnet` crate provides a BACnet/IP server implementation for simulating Building Automation and Control networks. This document describes the architecture, supported features, and usage patterns of the simulator.

## Table of Contents

- [Architecture](#architecture)
- [Verification Strategy](#verification-strategy)
- [Supported Object Types](#supported-object-types)
- [Supported Services](#supported-services)
- [Network Layer](#network-layer)
- [Configuration](#configuration)
- [Usage](#usage)
- [API Reference](#api-reference)

## Verification Strategy

The simulator architecture is now paired with a dedicated BACnet verification
strategy that defines how external BACnet open-source peers are integrated into
the repository as verification assets instead of production dependencies.

See [verification-strategy.md](./verification-strategy.md) for the canonical
engineering plan covering:

- deterministic BACnet integration profiles
- self-contained interop matrix design
- active non-GUI peer selection
- GUI capture/manual lanes
- CI and perf policy boundaries

Current verification source-of-truth and lane documentation live here as well:

- [verification-baseline.md](./verification-baseline.md)
- [verification-contract.yaml](./verification-contract.yaml)
- [yabe-discovery-compatibility-plan.md](./yabe-discovery-compatibility-plan.md)
- [../../verification/bacnet/README.md](../../verification/bacnet/README.md)
- [../../verification/bacnet/captures/README.md](../../verification/bacnet/captures/README.md)

The current Phase 5 policy keeps the default workspace path deterministic:
YABE is manual/capture-only, BACpypes3/BAC0 YABE surrogates are ignored interop
checks, perf is release-only ignored, and Docker, GUI tools, external peers,
and perf thresholds do not belong in `cargo test --workspace`.

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

The simulator implements 14 BACnet object types conforming to ASHRAE Standard 135:

### Standard I/O Objects

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

### Extended Object Types

| Object Type | Type ID | Description | ASHRAE 135 Clause |
|-------------|---------|-------------|-------------------|
| Device | 8 | Mandatory device object with system status, vendor info, protocol services supported | Clause 12.11 |
| Event Enrollment | 9 | Event monitoring with 23+ event types, transition bits, notification classes | Clause 12.12 |
| File | 10 | Data file access with stream and record modes, atomic read/write | Clause 12.13 |
| Schedule | 17 | Time-based value switching with weekly/exception schedules | Clause 12.24 |
| Trend Log | 20 | Property value logging with circular buffer, COV/interval modes | Clause 12.25 |

### Priority Array

Output objects (AO, BO, MSO) implement a 16-level priority array as defined in BACnet. The priority array allows multiple control sources to write to the same object, with the highest priority (lowest number) taking precedence.

### Device Object

The Device object (mandatory per ASHRAE 135) provides:

- `DeviceSystemStatus`: Operational, OperationalReadOnly, DownloadRequired, DownloadInProgress, NonOperational, BackupInProgress
- Protocol services supported bitmask
- Clock synchronization (Local and UTC)
- Communication control state management
- Vendor information, model name, firmware revision

### Event Enrollment Object

Monitors properties of other objects and generates event notifications:

- 23+ `EventType` variants (ChangeOfState, ChangeOfValue, OutOfRange, FloatingLimit, CommandFailure, etc.)
- `EventTransitionBits` tracking (to_offnormal, to_fault, to_normal)
- `NotificationClass` for recipient distribution rules
- Confirmed and unconfirmed notification delivery

### File Object

Provides data file access with two modes:

- **Stream Access**: Contiguous byte-level positioning and read/write
- **Record Access**: Variable-length records with record-level positioning
- Atomic read/write operations with modification counter tracking
- Read-only flag support

### Schedule Object

Time-based value switching:

- **Weekly Schedule**: 7-day time/value lists (Monday through Sunday)
- **Exception Schedule**: Date/date-range overrides with higher priority
- Effective period with start/end date bounds
- Calendar entries: Date, DateRange, WeekNDay patterns

### Trend Log Object

Property value logging with a circular buffer:

- Configurable buffer capacity using `VecDeque<LogRecord>`
- COV-based or interval-based logging modes
- `LogRecord` with timestamp, datum, status flags, sequence numbers
- Stop-when-full and enable/disable controls
- Supports `ReadRange` service for historical data access

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

**Device-specific:**
- `SystemStatus`, `VendorName`, `ModelName`, `FirmwareRevision`, `ProtocolServicesSupported`
- `MaxApduLengthAccepted`, `SegmentationSupported`, `DatabaseRevision`

**Event-specific:**
- `EventType`, `NotifyType`, `EventEnable`, `AckedTransitions`, `EventState`

**File-specific:**
- `FileAccessMethod`, `FileSize`, `ModificationDate`, `ReadOnly`

**Schedule-specific:**
- `WeeklySchedule`, `ExceptionSchedule`, `ScheduleDefault`, `EffectivePeriod`

**Trend Log-specific:**
- `LogBuffer`, `RecordCount`, `TotalRecordCount`, `BufferSize`, `LoggingObject`

## Supported Services

### Confirmed Services

| Service | Choice | Description |
|---------|--------|-------------|
| AcknowledgeAlarm | 0 | Acknowledge an alarm event |
| ConfirmedEventNotification | 2 | Receive and process event notifications |
| GetAlarmSummary | 3 | Retrieve list of active alarms |
| GetEnrollmentSummary | 4 | Query EventEnrollment objects |
| SubscribeCOV | 5 | Subscribe to Change of Value notifications |
| AtomicReadFile | 6 | Atomic file read (stream or record mode) |
| AtomicWriteFile | 7 | Atomic file write (stream or record mode) |
| CreateObject | 10 | Dynamically create BACnet objects |
| DeleteObject | 11 | Remove dynamically-created objects |
| ReadProperty | 12 | Read a single property from an object |
| ReadPropertyMultiple | 14 | Batch read properties across multiple objects |
| WriteProperty | 15 | Write a single property to an object |
| WritePropertyMultiple | 16 | Batch write properties to multiple objects |
| DeviceCommunicationControl | 17 | Enable/disable device communication |
| ReinitializeDevice | 20 | Coldstart or warmstart the device |
| ReadRange | 26 | Read range of log records (by position, sequence, or time) |
| GetEventInformation | 29 | Retrieve detailed event state information |

### Unconfirmed Services

| Service | Choice | Description |
|---------|--------|-------------|
| I-Am | 0 | Device identification response |
| UnconfirmedCOVNotification | 2 | COV notification without acknowledgment |
| TimeSynchronization | 6 | Synchronize device local clock |
| Who-Is | 8 | Device discovery with optional instance range filtering |
| UTCTimeSynchronization | 9 | Synchronize device UTC clock |

### Transaction State Machine (TSM)

The simulator implements a server-side TSM per ASHRAE 135 Clause 5.4:

- Duplicate request detection within configurable time windows
- Transaction tracking by `(SocketAddr, invoke_id)` keys
- Cached responses for duplicate requests
- Configurable `TsmConfig`: duplicate window, max concurrent transactions
- Chaos testing support: intentional delays and drop probability for resilience testing

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

The `ApduEncoder` handles structured encoding of BACnet APDU messages. It provides type-safe encoding based on Application Tags and includes dedicated methods for constructing complex PDUs such as Error PDUs, Abort PDUs, Reject PDUs, and Segment ACKs.

Supported encoding features:
- Application tags: Null, Boolean, Unsigned, Signed, Real, Double, OctetString, CharacterString, BitString, Enumerated, ObjectIdentifier
- Context tags with implicit/explicit encoding
- Opening/closing tags for constructed types
- Segmented ComplexACK header encoding

#### Error PDU (ASHRAE 135, Clause 21.8)

Error PDUs are returned when a Confirmed Service request fails. The `error-class` and `error-code` are encoded as **Enumerated** values (Application Tag 9).

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
# Start BACnet/IP server with the mandatory Device object only
mabi serve bacnet --port 47808 --instance 1234

# Add opt-in demo/sample objects for explorer demos
mabi serve bacnet --port 47808 --instance 1234 --objects 100
```

The default CLI path mirrors `BACnetServer::new(...)`: it does not silently add
analog or binary sample points. With an empty user registry, BACnet explorers
such as YABE should discover the Device object, resolve `Object_Name`, read
`Object_List`, and see only that mandatory Device object. Use `--objects <N>`
when you want demo objects to appear immediately in an explorer tree.

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

For CLI and large-scale simulation scenarios, data-driven creation via `ObjectTypeDescriptor` is used. Instance numbers start from **0** per ASHRAE 135 conventions, and names follow the `{prefix}_{instance}` pattern.

```rust
use mabi_bacnet::prelude::*;

let registry = ObjectRegistry::new();

// Use default 4 types (AI, AO, BI, BO) descriptors
let descriptors = default_object_descriptors();
registry.populate_standard_objects(&descriptors, 50);
// → AI_0..AI_49, AO_0..AO_49, BI_0..BI_49, BO_0..BO_49 (200 total)

// Custom descriptors for specific types
let custom = vec![
    ObjectTypeDescriptor {
        prefix: "AI",
        create: |instance, name| Arc::new(AnalogInput::new(instance, name)),
    },
];
registry.populate_standard_objects(&custom, 100);
```

### Dynamic Object Creation

The `CreateObject` service enables runtime object instantiation using the `ObjectFactory`:

```rust
use mabi_bacnet::prelude::*;

// ObjectFactory is pre-loaded with 11 standard types
let factory = default_object_factory();

// Objects can be created/deleted at runtime via BACnet services
// CreateObject (service 10) and DeleteObject (service 11)
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
- Service-specific counters (ReadProperty, WriteProperty, Who-Is, COV, etc.)
- COV subscription and notification counts
- Segmentation metrics (segments sent/received, ACKs, reassembly)
- BBMD statistics (forwarded broadcasts, foreign device registrations)
- Latency statistics with sample counting
- Uptime tracking

## Module Structure

```
crates/mabi-bacnet/src/
├── lib.rs                      # Public API exports and prelude
├── apdu/
│   ├── encoding.rs             # APDU encoder (tags, PDUs, segmentation)
│   └── types.rs                # APDU type definitions
├── object/
│   ├── mod.rs                  # Object module exports
│   ├── traits.rs               # BACnetObject, WritableObject, CovSupport traits
│   ├── types.rs                # ObjectType (60+ variants), ObjectId
│   ├── property.rs             # PropertyId (200+), BACnetValue, PropertyStore
│   ├── registry.rs             # ObjectRegistry (DashMap-based)
│   ├── standard.rs             # AI/AO/AV/BI/BO/BV/MSI/MSO/MSV implementations
│   ├── device.rs               # Device object (ASHRAE 135 Clause 12.11)
│   ├── event_enrollment.rs     # EventEnrollment + NotificationClass
│   ├── file.rs                 # File object (stream/record access)
│   ├── schedule.rs             # Schedule object (weekly/exception)
│   └── trend_log.rs            # TrendLog object (circular buffer)
├── server/
│   ├── bacnet_server.rs        # Main server (UDP, BVLC, service dispatch, TSM)
│   └── metrics.rs              # ServerMetrics (24+ atomic counters)
└── service/
    ├── mod.rs                  # Service registry and handler dispatch
    ├── handler.rs              # ServiceContext, ServiceResult, handler traits
    ├── cov.rs                  # CovManager (subscription lifecycle)
    ├── subscribe_cov.rs        # SubscribeCOV handler (service 5)
    ├── alarm.rs                # Alarm services (Acknowledge, GetSummary, GetEventInfo)
    ├── create_delete.rs        # CreateObject/DeleteObject with ObjectFactory
    ├── device_control.rs       # TimeSynchronization, DeviceCommunicationControl, Reinitialize
    ├── file_access.rs          # AtomicReadFile/AtomicWriteFile
    ├── read_range.rs           # ReadRange (by position, sequence, time)
    └── tsm.rs                  # Transaction State Machine (duplicate detection)
```

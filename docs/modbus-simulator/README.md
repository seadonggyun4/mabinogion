# mabi-modbus

Modbus TCP/RTU protocol simulator for the Mabinogion industrial protocol simulation framework.

## Overview

The `mabi-modbus` crate provides a Modbus protocol simulation environment supporting both TCP and RTU (serial) communication modes. The current architecture is organized around a shared protocol core, a shared datastore/context layer, transport adapters, and profile-driven simulator construction.

The implementation adheres to the Modbus Application Protocol Specification V1.1b3 and keeps protocol semantics, address validation, and exception behavior consistent across dense and sparse backends.

## DX Documents

The following documents define the current DX-oriented source of truth for `mabi-modbus`:

- [simulator DX plan](./simulator-dx-plan.md)
- [simulator config spec](./simulator-config-spec.md)
- [simulator control-plane spec](./simulator-control-plane-spec.md)
- [DX reference matrix](./dx-reference-matrix.yaml)

## Canonical Operator Flow

The recommended path is now session-centric:

1. Author a `ModbusSimulatorConfig` file.
2. Validate it with `mabi validate modbus-config <file>`.
3. Inspect the typed schema with `mabi inspect modbus-schema`.
4. Inspect the config with `mabi inspect modbus-config <file>`.
5. Start a named session with `mabi serve modbus --config <file> --session <name>`.
6. Use `mabi control modbus ...` for session, point, trace, and fault operations.

## Architecture

### Layered Runtime

```text
┌──────────────┐    ┌────────────────────┐    ┌────────────────────┐
│ tcp / rtu    │───▶│ ModbusService      │───▶│ ServerContext      │
│ adapters     │    │ request execution  │    │ DeviceContext      │
└──────────────┘    └────────────────────┘    │ AddressSpace       │
                                               └────────────────────┘
                                                         │
                                               ┌────────────────────┐
                                               │ dense / sparse     │
                                               │ datastore backends │
                                               └────────────────────┘
```

### Canonical Surfaces

- `Builder`, `Config`, `Profile`, `Server`, `Device`, `Stats`, `Error`, `Result`
- `DeviceContext`, `ServerContext`, `AddressSpace`
- `driver()` and `descriptor()` for runtime integration

Low-level modules such as `tcp`, `rtu`, `handler`, `register`, and `registers` are still available for specialized integrations, but the recommended entry point is the root builder/profile surface.

### Session-Centric Construction

```text
ModbusSimulatorConfig
├── transports
├── devices
├── sessions
└── presets
    │
    ▼
CompiledModbusSession
├── ProtocolLaunchSpec
├── SimulatorProfile
├── trace policy
└── fault preset metadata
```

## Modules

| Module | Description |
|--------|-------------|
| [`core`](#architecture) | Protocol-facing request/response types and shared semantics |
| [`context`](#architecture) | Shared `AddressSpace`, `DeviceContext`, and `ServerContext` abstractions |
| [`profile`](#profile-driven-construction) | Profile-driven unit and point modeling |
| [`tcp`](#tcp-server) | Modbus TCP transport adapter with MBAP framing |
| [`rtu`](#rtu-server) | Modbus RTU transport adapter with CRC-16 validation |
| [`handler`](#function-handlers) | Function code handler registry and implementations |
| [`register`](#register-storage) | Dense register store implementation |
| [`registers`](#sparse-register-storage) | Sparse register store implementation with callbacks |
| [`types`](#data-types) | Data type definitions and register conversion utilities |
| [`unit`](#multi-unit-management) | Multi-unit (slave) management and broadcast handling |
| [`runtime`](#runtime-configuration) | Dynamic runtime configuration management |
| [`fault_injection`](#fault-injection-framework) | Optional protocol-aware fault injection pipeline |
| [`testing`](#testing-utilities) | Optional load generation and profiling helpers |
| [`scalability`](#scalability) | Optional high-volume connection and request handling |

## Protocols

### TCP Server

The TCP server implements the Modbus TCP protocol with MBAP framing. It now sits on top of the shared `ModbusService + ServerContext` stack, so request execution semantics are shared with RTU while the adapter owns transport lifecycle, listener state, and connection management.

```rust
use mabi_modbus::{Builder, Config};

let server = Builder::new()
    .config(Config::default())
    .generated_profile(2, 8)
    .build()?;
```

If you need low-level TCP control, `mabi_modbus::tcp::ModbusTcpServerV2` is still available.

#### TCP Configuration

| Parameter | Default | Description |
|-----------|---------|-------------|
| `bind_address` | `0.0.0.0:502` | Server bind address |
| `max_connections` | 1000 | Maximum concurrent connections |
| `connection_timeout` | 30s | Connection idle timeout |
| `tcp_keepalive` | enabled | TCP keepalive |
| `tcp_nodelay` | enabled | Disable Nagle algorithm |
| `rate_limit_rps` | 0 (unlimited) | Requests per second limit |

#### TCP Server Events

The server broadcasts lifecycle and connection events via `ServerEvent`:

| Event | Description |
|-------|-------------|
| `ConnectionAccepted` | New client connection established |
| `ConnectionClosed` | Client connection terminated |
| `RequestProcessed` | Modbus request handled successfully |
| `FaultInjected` | Fault pipeline activated for a request |
| `ServerStarted` | Server bind and listen initiated |
| `ServerStopped` | Server shutdown completed |

#### TCP Fault Pipeline Integration

The TCP server applies the `FaultPipeline` to all generated responses. Supported fault actions include `SendResponse`, `DropResponse`, `DelayThenSend`, `SendRawBytes`, and `OverrideTransactionId`. TCP-specific `ConnectionDisruption` configuration enables mid-frame disconnection, RST simulation, and connection hold-open scenarios for testing client reconnection logic.

### RTU Server

The RTU server simulates Modbus RTU serial communication with proper framing and timing.

```rust
use mabi_modbus::rtu::{ModbusRtuServer, RtuServerConfig};

let config = RtuServerConfig::default()
    .with_unit_ids(vec![1, 2, 3])
    .with_broadcast(true);

let server = ModbusRtuServer::new(config);
server.run().await?;
```

#### RTU Features

- Virtual serial port support (PTY-based on Unix systems)
- CRC-16 frame validation with lookup table (fast path) and polynomial computation (slow path)
- Inter-frame timing based on baud rate (3.5 character times per Modbus RTU specification)
- Inter-character timeout detection (1.5 character times)
- Auto-adjusted timing for high baud rates (>19200 baud)
- Multiple transport options: VirtualSerial, TcpBridge, Channel
- Streaming codec (`StreamingRtuCodec`) with byte-by-byte frame detection
- Unit ID filtering for multi-unit setups
- Strict timing mode for precise frame boundary detection
- Fault injection pipeline with RTU-specific timing faults
- Server state tracking (Stopped, Starting, Running, Stopping) with event broadcasting
- Graceful shutdown with configurable timeout

#### RTU Timing Reference

The `RtuTiming` struct computes protocol-compliant timing from baud rate:

| Baud Rate | 1 Character Time | Inter-Character (1.5 chars) | Inter-Frame (3.5 chars) |
|-----------|------------------|---------------------------|------------------------|
| 9600 | 1.042 ms | 1.563 ms | 3.646 ms |
| 19200 | 0.521 ms | 0.781 ms | 1.823 ms |
| 38400+ | 0.260 ms | 0.750 ms (fixed) | 1.750 ms (fixed) |

## Register Types

Four standard Modbus register types are implemented per the Modbus specification:

| Type | Access | Size | Address Range | Function Codes |
|------|--------|------|---------------|----------------|
| Coil | Read/Write | 1 bit | 0-65535 | FC01, FC05, FC0F |
| Discrete Input | Read-only | 1 bit | 0-65535 | FC02 |
| Holding Register | Read/Write | 16 bits | 0-65535 | FC03, FC06, FC10, FC17 |
| Input Register | Read-only | 16 bits | 0-65535 | FC04 |

### Read/Write Limits (per Modbus specification)

| Register Type | Maximum Read | Maximum Write |
|--------------|--------------|---------------|
| Coils | 2000 bits | 1968 bits |
| Discrete Inputs | 2000 bits | N/A |
| Holding Registers | 125 registers | 123 registers |
| Input Registers | 125 registers | N/A |

## Function Handlers

### Implemented Function Codes

| Code | Name | Description |
|------|------|-------------|
| FC01 (0x01) | Read Coils | Read ON/OFF status of discrete coils |
| FC02 (0x02) | Read Discrete Inputs | Read ON/OFF status of discrete inputs |
| FC03 (0x03) | Read Holding Registers | Read contents of holding registers |
| FC04 (0x04) | Read Input Registers | Read contents of input registers |
| FC05 (0x05) | Write Single Coil | Force single coil ON/OFF |
| FC06 (0x06) | Write Single Register | Write single holding register |
| FC0F (0x0F) | Write Multiple Coils | Force multiple coils |
| FC10 (0x10) | Write Multiple Registers | Write multiple holding registers |
| FC16 (0x16) | Mask Write Register | Atomic AND/OR mask operation on a single holding register |
| FC17 (0x17) | Read/Write Multiple Registers | Atomic read-write operation |

### Custom Handler Implementation

The handler architecture supports custom function code implementations:

```rust
use mabi_modbus::handler::{FunctionHandler, HandlerContext, ExceptionCode};

pub struct CustomHandler;

impl FunctionHandler for CustomHandler {
    fn function_code(&self) -> u8 { 0x42 }

    fn handle(&self, pdu: &[u8], ctx: &HandlerContext) -> Result<Vec<u8>, ExceptionCode> {
        Ok(vec![0x42, 0x00])
    }

    fn name(&self) -> &'static str { "Custom Handler" }
}
```

### Exception Codes

Standard Modbus exception codes per specification:

| Code | Name | Description |
|------|------|-------------|
| 0x01 | Illegal Function | Function code not recognized or allowed |
| 0x02 | Illegal Data Address | Data address not allowable |
| 0x03 | Illegal Data Value | Data value not allowable |
| 0x04 | Slave Device Failure | Unrecoverable error occurred |
| 0x05 | Acknowledge | Request accepted, processing in progress |
| 0x06 | Slave Device Busy | Server busy processing another request |
| 0x08 | Memory Parity Error | Memory parity error detected |
| 0x0A | Gateway Path Unavailable | Gateway path unavailable |
| 0x0B | Gateway Target Failed | Target device failed to respond |

## Data Types

### Supported Register Data Types

| Type | Register Count | Description |
|------|----------------|-------------|
| Bool | 1 | Boolean (0x0000 false, 0xFF00 true) |
| Int16 | 1 | Signed 16-bit integer |
| UInt16 | 1 | Unsigned 16-bit integer |
| Int32 | 2 | Signed 32-bit integer |
| UInt32 | 2 | Unsigned 32-bit integer |
| Float32 | 2 | IEEE 754 single precision |
| Int64 | 4 | Signed 64-bit integer |
| UInt64 | 4 | Unsigned 64-bit integer |
| Float64 | 4 | IEEE 754 double precision |
| String(n) | n | ASCII/UTF-8 encoded string |
| Bytes(n) | n | Raw byte array |

### Word Order (Byte Ordering)

Four multi-register byte orderings are supported:

| Order | Pattern | Common Usage |
|-------|---------|--------------|
| Big Endian | AB CD | Default, most common |
| Little Endian | DC BA | Least significant byte first |
| Big Endian Word Swap | CD AB | ABB, some Allen-Bradley devices |
| Little Endian Word Swap | BA DC | Rare, legacy systems |

### Register Conversion

```rust
use mabi_modbus::{RegisterConverter, WordOrder};

let converter = RegisterConverter::new(WordOrder::BigEndian);

// Float32 to registers
let registers = converter.f32_to_registers(25.5);

// Registers to Float32
let value = converter.registers_to_f32(&registers);
```

## Multi-Unit Management

The `MultiUnitManager` supports multiple Modbus slave units (1-247) with individual configuration:

```rust
use mabi_modbus::unit::{MultiUnitManager, UnitConfig, UnitManagerConfig};

let config = UnitManagerConfig::default();
let manager = MultiUnitManager::new(config);

// Add unit with specific configuration
manager.add_unit(1, UnitConfig::default())?;
manager.add_unit(2, UnitConfig::default())?;
```

### Unit Configuration

| Parameter | Description |
|-----------|-------------|
| `unit_id` | Unit identifier (1-247 per Modbus spec) |
| `word_order` | Byte ordering for multi-register values |
| `response_delay_us` | Simulated response delay |
| `enabled` | Unit enabled/disabled state |
| `broadcast_response` | Whether unit responds to broadcasts |

### Broadcast Modes

| Mode | Description |
|------|-------------|
| WriteAll | Broadcast writes to all units (standard) |
| Disabled | Ignore broadcasts |
| SelectiveList | Route broadcasts to specific units |
| EchoToUnit | Route broadcasts to designated unit |

## Register Storage

### Standard Register Store

Dense vector-based storage for all register types:

```rust
use mabi_modbus::{RegisterStore, RegisterType};

let mut store = RegisterStore::new(10000, 10000, 10000, 10000);

// Read/write holding registers
store.write_holding_registers(0, &[100, 200, 300])?;
let values = store.read_holding_registers(0, 3)?;
```

### Sparse Register Store

Memory-efficient storage for large address spaces with callback support:

```rust
use mabi_modbus::{SparseRegisterStore, RegisterStoreConfig, InitializationMode};

let config = RegisterStoreConfig {
    initialization_mode: InitializationMode::OnDemand,
    ..Default::default()
};

let store = SparseRegisterStore::new(config);
```

### Callback System

Register read/write callbacks for dynamic value generation:

```rust
use mabi_modbus::{CallbackManager, CallbackPriority, ReadCallback};

let mut callbacks = CallbackManager::new();

callbacks.register_read_callback(
    0..100,
    CallbackPriority::Normal,
    Box::new(|address| {
        // Dynamic value generation
        Some(address as u16)
    }),
);
```

## Runtime Configuration

Dynamic configuration updates without server restart:

```rust
use mabi_modbus::runtime::{RuntimeConfigManager, ConfigUpdate};

let manager = RuntimeConfigManager::new();

// Update configuration at runtime
manager.apply_update(ConfigUpdate::SetRegister {
    address: 100,
    value: 42,
})?;

// Enable/disable units
manager.apply_update(ConfigUpdate::UnitEnabled {
    unit_id: 1,
    enabled: false,
})?;
```

### Configuration Update Types

| Update Type | Description |
|-------------|-------------|
| `SetRegister` | Set specific register value |
| `SetRegisters` | Bulk set multiple registers |
| `SetCoil` | Set coil value |
| `UnitEnabled` | Enable/disable unit |
| `RegisterReadAccess` | Control read access for address range |
| `RegisterWriteAccess` | Control write access for address range |

## Device Tags

Modbus devices support tags for organization and filtering:

```rust
use mabi_modbus::ModbusDeviceConfig;
use mabi_core::tags::Tags;

// Using builder pattern
let config = ModbusDeviceConfig::new(1, "HVAC Unit 1")
    .with_tag("location", "building-a")
    .with_tag("floor", "3")
    .with_label("hvac")
    .with_label("critical");

// Using Tags directly
let tags = Tags::new()
    .with_tag("zone", "production")
    .with_label("monitored");

let config = ModbusDeviceConfig::new(2, "Sensor 1")
    .with_tags(tags);
```

### ModbusDeviceConfig Tag Methods

| Method | Description |
|--------|-------------|
| `with_tags(Tags)` | Set tags from existing Tags instance |
| `with_tag(key, value)` | Add a key-value tag |
| `with_label(label)` | Add a label |

Tags are propagated to `DeviceInfo` when the device is created, making them accessible via the `Device` trait.

---

## Fault Injection Framework

The `fault_injection` module provides a production-grade, Modbus-aware fault injection system designed for chaos engineering and protocol conformance testing. The system operates as an ordered pipeline applied to every response before transmission.

### Architecture

```text
┌─────────────────────────────────────────────────────────────────────┐
│                        FaultPipeline                                 │
│                                                                      │
│  Stage 1: Short-Circuit Faults                                       │
│  ├── NoResponse (silent drop)                                        │
│  └── PartialFrame (RTU only)                                        │
│                    ↓ (if not activated)                               │
│  Stage 2: Response-Replacing Faults                                  │
│  └── ExceptionInjection (force exception code)                       │
│                    ↓                                                  │
│  Stage 3: Response-Modifying Faults                                  │
│  ├── WrongUnitId                                                     │
│  ├── WrongFunctionCode                                               │
│  ├── TruncatedResponse                                               │
│  └── ExtraData                                                       │
│                    ↓                                                  │
│  Stage 4: Wire-Level Faults                                          │
│  └── CrcCorruption (RTU only)                                       │
│                    ↓                                                  │
│  Stage 5: Timing Faults                                              │
│  └── DelayedResponse                                                 │
└─────────────────────────────────────────────────────────────────────┘
```

### Supported Fault Types

| Fault Type | Transport | Description | Configuration Modes |
|------------|-----------|-------------|---------------------|
| `CrcCorruption` | RTU | Corrupts CRC-16 checksum | Zero, Invert, RandomXor, SetValue, SwapBytes |
| `WrongUnitId` | TCP/RTU | Modifies unit ID in response | Random, Fixed, Increment, SwapNibbles |
| `WrongFunctionCode` | TCP/RTU | Corrupts function code | Random, Fixed, Increment, HighBitToggle |
| `WrongTransactionId` | TCP | Overrides MBAP transaction ID | Random, Fixed, Increment, Decrement |
| `TruncatedResponse` | TCP/RTU | Truncates response PDU | FixedBytes, RemoveLastN, Percentage, HeaderOnly |
| `ExtraData` | TCP/RTU | Appends extra bytes to response | RandomBytes, AppendBytes, DuplicatePayload, PaddingPattern |
| `DelayedResponse` | TCP/RTU | Adds latency with jitter | Base delay (ms) + random jitter (ms) |
| `NoResponse` | TCP/RTU | Silently drops response | Probability-based activation |
| `ExceptionInjection` | TCP/RTU | Forces Modbus exception code | Configurable exception code (0x01-0x0B) |
| `PartialFrame` | RTU | Sends incomplete frame | FixedCount, Percentage, HeaderOnly, Random |

### RTU Timing Faults

The `RtuTimingFaultConfig` provides sub-module level timing violation injection for serial communication testing:

| Timing Fault | Description | Reference |
|--------------|-------------|-----------|
| Inter-Frame Delay Violation | Responds faster than 3.5 character times | Modbus RTU Spec. Section 2 |
| Inter-Character Gap Injection | Inserts gaps exceeding 1.5 character times within a frame | Modbus RTU Spec. Section 2 |
| Bus Collision Simulation | Overlapping transmissions on RS-485 bus | RS-485 Half-Duplex |
| Byte-Level Jitter | Random per-byte transmission delays | Serial transport noise |

### Connection Disruption (TCP)

The `ConnectionDisruptionConfig` enables TCP connection-level fault scenarios:

| Disruption Mode | Description |
|-----------------|-------------|
| Mid-Frame Disconnect | Closes connection during response transmission |
| RST After Partial Data | Sends TCP RST after partial response |
| Connection Hold-Open | Keeps connection open without responding |
| Clean Close | Graceful TCP FIN after configurable delay |

### Fault Targeting

Faults are selectively activated using the `FaultTarget` system:

```rust
use mabi_modbus::fault_injection::{FaultTarget, FaultPipeline};

// Target specific unit IDs and function codes
let target = FaultTarget::new()
    .with_unit_ids(vec![1, 2, 3])
    .with_function_codes(vec![0x03, 0x10])
    .with_probability(0.1);  // 10% activation rate
```

### Fault Statistics

Each fault maintains real-time counters via `FaultStats`:

| Metric | Description |
|--------|-------------|
| `checks` | Total times the fault was evaluated |
| `activations` | Times the fault was triggered |
| `affected` | Requests that were modified |
| `enabled` | Runtime enable/disable flag |

### YAML Configuration

Faults can be defined declaratively in scenario files:

```yaml
fault_injection:
  faults:
    - type: crc_corruption
      enabled: true
      target:
        unit_ids: [1, 2]
        probability: 0.05
      config:
        crc_mode: invert

    - type: delayed_response
      enabled: true
      config:
        delay_ms: 500
        jitter_ms: 200

    - type: no_response
      target:
        function_codes: [0x10]
        probability: 0.02
```

### Legacy Fault Injection

Simple delay and access control faults remain available for basic scenarios:

```rust
// Per-device delay
let config = ModbusDeviceConfig {
    response_delay_ms: 100,
    ..Default::default()
};

// Per-unit delay
let unit_config = UnitConfig {
    response_delay_us: 5000,
    ..Default::default()
};
```

### Access Control

```rust
use mabi_modbus::runtime::AccessControl;

let access = AccessControl {
    read_only_ranges: vec![(0, 99)],
    write_only_ranges: vec![(100, 199)],
    blocked_ranges: vec![(200, 299)],
};
```

## Port Safety and Process Lifecycle

### Server Bind Error Detection

When `ModbusTcpServerV2::run()` fails to bind the port (e.g., `EADDRINUSE`),
the error is detected within 100 ms of spawning the server task. The CLI
surfaces it as `CliError::PortInUse` (exit code 5) with actionable diagnostic
commands:

```
Error: Port 5020 is already in use.
  A previous mabi process may have been suspended (Ctrl+Z) and is still holding the port.
  Diagnostic: lsof -i :5020 | grep LISTEN
  To kill:    kill $(lsof -ti :5020 -sTCP:LISTEN)
```

### SIGTSTP (Ctrl+Z) Handling

The `CommandRunner::run_with_shutdown()` method intercepts `SIGTSTP` using
`tokio::signal::unix::Signal` and converts it into a graceful shutdown event.
This prevents the process from being suspended while still holding the TCP
listener socket — a condition known as a *zombie-port*.

```
┌─────────────────────────────────────────────┐
│           Signal Flow (Unix only)            │
├─────────────────────────────────────────────┤
│  Ctrl+C  →  ctrlc handler  →  shutdown_notify.notify_waiters()  │
│  Ctrl+Z  →  SIGTSTP handler → shutdown_notify.notify_waiters()  │
│                                  ↓                               │
│                         Graceful Shutdown                         │
│                     (server.shutdown() + port released)           │
└─────────────────────────────────────────────┘
```

### Advisory Port Pre-check

Before the server starts, the CLI performs a non-blocking port availability
check:

| Probe Result | Meaning | Action |
|-------------|---------|--------|
| Connection refused / timeout | Port is available | Proceed normally |
| TCP connects + Modbus response | Another server is running | Warn (will fail on bind) |
| TCP connects + no Modbus response | Possible zombie process | Warn with `lsof` diagnostic |

This is advisory only — it warns but does not block, since the port may be held
by a legitimate external process.

---

## Testing Utilities

### Load Generation

```rust
use mabi_modbus::testing::{LoadGenerator, LoadConfig, LoadPattern};

let config = LoadConfig {
    pattern: LoadPattern::Constant,
    target_rps: 1000,
    duration: Duration::from_secs(60),
};

let generator = LoadGenerator::new(config);
let results = generator.run().await;
```

### Performance Validation

```rust
use mabi_modbus::testing::{PerformanceValidator, PerformanceConfig};

let validator = PerformanceValidator::new(PerformanceConfig::default());
let result = validator.validate(&metrics);

assert!(result.passed);
```

### Memory Profiling

```rust
use mabi_modbus::testing::{MemoryProfiler, MemorySnapshot};

let profiler = MemoryProfiler::new();
let snapshot = profiler.snapshot();
let report = profiler.report();
```

## Public API

### Core Types

```rust
pub use mabi_modbus::{
    // TCP Server
    ModbusTcpServer,
    ModbusTcpServerV2,

    // RTU Server
    ModbusRtuServer,
    RtuServerConfig,
    RtuCodec,
    RtuFrame,
    VirtualSerial,

    // Register Storage
    RegisterStore,
    RegisterType,
    SparseRegisterStore,

    // Data Types
    RegisterConverter,
    RegisterDataType,
    WordOrder,

    // Multi-Unit
    MultiUnitManager,
    UnitConfig,
    BroadcastMode,

    // Fault Injection
    FaultPipeline,
    FaultInjectionConfig,
    FaultTarget,
    ModbusFault,
    ModbusFaultContext,
    FaultAction,
    FaultType,
    FaultTypeConfig,
    FaultStats,
    FaultStatsSnapshot,
    ConnectionDisruptionConfig,
    RtuTimingFaultConfig,

    // Runtime
    RuntimeConfigManager,
    ConfigUpdate,

    // Error Handling
    ModbusError,
    ModbusResult,
};
```

## Testing

```bash
# Run all tests
cargo test --package mabi-modbus

# Enable load/profiling helpers
cargo test --package mabi-modbus --features testing

# Run specific module tests
cargo test --package mabi-modbus handler::
cargo test --package mabi-modbus register::

# Enable explicit performance threshold suites
cargo test --package mabi-modbus --features performance-tests --test performance_validation

# Run with output
cargo test --package mabi-modbus -- --nocapture
```

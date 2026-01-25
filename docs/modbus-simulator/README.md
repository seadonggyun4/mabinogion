# mabi-modbus

Modbus TCP/RTU protocol simulator for the Mabinogion industrial protocol simulation framework.

## Overview

The `mabi-modbus` crate provides a comprehensive Modbus protocol simulation environment supporting both TCP and RTU (serial) communication modes. The implementation adheres to the Modbus Application Protocol Specification V1.1b3 and provides extensible handler architecture for custom function code implementations.

## Architecture

### TCP Server

```text
┌─────────────────────────────────────────────────────────────┐
│                    ModbusTcpServerV2                        │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────────┐  │
│  │ TcpListener  │──│ConnectionPool│──│  HandlerRegistry │  │
│  └──────────────┘  └──────────────┘  └──────────────────┘  │
│                                              │              │
│                        ┌─────────────────────┴───────────┐  │
│                        │     FunctionHandler Trait       │  │
│                        │  FC01  FC02  FC03  ...  Custom  │  │
│                        └─────────────────────────────────┘  │
└─────────────────────────────────────────────────────────────┘
```

### RTU Server

```text
┌─────────────────────────────────────────────────────────────┐
│                     ModbusRtuServer                         │
│  ┌──────────────────┐  ┌────────────────┐                  │
│  │ TransportManager │──│ HandlerRegistry│                  │
│  └────────┬─────────┘  └────────────────┘                  │
│           │                                                 │
│  ┌────────┴─────────────────────────────────┐              │
│  │           Transport Trait                 │              │
│  │  VirtualSerial  │  TcpBridge  │  Channel │              │
│  └──────────────────────────────────────────┘              │
│                           │                                 │
│           ┌───────────────┴───────────────────┐            │
│           │          RtuCodec                  │            │
│           │  Frame Detection │ CRC Validation │            │
│           └────────────────────────────────────┘            │
└─────────────────────────────────────────────────────────────┘
```

## Modules

| Module | Description |
|--------|-------------|
| [`tcp`](#tcp-server) | Modbus TCP server implementation with MBAP framing |
| [`rtu`](#rtu-server) | Modbus RTU serial simulation with CRC-16 validation |
| [`handler`](#function-handlers) | Function code handler registry and implementations |
| [`register`](#register-storage) | Standard register storage for all register types |
| [`registers`](#sparse-register-storage) | Sparse register storage with callback support |
| [`types`](#data-types) | Data type definitions and register conversion utilities |
| [`unit`](#multi-unit-management) | Multi-unit (slave) management and broadcast handling |
| [`runtime`](#runtime-configuration) | Dynamic runtime configuration management |
| [`testing`](#testing-utilities) | Load generation, performance validation, and profiling |
| [`scalability`](#scalability) | High-volume connection and request handling |

## Protocols

### TCP Server

The TCP server implements the Modbus TCP protocol with MBAP (Modbus Application Protocol) header framing.

```rust
use mabi_modbus::{ModbusTcpServerV2, tcp::ServerConfigV2};

let config = ServerConfigV2::default();
let server = ModbusTcpServerV2::new(config);
server.run().await?;
```

#### TCP Configuration

| Parameter | Default | Description |
|-----------|---------|-------------|
| `bind_address` | `0.0.0.0:502` | Server bind address |
| `max_connections` | 1000 | Maximum concurrent connections |
| `connection_timeout` | 30s | Connection idle timeout |
| `tcp_keepalive` | enabled | TCP keepalive |
| `tcp_nodelay` | enabled | Disable Nagle algorithm |
| `rate_limit_rps` | 0 (unlimited) | Requests per second limit |

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
- CRC-16 frame validation
- Inter-frame timing based on baud rate
- Multiple transport options: VirtualSerial, TcpBridge, Channel
- Streaming codec with frame detection

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

## Fault Injection

### Response Delay Simulation

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

# Run specific module tests
cargo test --package mabi-modbus handler::
cargo test --package mabi-modbus register::

# Run with output
cargo test --package mabi-modbus -- --nocapture
```

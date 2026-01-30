<div align="center">
  <img width="500px" alt="Truthound Banner" src="https://github.com/user-attachments/assets/34faf6ca-1ae7-4989-9468-1ea00eb00345" />
</div>

<h1 align="center">Mabinogion</h1>

<p align="center">
  <strong>Industrial Protocol Simulator Server</strong>
</p>

<p align="center">
  <em>"Spawn protocols at will"</em>
</p>

[![Crates.io](https://img.shields.io/crates/v/mabi-cli.svg)](https://crates.io/crates/mabi-cli)
[![License: Apache 2.0](https://img.shields.io/badge/License-Apache%202.0-orange.svg)](https://opensource.org/licenses/Apache-2.0)
[![Powered by Rust](https://img.shields.io/badge/Powered%20by-Rust-orange.svg)](https://www.rust-lang.org/)
[![Downloads](https://img.shields.io/crates/d/mabi-cli.svg?color=brightgreen)](https://crates.io/crates/mabi-cli)

---

## What is Mabinogion?

**Mabinogion** is an industrial protocol simulator server written in Rust. It creates virtual devices that speak real industrial protocols, allowing you to develop and test client software without physical hardware.

### Why Use It?

- **No Hardware Required**: Simulate PLCs, sensors, and building automation systems
- **Realistic Testing**: Virtual devices respond exactly like real equipment
- **Scale Testing**: Spawn 10,000+ devices to stress test your client
- **Chaos Engineering**: Inject network delays, packet loss, and device failures

### Supported Protocols

| Protocol | Use Case |
|----------|----------|
| **Modbus TCP/RTU** | Factory automation, PLCs, sensors |
| **OPC UA** | Industrial IoT, SCADA systems |
| **BACnet/IP** | Building automation (HVAC, lighting) |
| **KNXnet/IP** | Smart home/building systems |

---

## The Name

**Mabinogion** comes from Welsh mythology — ancient tales of magical forges where legendary weapons were crafted.

Your protocol clients are forged here too.

> *And yes, I spent way too many hours playing [Mabinogi](https://mabinogi.nexon.com/page/main/index.asp) back in the day.*

---

## Quick Example

```bash
# Start a Modbus server with 10 virtual devices
mabi modbus --port 5020 --devices 10

# Connect with any Modbus client
mbpoll -a 1 -r 1 -c 10 -p 5020 127.0.0.1
```

That's it. Your client now talks to virtual PLCs.

---

## Features

### Virtual Device Simulation
- Create virtual industrial devices that respond to real protocol requests
- Read/write registers, coils, and data points
- Each device maintains its own state

### Large-Scale Testing
- **10,000+** concurrent virtual devices
- **1,000,000+** real-time data points
- **100,000 msg/s** message throughput

### Protocol Support
| Protocol | Features |
|----------|----------|
| **Modbus TCP/RTU** | Read/Write Coils, Registers, Multi-unit support |
| **OPC UA** | Subscriptions, History, Security, Address Space |
| **BACnet/IP** | Property R/W, COV Subscriptions, BBMD, Discovery |
| **KNXnet/IP** | Tunneling, Group Addressing, DPT support |

### Chaos Engineering
- Network latency and packet loss simulation
- Device failure and recovery scenarios
- Protocol error injection

### Scenario-Based Testing
```yaml
name: stress_test_scenario
devices:
  - id: plc-001
    protocol: modbus_tcp
    points: 1000
events:
  - name: high_load
    trigger: "0 */5 * * * *"
    actions:
      - inject_latency: 100ms
      - corrupt_responses: 5%
```

---

## Installation

### For Simulator Users (Recommended)

**Just one command** - all simulator features included:

```bash
cargo install mabi-cli
```

This installs the `mabi` CLI with **all protocols** (Modbus, OPC UA, BACnet, KNX), scenario engine, and chaos engineering built-in. No additional packages required.

### For Library Developers (Optional)

If you want to build your own tools using Mabinogion as a library:

```toml
[dependencies]
mabi-core = "1.0"        # Core abstractions (required)
mabi-modbus = "1.0"      # Modbus TCP/RTU (optional)
mabi-opcua = "1.0"       # OPC UA (optional)
mabi-bacnet = "1.0"      # BACnet/IP (optional)
mabi-knx = "1.0"         # KNXnet/IP (optional)
mabi-scenario = "1.0"    # Scenario engine (optional)
mabi-chaos = "1.0"       # Chaos engineering (optional)
```

```rust
use mabi_core::prelude::*;

fn main() {
    let protocol = Protocol::ModbusTcp;
    let value = Value::float(23.5);
    println!("Protocol: {:?}, Value: {:?}", protocol, value);
}
```

---

## Quick Start

```bash
# See what awaits you
mabi

# Start Modbus TCP server
mabi modbus --port 502 --devices 10 --points 100

# Start OPC UA server
mabi opcua --port 4840 --nodes 1000

# Start BACnet/IP server
mabi bacnet --port 47808 --instance 1234

# Start KNXnet/IP server
mabi knx --port 3671 --address 1.1.1
```

### Running Scenarios

```bash
# Run simulator with scenario file
mabi run scenario.yaml

# Run at 2x speed for 10 minutes
mabi run scenario.yaml --time-scale 2.0 --duration 10m

# Validation only (dry-run)
mabi run scenario.yaml --dry-run
```

### Resource Queries

```bash
# List supported protocols
mabi list protocols

# List devices (JSON output)
mabi list devices --format json

# List devices for specific protocol
mabi list devices --protocol modbus
```

---

## Project Structure

```
mabinogion/
├── crates/
│   ├── mabi-core/       # Core abstractions and utilities
│   ├── mabi-modbus/     # Modbus TCP/RTU simulator
│   ├── mabi-opcua/      # OPC UA server simulator
│   ├── mabi-bacnet/     # BACnet/IP simulator
│   ├── mabi-knx/        # KNXnet/IP simulator
│   ├── mabi-scenario/   # Scenario engine
│   ├── mabi-chaos/      # Chaos engineering
│   └── mabi-cli/        # CLI (mabi)
├── scenarios/           # Scenario files
└── tests/               # Integration/E2E tests
```

---

## Crates

| Crate | crates.io | Description |
|-------|-----------|-------------|
| [mabi-core](https://crates.io/crates/mabi-core) | [![](https://img.shields.io/crates/v/mabi-core.svg)](https://crates.io/crates/mabi-core) | Core abstractions and utilities |
| [mabi-modbus](https://crates.io/crates/mabi-modbus) | [![](https://img.shields.io/crates/v/mabi-modbus.svg)](https://crates.io/crates/mabi-modbus) | Modbus TCP/RTU simulator |
| [mabi-opcua](https://crates.io/crates/mabi-opcua) | [![](https://img.shields.io/crates/v/mabi-opcua.svg)](https://crates.io/crates/mabi-opcua) | OPC UA server simulator |
| [mabi-bacnet](https://crates.io/crates/mabi-bacnet) | [![](https://img.shields.io/crates/v/mabi-bacnet.svg)](https://crates.io/crates/mabi-bacnet) | BACnet/IP simulator |
| [mabi-knx](https://crates.io/crates/mabi-knx) | [![](https://img.shields.io/crates/v/mabi-knx.svg)](https://crates.io/crates/mabi-knx) | KNXnet/IP simulator |
| [mabi-scenario](https://crates.io/crates/mabi-scenario) | [![](https://img.shields.io/crates/v/mabi-scenario.svg)](https://crates.io/crates/mabi-scenario) | Scenario engine |
| [mabi-chaos](https://crates.io/crates/mabi-chaos) | [![](https://img.shields.io/crates/v/mabi-chaos.svg)](https://crates.io/crates/mabi-chaos) | Chaos engineering |
| [mabi-cli](https://crates.io/crates/mabi-cli) | [![](https://img.shields.io/crates/v/mabi-cli.svg)](https://crates.io/crates/mabi-cli) | CLI tool |

## Documentation

| Module | Guide | API Reference |
|--------|-------|---------------|
| Core | [docs/core](./docs/core/README.md) | [docs.rs](https://docs.rs/mabi-core) |
| Modbus | [docs/modbus-simulator](./docs/modbus-simulator/README.md) | [docs.rs](https://docs.rs/mabi-modbus) |
| OPC UA | [docs/opcua-simulator](./docs/opcua-simulator/README.md) | [docs.rs](https://docs.rs/mabi-opcua) |
| BACnet | [docs/bacnet-simulator](./docs/bacnet-simulator/README.md) | [docs.rs](https://docs.rs/mabi-bacnet) |
| KNX | [docs/knx-simulator](./docs/knx-simulator/README.md) | [docs.rs](https://docs.rs/mabi-knx) |
| Scenario | [docs/scenario-engine](./docs/scenario-engine/README.md) | [docs.rs](https://docs.rs/mabi-scenario) |
| Chaos | [docs/chaos-engine](./docs/chaos-engine/README.md) | [docs.rs](https://docs.rs/mabi-chaos) |
| CLI | [docs/cli](./docs/cli/README.md) | [docs.rs](https://docs.rs/mabi-cli) |

---

## Performance Targets

| Metric | Target |
|--------|--------|
| Concurrent Devices | 10,000+ |
| Data Points | 1,000,000+ |
| Message Throughput | 100,000 msg/s |
| Memory (10K devices) | < 2GB |
| Latency (p99) | < 10ms |

---

## Development

### Build

```bash
cargo build --workspace
```

### Testing

```bash
# Full test suite
cargo test --workspace

# Integration tests
cargo test --test integration_tests

# E2E tests
cargo test --test e2e_protocol_tests
```

### Benchmarks

```bash
cargo bench
```

---

## Requirements

- Rust 1.75 or higher
- Tokio async runtime

---

## License

Licensed under the Apache License, Version 2.0.

---

<p align="center">
  <sub>
    <em>"Your protocols are forged in the crucible of mythology"</em>
  </sub>
</p>

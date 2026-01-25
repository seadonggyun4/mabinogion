<div align="center">
  <img width="500px" alt="Truthound Banner" src="https://github.com/user-attachments/assets/34faf6ca-1ae7-4989-9468-1ea00eb00345" />
</div>

<h1 align="center">Mabinogion</h1>

<p align="center">
  <strong>Where all industrial protocols are born and tested</strong>
</p>

<p align="center">
  <em>"Spawn protocols at will"</em>
</p>

[![License: Apache 2.0](https://img.shields.io/badge/License-Apache%202.0-orange.svg)](https://opensource.org/licenses/Apache-2.0)
[![Powered by Rust](https://img.shields.io/badge/Powered%20by-Rust-orange.svg)](https://www.rust-lang.org/)

---

## A Name Inspired by Mythology

**Mabinogion** is the name of a collection of Welsh mythological tales. These ancient legends tell of magical forges where the weapons of gods were created and heroes were tested through trials.

Our **Mabinogion** follows the same philosophy:

- **A Place of Birth**: Where industrial protocol clients first meet the world
- **A Space for Trials**: Where they are tested under extreme loads, edge cases, and chaos
- **A Crucible of Growth**: Where tens of thousands of virtual devices and millions of data points converge

Just as a hero's sword is tempered in the forge of mythology, your protocol clients are forged in Mabinogion.

> *And yes, I spent way too many hours playing [Mabinogi](https://mabinogi.nexon.com/page/main/index.asp) back in the day.*
---

## Features

### Large-Scale Simulation
- **10,000+** concurrent virtual devices
- **1,000,000+** real-time data points
- **100,000 msg/s** message throughput

### Multi-Protocol Support
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
- Connection drop and reconnection testing

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

## Quick Start

### Installation

```bash
cargo install mabi-cli
```

### Basic Usage

```bash
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
│   ├── mabi-web/        # Web API
│   └── mabi-cli/        # CLI (mabi)
├── scenarios/           # Scenario files
└── tests/               # Integration/E2E tests
```

---

## Documentation

| Crate | Description |
|-------|-------------|
| [mabi-cli](docs/cli/README.md) | Command-line interface reference |
| [mabi-core](docs/core/README.md) | Core abstractions, engine, metrics, and utilities |
| [mabi-modbus](docs/modbus-simulator/README.md) | Modbus TCP/RTU protocol simulator |
| [mabi-opcua](docs/opcua-simulator/README.md) | OPC UA server simulator |
| [mabi-bacnet](docs/bacnet-simulator/README.md) | BACnet/IP protocol simulator |
| [mabi-knx](docs/knx-simulator/README.md) | KNXnet/IP protocol simulator |
| [mabi-scenario](docs/scenario-engine/README.md) | Scenario engine for time-based simulation orchestration |
| [mabi-chaos](docs/chaos-engine/README.md) | Chaos engineering framework for fault injection |

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

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

For the 1.6.x release line, OPC UA, Modbus, BACnet/IP, and KNXnet/IP are substantially stabilized with deterministic regression lanes and optional external/interop lanes where applicable.

### Why Use It?

- **No Hardware Required**: Simulate PLCs, sensors, and building automation systems
- **Realistic Testing**: Virtual devices respond exactly like real equipment
- **Scale Testing**: Spawn 10,000+ devices to stress test your client
- **Chaos Engineering**: Inject network delays, packet loss, and device failures

### The Name

**Mabinogion** comes from Welsh mythology — ancient tales of magical forges where legendary weapons were crafted.

Your protocol clients are forged here too.

> *And yes, I spent way too many hours playing [Mabinogi](https://mabinogi.nexon.com/page/main/index.asp) back in the day.*

---

## Quick Example

```bash
# Start a Modbus server with 10 virtual devices
mabi serve modbus --port 5020 --devices 10

# Connect with any Modbus client
mbpoll -a 1 -r 1 -c 10 -p 5020 127.0.0.1
```

---

## Installation

### For Simulator Users (Recommended)

```bash
cargo install mabi-cli
mabi doctor
```

`cargo install mabi-cli` installs the self-contained CLI and all Rust protocol
simulators. Run `mabi doctor` after installation to verify the local binary.

Included in the installed CLI: Modbus, OPC UA, BACnet/IP, KNXnet/IP, scenario
workflows, chaos workflows, and the shared runtime. Optional source-tree
interop matrices still require Docker or peer tooling only when explicitly run
from the repository.

The default installed CLI keeps OPC UA on the lightweight UA-TCP path. OPC UA
Binary over HTTPS remains available from source with
`cargo install mabi-cli --features opcua-https`, which may require the platform
TLS/C toolchain expected by Rust TLS providers.

### For Library Developers (Optional)

```toml
[dependencies]
mabi-core = "1.6.2"        # Core abstractions (required)
mabi-modbus = "1.6.2"      # Modbus TCP/RTU (optional)
mabi-opcua = "1.6.2"       # OPC UA (optional)
mabi-bacnet = "1.6.2"      # BACnet/IP (optional)
mabi-knx = "1.6.2"         # KNXnet/IP (optional)
mabi-scenario = "1.6.2"    # Scenario engine (optional)
mabi-chaos = "1.6.2"       # Chaos engineering (optional)
```

---

## Quick Start

```bash
mabi serve modbus --port 502 --devices 10 --points 100
mabi serve opcua --config opcua.yaml --session default
mabi serve bacnet --port 47808 --instance 1234
mabi serve bacnet --port 47808 --instance 1234 --objects 100  # opt-in demo objects
mabi serve knx --port 3671 --address 1.1.1
mabi scenario run scenario.yaml --time-scale 2.0 --duration 10m
```

BACnet starts with only the mandatory Device object by default. Use
`--objects <N>` when you explicitly want demo/sample analog and binary objects
to appear in BACnet explorer tools.

---

## Documentation

All detailed documentation lives in the [`docs/`](../../docs/) directory. Each module has a comprehensive guide covering architecture, API reference, configuration, and usage examples.

### Protocol Simulators

| Protocol | Use Case | Key Features | Guide |
|----------|----------|--------------|-------|
| **Modbus TCP/RTU** | Factory automation, PLCs, sensors | TCP/RTU dual mode, handler registry, sparse registers, multi-unit, fault injection | [docs/modbus-simulator](../../docs/modbus-simulator/README.md) |
| **OPC UA** | Industrial IoT, SCADA systems | Address space, subscriptions, historical access, 22+ aggregates, security policies | [docs/opcua-simulator](../../docs/opcua-simulator/README.md) |
| **BACnet/IP** | Building automation (HVAC, lighting) | 9 object types, COV subscriptions, priority array, BBMD, APDU segmentation | [docs/bacnet-simulator](../../docs/bacnet-simulator/README.md) |
| **KNXnet/IP** | Smart home/building systems | Tunneling, 25+ datapoint types, group addressing, DPT codec system | [docs/knx-simulator](../../docs/knx-simulator/README.md) |

### Core & Infrastructure

| Module | Description | Key Features | Guide |
|--------|-------------|--------------|-------|
| **Core** | Common abstractions and utilities | Device trait, SimulatorEngine, factory system, metrics, capabilities, lifecycle | [docs/core](../../docs/core/README.md) |
| **Scenario Engine** | Declarative time-based simulation | 9 pattern types, event triggers/actions, time scaling, YAML schema, replay | [docs/scenario-engine](../../docs/scenario-engine/README.md) |
| **Chaos Engine** | Fault injection framework | Network/device/protocol faults, latency models, scheduler, middleware, config | [docs/chaos-engine](../../docs/chaos-engine/README.md) |
| **CLI** | Command-line interface | All commands, global options, output formats, exit codes, validation | [docs/cli](../../docs/cli/README.md) |

### API Reference

| Crate | crates.io | docs.rs |
|-------|-----------|---------|
| [mabi-core](https://crates.io/crates/mabi-core) | [![](https://img.shields.io/crates/v/mabi-core.svg)](https://crates.io/crates/mabi-core) | [docs.rs](https://docs.rs/mabi-core) |
| [mabi-modbus](https://crates.io/crates/mabi-modbus) | [![](https://img.shields.io/crates/v/mabi-modbus.svg)](https://crates.io/crates/mabi-modbus) | [docs.rs](https://docs.rs/mabi-modbus) |
| [mabi-opcua](https://crates.io/crates/mabi-opcua) | [![](https://img.shields.io/crates/v/mabi-opcua.svg)](https://crates.io/crates/mabi-opcua) | [docs.rs](https://docs.rs/mabi-opcua) |
| [mabi-bacnet](https://crates.io/crates/mabi-bacnet) | [![](https://img.shields.io/crates/v/mabi-bacnet.svg)](https://crates.io/crates/mabi-bacnet) | [docs.rs](https://docs.rs/mabi-bacnet) |
| [mabi-knx](https://crates.io/crates/mabi-knx) | [![](https://img.shields.io/crates/v/mabi-knx.svg)](https://crates.io/crates/mabi-knx) | [docs.rs](https://docs.rs/mabi-knx) |
| [mabi-scenario](https://crates.io/crates/mabi-scenario) | [![](https://img.shields.io/crates/v/mabi-scenario.svg)](https://crates.io/crates/mabi-scenario) | [docs.rs](https://docs.rs/mabi-scenario) |
| [mabi-chaos](https://crates.io/crates/mabi-chaos) | [![](https://img.shields.io/crates/v/mabi-chaos.svg)](https://crates.io/crates/mabi-chaos) | [docs.rs](https://docs.rs/mabi-chaos) |
| [mabi-cli](https://crates.io/crates/mabi-cli) | [![](https://img.shields.io/crates/v/mabi-cli.svg)](https://crates.io/crates/mabi-cli) | [docs.rs](https://docs.rs/mabi-cli) |

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
├── docs/                # Detailed documentation
├── scenarios/           # Scenario files
└── tests/               # Integration/E2E tests
```

---

## Development

```bash
cargo build --workspace          # Build
cargo test --workspace           # Full test suite
cargo test --test integration_tests   # Integration tests
cargo test --test e2e_protocol_tests  # E2E tests
cargo bench                      # Benchmarks
```

**Requirements**: Rust 1.75+ for `cargo install`. No Docker, Python, Java, or
Node runtime is required for the installed CLI's built-in `mabi doctor` smoke
checks.

---

## License

Licensed under the Apache License, Version 2.0.

---

<p align="center">
  <sub>
    <em>"Your protocols are forged in the crucible of mythology"</em>
  </sub>
</p>

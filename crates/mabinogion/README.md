# Mabinogion

> Industrial Protocol Simulator - Where all industrial protocols are born and tested

[![Crates.io](https://img.shields.io/crates/v/mabinogion.svg)](https://crates.io/crates/mabinogion)
[![Documentation](https://docs.rs/mabinogion/badge.svg)](https://docs.rs/mabinogion)
[![License](https://img.shields.io/crates/l/mabinogion.svg)](https://github.com/seadonggyun4/mabinogion/blob/main/LICENSE)

## Overview

Mabinogion is a high-performance industrial protocol simulator designed for:

- **Stress Testing**: Validate protocol clients under extreme load
- **Edge Case Verification**: Test unusual scenarios and error conditions
- **Large-Scale Simulation**: 10,000+ devices, 1,000,000+ data points
- **Chaos Engineering**: Inject faults to test resilience

## Supported Protocols

| Protocol | Description |
|----------|-------------|
| **Modbus TCP/RTU** | PLC and industrial device simulation |
| **OPC UA** | Industrial automation server |
| **BACnet/IP** | Building automation |
| **KNXnet/IP** | Home and building automation |

## Installation

Add to your `Cargo.toml`:

```toml
[dependencies]
mabinogion = "0.1"
```

Or with specific features:

```toml
[dependencies]
mabinogion = { version = "0.1", default-features = false, features = ["modbus", "chaos"] }
```

## Quick Start

```rust
use mabinogion::prelude::*;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create a Modbus TCP server with 100 devices
    let server = ModbusTcpServer::builder()
        .port(502)
        .devices(100)
        .points_per_device(1000)
        .build()?;

    server.start().await?;
    Ok(())
}
```

## Feature Flags

| Feature | Default | Description |
|---------|---------|-------------|
| `modbus` | Yes | Modbus TCP/RTU simulator |
| `opcua` | Yes | OPC UA server simulator |
| `bacnet` | Yes | BACnet/IP simulator |
| `knx` | Yes | KNXnet/IP simulator |
| `scenario` | Yes | Scenario engine |
| `chaos` | Yes | Chaos engineering |
| `full` | No | All features |

## CLI Tool

Install the CLI for quick testing:

```bash
cargo install mabi-cli
```

```bash
# Start Modbus server
mabi modbus --port 502 --devices 10

# Run scenario
mabi run scenario.yaml

# Validate scenario
mabi validate scenario.yaml
```

## Performance Targets

| Metric | Target |
|--------|--------|
| Concurrent Devices | 10,000+ |
| Data Points | 1,000,000+ |
| Message Throughput | 100,000 msg/s |
| Memory (10K devices) | < 2GB |
| Latency (p99) | < 10ms |

## License

Licensed under the Apache License, Version 2.0.

## Links

- [GitHub Repository](https://github.com/seadonggyun4/mabinogion)
- [Documentation](https://docs.rs/mabinogion)
- [Crates.io](https://crates.io/crates/mabinogion)

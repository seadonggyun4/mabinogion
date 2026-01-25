# Mabinogion CLI Reference

This document provides a comprehensive reference for the `mabi` command-line interface, the primary user-facing component of the Mabinogion industrial protocol simulator.

## Table of Contents

- [Overview](#overview)
- [Installation](#installation)
- [Global Options](#global-options)
- [Commands](#commands)
  - [run](#run)
  - [modbus](#modbus)
  - [opcua](#opcua)
  - [bacnet](#bacnet)
  - [knx](#knx)
  - [validate](#validate)
  - [list](#list)
  - [version](#version)
- [Output Formats](#output-formats)
- [Exit Codes](#exit-codes)
- [Architecture](#architecture)

## Overview

The `mabi` CLI is built using Rust with the `clap` crate for argument parsing and the Tokio async runtime for concurrent operations. It serves as the primary interface for configuring and executing protocol simulations.

## Installation

```bash
cargo install --path crates/mabi-cli
```

Or build from source:

```bash
cargo build --release --package mabi-cli
```

The binary will be available at `target/release/mabi`.

## Global Options

All commands accept the following global options:

| Option | Short | Type | Default | Description |
|--------|-------|------|---------|-------------|
| `--verbose` | `-v` | count | 0 | Verbosity level (repeatable: `-v`, `-vv`, `-vvv`) |
| `--quiet` | `-q` | flag | false | Suppress output except errors |
| `--format` | | enum | `table` | Output format: `table`, `json`, `yaml`, `compact` |
| `--no-color` | | flag | false | Disable colored output |
| `--config` | `-c` | path | none | Configuration file path |

### Verbosity Levels

| Level | Flag | Log Level |
|-------|------|-----------|
| Default | (none) | Warn |
| Verbose | `-v` | Info |
| Debug | `-vv` | Debug |
| Trace | `-vvv` | Trace |

## Commands

### run

Execute a simulation scenario from a YAML configuration file.

```bash
mabi run <SCENARIO> [OPTIONS]
```

#### Arguments

| Argument | Required | Description |
|----------|----------|-------------|
| `SCENARIO` | Yes | Path to scenario file (YAML format) |

#### Options

| Option | Short | Type | Default | Description |
|--------|-------|------|---------|-------------|
| `--time-scale` | `-s` | float | 1.0 | Time scale factor (e.g., 2.0 for 2x speed) |
| `--duration` | `-d` | string | none | Maximum duration (e.g., `10s`, `5m`, `1h`) |
| `--dry-run` | | flag | false | Validate scenario without execution |

#### Examples

```bash
# Run a scenario with default settings
mabi run scenario.yaml

# Run at double speed for 10 minutes
mabi run scenario.yaml --time-scale 2.0 --duration 10m

# Validate scenario syntax only
mabi run scenario.yaml --dry-run
```

---

### modbus

Start a standalone Modbus TCP or RTU simulator.

```bash
mabi modbus [OPTIONS]
```

#### Options

| Option | Short | Type | Default | Description |
|--------|-------|------|---------|-------------|
| `--port` | `-p` | u16 | 502 | TCP port to bind |
| `--bind` | | string | 0.0.0.0 | Bind address |
| `--devices` | `-d` | usize | 1 | Number of unit IDs to simulate |
| `--points` | | usize | 100 | Data points per device |
| `--rtu` | | flag | false | Enable RTU mode (serial) |
| `--serial` | | string | none | Serial port path (required for RTU) |

#### Examples

```bash
# Start Modbus TCP server on default port
mabi modbus

# Start with 100 devices, 1000 points each
mabi modbus --port 5020 --devices 100 --points 1000

# Start Modbus RTU on serial port
mabi modbus --rtu --serial /dev/ttyUSB0
```

---

### opcua

Start an OPC UA server simulator.

```bash
mabi opcua [OPTIONS]
```

#### Options

| Option | Short | Type | Default | Description |
|--------|-------|------|---------|-------------|
| `--port` | `-p` | u16 | 4840 | TCP port to bind |
| `--endpoint` | | string | / | Endpoint path |
| `--nodes` | `-n` | usize | 1000 | Number of nodes to create |
| `--security` | | string | None | Security mode: `None`, `Sign`, `SignAndEncrypt` |

#### Examples

```bash
# Start OPC UA server with defaults
mabi opcua

# Start with 10000 nodes
mabi opcua --port 4840 --nodes 10000

# Start with signing security
mabi opcua --security Sign --nodes 5000
```

---

### bacnet

Start a BACnet/IP simulator.

```bash
mabi bacnet [OPTIONS]
```

#### Options

| Option | Short | Type | Default | Description |
|--------|-------|------|---------|-------------|
| `--port` | `-p` | u16 | 47808 | UDP port to bind |
| `--instance` | `-i` | u32 | 1234 | BACnet device instance number |
| `--objects` | `-o` | usize | 100 | Number of BACnet objects |
| `--bbmd` | | flag | false | Enable BBMD functionality |

#### Examples

```bash
# Start BACnet server with defaults
mabi bacnet

# Start with custom instance and objects
mabi bacnet --instance 5000 --objects 1000

# Enable BBMD
mabi bacnet --bbmd --objects 500
```

---

### knx

Start a KNXnet/IP simulator.

```bash
mabi knx [OPTIONS]
```

#### Options

| Option | Short | Type | Default | Description |
|--------|-------|------|---------|-------------|
| `--port` | `-p` | u16 | 3671 | UDP port to bind |
| `--address` | `-a` | string | 1.1.1 | Individual address (format: X.X.X) |
| `--groups` | `-g` | usize | 100 | Number of group objects |

#### Examples

```bash
# Start KNX server with defaults
mabi knx

# Start with custom address and groups
mabi knx --address 1.2.3 --groups 500
```

---

### validate

Validate configuration and scenario files.

```bash
mabi validate <FILES>... [OPTIONS]
```

#### Arguments

| Argument | Required | Description |
|----------|----------|-------------|
| `FILES` | Yes | File paths to validate (glob patterns supported) |

#### Options

| Option | Short | Type | Default | Description |
|--------|-------|------|---------|-------------|
| `--detailed` | `-d` | flag | false | Show detailed validation results |
| `--strict` | | flag | false | Treat warnings as errors |

#### Validation Checks

The command performs the following validations:

1. **File existence** - Verifies the file exists
2. **Format syntax** - Validates YAML/JSON/TOML parsing
3. **Scenario structure** - For scenario files:
   - Required fields: `name`, `devices`
   - Device requirements: `id`, `protocol`
   - Valid protocols: `modbus_tcp`, `modbus_rtu`, `opcua`, `bacnet`, `knx`
   - Point requirements: `id`

#### Examples

```bash
# Validate a single file
mabi validate scenario.yaml

# Validate with detailed output
mabi validate scenario.yaml --detailed

# Validate multiple files with strict mode
mabi validate *.yaml --strict
```

---

### list

List available resources.

```bash
mabi list <RESOURCE> [OPTIONS]
```

#### Arguments

| Argument | Aliases | Description |
|----------|---------|-------------|
| `devices` | `device`, `d` | List simulated devices |
| `protocols` | `protocol`, `p` | List supported protocols |
| `points` | | List data points |
| `scenarios` | `scenario`, `s` | List scenario files |

#### Options

| Option | Short | Type | Default | Description |
|--------|-------|------|---------|-------------|
| `--protocol` | | string | none | Filter by protocol |
| `--filter` | `-f` | string | none | Filter by pattern (substring match) |
| `--limit` | `-l` | usize | none | Maximum items to display |

#### Examples

```bash
# List all devices
mabi list devices

# List devices filtered by protocol
mabi list devices --protocol modbus --limit 10

# List supported protocols
mabi list protocols

# List scenarios in JSON format
mabi list scenarios --format json
```

#### Protocol Information

The `list protocols` command displays the following supported protocols:

| Protocol | Default Port | Features |
|----------|--------------|----------|
| Modbus TCP | 502 | Read/Write Coils, Read/Write Registers, Multi-unit support |
| Modbus RTU | N/A (serial) | Serial communication, CRC validation, Multi-device bus |
| OPC UA | 4840 | Subscriptions, History, Security, Address space |
| BACnet/IP | 47808 | Read/Write Properties, COV Subscriptions, BBMD, Device discovery |
| KNXnet/IP | 3671 | Tunneling, Group addressing, DPT support |

---

### version

Display version and build information.

```bash
mabi version
```

#### Output

```
mabi X.Y.Z (Mabinogion)
Rust X.Y.Z
Supported protocols:
  - Modbus TCP/RTU
  - OPC UA
  - BACnet/IP
  - KNXnet/IP
```

## Output Formats

The `--format` option controls output rendering:

| Format | Description |
|--------|-------------|
| `table` | Human-readable table with UTF-8 box characters |
| `json` | Pretty-printed JSON |
| `yaml` | YAML structured output |
| `compact` | Single-line JSON (no formatting) |

## Exit Codes

| Code | Description |
|------|-------------|
| 0 | Success |
| 1 | General execution failure |
| 2 | Invalid configuration or scenario |
| 3 | Unsupported protocol |
| 4 | Device not found |
| 5 | Port already in use |
| 6 | Validation failed |
| 7 | I/O error |
| 8 | YAML/JSON parsing error |
| 9 | Simulator core error |
| 124 | Operation timeout |
| 130 | User interrupted (Ctrl+C) |

## Architecture

### Command Framework

The CLI implements a command pattern with the following components:

| Component | Description |
|-----------|-------------|
| `Command` trait | Interface for all CLI commands |
| `CommandRunner` | Execution lifecycle management |
| `CliContext` | Shared state and configuration |
| `OutputWriter` | Multi-format output rendering |

### Key Abstractions

```
Command (trait)
├── name() -> &str
├── description() -> &str
├── execute() -> CliResult<CommandOutput>
├── validate() -> CliResult<()>
├── requires_engine() -> bool
└── supports_shutdown() -> bool
```

### Context Management

`CliContext` provides:

- Simulator engine instance (lazy initialization)
- Engine and logging configuration
- Metrics collection
- Output writer with format/color settings
- Graceful shutdown signal handling

### Source Files

| File | Purpose |
|------|---------|
| `src/main.rs` | Entry point and command dispatch |
| `src/commands/run.rs` | Scenario execution |
| `src/commands/protocol.rs` | Protocol simulator commands |
| `src/commands/validate.rs` | Configuration validation |
| `src/commands/list.rs` | Resource listing |
| `src/context.rs` | CLI context and state |
| `src/output.rs` | Output formatting |
| `src/runner.rs` | Command execution framework |
| `src/error.rs` | Error types and handling |

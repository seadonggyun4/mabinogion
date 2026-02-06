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

The verbosity system operates at two layers: **log level** (tracing output) and **command output** (protocol server startup diagnostics). All protocol commands (`modbus`, `opcua`, `bacnet`, `knx`) respond to verbosity flags as follows:

| Level | Flag | Log Level | Command Output Behavior |
|-------|------|-----------|------------------------|
| Quiet | `-q` / `--quiet` | Error only | All header, table, and info messages suppressed |
| Default | (none) | Warn | Standard header, key-value summary, and status table |
| Verbose | `-v` | Info | Additional configuration details (e.g., bind address, subscription limits, object distribution) |
| Debug | `-vv` | Debug | Full configuration dump with `[DEBUG]` prefix |
| Trace | `-vvv` | Trace | Framework-level trace logging |

When `--quiet` is active, the server starts and awaits shutdown without producing any terminal output. This is suitable for automated test harnesses and CI environments where only the exit code is meaningful.

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
| `--port` | `-p` | u16 | 502 | TCP port to bind (1–65535) |
| `--bind` | | string | 0.0.0.0 | Bind address |
| `--devices` | `-d` | usize | 1 | Number of unit IDs to simulate (≥ 1) |
| `--points` | | usize | 100 | Data points per device (≥ 1) |
| `--tag` | | string | none | Device tags (repeatable, format: `key=value` or `label`) |
| `--rtu` | | flag | false | Enable RTU mode (serial) |
| `--serial` | | string | none | Serial port path (required for RTU) |

#### Tag Format

Tags can be specified in two formats:

- **Key-value**: `--tag key=value` (e.g., `--tag location=building-a`)
- **Label**: `--tag label` (e.g., `--tag critical`)

Multiple `--tag` options can be provided to add multiple tags to all devices.

#### Examples

```bash
# Start Modbus TCP server on default port
mabi modbus

# Start with 100 devices, 1000 points each
mabi modbus --port 5020 --devices 100 --points 1000

# Start with tags on all devices
mabi modbus --port 5020 --devices 10 --tag location=building-a --tag floor=3 --tag hvac

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
| `--port` | `-p` | u16 | 4840 | TCP port to bind (1–65535) |
| `--endpoint` | | string | / | Endpoint path |
| `--nodes` | `-n` | usize | 1000 | Number of nodes to create (≥ 1) |
| `--security` | | enum | `None` | Security mode (case-insensitive): `None`, `Sign`, `SignAndEncrypt` |
| `--tag` | | string | none | Device tags (repeatable, format: `key=value` or `label`) |

#### Node Creation Behavior

Nodes are created in namespace 2 with numeric identifiers starting at `i=1000`. The actual node count is capped at **100** regardless of the `--nodes` value (i.e., `ns=2;i=1000` through `ns=2;i=1099`).

Even-indexed nodes (`i=1000, 1002, 1004, ...`) are **writable** (AccessLevel: ReadWrite). Odd-indexed nodes (`i=1001, 1003, 1005, ...`) are **read-only** (AccessLevel: CurrentRead). All nodes are of type `Double`.

#### Examples

```bash
# Start OPC UA server with defaults
mabi opcua

# Start with 100 nodes on port 4841
mabi opcua --port 4841 --nodes 100

# Start with signing security
mabi opcua --security Sign --nodes 100

# Start with device tags
mabi opcua --port 4840 --nodes 500 --tag location=building-a --tag scada --tag critical
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
| `--port` | `-p` | u16 | 47808 | UDP port to bind (1–65535) |
| `--instance` | `-i` | u32 | 1234 | BACnet device instance number |
| `--objects` | `-o` | usize | 100 | Number of BACnet objects (≥ 1). 4개 타입(AI, AO, BI, BO)에 균등 분배, 인스턴스 0부터 시작 |
| `--bbmd` | | flag | false | Enable BBMD functionality |
| `--tag` | | string | none | Device tags (repeatable, format: `key=value` or `label`) |

#### Examples

```bash
# Start BACnet server with defaults
mabi bacnet

# Start with custom instance and objects
# → AI_0..AI_249, AO_0..AO_249, BI_0..BI_249, BO_0..BO_249
mabi bacnet --instance 5000 --objects 1000

# Enable BBMD
mabi bacnet --bbmd --objects 500

# Start with device tags
mabi bacnet --instance 1234 --objects 200 --tag location=building-b --tag floor=2 --tag hvac
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
| `--port` | `-p` | u16 | 3671 | UDP port to bind (1–65535) |
| `--address` | `-a` | string | 1.1.1 | Individual address (format: X.X.X) |
| `--groups` | `-g` | usize | 100 | Number of group objects (≥ 1) |
| `--tag` | | string | none | Device tags (repeatable, format: `key=value` or `label`) |

#### Examples

```bash
# Start KNX server with defaults
mabi knx

# Start with custom address and groups
mabi knx --address 1.2.3 --groups 500

# Start with device tags
mabi knx --address 1.1.1 --groups 200 --tag location=building-c --tag lighting --tag smart-home
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

The `--format` option controls output rendering across all commands. For protocol server commands (`modbus`, `opcua`, `bacnet`, `knx`), the format determines how the server startup information is presented:

| Format | Description |
|--------|-------------|
| `table` | Human-readable header with key-value pairs and UTF-8 box-character status table (default) |
| `json` | Pretty-printed JSON with structured server metadata (protocol, endpoint, status, nested objects) |
| `yaml` | YAML structured output with identical schema to JSON |
| `compact` | Single-line JSON without whitespace formatting, suitable for log ingestion pipelines |

### Protocol Command Output Structure

When a non-table format is selected, protocol commands emit a serializable structure containing the full server configuration and status. Each protocol defines its own schema appropriate to the protocol semantics.

#### Modbus JSON Output

`mabi modbus --port 5020 --devices 3 --points 100 --format json` produces:

```json
{
  "protocol": "Modbus TCP",
  "bind_address": "0.0.0.0:5020",
  "devices": 3,
  "points_per_device": 100,
  "total_points": 300,
  "rtu_mode": false,
  "serial_port": null,
  "device_list": [
    {
      "unit_id": 1,
      "holding_registers": 25,
      "input_registers": 25,
      "coils": 25,
      "discrete_inputs": 25,
      "status": "Online"
    },
    {
      "unit_id": 2,
      "holding_registers": 25,
      "input_registers": 25,
      "coils": 25,
      "discrete_inputs": 25,
      "status": "Online"
    },
    {
      "unit_id": 3,
      "holding_registers": 25,
      "input_registers": 25,
      "coils": 25,
      "discrete_inputs": 25,
      "status": "Online"
    }
  ],
  "status": "Online"
}
```

The `device_list` array enumerates every simulated unit with per-register-type point counts, enabling programmatic consumption by external test harnesses. Points are distributed uniformly across the four register types (holding, input, coils, discrete).

#### OPC UA JSON Output

`mabi opcua --format json --nodes 5` produces:

```json
{
  "protocol": "OPC UA",
  "endpoint": "opc.tcp://0.0.0.0:4840/",
  "nodes": 5,
  "security_mode": "None",
  "namespaces": [
    { "index": 0, "nodes": "Standard", "subscriptions": 0, "status": "Ready" },
    { "index": 1, "nodes": "5", "subscriptions": 0, "status": "Online" }
  ],
  "status": "Online"
}
```

The `list` command similarly emits structured output for device and protocol enumeration.

### Table Output Pagination

When the `table` format renders a large number of rows (e.g., devices or objects), the CLI applies automatic pagination via the `PaginatedTable` component to maintain terminal readability:

| Condition | Rendering Behavior |
|-----------|-------------------|
| Total rows ≤ 20 | All rows are displayed in full |
| Total rows > 20 | First 10 rows, a dim summary row (`... N more devices ...`), and the last 5 rows |

The pagination thresholds (max visible: 20, head: 10, tail: 5) are configurable at the call site, allowing protocol-specific tuning. This truncation is purely presentational; the underlying server instantiates all requested devices regardless of display limits. For complete enumeration, use `--format json` or `--format yaml`.

Example with 25 devices (`mabi modbus --devices 25 --points 100`):

```
┌─────────┬──────────────┬────────────┬───────┬──────────┬────────┐
│ Unit ID ┆ Holding Regs ┆ Input Regs ┆ Coils ┆ Discrete ┆ Status │
╞═════════╪══════════════╪════════════╪═══════╪══════════╪════════╡
│ 1       ┆ 25           ┆ 25         ┆ 25    ┆ 25       ┆ Online │
│ 2       ┆ 25           ┆ 25         ┆ 25    ┆ 25       ┆ Online │
│ ...     ┆              ┆            ┆       ┆          ┆        │
│ 10      ┆ 25           ┆ 25         ┆ 25    ┆ 25       ┆ Online │
│ ... 10 more devices ...                                          │
│ 21      ┆ 25           ┆ 25         ┆ 25    ┆ 25       ┆ Online │
│ ...     ┆              ┆            ┆       ┆          ┆        │
│ 25      ┆ 25           ┆ 25         ┆ 25    ┆ 25       ┆ Online │
└─────────┴──────────────┴────────────┴───────┴──────────┴────────┘
```

## Unified Device Tagging System

All protocol commands (`modbus`, `opcua`, `bacnet`, `knx`) support a unified tagging system via the `--tag` option. This enables consistent metadata annotation across heterogeneous protocol simulators, facilitating unified monitoring, filtering, and operational management.

### Tag Syntax

Tags are specified using the `--tag` option, which can be repeated multiple times:

```bash
mabi <protocol> --tag key=value --tag label
```

| Format | Example | Semantics |
|--------|---------|-----------|
| Key-Value | `--tag location=building-a` | Dimensional metadata with explicit attribute-value relationship |
| Label | `--tag critical` | Boolean predicate indicating group membership |

### Cross-Protocol Consistency

The tagging system is protocol-agnostic, enabling unified operational patterns across all supported protocols:

```bash
# Deploy building automation simulation with consistent organizational tags
mabi modbus --port 5020 --devices 10 \
    --tag location=building-a --tag floor=3 --tag system=hvac &

mabi opcua --port 4840 --nodes 500 \
    --tag location=building-a --tag floor=3 --tag system=scada &

mabi bacnet --port 47808 --objects 200 \
    --tag location=building-a --tag floor=3 --tag system=bms &

mabi knx --port 3671 --groups 100 \
    --tag location=building-a --tag floor=3 --tag system=lighting &
```

### Tag Use Cases

| Use Case | Example Tags | Description |
|----------|--------------|-------------|
| **Physical Location** | `location=building-a`, `floor=3`, `room=101` | ISA-95 style equipment hierarchy |
| **Functional Classification** | `system=hvac`, `function=temperature` | Cross-cutting functional categories |
| **Operational State** | `critical`, `monitored`, `maintenance` | Boolean operational flags |
| **Environment Segregation** | `env=prod`, `env=staging`, `env=dev` | Deployment environment classification |

### Metrics Integration

Tags propagate to Prometheus metrics labels, enabling dimensional queries:

```promql
# Aggregate requests by location and protocol
sum(mabi_requests_total{location="building-a"}) by (protocol, system)

# Filter critical devices
mabi_devices_active{critical="true"}
```

For detailed tag API documentation and query semantics, see [mabi-core Tags](../core/README.md#tags).

---

## Input Validation

All protocol commands enforce argument constraints at parse time via clap `value_parser` functions defined in `src/validation.rs`. Invalid values are rejected before any server initialization occurs, ensuring deterministic early failure with a descriptive error message and exit code 2.

### Constraint Summary

| Constraint | Affected Options | Rationale |
|------------|-----------------|-----------|
| Port ∈ [1, 65535] | `--port` (all protocols) | Port 0 triggers OS ephemeral port assignment, yielding a non-deterministic bind address that external clients cannot connect to. This is incompatible with the simulator's role as a known-address test endpoint. |
| Count ≥ 1 | `--devices`, `--points`, `--nodes`, `--objects`, `--groups` | A zero-count resource produces a server with no simulatable entities. This invariant violation is caught at the CLI boundary rather than propagated to protocol-layer initialization. |
| Tag format | `--tag` | Must be non-empty; key-value format requires non-empty key (e.g., `=value` is rejected). |

### Extensibility

The `validation` module exposes `value_parser`-compatible functions with the signature `fn(&str) -> Result<T, String>`. Adding a new constraint requires:

1. Defining a validator function in `src/validation.rs`
2. Annotating the target `#[arg]` with `value_parser = new_validator`

This design decouples validation logic from both the argument definition layer (clap) and the command execution layer, permitting reuse across protocol commands without duplication.

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
| `TableBuilder` | Fluent API for UTF-8 box-character table construction |
| `PaginatedTable` | Protocol-agnostic row pagination with configurable head/tail thresholds |

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
| `src/validation.rs` | Reusable argument validators |
| `src/error.rs` | Error types and handling |

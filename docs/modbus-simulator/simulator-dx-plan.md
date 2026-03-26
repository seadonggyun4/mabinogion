# Modbus Simulator DX Plan

## Goal

`mabi-modbus` now treats developer experience as a first-class surface. The next stage is no longer "prove protocol closure in documents", but "make simulator sessions easy to describe, inspect, validate, run, and control".

The canonical direction is:

- file-backed simulator config as source of truth
- session-centric execution instead of ad hoc profile assembly
- typed CLI flows for validate, inspect, serve, and live control
- in-process control ports before any external admin API
- bounded runtime state instead of free-form scripting surfaces

## Design Principles

- Config is durable and Git-friendly.
- Runtime state is ephemeral and resettable.
- Session names are the main operator handle.
- Devices, transports, and presets are reusable building blocks.
- Control actions are typed commands, not mutable config rewrites.
- Trace and fault surfaces are bounded, explicit, and opt-in.

## Canonical Workflow

1. Write a `ModbusSimulatorConfig` file.
2. Run `mabi validate modbus-config <file>`.
3. Run `mabi inspect modbus-schema` to learn the typed surface.
4. Run `mabi inspect modbus-config <file>` to confirm the compiled sessions.
5. Start a session with `mabi serve modbus --config <file> --session <name>`.
6. Use `mabi control modbus ...` commands to inspect points, read/write values, tail traces, or apply fault presets.

## Reference Mapping

- `PyModbus`: named server/device config, simulator-oriented datastore modeling, response manipulation concepts
- `Mockoon`: file-backed environments, resettable runtime state, log-and-events mindset
- `WireMock`: runtime control duality, scenario/reset semantics
- `Prism`: schema-first CLI and validation-first operator flow

## Delivery Stages

### Stage 1

- canonical `ModbusSimulatorConfig`
- named transports, devices, sessions, presets
- `serve modbus --config --session`
- `validate modbus-config`
- `inspect modbus-schema`
- `inspect modbus-config`
- in-process control ports for session, point, trace, and fault operations

### Stage 2

- richer point catalog filters
- import/export flows for register state
- trace subscriptions for long-running operator sessions
- more discoverable schema rendering in CLI

### Stage 3

- reusable control-port patterns across protocol crates
- optional external admin transport if the in-process surface proves stable

## Non-Goals

- no web UI in this phase
- no external HTTP admin API in this phase
- no free-form scripting DSL
- no config mutation through control commands

## Success Criteria

- A new operator can go from config file to running simulator without reading internal Rust types.
- Session validation failures explain the exact broken reference.
- Inspect output shows the reusable structure of a config before launch.
- Live control works with the same point semantics regardless of dense or sparse storage.
- Runtime reset clears ephemeral state without rewriting config.

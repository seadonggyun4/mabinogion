# Modbus Simulator Control Plane Spec

## Goal

The control plane exists to make a running Modbus simulator inspectable and operable without editing config files or embedding custom scripts.

The first implementation is in-process and CLI-driven.

## Control Ports

### `SessionControlPort`

Lifecycle-oriented session actions:

- `status`
- `snapshot`
- `reset`

Expected behavior:

- `status` reports session name, service count, device count, trace state, and active fault preset
- `snapshot` returns current runtime session snapshots
- `reset` rebuilds ephemeral session state without mutating the source config

### `PointCatalogPort`

Catalog and filter surface for operator discovery.

Current query axes:

- device id
- tag filters
- label filters

Expected output:

- stable point descriptors
- unit id when present
- point metadata including data type, access mode, and tags

### `RegisterControlPort`

Operator read/write surface.

Selection modes:

- symbolic point id
- `unit + register type + address`

Expected behavior:

- reads and writes must resolve through the same device semantics used by runtime controllers
- dense and sparse backends must present the same logical point surface

### `TracePort`

Bounded in-memory trace surface.

Operations:

- `tail`
- `clear`
- `subscribe`

Expected behavior:

- recent control-plane reads and writes are retained in a ring buffer
- trace state is resettable
- trace does not imply long-term storage

### `FaultPresetPort`

Named runtime fault control.

Operations:

- `apply`
- `clear`
- `list`
- `active`

Expected behavior:

- presets are named in config
- applying or clearing a preset rebuilds ephemeral runtime state, not config files

## CLI Mapping

Canonical commands:

- `mabi serve modbus --config <file> --session <name>`
- `mabi validate modbus-config <file>`
- `mabi inspect modbus-schema`
- `mabi inspect modbus-config <file>`
- `mabi control modbus --config <file> --session <name> session status`
- `mabi control modbus --config <file> --session <name> session reset`
- `mabi control modbus --config <file> --session <name> session snapshot`
- `mabi control modbus --config <file> --session <name> point list`
- `mabi control modbus --config <file> --session <name> point read ...`
- `mabi control modbus --config <file> --session <name> point write ...`
- `mabi control modbus --config <file> --session <name> trace tail`
- `mabi control modbus --config <file> --session <name> trace clear`
- `mabi control modbus --config <file> --session <name> faults apply <name>`
- `mabi control modbus --config <file> --session <name> faults clear`

## Operator Flow

1. Validate config.
2. Inspect schema when authoring or reviewing new files.
3. Inspect config to confirm named sessions and reusable components.
4. Serve one named session.
5. Use control commands for live point, trace, and fault work.
6. Reset the session when a clean ephemeral state is needed.

## Guardrails

- Control commands must never rewrite the source config file.
- Control commands operate on runtime state only.
- Trace is bounded in memory.
- Fault presets are named and typed; arbitrary scripting is out of scope.
- The control plane should stay generic enough to inform future cross-protocol reuse.

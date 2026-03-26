# Modbus Simulator Config Spec

## Canonical Type

The canonical file-backed surface for `mabi-modbus` is `ModbusSimulatorConfig`.

Top-level sections:

- `defaults`
- `transports`
- `devices`
- `sessions`
- `presets`

This structure is session-centric. The runtime always launches a named session. Presets are only sugar that compile into a session profile.

## Sections

### `defaults`

Shared defaults applied during session compilation.

Current fields:

- `readiness_timeout_ms`
- `trace.enabled`
- `trace.capacity`

### `transports`

Named endpoint definitions. A session references one transport by name.

Supported variants:

- `tcp`
  - `bind`
  - `port`
  - `performance_preset`
- `rtu`
  - `config`

### `devices`

Named device bundles backed by `SimulatorProfile`.

Bundle contents:

- unit definitions
- points
- datastore selection
- tags
- identity
- broadcast-related unit flags
- timing-related device behavior when modeled in the profile

### `sessions`

Named execution units.

Session responsibilities:

- choose one transport
- attach one or more named device bundles
- optionally attach one generated preset
- configure trace behavior
- configure reset behavior
- declare named fault presets
- choose one active fault preset
- optionally override readiness timeout

### `presets`

Quickstart sugar for generated device topologies.

Current role:

- preserve the convenience of numeric generator flows
- compile into a `SimulatorProfile`
- stay below the canonical session surface

## File Formats

The loader accepts:

- YAML
- JSON
- TOML

YAML is the preferred documentation format because it is concise and diff-friendly.

## Example

```yaml
defaults:
  readiness_timeout_ms: 4000
  trace:
    enabled: true
    capacity: 128

transports:
  plant-tcp:
    kind: tcp
    bind: 127.0.0.1
    port: 1502
    performance_preset: default

devices:
  line-a:
    units:
      - unit_id: 1
        name: line-a-main
        datastore: dense
        points:
          - id: line_a_temp
            name: Line A Temperature
            register_type: holding
            address: 0
            data_type: f32
            writable: true
            tags:
              area: line-a
              kind: telemetry

sessions:
  local-dev:
    transport: plant-tcp
    devices: [line-a]
    trace:
      enabled: true
      capacity: 256
    fault_presets:
      delayed:
        enabled: true
        response_delay_ms: 50

presets: {}
```

## Compilation Rules

- A config must define at least one session.
- Every session transport reference must exist.
- A session must reference at least one device bundle or one preset.
- Every device bundle reference must exist.
- Every preset reference must exist.
- Every active fault preset must exist inside the session.
- Unit IDs must be unique inside a compiled session profile.
- Point IDs must be unique inside each compiled unit.

## Compatibility Surface

Existing `Builder/Profile` and numeric CLI flows are still supported, but they are compatibility paths. Internally they should converge toward `preset -> session` compilation instead of becoming separate first-class surfaces.

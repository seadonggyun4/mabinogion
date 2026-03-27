# OPC UA Simulator Config Spec

## Canonical Type

The canonical file-backed surface for `mabi-opcua` is `OpcUaSimulatorConfig`.

Top-level sections:

- `defaults`
- `transports`
- `nodesets`
- `models`
- `devices`
- `sessions`
- `presets`

This structure is session-centric. The runtime always launches a named session. Presets are convenience sugar that compile into a generated session.

## Sections

### `defaults`

Shared defaults applied during session compilation.

Current fields:

- `namespace_uri`
- `readiness_timeout_ms`
- `server_name`
- `min_publishing_interval_ms`
- `security_profile`

### `transports`

Named OPC UA endpoint definitions. A session references one transport by name.

Current fields:

- `bind`
- `port`
- `endpoint_path`
- `security_profile`
- `server_name`

### `nodesets`

Named NodeSet2 import sources.

Supported source kinds:

- `file`
  - `path`
  - `namespace_uri_override`
- `embedded`
  - `alias`
  - `namespace_uri_override`

Remote fetch is intentionally unsupported in this phase.

### `models`

Address-space composition units.

Model responsibilities:

- reference one or more NodeSet2 sources
- add overlay nodes
- add structural references
- declare structural methods and events
- optionally hint a namespace URI

### `devices`

Named runtime-visible point bindings over a model.

Device responsibilities:

- choose one model
- bind stable point ids to OPC UA node ids
- carry tags and display metadata
- override writable / historizing / sampling metadata when needed

### `sessions`

Named execution units.

Session responsibilities:

- choose one transport
- reference one or more models
- optionally reference named devices
- optionally attach one preset
- optionally override readiness timeout
- declare control-plane defaults

### `presets`

Quickstart sugar for generated address spaces.

Current role:

- preserve the convenience of numeric `serve opcua --nodes ...` flows
- compile into a generated model + device + named session
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
  namespace_uri: urn:mabinogion:opcua:demo
  readiness_timeout_ms: 4000
  server_name: Demo OPC UA Server
  min_publishing_interval_ms: 100

transports:
  local:
    bind: 127.0.0.1
    port: 4840
    endpoint_path: /sim
    security_profile: None

nodesets:
  machine:
    kind: file
    path: ./machine.xml

models:
  machine:
    nodesets: [machine]

devices:
  machine-a:
    model: machine
    node_bindings:
      - point_id: machine_temperature
        node_id: ns=2;s=Machine.Temperature
        label: Temperature

sessions:
  local-dev:
    transport: local
    devices: [machine-a]
    service_name: demo-opcua

presets: {}
```

## Compilation Rules

- A config must define at least one session.
- Every session transport reference must exist.
- A session must reference at least one model, device, or preset.
- Every model, device, and preset reference must exist.
- Every NodeSet source must resolve to a readable file or a supported embedded alias.
- Imported and overlay nodes must not collide on final `NodeId`.
- Added references must resolve to real nodes or standard namespace nodes.
- Point ids must be unique inside a compiled session.
- Device bindings must target variable nodes.
- Method and event declarations are structural only in this phase.

## Compatibility Surface

Existing `nodes::*Builder`, `OpcUaDeviceBuilder`, `OpcUaDeviceFactory`, and numeric `serve opcua` flows are still supported, but they are compatibility paths. Internally they should converge toward `preset -> session` compilation instead of becoming separate first-class surfaces.

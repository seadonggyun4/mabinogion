# OPC UA Simulator Control-Plane Spec

## Scope

The canonical in-process control surface for `mabi-opcua` is intentionally small in this phase.

Current stable operations:

- `session status`
- `session snapshot`
- `session reset`
- `node list`
- `node read`
- `node write`

The control plane is built on top of:

- `CompiledOpcUaSession`
- `OpcUaControlSession`
- `RuntimeSession`

## CLI Entry Points

- `mabi inspect opcua-schema`
- `mabi inspect opcua-config <file>`
- `mabi validate opcua-config <file>`
- `mabi serve opcua --config <file> --session <name>`
- `mabi control opcua --config <file> --session <name> session status`
- `mabi control opcua --config <file> --session <name> session snapshot`
- `mabi control opcua --config <file> --session <name> session reset`
- `mabi control opcua --config <file> --session <name> node list`
- `mabi control opcua --config <file> --session <name> node read --point <id>`
- `mabi control opcua --config <file> --session <name> node read --node-id <node-id>`
- `mabi control opcua --config <file> --session <name> node write --point <id> <value>`
- `mabi control opcua --config <file> --session <name> node write --node-id <node-id> <value>`

## Status Surface

`session status` returns:

- session name
- service count
- device count
- node count
- namespace count
- whether raw node access is allowed

`session snapshot` returns:

- the same status surface
- current runtime service snapshots

`session reset`:

- stops the current runtime session
- rebuilds the runtime from the same compiled session
- returns a fresh snapshot

## Node Catalog Surface

`node list` returns one descriptor per compiled point binding.

Current fields:

- `device_id`
- `point_id`
- `node_id`
- `browse_name`
- `display_name`
- `node_class`
- `writable`
- `historizing`
- `sampling_interval_ms`

## Read / Write Surface

Read and write targets can be selected in two ways:

- stable point binding: `--point <id>`
- raw NodeId fallback: `--node-id <node-id>`

If `allow_raw_node_access` is disabled in the compiled session control defaults, raw NodeId fallback is rejected.

Values are accepted as:

- JSON literals where possible
- raw strings as a fallback

## Out of Scope

The following are intentionally not canonical yet:

- browse tree editing
- method body injection
- history administration
- subscription live tuning
- runtime graph mutation
- remote control transport / IPC

Those will sit on top of the same compiled-session surface later instead of creating a second configuration model.

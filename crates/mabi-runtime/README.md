# mabi-runtime

Shared runtime contracts and service orchestration for the Mabinogion simulator stack.

`mabi-runtime` provides the common runtime traits, session handles, control surfaces, and service wiring used by the protocol crates in this workspace.

For the 1.6.x release line, OPC UA, Modbus, BACnet/IP, and KNXnet/IP are substantially stabilized with deterministic regression lanes and optional external/interop lanes where applicable.

## Installation

```toml
[dependencies]
mabi-runtime = "1.6.1"
```

## What It Provides

- Shared runtime contracts for launching and controlling simulator services
- Session and lifecycle abstractions reused across protocol implementations
- Common orchestration utilities for workspace crates such as `mabi-modbus` and `mabi-opcua`

## Versioning

The crate follows the workspace release version. For this release line, the published version is `1.6.1`.

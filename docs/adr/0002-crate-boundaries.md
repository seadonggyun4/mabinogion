# ADR 0002: Workspace Boundaries

## Status
Accepted

## Context
The workspace needed clearer boundaries before protocol-specific performance work could proceed safely.

## Decision
Adopt the following boundary model:

- `mabi-core`: shared domain types and low-level abstractions
- `mabi-runtime`: service lifecycle and controller/runtime contracts
- `mabi-modbus`, `mabi-opcua`, `mabi-bacnet`, `mabi-knx`: protocol implementations with normalized root surfaces
- `mabi-scenario`: control-plane scenario execution over `DevicePort`
- `mabi-chaos`: control-plane fault orchestration and middleware
- `mabi-cli`: composition root and driver registry consumer

Each protocol crate should expose a stable architecture-facing surface:
`Config`, `Builder`, `Server`, `Device`, `Factory`, `Stats`, `Error`, `Result`.

## Consequences
- Public architecture-level usage becomes more uniform.
- Internal protocol modules remain free to evolve without leaking as default entry points.

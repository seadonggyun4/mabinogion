# ADR 0001: Shared Runtime Contracts

## Status
Accepted

## Context
`mabi-core` contained both domain concepts and process orchestration concerns, which made scenario control, protocol lifecycle handling, and CLI execution difficult to align.

## Decision
Introduce `mabi-runtime` as the shared runtime contract crate.

`mabi-runtime` owns:
- `ManagedService`
- `ServiceContext`
- `ServiceHandle`
- `ServiceStatus` and `ServiceSnapshot`
- `DevicePort` and `DeviceRegistry`
- `ProtocolDriver` and `ProtocolDriverRegistry`

`mabi-core` remains the shared domain kernel for protocol enums, values, device handles, and shared error types.

## Consequences
- Protocol lifecycle orchestration now has one shared contract.
- Controllers such as `mabi-scenario` can target a protocol-agnostic `DevicePort`.
- CLI and tests can supervise services through one handle type.

# ADR 0003: CLI Composition Model

## Status
Accepted

## Context
The CLI had protocol-specific lifecycle logic spread across bespoke command implementations.

## Decision
Move toward a registry-driven CLI model built around:

- reusable clap argument groups
- protocol descriptors from `ProtocolDriverRegistry`
- service supervision through `ServiceHandle`

The target top-level command graph is:

- `mabi serve <protocol>`
- `mabi scenario run <file>`
- `mabi chaos run <file>`
- `mabi inspect ...`
- `mabi validate ...`

## Consequences
- New protocol entry points can be added behind driver registration.
- The CLI surface aligns with the workspace runtime contracts instead of per-protocol ad hoc logic.

# mabi-modbus

Modbus TCP/RTU runtime driver for the Mabinogion protocol resilience engine.

## What this crate owns

- Modbus launch/config handling for local `mabi serve modbus` sessions.
- Runtime integration through the shared protocol driver registry.
- Modbus capability/profile coverage documented by the Unified Readiness Contract.

## How it fits in Mabinogion

`mabi-modbus` provides Modbus protocol execution for local simulator use and
for resilience scenarios that target Modbus function codes, register maps,
multi-unit behavior, exceptions, timeout behavior, partial responses, and slow
device lanes.

## Versioning / contracts

```toml
[dependencies]
mabi-modbus = "1.7.1"
```

The crate follows the workspace release version. Protocol capability
metadata is reported through `mabi --format json version` and documented in
`docs/protocol-readiness/`.

## Not owned here

`mabi-modbus` does not score results, publish proof
reports, issue certification, or replace official certification programs.

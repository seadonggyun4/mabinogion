# mabi-knx

KNXnet/IP runtime driver for the Mabinogion protocol resilience engine.

## What this crate owns

- KNXnet/IP launch/config handling for local `mabi serve knx` sessions.
- Group object I/O, DPT codec, tunneling lifecycle, sequence validation, and heartbeat execution surfaces.
- KNX capability/profile coverage documented by the Unified Readiness Contract.

## How it fits in Mabinogion

`mabi-knx` provides KNXnet/IP protocol execution for local simulator use and for
Mabinogion trials that need stable group value and tunnel lifecycle behavior.

## Versioning / contracts

```toml
[dependencies]
mabi-knx = "1.7.0"
```

The crate follows the workspace release version. KNX capability metadata is
reported through `mabi --format json version` and documented in
`docs/protocol-readiness/`.

## Not owned here

`mabi-knx` does not define trial suites, score trial results, publish proof
reports, issue certification, or replace official certification programs.

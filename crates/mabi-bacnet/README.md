# mabi-bacnet

BACnet/IP runtime driver for the Mabinogion protocol resilience engine.

## What this crate owns

- BACnet/IP launch/config handling for local `mabi serve bacnet` sessions.
- Device object, object registry, COV, segmentation, BBMD/FDR, and duplicate-handling execution surfaces.
- BACnet capability/profile coverage documented by the Unified Readiness Contract.

## How it fits in Mabinogion

`mabi-bacnet` provides BACnet/IP protocol execution for local simulator use and
for resilience scenarios that need deterministic readiness lanes and optional
interop verification.

## Versioning / contracts

```toml
[dependencies]
mabi-bacnet = "1.7.1"
```

The crate follows the workspace release version. BACnet capability metadata is
reported through `mabi --format json version` and documented in
`docs/protocol-readiness/`.

## Not owned here

`mabi-bacnet` does not score results, publish proof
reports, issue certification, or replace official certification programs.

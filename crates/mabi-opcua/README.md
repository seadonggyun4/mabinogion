# mabi-opcua

OPC UA runtime driver for the Mabinogion protocol resilience engine.

## What this crate owns

- OPC UA session/config execution for local `mabi serve opcua` sessions.
- Address space, subscription, secure channel, reconnect, timeout, and operation-limit surfaces used by the runtime.
- OPC UA capability/profile coverage documented by the Unified Readiness Contract.

## How it fits in Mabinogion

`mabi-opcua` provides OPC UA protocol execution for local simulator use and for
Mabinogion trials that need stable session lifecycle and evidence-producing
runtime behavior.

## Versioning / contracts

```toml
[dependencies]
mabi-opcua = "1.7.0"
```

The crate follows the workspace release version. OPC UA capability metadata is
reported through `mabi --format json version` and documented in
`docs/protocol-readiness/`.

## Not owned here

`mabi-opcua` does not define trial suites, score trial results, publish proof
reports, issue certification, or replace official certification programs.

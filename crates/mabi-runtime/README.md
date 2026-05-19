# mabi-runtime

Runtime/session contract layer for the Mabinogion protocol resilience engine.

## What this crate owns

- Runtime service lifecycle, readiness, stop, and snapshot contracts.
- Protocol driver registry types and session orchestration.
- `runtime-contract-v1`, `snapshot-metadata-v1`, Run Evidence primitives, and artifact metadata types.

## How it fits in Mabinogion

`mabi-runtime` is the shared execution substrate used by Modbus, OPC UA,
BACnet/IP, KNXnet/IP, `mabi doctor`, `mabi serve`, and future Mabinogion trials
runner surfaces. It lets Forge and Trials consume a stable runtime shape without
depending on protocol-specific internals.

## Versioning / contracts

```toml
[dependencies]
mabi-runtime = "1.7.0"
```

The crate follows the workspace release version. Runtime compatibility is
tracked separately through `RUNTIME_CONTRACT_VERSION`,
`SNAPSHOT_METADATA_VERSION`, `RUN_EVIDENCE_SCHEMA_VERSION`, and
`TRIAL_ARTIFACT_CONTRACT_VERSION`.

## Not owned here

`mabi-runtime` does not define trial suites, score trial results, publish proof
reports, issue certification, or replace official certification programs.

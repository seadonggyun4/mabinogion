# mabi-core

Shared domain kernel for the Mabinogion protocol resilience engine.

## What this crate owns

- Common protocol enums, values, device abstractions, tags, capabilities, and release version helpers.
- Core utilities reused by protocol crates and the installed `mabi` CLI.
- The canonical product release version exported through `mabi_core::RELEASE_VERSION`.

## How it fits in Mabinogion

`mabi-core` is the protocol-agnostic foundation underneath runtime drivers,
scenario execution, and chaos orchestration. It supports local protocol
simulator use while keeping shared types stable for external consumers.

## Versioning / contracts

```toml
[dependencies]
mabi-core = "1.7.1"
```

The crate follows the workspace release version. Compatibility contracts are
owned by higher-level runtime, CLI, evidence, and release metadata documents.

## Not owned here

`mabi-core` does not score results, publish proof
reports, issue certification, or replace official certification programs.

# mabi-chaos

Fault orchestration support for the Mabinogion protocol resilience engine.

## What this crate owns

- Chaos configuration and runtime controls for local protocol sessions.
- Fault scheduling and injection primitives used by `mabi chaos`.
- Support surfaces for resilience exercises over protocol/session execution.

## How it fits in Mabinogion

`mabi-chaos` lets local users and future runner integrations exercise protocol
behavior under controlled fault conditions. It complements Mabinogion trials by
providing execution mechanics, not scoring policy.

## Versioning / contracts

```toml
[dependencies]
mabi-chaos = "1.7.1"
```

The crate follows the workspace release version. Runner-facing chaos behavior
is mediated through `mabi-cli` and runtime evidence export.

## Not owned here

`mabi-chaos` does not define trial suites, score trial results, publish proof
reports, issue certification, or replace official certification programs.

# mabi-cli

Installed `mabi` command for local operators, Imugi runners, and Mabinogion
trials integration.

## What this crate owns

- The installed CLI surface: `doctor`, `serve`, `inspect`, `validate`, `scenario`, `chaos`, `control`, `generate`, and `version`.
- Machine-readable runner envelopes for Imugi and Trials.
- Version metadata that reports engine release, protocol capabilities, contracts, and trial compatibility hints.

## How it fits in Mabinogion

`mabi-cli` is the user-facing and runner-facing entrypoint for the protocol
resilience engine. Local users can start protocol simulators directly, while
Imugi and Trials can parse JSON/YAML/compact output from stable runner-facing
commands.

Legacy compatibility note: current implementation docs and tests may still
refer to pre-Imugi runners until Imugi backend PHASE 10 is implemented.

## Versioning / contracts

Install the CLI with:

```bash
cargo install mabi-cli
mabi doctor
mabi --format json version
```

Runner-facing output follows `local-runner-contract-v1` and
`cli-output-envelope-v1`. Version output also reports runtime, readiness,
evidence, artifact, and version metadata contract versions.

## Not owned here

`mabi-cli` does not define trial suites, score trial results, publish proof
reports, issue certification, or replace official certification programs.

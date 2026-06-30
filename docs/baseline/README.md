# Current Baseline

This directory records the PHASE 0 baseline for `mabinogion`.

The goal is not to add simulator behavior. The goal is to make the current
runtime, CLI, and protocol verification surface explicit before later phases
freeze Imugi and Trials contracts on top of it.

`current-baseline.yaml` is the machine-readable source of truth. This README is
the human-facing map for maintainers.

## Ownership Boundary

`mabinogion` owns protocol/session execution, the shared runtime contract, the
installed CLI surface, and the primitives needed to export run evidence.

`mabinogion` does not own trial definition authoring, scoring policy, proof
report publication, or certification issuance. Those belong to the surrounding
Trials and Imugi repos.

## Workspace Responsibility Map

| Crate | Current responsibility |
| --- | --- |
| `mabi-core` | Shared domain kernel for protocol values, device handles, and common error types. |
| `mabi-runtime` | Shared service lifecycle, driver registry, readiness, stop, and snapshot contracts. |
| `mabi-modbus` | Modbus simulator implementation and normalized protocol driver. |
| `mabi-opcua` | OPC UA simulator implementation and normalized protocol driver. |
| `mabi-bacnet` | BACnet/IP simulator implementation with verification contract coverage. |
| `mabi-knx` | KNXnet/IP simulator implementation with verification contract coverage. |
| `mabi-scenario` | Scenario parsing and control-plane execution over runtime device ports. |
| `mabi-chaos` | Fault orchestration and middleware for simulator sessions. |
| `mabi-cli` | CLI composition root, installed command surface, and protocol registry consumer. |

## Runtime Baseline

The shared runtime contract is implemented in `mabi-runtime` and frozen as
`runtime-contract-v1`. The detailed contract lives in
`docs/runtime/runtime-contract.md` and `docs/runtime/runtime-contract.yaml`.

| Area | Current source | Baseline symbols |
| --- | --- | --- |
| Service lifecycle | `crates/mabi-runtime/src/service.rs` | `ManagedService`, `ServiceHandle`, `ServiceStatus`, `ServiceSnapshot`, `ServiceRuntimeMetadata`, `ServiceReadinessReport`, `ServiceState`, `RuntimeError`, `RuntimeErrorKind`, `RuntimeErrorInfo` |
| Runtime session | `crates/mabi-runtime/src/session.rs` | `RuntimeSessionSpec`, `RuntimeSession`, `RuntimeSessionSnapshot` |
| Protocol registry | `crates/mabi-runtime/src/driver.rs` | `ProtocolDescriptor`, `ProtocolLaunchSpec`, `ProtocolDriverRegistry` |

The current lifecycle baseline is:

1. Construct a runtime session.
2. Start managed services.
3. Wait for readiness.
4. Capture service snapshots.
5. Stop managed services.

PHASE 1 froze this contract rather than recreating it. Runtime errors now expose
`protocol_error`, `config_error`, `bind_error`, `timeout`, and `internal_error`
through `RuntimeError::kind()` and `RuntimeError::info()`. Service snapshots
returned through runtime handles include stable `_runtime` metadata for Imugi and
Trials consumers.

## Run Evidence Baseline

PHASE 4 adds `run-evidence-schema-v1` and `trial-artifact-contract-v1` as the
execution evidence layer on top of `runtime-contract-v1`.

| Artifact | Purpose |
| --- | --- |
| `docs/evidence/run-evidence-schema.yaml` | Machine-readable Run Evidence Schema. |
| `docs/evidence/trial-artifact-contract.yaml` | Failure replay artifact metadata and visibility policy. |
| `docs/evidence/sample-run-evidence.json` | Imugi/Trials Proof Report input sample. |

`mabinogion` owns evidence serialization and runtime snapshot export. It still
does not own scoring, public proof publication, or certification issuance.

## Release Version Baseline

PHASE 5 adds `version-metadata-contract-v1` so Imugi and Trials can evaluate an
installed engine without scraping human output.

| Artifact | Purpose |
| --- | --- |
| `docs/release/version-metadata-contract.yaml` | Machine-readable version output contract. |
| `docs/release/compatibility-matrix.yaml` | Engine, protocol capability, contract, and trial-suite compatibility metadata. |
| `docs/release/release-checklist.md` | Release-candidate metadata checklist. |
| `docs/release/changelog-policy.md` | Breaking-change categories for runner compatibility. |

`mabinogion` owns release metadata export. Imugi and Trials own engine/trial
suite allow or deny decisions.

## CLI Baseline

The installed CLI surface is already centralized in `crates/mabi-cli/src/main.rs`
and backed by `crates/mabi-cli/src/runtime_registry.rs`.

| Command | Current status | Baseline role |
| --- | --- | --- |
| `serve` | Implemented | Shared runtime protocol launch. |
| `scenario` | Implemented | Scenario controller commands. |
| `chaos` | Implemented | Fault orchestration commands. |
| `inspect` | Implemented | Runtime and schema inspection. |
| `validate` | Implemented | Scenario and config validation. |
| `control` | Implemented | Runtime control commands for simulator sessions. |
| `generate` | Implemented | Deterministic generation from canonical configs. |
| `doctor` | Implemented | Installed CLI and built-in protocol smoke report. |
| `version` | Implemented structured output | Workspace release, protocol, and contract version reporting. |

PHASE 3 freezes the local runner contract around existing CLI commands in
`docs/cli/local-runner-contract.yaml` and `docs/cli/local-runner-contract.md`.
Runner-facing machine output for `doctor`, `inspect`, `validate`, and `version`
uses `local-runner-contract-v1`. `mabi trial run` is still future work and
should consume execution specs owned by `mabinogion-trials`; it should not own
trial definition.

## Protocol Verification Baseline

PHASE 2 normalized all four protocol verification surfaces around
`unified-readiness-contract-v1`. The shared contract lives in
`docs/protocol-readiness/`, while each protocol keeps its own
`verification-contract.yaml` and `verification-baseline.md`.

| Protocol | Current verification state | PHASE 0 decision |
| --- | --- | --- |
| BACnet | `docs/bacnet-simulator/verification-contract.yaml` includes a `unified_readiness` overlay. | Treat as contract-present baseline. |
| KNX | `docs/knx-simulator/verification-contract.yaml` includes a `unified_readiness` overlay. | Treat as contract-present baseline. |
| Modbus | `docs/modbus-simulator/verification-contract.yaml` and `verification-baseline.md` now exist. | Treat as contract-present baseline. |
| OPC UA | `docs/opcua-simulator/verification-contract.yaml` and `verification-baseline.md` now exist. | Treat as contract-present baseline. |

Every protocol readiness profile now provides:

- capability id
- profile id
- lane
- coverage status
- optional interop policy
- trial profile id
- trial level
- required evidence
- engine requirement
- `forge_display_label`, retained as a legacy field until contract v2

## Test And Verification Policy

PHASE 0 keeps service operation CI separate from development checks.

| Lane | Status | Meaning |
| --- | --- | --- |
| Deterministic local lane | Supported | `cargo test --workspace` remains the local Rust workspace validation path. |
| Deploy-blocking operational CI | Strategy-doc owned | Release binary, installed CLI smoke, version contract, and evidence compatibility checks are described in the repo plan. |
| Manual/nightly operational verification | Optional | External peer interop, Docker matrices, and performance soak are too expensive or environment-dependent for this baseline. |

This directory adds one drift guard:

```console
cargo test -p mabi-cli --test baseline_docs
```

That test parses `current-baseline.yaml` and checks that the documented crate
paths, runtime symbols, CLI commands, and all four protocol verification
contracts still exist. PHASE 2 adds a second drift guard:

```console
cargo test -p mabi-cli --test readiness_contract
```

That test parses the Unified Readiness Contract and each protocol contract,
then verifies required fields, enum values, profile IDs, capability references,
trial profile IDs, and documented source/test refs.

PHASE 3 adds a third drift guard:

```console
cargo test -p mabi-cli --test local_runner_contract
```

That test parses the Local Runner Contract and checks the envelope fields, exit
code categories, runner-facing commands, future `trial run` execution spec, and
key CLI smoke outputs.

PHASE 4 adds a fourth drift guard:

```console
cargo test -p mabi-cli --test run_evidence_contract
```

That test parses the evidence contracts, validates the sample evidence, and
checks that `mabi --format json version` reports the evidence contract versions.

PHASE 5 adds a fifth drift guard:

```console
cargo test -p mabi-cli --test version_discipline
```

That test parses the release contracts, checks the compatibility matrix against
the workspace release version, and verifies that CLI `version` metadata matches
the documented protocol readiness profiles.

## Maintenance Rules

- Update `current-baseline.yaml` in the same change that adds, removes, or
  renames workspace crates, runtime symbols, or CLI command variants.
- Keep all four protocol entries marked as `contract_present` unless their
  verification contract files are intentionally removed.
- Update `docs/protocol-readiness/protocol-readiness-matrix.yaml` when a
  protocol readiness profile is added, removed, or renamed.
- Update `docs/evidence/run-evidence-schema.yaml` and the Rust evidence types
  together when run evidence fields are added, removed, or renamed.
- Update `docs/release/compatibility-matrix.yaml`, `docs/release/version-metadata-contract.yaml`,
  and the CLI version metadata together when release, contract, protocol
  capability, or trial-suite compatibility fields change.
- Do not use this baseline to define scoring, certification, or public proof
  publication policy.

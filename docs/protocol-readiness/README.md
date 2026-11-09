# Unified Protocol Readiness Contract

This directory owns `unified-readiness-contract-v2`, the shared metadata
contract for `mabinogion` protocol engines.

The contract lets local runners refer to a stable protocol capability/profile
without asking each
protocol crate to invent different names, lane labels, or evidence language.

## Artifacts

| Artifact | Role |
| --- | --- |
| `unified-readiness-contract.yaml` | Versioned field, enum, ownership, and protocol contract policy. |
| `protocol-readiness-matrix.yaml` | Compact cross-protocol map for external consumers. |
| `docs/*-simulator/verification-contract.yaml` | Protocol-owned capability/profile source of truth. |
| `docs/*-simulator/verification-baseline.md` | Human baseline explaining current coverage and gaps. |

## Required Profile Fields

Every unified profile must provide:

| Field | Meaning |
| --- | --- |
| `capability_id` | Stable semantic capability, local to a protocol contract. |
| `profile_id` | Stable protocol profile id used by engine-facing tests and docs. |
| `lane` | Evidence collection lane: deterministic, ignored interop, release-only perf, or artifact-only capture. |
| `coverage_status` | Current coverage state: implemented, partial, planned, unsupported recorded, or future. |
| `optional_interop_policy` | How optional peer or heavy verification should be run. |
| `required_evidence` | Evidence types the engine can export or help produce. |
| `engine_requirement` | Minimum engine surface needed to execute the profile. |
| `forge_display_label` | Stable human label retained as a legacy field for external consumer reports. |

## Protocol Mapping

| Protocol | Readiness focus |
| --- | --- |
| OPC UA | Session lifecycle, secure channel renewal, reconnect, subscription, timeout, malformed service response, operation limit. |
| BACnet/IP | Discovery, object/property I/O, COV, segmentation, BBMD/FDR, duplicate handling. |
| Modbus | Function code, register map, multi-unit, exception response, timeout, partial response, slow device. |
| KNXnet/IP | Discovery, tunneling lifecycle, group value I/O, DPT codec, sequence validation, heartbeat/connection state. |

## Ownership Boundary

`mabinogion` owns protocol/session execution and exports readiness metadata for
local runners. Job orchestration, proof publication, and certification
decisions remain outside this repository.

This contract must not claim official certification equivalence. It is a stable
engine-readiness input for local protocol scenarios.

## Maintenance Rules

- Add or update a protocol profile in the protocol-specific
  `verification-contract.yaml` first.
- Mirror only the compact profile identity in
  `protocol-readiness-matrix.yaml`.
- Keep profile IDs stable after publication.
- Use `planned`, `partial`, `unsupported_recorded`, or `future` instead of
  hiding known gaps.
- Keep interop, perf, and capture evidence outside the deterministic lane unless
  it is self-contained and cheap.

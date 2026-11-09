# Modbus Verification Baseline

This baseline records the PHASE 2 readiness contract for `mabi-modbus`.

The Modbus simulator already has deterministic coverage for core protocol
behavior, register maps, multi-unit addressing, exception responses, malformed
frames, and transport behavior. PHASE 2 turns those existing surfaces into a
stable profile contract for local runners and external consumers.

## Contract Files

| Artifact | Role |
| --- | --- |
| `verification-contract.yaml` | Machine-readable Modbus capability, profile, and readiness mapping. |
| `../protocol-readiness/unified-readiness-contract.yaml` | Shared field and enum contract for all protocols. |
| `../protocol-readiness/protocol-readiness-matrix.yaml` | Compact cross-protocol profile matrix. |

## Unified Profiles

| Profile | Coverage | Lane |
| --- | --- | --- |
| `function_code_core` | Implemented | Deterministic |
| `register_map` | Implemented | Deterministic |
| `exception_response` | Implemented | Deterministic |
| `multi_unit` | Implemented | Deterministic |
| `timeout` | Partial | Deterministic |
| `partial_response` | Partial | Deterministic |
| `slow_device` | Partial | Deterministic with release-only optional perf evidence |

## Current Gaps

- No external Modbus interop plane is contract-present in PHASE 2.
- Slow-device evidence is readiness metadata, not a default performance gate.
- Partial-response and timeout profiles are marked `partial` because their
  deterministic coverage is incomplete.

`mabinogion` owns protocol/session execution and readiness evidence export. It
does not own scoring policy, proof publication, or
certification issuance.

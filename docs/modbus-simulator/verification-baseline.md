# Modbus Verification Baseline

This baseline records the PHASE 2 readiness contract for `mabi-modbus`.

The Modbus simulator already has deterministic coverage for core protocol
behavior, register maps, multi-unit addressing, exception responses, malformed
frames, and transport behavior. PHASE 2 turns those existing surfaces into a
stable profile contract that `mabinogion-trials` can reference later.

## Contract Files

| Artifact | Role |
| --- | --- |
| `verification-contract.yaml` | Machine-readable Modbus capability, profile, and readiness mapping. |
| `../protocol-readiness/unified-readiness-contract.yaml` | Shared field and enum contract for all protocols. |
| `../protocol-readiness/protocol-readiness-matrix.yaml` | Compact cross-protocol profile matrix. |

## Unified Profiles

| Profile | Level | Coverage | Lane |
| --- | --- | --- | --- |
| `function_code_core` | 1 | Implemented | Deterministic |
| `register_map` | 1 | Implemented | Deterministic |
| `exception_response` | 1 | Implemented | Deterministic |
| `multi_unit` | 2 | Implemented | Deterministic |
| `timeout` | 2 | Partial | Deterministic |
| `partial_response` | 2 | Partial | Deterministic |
| `slow_device` | 2 | Partial | Deterministic with release-only optional perf evidence |

## Current Gaps

- No external Modbus interop plane is contract-present in PHASE 2.
- Slow-device evidence is readiness metadata, not a default performance gate.
- Partial-response and timeout profiles are intentionally marked `partial`
  until a future trial corpus fixes exact pass criteria.

`mabinogion` owns protocol/session execution and readiness evidence export. It
does not own trial definitions, scoring policy, proof publication, or
certification issuance.

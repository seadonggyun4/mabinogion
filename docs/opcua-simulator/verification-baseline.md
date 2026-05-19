# OPC UA Verification Baseline

This baseline records the PHASE 2 readiness contract for `mabi-opcua`.

The OPC UA simulator already has session-centric runtime support, secure
channel renewal behavior, subscription surfaces, self-contained interop
harnesses, and release-only transport perf policy. PHASE 2 normalizes that
coverage into `unified-readiness-contract-v1` so Trials can target stable
profiles.

## Contract Files

| Artifact | Role |
| --- | --- |
| `verification-contract.yaml` | Machine-readable OPC UA capability, profile, peer, and readiness mapping. |
| `../protocol-readiness/unified-readiness-contract.yaml` | Shared field and enum contract for all protocols. |
| `../protocol-readiness/protocol-readiness-matrix.yaml` | Compact cross-protocol profile matrix. |
| `../../verification/opcua/interop-matrix.toml` | Optional ignored interop matrix for peer verification. |

## Unified Profiles

| Profile | Level | Coverage | Lane |
| --- | --- | --- | --- |
| `session_lifecycle` | 1 | Implemented | Deterministic |
| `secure_channel_renewal` | 2 | Implemented | Deterministic |
| `subscription` | 2 | Implemented | Deterministic |
| `operation_limit` | 2 | Implemented | Deterministic |
| `reconnect` | 3 | Partial | Ignored interop |
| `timeout` | 3 | Partial | Deterministic |
| `malformed_service_response` | 3 | Partial | Deterministic |

## Current Gaps

- Reconnect evidence is intentionally tied to ignored/nightly interop until
  Trials defines exact Level 3 pass criteria.
- Timeout and malformed-service-response profiles are `partial` because they
  expose engine readiness without claiming a complete public corpus yet.
- Perf contracts remain release-only and do not enter the deterministic lane.

`mabinogion` owns protocol/session execution and readiness evidence export. It
does not own trial definitions, scoring policy, proof publication, or
certification issuance.

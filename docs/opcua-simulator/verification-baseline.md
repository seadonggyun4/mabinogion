# OPC UA Verification Baseline

This baseline records the PHASE 2 readiness contract for `mabi-opcua`.

The OPC UA simulator already has session-centric runtime support, secure
channel renewal behavior, subscription surfaces, self-contained interop
harnesses, and release-only transport perf policy. PHASE 2 normalizes that
coverage into `unified-readiness-contract-v2` for stable protocol profiles.

## Contract Files

| Artifact | Role |
| --- | --- |
| `verification-contract.yaml` | Machine-readable OPC UA capability, profile, peer, and readiness mapping. |
| `../protocol-readiness/unified-readiness-contract.yaml` | Shared field and enum contract for all protocols. |
| `../protocol-readiness/protocol-readiness-matrix.yaml` | Compact cross-protocol profile matrix. |
| `../../verification/opcua/interop-matrix.toml` | Optional ignored interop matrix for peer verification. |

## Unified Profiles

| Profile | Coverage | Lane |
| --- | --- | --- |
| `session_lifecycle` | Implemented | Deterministic |
| `secure_channel_renewal` | Implemented | Deterministic |
| `subscription` | Implemented | Deterministic |
| `operation_limit` | Implemented | Deterministic |
| `reconnect` | Partial | Ignored interop |
| `timeout` | Partial | Deterministic |
| `malformed_service_response` | Partial | Deterministic |

## Current Gaps

- Reconnect evidence is tied to ignored/nightly interop because it depends on
  optional peer verification.
- Timeout and malformed-service-response profiles are `partial` because they
  expose engine readiness with incomplete deterministic coverage.
- Perf contracts remain release-only and do not enter the deterministic lane.

`mabinogion` owns protocol/session execution and readiness evidence export. It
does not own scoring policy, proof publication, or
certification issuance.

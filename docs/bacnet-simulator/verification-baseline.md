# BACnet Verification Baseline

This document defines the Phase 0 baseline for `mabi-bacnet`.
It is the human-readable companion to
[verification-contract.yaml](./verification-contract.yaml), which is the
machine-readable source of truth for capability names, profile names, peer
lanes, and policy boundaries.

## Purpose and Operating Principles

Phase 0 exists to freeze the verification vocabulary before implementation
work begins. It is not a feature phase and it does not introduce runtime
behavior changes.

The operating principles are fixed as follows:

- the production crate remains free of third-party BACnet verification
  dependencies
- verification grows in explicit layers instead of one-off test additions
- the default workspace path must stay deterministic and green
- GUI-oriented tooling stays outside the automated path in the current phase
- naming contracts are decided here so later phases consume them rather than
  reinvent them

## Current Code Baseline

The current crate already exposes the seams needed for a strong verification
plane.

### Object assembly seam

- [ObjectRegistry](../../crates/mabi-bacnet/src/object/registry.rs) is the
  canonical object assembly seam.
- It already supports registration, lookup, typed statistics, and descriptor-
  driven bulk population.
- This makes it the right anchor for deterministic object fixture builders in
  later phases.

### Service dispatch seam

- [ServiceRegistry](../../crates/mabi-bacnet/src/service/handler.rs) is the
  canonical confirmed and unconfirmed service dispatch seam.
- It already exposes supported-service snapshots and stable dispatch behavior.
- This makes it the right anchor for service capability snapshots and
  integration profile coverage.

### Canonical server assembly point

- [BACnetServer::new(...)](../../crates/mabi-bacnet/src/server/bacnet_server.rs)
  is the canonical assembly point for:
  - Device object registration
  - COV manager setup
  - default service registration
  - segmentation configuration
  - BBMD setup
  - TSM setup
- Later verification layers should launch this path rather than invent a second
  test-only server stack.

### Canonical runtime launch path

- [runtime.rs](../../crates/mabi-bacnet/src/runtime.rs) is the canonical
  `mabi-runtime` launch path.
- It already wires `BACnetServer` into `ManagedService`, `DevicePort`,
  `ProtocolDriver`, and snapshot reporting.
- This makes it the production truth for runtime-backed verification profiles.

## Current Verification State

At the current Phase 5 baseline:

- `cargo test -p mabi-bacnet` is green
- the crate has broad unit coverage across APDU, BBMD, objects, services, TSM,
  and server behavior
- `crates/mabi-bacnet/tests/` now provides deterministic loopback integration
  profiles plus a small canonical runtime smoke lane
- `verification/bacnet/` now exists as a repo-owned self-contained interop
  plane with active `bacnet-stack`, `BAC0`, `BACpypes3`, and `BACnet4J`
  harnesses
- `verification/bacnet/captures/` now exists as a manual-only corpus lane with
  seeded `YABE` and `VTS` replay artifacts
- a release-only ignored BACnet perf contract lane now exists to keep perf
  policy explicit without pushing threshold assertions into the default
  workspace path
- BACnet scope is BACnet/IP only for the current verification program
- GUI automation is out of scope
- the default developer validation path is expected to remain deterministic

## Current Missing Verification Layers

The remaining gaps are outside the production core:

- multi-container broadcast, BBMD, and foreign-device topologies still belong
  to a later interop expansion phase
- multi-container broadcast, BBMD, and foreign-device interop topologies are
  still deferred
- there are still no BACnet threshold benchmarks or performance budgets promoted
  into the default workspace suite

These gaps are intentional inputs to later phases, not evidence of production
instability in the core crate.

## Canonical Capability Model

The canonical capability IDs are frozen in
[verification-contract.yaml](./verification-contract.yaml) and are summarized
here for humans:

| Capability ID | Meaning | Current core status |
|---|---|---|
| `discovery` | Who-Is / I-Am identity and discovery flows | Implemented |
| `property_io` | ReadProperty / WriteProperty | Implemented |
| `property_multiple` | ReadPropertyMultiple / WritePropertyMultiple | Implemented |
| `cov` | SubscribeCOV, subscriptions, notifications | Implemented |
| `file_access` | Atomic read/write file and file object flows | Implemented |
| `read_range_trend_log` | ReadRange and TrendLog history access | Implemented |
| `schedule_calendar` | Schedule and Calendar object behavior | Implemented |
| `device_control_time_sync` | Device communication control, reinitialize, time sync | Implemented |
| `create_delete` | Dynamic object create/delete policy | Implemented |
| `segmentation` | Segmented APDU send/receive behavior | Implemented |
| `bbmd_foreign_device` | Cross-subnet broadcast management and foreign-device flows | Implemented in core, deferred to a later interop expansion phase |
| `tsm_duplicate_handling` | Duplicate detection and transaction state behavior | Implemented |

All of the Phase 1 capability IDs above now have deterministic integration
coverage through `crates/mabi-bacnet/tests/`.
`discovery`, `property_io`, and `property_multiple` additionally have active
interop coverage through the Phase 3 peer harness matrix.
`discovery`, `property_io`, and `tsm_duplicate_handling` additionally have
seeded manual capture coverage through the Phase 4 corpus lane.

## Canonical Profile Naming

The canonical profile IDs are also frozen in
[verification-contract.yaml](./verification-contract.yaml).

| Profile ID | Intent | Canonical first lane |
|---|---|---|
| `basic_ip` | Basic BACnet/IP server lifecycle and reachability | `deterministic` |
| `property_io` | Single-property read/write flows | `deterministic` |
| `property_multiple` | Batch property access flows | `deterministic` |
| `cov_flow` | COV subscribe/notify/cancel flows | `deterministic` |
| `file_and_trend` | File access plus TrendLog and ReadRange flows | `deterministic` |
| `schedule_calendar` | Schedule/Calendar object behavior | `deterministic` |
| `device_control` | DeviceCommunicationControl, ReinitializeDevice, time sync | `deterministic` |
| `create_delete` | Dynamic object lifecycle flows | `deterministic` |
| `segmentation` | Segmented request/response behavior | `deterministic` |
| `bbmd_fdr` | BBMD and foreign-device registration behavior | `deterministic` |
| `tsm_resilience` | Duplicate request and transaction resilience behavior | `deterministic` |

## Peer Role Baseline

The peer role split is frozen now so later phases do not reinterpret tool scope.

| Peer | Lane | Current role |
|---|---|---|
| `bacnet-stack` | `active_interop` | Active reference C peer in the current interop matrix |
| `bac0` | `active_interop` | Active controller-style Python peer in the current interop matrix |
| `bacpypes3` | `active_interop` | Active programmable Python peer in the current interop matrix |
| `bacnet4j` | `active_interop` | Active JVM peer in the current interop matrix |
| `yabe` | `capture_manual` | Manual acceptance source with a seeded replay corpus |
| `vts` | `capture_manual` | Manual protocol shell source with a seeded negative-case corpus |

## Policy Boundaries

The following policy boundaries are fixed in Phase 0:

- protocol scope: BACnet/IP only
- default workspace lane: deterministic
- external interop lane: ignored
- active interop peer set: `bacnet-stack`, `bac0`, `bacpypes3`, `bacnet4j`
- capture corpus lane: `manual_only`
- perf lane: release-only ignored
- threshold-based perf assertions are forbidden in the default workspace path
- GUI automation: out of scope
- third-party BACnet tools: verification assets only, not production
  dependencies

## Phase 0 Completion Checklist

Phase 0 is complete only when all of the following are true:

- this baseline document reflects the current crate structure and verification
  state
- [verification-contract.yaml](./verification-contract.yaml) contains the
  canonical capability, profile, peer, and policy contracts
- [verification-strategy.md](./verification-strategy.md) treats these files as
  the Phase 0 source of truth
- [README.md](./README.md) points engineers to the baseline and strategy
  documents
- later phases can consume names and lanes from the contract without inventing
  replacements

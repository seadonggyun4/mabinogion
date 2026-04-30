# KNX Verification Baseline

This document records the KNX verification baseline for `mabi-knx`.
It is the human-readable companion to
[verification-contract.yaml](./verification-contract.yaml), which is the
machine-readable source of truth for capability names, profile names, peer
roles, and verification lane policy.

Phase 0 was intentionally contract-first, and the current tree has now consumed
that contract through Phase 4. The default path remains deterministic, the
`verification/knx` interop tree is available only through ignored tests and
repo-owned container harnesses, and the capture corpus is artifact-only static
evidence.

## Purpose and Operating Principles

The KNX simulator already has a substantial protocol core. The next
stabilization step is to make verification growth deliberate and repeatable.

The operating principles are fixed as follows:

- the production crate remains free of third-party KNX verification tools
- verification grows in explicit layers instead of one-off test additions
- the default workspace path stays deterministic and lightweight
- Docker, GUI tools, and physical KNX hardware stay out of default validation
- capability, profile, peer, and lane names are decided in the YAML contract
- later phases consume this contract rather than inventing replacement names

## Current Code Baseline

The current crate exposes the seams needed for a strong verification plane.

### Server and runtime seam

- `KnxServer` is the canonical KNXnet/IP server assembly point.
- It owns the UDP server path, connection manager, group object table, filter
  chain, heartbeat scheduler, group value cache, error tracker, metrics, and
  diagnostics.
- `runtime.rs` wires KNX into `mabi-runtime` through `KnxDriver`,
  `ManagedService`, `DevicePort`, and service snapshots.

### Protocol frame seam

- `KnxFrame`, `KnxNetIpHeader`, `ServiceType`, `Hpai`, and DIB types provide
  the KNXnet/IP frame model.
- These types are the right anchor for deterministic request/response fixtures
  and later malformed-frame replay fixtures.

### cEMI and group object seam

- `CemiFrame`, `Apci`, `MessageCode`, and additional-info types model the cEMI
  payloads used by tunneling, routing, bus monitor, property, and reset flows.
- `GroupObjectTable` owns group address state, typed DPT values, event
  publication, and fixture creation for group communication profiles.

### DPT seam

- `DptRegistry` owns standard and custom codec registration.
- DPT coverage already includes common boolean, scaling, float, counter,
  string, scene, HVAC, and RGB types.
- This registry is the right source for `dpt_matrix` profile fixtures.

### Tunnel resilience seam

- `TunnelConnection`, `SequenceTracker`, `AckWaiter`, and `TunnelFsm` provide
  the core tunneling lifecycle, sequence validation, ACK handling, and
  connection-state behavior.
- These types are the right anchors for duplicate, out-of-order, retry,
  heartbeat, and timeout verification.

## Current Verification State

At the current Phase 4 verification baseline:

- `cargo test -p mabi-knx` is green.
- the current unit suite has 336 passing tests.
- `crates/mabi-knx/tests/` now contains Docker-free deterministic profile tests.
- `verification/knx/` now contains the repo-owned Phase 3 interop matrix.
- XKNX, Calimero Tools, knxd, and Node `knx` are nightly active peers.
- OpenKNX/thelsing is an optional/manual peer for device-stack and corpus work.
- `verification/knx/captures/` contains the Phase 4 artifact-only corpus.
- `verification/knx/fixtures/` contains default-lane static negative fixtures.
- no KNX release-only perf contract exists yet.
- KNX scope is KNXnet/IP only for the current verification program.
- KNX Secure is tracked as a future capability, not as current acceptance.

The current verification strength is unit-level protocol coverage, deterministic
server/network/runtime integration, a repo-owned ignored container matrix with
explicit peer transcripts and failure taxonomy, and a static capture/negative
fixture corpus that can replay without external tools.

## Canonical Capability Model

The canonical capability IDs are frozen in
[verification-contract.yaml](./verification-contract.yaml) and summarized here
for humans.

| Capability ID | Meaning | Current core status |
|---|---|---|
| `discovery` | KNXnet/IP SearchRequest/SearchResponse behavior | Implemented |
| `description` | DescriptionRequest/DescriptionResponse and DIBs | Implemented |
| `tunneling_connect` | Connect, tunnel endpoint, and disconnect lifecycle | Implemented |
| `connection_state` | ConnectionState request/response and timeout behavior | Implemented |
| `group_value_read_write` | GroupValueRead, GroupValueResponse, and GroupValueWrite via cEMI | Implemented |
| `dpt_codec` | DPT encode/decode parity across common datapoint types | Implemented |
| `routing_multicast` | KNXnet/IP routing indication and multicast behavior | Planned |
| `busmonitor` | Bus monitor cEMI handling and passive capture | Implemented in core, needs integration/interop coverage |
| `device_management_property` | Device-management/property read/write semantics | Implemented in core, needs integration/interop coverage |
| `sequence_ack_retry` | Sequence duplicate/out-of-order, ACK timeout, and retry behavior | Implemented |
| `heartbeat_timeout` | Connection heartbeat, state polling, and timeout cleanup | Implemented |
| `secure_future` | KNX IP Secure/Data Secure tracking placeholder | Future |

## Canonical Profile Naming

The canonical profile IDs are also frozen in
[verification-contract.yaml](./verification-contract.yaml).

| Profile ID | Intent | Canonical first lane |
|---|---|---|
| `basic_discovery` | Search and description reachability | `deterministic` |
| `tunnel_lifecycle` | Connect, state, and disconnect lifecycle | `deterministic` |
| `group_io` | Group value read/write through the tunnel path | `deterministic` |
| `xknx_canary` | XKNX container canary for discovery, tunneling, and group IO | `interop` |
| `dpt_matrix` | DPT value parity across representative datapoint types | `deterministic` |
| `heartbeat_timeout` | Connection heartbeat/state timeout cleanup | `deterministic` |
| `runtime_smoke` | Canonical `mabi-runtime` launch and snapshot path | `deterministic` |
| `routing_busmonitor` | Routing and busmonitor behavior | `interop` |
| `device_management` | Device management and property-service behavior | `interop` |
| `tunnel_resilience` | Sequence, ACK, retry, and fatal-desync behavior | `deterministic` |
| `secure_future` | KNX Secure tracking placeholder | `future` |

## Peer Role Baseline

The peer role split is fixed now so later phases do not reinterpret tool scope.

| Peer | Lane | Current role |
|---|---|---|
| `xknx` | `canary_interop` | Python canary and Home Assistant ecosystem peer |
| `calimero_tools` | `active_interop` | JVM reference oracle in the active matrix |
| `knxd` | `active_interop` | Gateway/router realism peer in the active matrix |
| `knxjs` | `active_interop` | Node `knx@2.5.4` alternate stack in the active matrix |
| `openknx_thelsing` | `corpus_optional_interop` | Manual device-stack/corpus source and optional smoke peer |

## Policy Boundaries

The following policy boundaries are fixed in the Phase 0 contract:

- protocol scope: KNXnet/IP only
- default workspace lane: deterministic
- external interop lane: ignored
- interop execution model: repo-owned containers through `verification/knx`
- capture corpus lane: artifact-only static replay
- perf lane: release-only ignored
- threshold-based perf assertions are forbidden in the default workspace path
- GUI automation: out of scope
- physical KNX hardware: not required
- third-party KNX tools: verification assets only, not production dependencies
- KNX Secure: `secure_future` capability only

## Phase 0 Completion Checklist

Phase 0 is complete when all of the following are true:

- this baseline document records the current code seams and verification state
- [verification-contract.yaml](./verification-contract.yaml) contains the
  canonical capability, profile, peer, and policy contracts
- [verification-strategy.md](./verification-strategy.md) treats the baseline
  and YAML contract as the source of truth
- [README.md](./README.md) points engineers to the strategy, baseline, and
  contract
- later work can consume names and lanes from the YAML contract without
  inventing replacements
- no production code, public Rust API, Docker harness, or external peer is
  required by the deterministic baseline

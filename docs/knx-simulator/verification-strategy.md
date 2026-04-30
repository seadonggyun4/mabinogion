# KNX Verification Strategy

This document defines the internal verification strategy for `mabi-knx`.
It is a roadmap for bringing the KNX simulator up to the same verification
operating model used by the stabilized OPC UA and BACnet simulators.

The goal is not to add production dependencies or change public Rust APIs in
this step. The goal is to make the verification architecture decision-complete:
future implementation should consume the Phase 0 baseline and contract instead
of inventing capability names, profile names, peer roles, or lane boundaries.

## Phase 0 Source of Truth

Phase 0 is split into two canonical artifacts:

- [verification-baseline.md](./verification-baseline.md)
  - human-readable current state, code seam analysis, and policy explanation
- [verification-contract.yaml](./verification-contract.yaml)
  - machine-readable capability, profile, peer, and lane contract

Later phases should treat those files as the source of truth. This strategy
document explains how to implement the phases; it should not redefine names
that already exist in the YAML contract.

## Background and Current State

`mabi-knx` already has a substantial protocol core:

- `KnxServer` provides the UDP KNXnet/IP server and tunneling pipeline.
- `GroupObjectTable` owns group-address state and emits group events.
- `DptRegistry` and DPT codecs provide typed datapoint encoding and decoding.
- `TunnelConnection`, `SequenceTracker`, `AckWaiter`, and `TunnelFsm` model
  tunneling lifecycle, sequence validation, ACK handling, and state transitions.
- `FilterChain`, `HeartbeatScheduler`, `GroupValueCache`, `SendErrorTracker`,
  `KnxMetricsCollector`, and `KnxDiagnostics` provide timing simulation,
  resilience, observability, and diagnostics.
- `runtime.rs` integrates KNX with `mabi-runtime` through the canonical protocol
  driver and managed service path.

The current baseline is strong across unit, deterministic integration, ignored
interop, and static corpus lanes:

- `cargo test -p mabi-knx` is green.
- The current unit suite has 336 passing tests.
- `crates/mabi-knx/tests/` contains deterministic profile tests for Phase 1.
- `verification/knx/` contains the Phase 3 ignored active peer matrix.
- XKNX, Calimero Tools, knxd, and Node `knx` are active nightly peers.
- OpenKNX/thelsing is an optional/manual device-stack and corpus peer.
- `verification/knx/captures/` is the Phase 4 artifact-only capture corpus.
- `verification/knx/fixtures/` is the Phase 4 default-lane negative fixture corpus.

This means future stabilization work should deepen semantics and optional
coverage, not change the default deterministic lane.

## Current Codebase Analysis

The current implementation already exposes the right seams for a layered
verification plane.

| Seam | Current role | Verification use |
|---|---|---|
| `KnxServer` | Canonical KNXnet/IP server assembly and UDP runtime | Loopback integration profiles and interop SUT |
| `KnxServerConfig` | Bind address, multicast address, tunneling/routing flags, tunnel behavior | Profile-specific server fixtures |
| `GroupObjectTable` | Group object creation, read/write, events | Group value read/write and DPT profile fixtures |
| `DptRegistry` | Standard and custom DPT lookup | DPT matrix and external peer value parity |
| `KnxFrame` / `ServiceType` / `Hpai` | KNXnet/IP frame model | Frame client, malformed frame, and service parity fixtures |
| `CemiFrame` / `Apci` | cEMI and group value operation model | Tunneling request, busmonitor, group value corpus |
| `TunnelConnection` | Per-channel connection state | Connect/state/disconnect lifecycle assertions |
| `SequenceTracker` / `AckWaiter` / `TunnelFsm` | Resilience behavior | Duplicate, retry, timeout, and desync profiles |
| `runtime.rs` | `mabi-runtime` launch path | Runtime smoke profile |

The integration layer must use these seams through public or test-facing
surfaces, but it must cross at least one real runtime boundary:

- server plus UDP socket
- KNXnet/IP frame encode/decode
- tunnel connection lifecycle
- runtime managed-service launch

Tests that only call object methods or DPT codecs directly belong in unit tests,
not in the Phase 1 integration lane.

## Open-Source KNX Peer Roles

External KNX tools are verification assets only. They must not become production
dependencies of `mabi-knx`.

| Peer | Role | Primary value | Main risk | Verification lane |
|---|---|---|---|---|
| XKNX | Python async peer and first canary | Programmable tunneling/routing client with Home Assistant ecosystem relevance | Python dependency drift | Phase 2 canary, Phase 3 active peer |
| Calimero Tools | JVM reference oracle | Discovery, self-description, process communication, network monitor, property tooling | JDK/Gradle/container cost | Phase 3 active peer |
| knxd | Router/gateway realism peer | Gateway behavior, routing, reconnect and daemon-style tunnel behavior | Build/config complexity | Phase 3 active peer |
| Node `knx` | Lightweight alternate stack | Node-based DPT/group telegram parity and simple smoke scripts | Smaller maintenance surface than Calimero/XKNX | Phase 3 active peer |
| OpenKNX/thelsing | Device and ETS-oriented corpus source | Device-stack examples, System-B behavior, ETS-oriented models | Embedded/Linux harness complexity and GPL boundary | Phase 3 optional smoke, Phase 4 corpus |

The recommended attachment order is:

1. XKNX canary.
2. Calimero Tools reference oracle.
3. knxd gateway realism.
4. Node `knx` alternate stack.
5. OpenKNX/thelsing corpus and optional Linux device-stack smoke.

## Verification Lane Policy

The KNX verification model follows the same operating split used elsewhere in
the repository.

| Lane | Command shape | Requirements | Policy |
|---|---|---|---|
| Unit regression | `cargo test -p mabi-knx` | Rust toolchain only | Always default |
| Deterministic integration | `cargo test -p mabi-knx --tests` | Rust toolchain only | Included in workspace once added |
| Self-contained interop | ignored test invoking `verification/knx` | Docker/Compose when available | Skips locally if Docker is absent |
| Nightly/manual interop | ignored test invoking full matrix | Docker/Compose required | Fails on peer or harness failure |
| Static corpus replay | `cargo test -p mabi-knx --test capture_corpus --test negative_fixtures` | Rust toolchain only | Included in default regression |
| Perf/soak | release-only ignored test or bench | Release build | Never blocks default workspace path |
| Secure future | tracking capability only | Not implemented in current plan | No current acceptance gate |

Default `cargo test --workspace` must remain deterministic and must not depend
on Docker, GUI tools, network multicast availability, external KNX hardware, or
threshold-based performance assertions.

## Phase 0. KNX Verification Contract

### Goal

Create a stable source of truth for KNX verification names, capability
coverage, profile ownership, peer roles, and lane policy.

### Deliverables

- [verification-baseline.md](./verification-baseline.md)
- [verification-contract.yaml](./verification-contract.yaml)
- README links from `docs/knx-simulator/README.md`
- A documented rule that Phase 1-4 work consumes the contract instead of
  inventing new names

### Capability IDs

| Capability ID | Meaning | Initial lane |
|---|---|---|
| `discovery` | KNXnet/IP search request/response behavior | deterministic |
| `description` | Description request/response and supported service DIBs | deterministic |
| `tunneling_connect` | Connect, tunnel endpoint, disconnect lifecycle | deterministic |
| `connection_state` | Connection state request/response and timeout behavior | deterministic |
| `group_value_read_write` | GroupValueRead, GroupValueResponse, GroupValueWrite via cEMI | deterministic |
| `dpt_codec` | DPT encode/decode parity across common datapoint types | deterministic |
| `routing_multicast` | KNXnet/IP routing indication and multicast behavior | interop |
| `busmonitor` | Bus monitor cEMI frame handling and passive capture | interop |
| `device_management_property` | Device management and property read/write semantics | interop |
| `sequence_ack_retry` | Sequence duplicate/out-of-order, ACK timeout, retry behavior | deterministic |
| `heartbeat_timeout` | Connection heartbeat, state polling, and timeout handling | deterministic |
| `secure_future` | KNX IP Secure/Data Secure tracking placeholder | future |

### Profile IDs

| Profile ID | Capabilities | Lane | Phase introduced |
|---|---|---|---|
| `basic_discovery` | `discovery`, `description` | deterministic | Phase 1 |
| `tunnel_lifecycle` | `tunneling_connect`, `connection_state` | deterministic | Phase 1 |
| `group_io` | `group_value_read_write` | deterministic, interop | Phase 1/2 |
| `xknx_canary` | `discovery`, `tunneling_connect`, `group_value_read_write` | interop | Phase 2 |
| `dpt_matrix` | `dpt_codec`, `group_value_read_write` | deterministic | Phase 1 |
| `heartbeat_timeout` | `connection_state`, `heartbeat_timeout` | deterministic | Phase 1 |
| `runtime_smoke` | `tunneling_connect`, `connection_state`, `group_value_read_write` | deterministic | Phase 1 |
| `routing_busmonitor` | `routing_multicast`, `busmonitor` | interop | Phase 3 |
| `device_management` | `device_management_property`, `description` | interop | Phase 3 |
| `tunnel_resilience` | `sequence_ack_retry` | deterministic, interop | Phase 1/3 |
| `secure_future` | `secure_future` | future | Future |

### Detailed Tasks

| Task | Description | Acceptance |
|---|---|---|
| `phase0.baseline` | Record the reusable verification baseline and update it as lanes come online | Baseline mentions 336 passing unit tests, active `crates/mabi-knx/tests/`, and the Phase 3 `verification/knx` matrix |
| `phase0.contract_schema` | Define YAML fields for baseline, policies, capabilities, profiles, and peers | Later tests can `include_str!` or parse the contract without guessing names |
| `phase0.capability_matrix` | Encode the capability IDs above with current coverage status | No Phase 1 implementation needs to invent a capability name |
| `phase0.profile_matrix` | Encode the profile IDs above and map them to capability IDs | No Phase 1 implementation needs to invent a profile name |
| `phase0.peer_matrix` | Encode XKNX, Calimero Tools, knxd, Node `knx`, and OpenKNX/thelsing roles | Phase 2/3 harness order is fixed |
| `phase0.policy_boundary` | Document deterministic, interop, perf/soak, and secure-future boundaries | Default workspace path remains lightweight by policy |

### Completion Criteria

- The contract names every capability, profile, peer, and lane used later.
- The baseline describes the current implementation without overstating
  interop maturity.
- README links make the strategy and baseline discoverable.
- No production code or public Rust API changes are introduced.

## Phase 1. Deterministic Integration

### Goal

Add Docker-free integration coverage that validates real KNXnet/IP behavior
through `KnxServer`, UDP sockets, encoded `KnxFrame`s, and cEMI frames.

### Deliverables

- `crates/mabi-knx/tests/`
- `tests/support/contract`
- `tests/support/frame_client`
- `tests/support/server_harness`
- `tests/support/fixtures`
- `tests/support/assertions`
- `tests/support/runtime_smoke`
- Profile test files grouped by behavior rather than by internal module

### Harness Rules

- Use ephemeral loopback ports, never fixed port 3671.
- Start a real `KnxServer` for all profile tests except pure contract checks.
- Send and receive real UDP KNXnet/IP frames.
- Build group objects through `GroupObjectTable` and public DPT surfaces.
- Reuse the Phase 0 YAML contract for profile ID and lane assertions.
- Keep all tests deterministic and compatible with `cargo test --workspace`.

### Profile Smoke Contracts

| Profile | Required smoke |
|---|---|
| `basic_discovery` | Send SearchRequest and DescriptionRequest, assert response type, HPAI, device name limit, and supported service families |
| `tunnel_lifecycle` | Connect, assert channel allocation, query connection state, disconnect, and assert channel removal |
| `group_io` | Connect tunnel, send GroupValueWrite, read back via GroupValueRead/Response, assert group event emission |
| `dpt_matrix` | Exercise DPT 1, 5, 9, 12, 13, 14, 16, 20, and 232 through encoded group values |
| `tunnel_resilience` | Send duplicate and out-of-order sequence numbers, assert ACK behavior and diagnostics counters |
| `heartbeat_timeout` | Use short heartbeat config, assert connection state polling and timeout cleanup |
| `runtime_smoke` | Launch through `KnxDriver`, register runtime device, snapshot metadata, and clean stop |

### Detailed Tasks

| Task | Description | Acceptance |
|---|---|---|
| `phase1.contract_loader` | Add test-only loader for `verification-contract.yaml` | Every integration file asserts profile existence and deterministic lane |
| `phase1.frame_client` | Add UDP helper for KNXnet/IP request/response frames | Helpers support Search, Description, Connect, State, Disconnect, and Tunnelling |
| `phase1.server_harness` | Add loopback server harness with readiness and shutdown handling | Parallel tests do not collide on ports |
| `phase1.fixtures` | Add standard group-object fixture presets | Profiles can create switch, scaling, temperature, counter, string, RGB, and HVAC groups |
| `phase1.basic_discovery` | Implement discovery/description profile | Uses real UDP frames and validates response payloads |
| `phase1.tunnel_lifecycle` | Implement connect/state/disconnect profile | Validates channel state and clean removal |
| `phase1.group_io` | Implement group value profile | Validates write/read round trip and event observation |
| `phase1.dpt_matrix` | Implement DPT matrix profile | Validates encoded values through group communication path |
| `phase1.resilience` | Implement sequence/ACK/heartbeat profile | Validates duplicate, retry, timeout, and diagnostics behavior |
| `phase1.runtime_smoke` | Implement runtime launch profile | Validates `mabi-runtime` path without duplicating all protocol profiles |

### Completion Criteria

- `cargo test --workspace` includes KNX deterministic integration.
- No Docker, GUI, physical KNX hardware, or external peer is required.
- Each integration test crosses at least one server/network/runtime boundary.
- Unit tests remain the place for isolated DPT, address, frame, or internal FSM
  behavior.

## Phase 2. Self-Contained Interop Canary

### Goal

Open the `verification/knx` plane with a repo-owned container harness and a
single meaningful external canary peer.

### Peer Decision

The first canary peer is XKNX because it is programmable, Python-based, actively
used by Home Assistant, and naturally fits the same container pattern used for
other protocol interop canaries.

Phase 2 keeps this target intentionally small: it runs in a pinned XKNX
container environment and validates a stable KNXnet/IP transcript contract.
Richer native XKNX object-level assertions are Phase 3 work.

### Deliverables

- `verification/knx/README.md`
- `verification/knx/compose.yaml`
- `verification/knx/interop-matrix.toml`
- `verification/knx/run-target.sh`
- `verification/knx/harness/xknx/Dockerfile`
- `verification/knx/harness/xknx/requirements.txt`
- `verification/knx/harness/xknx/run.sh`
- `verification/knx/harness/xknx/peer_client.py`
- `crates/mabi-knx/tests/interop_matrix.rs`
- `crates/mabi-knx/tests/interop_profiles.rs`

### Canary Contract

The XKNX canary must run as an ignored test and produce a JSON transcript as the
source of truth.

| Step | Contract |
|---|---|
| 1 | Rust test starts `KnxServer` on container-local `127.0.0.1:3671` with the standard group fixture |
| 2 | Rust test spawns XKNX peer script with SUT host/port, group address, write value, and transcript path |
| 3 | Peer performs KNXnet/IP discovery and direct tunnel connection |
| 4 | Peer establishes tunneling connection |
| 5 | Peer writes a switch/scaling group value |
| 6 | Peer reads or observes the value round trip |
| 7 | Peer disconnects cleanly |
| 8 | Rust test validates JSON transcript fields and SUT events |

### Transcript Schema

| Field | Meaning |
|---|---|
| `peer` | Static value `xknx` |
| `sut_addr` | Server address used by peer |
| `discovery_ok` | Search or direct connection setup succeeded |
| `tunnel_connect_ok` | Tunneling connect succeeded |
| `group_address` | Group address used for the canary switch |
| `group_write_ok` | GroupValueWrite succeeded |
| `group_read_ok` | GroupValueRead/Response succeeded |
| `round_trip_value` | Boolean value observed after the write/read round trip |
| `errors` | Structured error list |

### Detailed Tasks

| Task | Description | Acceptance |
|---|---|---|
| `phase2.plane` | Create `verification/knx` tree and README | README documents local skip and CI-required Docker behavior |
| `phase2.manifest` | Add one manifest target `xknx-canary` | Manifest uses repo-owned compose service, no env-injected runner |
| `phase2.compose` | Add build-based Compose service | Service mounts repo and runs harness script |
| `phase2.runner` | Add target runner with manifest/service validation | Unknown target names fail fast |
| `phase2.rust_matrix` | Add ignored Rust matrix test | Docker absence skips locally and fails in CI/nightly mode |
| `phase2.xknx_peer` | Add XKNX peer script and pinned requirements | Peer writes JSON transcript and exits non-zero on contract failure |
| `phase2.profile` | Add ignored `xknx_canary_profile_smoke_contract` | Discovery, tunneling connect, and group IO are validated |

### Completion Criteria

- Local default tests remain unaffected.
- `cargo test -p mabi-knx --test interop_matrix -- --ignored` executes the
  repo-owned matrix when Docker is available.
- Local Docker absence reports a stable skip summary.
- CI/nightly mode treats missing Docker or peer failure as failure.

## Phase 3. Active Peer Matrix

### Goal

Expand from one canary peer to a meaningful non-GUI KNX interop matrix.

### Target Peer Order

| Order | Peer | Role | Required smoke |
|---|---|---|---|
| 1 | XKNX | Python canary and Home Assistant ecosystem peer | Discovery/direct tunnel, group write/read, state observation |
| 2 | Calimero Tools | JVM reference oracle | Discover, Description, ProcComm read/write, NetworkMonitor capture |
| 3 | knxd | Gateway/router realism peer | Tunnel connect, routing indication, reconnect/timeout behavior |
| 4 | Node `knx` | Lightweight alternate stack | DPT/group telegram parity and routing/tunneling smoke |
| 5 | OpenKNX/thelsing | Device-stack/corpus peer | Optional Linux device-stack smoke and ETS-oriented fixture generation |

### Manifest Model

Each target in `verification/knx/interop-matrix.toml` must declare:

- `name`
- `compose_service`
- `timeout_seconds`
- `tier`
- optional `working_dir`
- `profiles`
- `capabilities`

### Detailed Tasks

| Task | Description | Acceptance |
|---|---|---|
| `phase3.calimero_tools` | Add JVM harness for Calimero Tools | Runs Discover and ProcComm against SUT with transcript |
| `phase3.knxd` | Add knxd harness | Validates gateway-style tunnel/routing behavior or records unsupported-mode skip |
| `phase3.knxjs` | Add Node harness | Validates DPT/group telegram parity through tunneling or routing |
| `phase3.openknx` | Add OpenKNX/thelsing optional harness | Builds or replays Linux/device-stack smoke without entering default lane |
| `phase3.capability_mapping` | Map every target to Phase 0 capabilities | No harness is accepted without explicit capability coverage |
| `phase3.shared_transcript` | Normalize transcript shape across peers | Matrix can produce stable pass/fail/skipped summary |
| `phase3.failure_taxonomy` | Normalize peer errors | Distinguish tool missing, build failure, protocol failure, and unsupported feature |

### Completion Criteria

- Every active peer runs at least one meaningful smoke contract.
- Harnesses do not grow their own naming systems; all coverage maps to Phase 0
  capabilities and profiles.
- `verification/knx/README.md` documents exact local and nightly commands.
- Default workspace tests remain deterministic and peer-free.
- Unsupported modes such as routing multicast in single-container topology are
  recorded as `unsupported` transcript capabilities rather than silently skipped.

## Phase 4. Corpus / Negative Fixtures

### Goal

Turn external peer traces and tool outputs into reusable packet/cEMI fixtures
without making GUI or external tools part of the default automation path.

Phase 4 is active. The canonical corpus lives under
`verification/knx/captures/`, and static negative fixtures live under
`verification/knx/fixtures/`.

### Corpus Sources

| Source | Corpus value | Automation status |
|---|---|---|
| Calimero NetworkMonitor | Reference monitor traces and process communication examples | Artifact source |
| XKNX telegram logs | Python peer telegram history and group state transitions | Artifact source |
| knxd traces | Gateway routing, reconnect, and daemon behavior traces | Artifact source |
| OpenKNX examples | Device model, ETS-oriented examples, System-B behavior | Artifact source |

### Directory Contract

```text
verification/knx/
├── captures/
│   ├── README.md
│   ├── catalog.toml
│   ├── calimero/
│   ├── xknx/
│   ├── knxd/
│   └── openknx/
└── fixtures/
    ├── README.md
    ├── catalog.toml
    ├── malformed/
    ├── sequence/
    ├── dpt/
    └── routing/
```

### Negative Fixture Categories

| Category | Examples |
|---|---|
| Malformed frame | Bad KNXnet/IP header size, body length mismatch, unknown service type |
| HPAI validation | Invalid protocol code, invalid endpoint length, unsupported NAT shape |
| Tunnel channel | Bad channel ID, stale channel, disconnected channel request |
| Sequence resilience | Duplicate sequence, out-of-order sequence, wraparound, fatal desync |
| Service support | Unsupported routing or device-management operation with stable error |
| DPT mismatch | Invalid payload length, invalid enum value, fallback codec behavior |

### Detailed Tasks

| Task | Description | Acceptance |
|---|---|---|
| `phase4.capture_readme` | Document how captures are collected and reviewed | Complete; corpus is artifact-only and does not imply live peer automation |
| `phase4.catalog` | Add capture catalog schema | Complete; each fixture records source, license note, protocol area, profile, and expected behavior |
| `phase4.seed_xknx` | Seed first XKNX transcript fixtures from Phase 2/3 | Complete; replay tests validate group IO transcript shape |
| `phase4.seed_calimero` | Seed Calimero monitor/process communication fixtures | Complete; replay tests validate frame decode and service mapping metadata |
| `phase4.seed_knxd` | Seed knxd gateway traces | Complete; replay tests validate tunnel/routing edge behavior metadata |
| `phase4.seed_openknx` | Seed OpenKNX/thelsing model references | Complete; corpus shapes device-model tests without vendoring production code |
| `phase4.replay_tests` | Add deterministic replay tests based on static fixtures | Complete; static fixtures enter the default lane without external peers |

### Completion Criteria

- Interop failures can be reduced into reusable fixtures.
- Corpus artifacts remain outside the production crate graph.
- GUI/manual tools cannot silently enter the default CI path.
- Negative fixtures produce stable, actionable regression failures.

## Acceptance Criteria

This strategy is complete when:

- Phase 0 creates a machine-readable KNX verification contract.
- Phase 1 validates KNX through deterministic server/network/runtime profiles.
- Phase 2 introduces a repo-owned XKNX canary matrix.
- Phase 3 expands to Calimero Tools, knxd, Node `knx`, and OpenKNX/thelsing.
- Phase 4 turns peer traces into reusable fixtures and negative regressions.
- `cargo test --workspace` remains green without Docker, external peers, GUI
  tools, physical KNX hardware, or perf thresholds.

## Non-Goals

- No public Rust API changes are required by this strategy document.
- No production dependency on XKNX, Calimero, knxd, Node `knx`, or OpenKNX is allowed.
- No KNX Secure implementation is required in this plan.
- No physical KNX hardware is required for default validation.
- No GUI automation is introduced.
- No performance threshold may enter the default workspace test path.

# BACnet Verification Strategy

This document defines the canonical verification strategy for `mabi-bacnet`.
It is an internal engineering document for building a product-grade verification
plane around the existing BACnet simulator without pulling third-party tools
into the production crate graph.

## Background and Goals

`mabi-bacnet` already has strong protocol and domain coverage:

- layered BACnet/IP architecture with object, service, APDU, and network layers
- dynamic object registration and service registration seams
- runtime integration through `mabi-runtime`
- broad unit-test coverage across APDU, BBMD, objects, services, TSM, and server

The next problem is not protocol closure inside the crate. The next problem is
verification architecture:

- how to validate `mabi-bacnet` against well-known open-source BACnet peers
- how to keep `cargo test --workspace` deterministic and green
- how to add interoperability confidence without contaminating production code
- how to use GUI-oriented tools later without letting them shape the current CI path

This strategy fixes those decisions up front.

## Decision Summary

- Verification lives in a dedicated `verification/bacnet` plane, not inside the
  production dependency graph.
- Default developer validation remains `cargo test --workspace`.
- Interop runs as a self-contained, ignored container matrix.
- Perf stays release-only and ignored.
- GUI tools are out of scope for automation in the current phase.
- `bacnet-stack`, `BAC0`, `BACpypes3`, and `BACnet4J` remain the canonical
  automation peers.
- The current Phase 5 baseline has a repo-owned interop plane with all four
  non-GUI peers active.
- `YABE` and `VTS` are capture/manual lanes in the current Phase 5 baseline.
- A seeded capture corpus exists under `verification/bacnet/captures/`.
- Regression, interop, capture, and perf lanes now have separate operating
  boundaries.

## Phase 0 Source of Truth

The original Phase 0 source of truth is split into two canonical artifacts:

- [verification-baseline.md](./verification-baseline.md)
  - human-readable baseline and policy explanation
- [verification-contract.yaml](./verification-contract.yaml)
  - machine-readable capability, profile, peer, and policy contract

Current and future verification work should consume these artifacts rather than
redefining names, lanes, or verification boundaries.

## Current Codebase Analysis

### Structural strengths

The current BACnet simulator already exposes the right seams for a dedicated
verification architecture:

- [crate surface](../../crates/mabi-bacnet/src/lib.rs):
  layered server, object, service, network, and runtime modules
- [object registry](../../crates/mabi-bacnet/src/object/registry.rs):
  canonical object assembly and bulk population seam
- [service registry](../../crates/mabi-bacnet/src/service/handler.rs):
  canonical confirmed/unconfirmed service dispatch seam
- [server assembly](../../crates/mabi-bacnet/src/server/bacnet_server.rs):
  canonical place where default objects, services, BBMD, segmentation, COV, and
  TSM are wired together
- [runtime integration](../../crates/mabi-bacnet/src/runtime.rs):
  canonical `mabi-runtime` launch path and service snapshot path

### What this means architecturally

The existing crate is already structured like a protocol core plus runtime
adapter. That is a good shape for introducing a verification plane because:

- object fixtures can be generated through `ObjectRegistry`
- service capability snapshots can be derived from `ServiceRegistry`
- end-to-end server profiles can be built from `BACnetServer::new(...)`
- runtime-driven launch remains the production truth

### Current verification layers

The verification pieces now present outside the production core are:

- `crates/mabi-bacnet/tests/` now exists as the deterministic profile lane
- `verification/bacnet/` now exists as the repo-owned interop plane
- the current interop lane now activates `bacnet-stack`, `BAC0`, `BACpypes3`,
  and `BACnet4J`
- a release-only BACnet perf contract lane now exists to keep perf policy
  explicit without contaminating the default workspace path

### Current baseline

At the current Phase 5 baseline:

- `cargo test -p mabi-bacnet` is green
- the crate has strong unit coverage
- deterministic integration profiles now exist under `crates/mabi-bacnet/tests/`
- `verification/bacnet/` now exists as the repo-owned interop harness tree
- the current interop lane now runs the full non-GUI peer set
- `mabi-opcua` already demonstrates the verification pattern we want to copy

## Role of the Six Open-Source BACnet Tools

GUI automation is not part of the current phase. The tools are therefore split
into active automation peers and deferred capture/manual lanes.

| Tool | Current role | Why it belongs here |
|---|---|---|
| `bacnet-stack` | Reference C peer and protocol oracle | Best for APDU/service correctness, segmentation, BBMD, FDR, and wire-level comparison |
| `BAC0` | High-level scenario peer | Strong for point read/write, COV, building-control style workflows, and controller-style interaction |
| `BACpypes3` | Programmable edge-case peer | Best for custom clients/servers, malformed cases, VLAN/topology experiments, and BBMD/FDR flows |
| `BACnet4J` | JVM interop peer | Covers enterprise/JVM ecosystem behavior and alternative stack semantics |
| `YABE` | Manual acceptance and capture corpus source | Valuable demo servers and traffic generation, but GUI-first and not a fit for current CI |
| `VTS` | Manual protocol shell and negative-case capture source | Strong for packet-level exploration and repeatable manual scripts, but not a fit for current CI |

### Automation lanes

- Active automation lane:
  - `bacnet-stack`
  - `BAC0`
  - `BACpypes3`
  - `BACnet4J`
- Capture/manual lane:
  - `YABE`
  - `VTS`

This split is intentional and should not be revisited during implementation
unless the product scope changes to include GUI-oriented automation.

## Target Architecture

The target architecture is a verification plane that sits next to the
production crate rather than inside it.

```text
repo root
├── crates/
│   └── mabi-bacnet/
│       ├── src/                    # production code
│       └── tests/                  # deterministic integration profiles
└── verification/
    └── bacnet/
        ├── README.md
        ├── compose.yaml
        ├── interop-matrix.toml
        ├── run-target.sh
        ├── harness/
        │   ├── bacnet-stack/
        │   ├── bac0/
        │   ├── bacpypes3/
        │   └── bacnet4j/
        └── captures/
            ├── README.md
            ├── catalog.toml
            ├── yabe/
            └── vts/
```

### Architecture rules

- Third-party BACnet tools must not become production dependencies of
  `mabi-bacnet`.
- All heavy external tooling belongs under `verification/bacnet`.
- The default workspace path must remain lightweight and deterministic.
- Interop and perf must be opt-in, ignored test layers.
- The production truth remains the runtime-backed server launched by
  `mabi-bacnet`.

### Reference pattern

BACnet should follow the same verification split already used by OPC UA:

- [verification/opcua/README.md](../../verification/opcua/README.md)
- [crates/mabi-opcua/tests/interop_matrix.rs](../../crates/mabi-opcua/tests/interop_matrix.rs)

The goal is not to copy OPC UA details one-to-one. The goal is to reuse the
same verification operating model:

- deterministic default regression
- ignored self-contained interop matrix
- release-only perf contract

## Verification Contracts

The canonical machine-readable contract for this section now lives in
[verification-contract.yaml](./verification-contract.yaml).

### Capability categories

Interop and integration tasks should be described in terms of capabilities, not
in terms of individual tool quirks.

The canonical capability categories are:

- `discovery`
- `property_io`
- `property_multiple`
- `cov`
- `file_access`
- `read_range_trend_log`
- `schedule_calendar`
- `device_control_time_sync`
- `create_delete`
- `segmentation`
- `bbmd_foreign_device`
- `tsm_duplicate_handling`

### Profile naming

Deterministic and interop profiles should use short, stable names:

- `basic_ip`
- `property_io`
- `property_multiple`
- `cov_flow`
- `file_and_trend`
- `schedule_calendar`
- `device_control`
- `create_delete`
- `segmentation`
- `bbmd_fdr`
- `tsm_resilience`

### Interop manifest contract

`verification/bacnet/interop-matrix.toml` should define a stable manifest with
at least these fields:

- `version`
- `targets[].name`
- `targets[].compose_service`
- `targets[].timeout_seconds`
- `targets[].tier`
- optional `targets[].working_dir`

### Harness responsibilities

Each harness is responsible for:

- starting its own peer process or peer fixture
- running the BACnet smoke contract assigned to it
- exiting with a clear success or failure status
- avoiding hidden environment requirements beyond Docker/Compose

## Phase Plan

## Phase 0. Baseline

| Field | Detail |
|---|---|
| Goal | Freeze the current verification baseline and name the capability model |
| Deliverables | `verification-baseline.md`, `verification-contract.yaml`, profile naming, explicit policy boundaries |
| Prerequisites | None |
| Completion criteria | Baseline recorded, contract file fixed, capability categories fixed, default green policy documented |
| Risks | Premature implementation decisions leak into the wrong phase |
| Mitigation | Keep this phase documentation-only and contract-first |

### Tasks

1. Record the current strengths of `mabi-bacnet` by seam in
   `verification-baseline.md`:
   `ObjectRegistry`, `ServiceRegistry`, `BACnetServer`, `runtime`.
2. Record the original baseline gap in `verification-baseline.md`, then keep the
   current-state section updated as later verification lanes are completed.
3. Fix the policy boundaries in `verification-contract.yaml`:
   BACnet/IP only, no GUI automation, default workspace green.
4. Freeze the canonical capability categories, profile names, peer lanes, and
   verification policies in `verification-contract.yaml`.

## Phase 1. Deterministic Integration Layer

| Field | Detail |
|---|---|
| Goal | Add a first-class integration-test layer inside `crates/mabi-bacnet/tests/` |
| Deliverables | Integration profiles, fixture builders, deterministic regression contracts |
| Prerequisites | Phase 0 capability matrix |
| Completion criteria | `cargo test --workspace` covers profile-level BACnet behavior without Docker |
| Risks | Integration tests become duplicate unit tests or overfit external tools |
| Mitigation | Keep tests profile-oriented and runtime-backed |

### Tasks

1. Create `crates/mabi-bacnet/tests/`.
2. Add deterministic integration profiles for:
   `basic_ip`, `property_io`, `cov_flow`, `segmentation`, `bbmd_fdr`, and
   `tsm_resilience`.
3. Introduce fixture builders that assemble object and service combinations
   through the existing registry seams instead of ad hoc test setup.
4. Ensure these tests launch the canonical server path, not a synthetic
   alternate stack.
5. Keep this layer Docker-free and deterministic.

## Phase 2. Self-Contained Interop Matrix

| Field | Detail |
|---|---|
| Goal | Create a BACnet counterpart to the OPC UA self-contained interop plane |
| Deliverables | `verification/bacnet/` tree, compose file, runner script, interop manifest, initial BACpypes3 canary |
| Prerequisites | Phase 1 profiles and capability names |
| Completion criteria | Ignored interop matrix exists and is runnable from repo-local assets with one canary target |
| Risks | Docker/network behavior makes default development paths flaky |
| Mitigation | Keep all interop in ignored tests and skip locally when Docker is unavailable |

### Tasks

1. Create `verification/bacnet/README.md`.
2. Create `verification/bacnet/compose.yaml`.
3. Create `verification/bacnet/interop-matrix.toml`.
4. Create `verification/bacnet/run-target.sh`.
5. Add `crates/mabi-bacnet/tests/interop_matrix.rs` modeled after OPC UA.
6. Add `crates/mabi-bacnet/tests/interop_profiles.rs` with one BACpypes3 canary contract.
7. Enforce this behavior:
   - local Docker unavailable: skip summary
   - CI nightly/manual: Docker required
   - push/PR: do not run interop by default

## Phase 3. Active Peer Harnesses

| Field | Detail |
|---|---|
| Goal | Add the remaining non-GUI BACnet peers as canonical automated interop targets |
| Deliverables | `bacnet-stack`, `BAC0`, `BACpypes3`, `BACnet4J` harnesses |
| Prerequisites | Phase 2 matrix and runner contract |
| Completion criteria | `bacnet-stack`, `BAC0`, `BACpypes3`, and `BACnet4J` each execute at least one meaningful smoke contract against the SUT |
| Risks | Harnesses drift into tool-specific snowflakes |
| Mitigation | Keep every harness mapped back to the shared capability matrix |

### Tasks

1. `bacnet-stack`
   - wire-level and service-correctness smoke
   - segmentation
   - BBMD / foreign-device scenarios
2. `BACpypes3`
   - expand beyond the Phase 2 canary into richer programmable and edge-case coverage
3. `BAC0`
   - high-level scenario workflow
   - point read/write and COV validation
4. `BACnet4J`
   - JVM interop contract
   - alternative stack behavior for object/service flows
5. For each harness, define:
   - supported capability categories
   - unsupported categories
   - smoke contract name
   - timeout budget

## Phase 4. Capture Corpus Lane

| Field | Detail |
|---|---|
| Goal | Use GUI-centric tools as artifact sources without forcing them into CI |
| Deliverables | `captures/README.md`, `captures/catalog.toml`, `captures/yabe/`, `captures/vts/`, replayable packet/script corpus |
| Prerequisites | Phase 3 harness taxonomy |
| Completion criteria | Manual tools contribute reusable artifacts without changing CI complexity |
| Risks | GUI tools creep back into the automated path |
| Mitigation | Keep this lane artifact-only and manual-only |

### Tasks

1. Create `verification/bacnet/captures/README.md`.
2. Create `verification/bacnet/captures/catalog.toml`.
3. Store YABE and VTS artifacts as normalized `manifest + replay + runbook`
   corpus entries rather than ad hoc screenshots or notes.
4. Use these captures to seed future deterministic fixtures where practical.
5. Keep this lane manual-only and do not build GUI automation around these
   tools in the current phase.

## Phase 5. CI and Release Policy

| Field | Detail |
|---|---|
| Goal | Lock the operating model so regression, interop, and perf do not bleed into each other |
| Deliverables | BACnet regression workflow, BACnet nightly/manual interop workflow, release-only perf contract policy |
| Prerequisites | Phases 1-4 |
| Completion criteria | Clear separation between default regression, interop, and perf execution |
| Risks | Perf or interop begins failing default workspace CI |
| Mitigation | Hard policy: no threshold-based perf in default path, interop ignored by default |

### Tasks

1. Add BACnet regression workflow for push/PR:
   `cargo test --workspace`
2. Add BACnet nightly/manual workflow for:
   - deterministic workspace regression
   - ignored BACnet interop matrix
   - ignored release-only perf suite
3. Document the policy that threshold-based perf assertions are forbidden in the
   default workspace path.
4. Keep the release gate and the developer gate distinct.

## Peer-to-Capability Mapping

| Peer | Primary capabilities | Secondary capabilities | Not the right tool for |
|---|---|---|---|
| `bacnet-stack` | `discovery`, `property_io`, `property_multiple`, `segmentation`, `bbmd_foreign_device` | `file_access`, `create_delete`, `device_control_time_sync` | GUI-driven exploratory flows |
| `BAC0` | `property_io`, `cov`, `schedule_calendar` | `discovery`, `device_control_time_sync` | malformed packet testing |
| `BACpypes3` | `discovery`, `property_io`, `cov`, `bbmd_foreign_device`, `segmentation` | `property_multiple`, custom negative cases | operator-style acceptance demos |
| `BACnet4J` | `property_io`, `property_multiple`, `cov`, `schedule_calendar` | `bbmd_foreign_device`, `device_control_time_sync` | packet-forensics style testing |
| `YABE` | manual acceptance, capture generation | demo-server fixture ideas | CI automation in the current phase |
| `VTS` | negative-case and packet-script capture generation | protocol forensics | CI automation in the current phase |

## CI and Operating Process

### Default path

The canonical default developer path must remain:

1. `cargo test --workspace`
2. optional crate-focused runs such as `cargo test -p mabi-bacnet`

This path must stay deterministic and green without:

- Docker
- external GUI tools
- perf thresholds

### Extended path

Ignored or release-only paths should cover:

1. self-contained containerized interop matrix with the four active non-GUI peers
2. release-only perf contracts

### Local behavior

- Docker available:
  ignored interop matrix may run locally
- Docker unavailable:
  ignored interop matrix prints skip summary and succeeds locally

### CI behavior

- push/PR:
  deterministic regression only
- nightly/manual:
  deterministic regression + interop matrix + release-only perf

## Non-Scope and Follow-Up

### Out of scope in this phase

- GUI automation
- adding third-party BACnet tools to production dependencies
- changing the public `mabi-bacnet` stable API just for verification
- making perf thresholds part of the default workspace suite

### Follow-up after this strategy

- expand the active BACnet peer matrix beyond the current single-container
  topology
- add multi-container BBMD and foreign-device interop topologies
- expand the seeded YABE/VTS capture corpus with refreshed manual artifacts
- define future BACnet perf benchmarks without allowing them into the default
  workspace suite

## Acceptance Statement

This strategy is complete when:

- a new engineer can implement the BACnet verification plane without making
  architectural decisions that are not already captured here
- BACnet regression, interop, and perf are clearly separated
- the default workspace path stays deterministic and green
- third-party BACnet tools are integrated as verification assets rather than
  production dependencies
- GUI tools remain capture/manual lanes until the scope explicitly changes

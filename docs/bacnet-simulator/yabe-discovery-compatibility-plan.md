# YABE Discovery Compatibility Plan

This document turns GitHub issue `#1` into an implementation-ready roadmap for
improving BACnet explorer compatibility after the normal `Who-Is` / `I-Am`
discovery path.

The current working hypothesis is that core discovery is not broken. The
default server already registers a `WhoIsHandler` and creates the mandatory
Device object during `BACnetServer::new(...)`. The reported gap is more likely
in the post-discovery metadata path used by YABE and similar BACnet explorers:
after a device is discovered, the client reads `Object_Name`, `Object_List`,
protocol capability properties, and then object-level names and properties.

This is a documentation and planning artifact only. It does not add a Rust API,
change default server behavior, or add a new interop harness by itself.

## Research Basis

- The default `mabi-bacnet` server currently wires the mandatory Device object,
  `ReadProperty`, `ReadPropertyMultiple`, and `Who-Is` handlers in the canonical
  server assembly path.
- YABE-style explorer flows do not stop at `I-Am`. In the Beckhoff YABE
  walkthrough, selecting a discovered device resolves the Device Object Name and
  starts evaluating the objects contained in the device:
  [Testing BACnet using a BACnet Explorer](https://infosys.beckhoff.com/content/1033/tf8020_bacnetrev14/12319254667.html).
- BACnet client tooling commonly reads individual properties and may use
  `ReadPropertyMultiple` for efficient object discovery. BAC0 documents both
  read and readMultiple style access:
  [BAC0 read from network](https://bac0.readthedocs.io/en/latest/read.html).
- BACnet array compatibility is important for `Object_List`. Array index `0`
  returns the number of elements, indexes `1..n` return individual elements,
  and omitting the index returns the full array:
  [ReadProperty service array results](https://ctlsys.com/support/readproperty_service_array_results/).

## Goals

- Make the default server understandable to BACnet explorers even when the user
  supplies an empty object registry.
- Verify that Device `Object_Name`, Device `Object_List`, and minimum protocol
  metadata are encoded in the shape BACnet clients expect.
- Add YABE-style regression coverage without making GUI tooling part of the
  default test lane.
- Keep the production model extensible: solve generic BACnet explorer
  compatibility, not a one-off YABE special case.

## Non-Goals

- Do not require YABE, Docker, Python, Java, or external BACnet tools in
  `cargo test --workspace`.
- Do not silently add sample points to `BACnetServer::new(...)`.
- Do not turn YABE into an automated GUI CI dependency.
- Do not change the existing Phase 5 verification boundary: deterministic tests
  remain default, interop remains ignored, GUI tools remain capture/manual.

## Expected YABE-Style Sequence

The compatibility contract to verify is:

1. Client sends `Who-Is`.
2. Server responds with `I-Am`.
3. Client reads `ReadProperty(Device, Object_Name)`.
4. Client reads `ReadProperty(Device, Object_List)` with no array index.
5. Client may read `ReadProperty(Device, Object_List, index = 0)` for count.
6. Client may read `ReadProperty(Device, Object_List, index = 1..n)` for
   individual object identifiers.
7. Client reads object-level `Object_Name`, `Object_Type`, and common properties
   for objects returned from `Object_List`.
8. Client may prefer `ReadPropertyMultiple`; if it fails for a particular
   request shape, single-property fallback must still be stable.

## Phase 0. Issue Baseline And Compatibility Hypotheses

Phase 0 is the source of truth for the rest of this roadmap. It freezes the
issue interpretation, the code baseline, the explorer sequence, and the risk
vocabulary so later implementation phases do not need to reinterpret the
GitHub report.

### `phase0.issue_summary`

The GitHub issue started as an apparent `Who-Is` non-response report. The
follow-up clarified that the server does answer discovery: YABE can see that
the device responds, but it cannot pull useful metadata such as the device name
or objects from the registry.

Maintainer baseline:

- Core discovery is expected to work automatically.
- Users should not need to manually build an `I-Am` response for normal server
  discovery.
- The issue should be tracked as post-discovery metadata compatibility:
  Device `Object_Name`, Device `Object_List`, protocol metadata, and object
  registry exposure after `I-Am`.

### `phase0.current_code_baseline`

The compatibility baseline is anchored in the canonical server assembly path:

- `BACnetServer::new(...)` creates and registers the mandatory Device object.
- The default service registry includes `ReadProperty`, `ReadPropertyMultiple`,
  `WriteProperty`, and `WritePropertyMultiple`.
- The default unconfirmed service registry includes `WhoIsHandler`, which
  emits `I-Am` responses for matching `Who-Is` requests.
- Empty user registries are still expected to expose the mandatory Device
  object because the server registers it during construction.
- Device `Object_Name`, `Object_List`, protocol services supported, protocol
  object types supported, vendor, model, firmware, and application software
  version are expected to be readable by BACnet explorers.

This phase does not assert that every encoding detail is already correct. It
only fixes the baseline that the default construction path intends to expose
explorer-readable Device metadata.

### `phase0.yabe_sequence_contract`

All later phases should use this sequence as the generic BACnet explorer
compatibility contract. It is named after YABE because the issue was observed
there, but it must not become a YABE-only special case.

| Step | Request / Observation | Expected Result |
|---|---|---|
| 1 | `Who-Is` | Server sends `I-Am` for the configured device instance. |
| 2 | Select discovered device | Explorer has enough address/device identity data to continue. |
| 3 | `ReadProperty(Device, Object_Name)` | Non-empty configured device name. |
| 4 | `ReadProperty(Device, Object_List)` without array index | Full object list containing at least the Device object. |
| 5 | `ReadProperty(Device, Object_List, index = 0)` | Object count, following BACnet array semantics. |
| 6 | `ReadProperty(Device, Object_List, index = 1..n)` | Individual object identifiers, with index `1` returning the Device object in the empty-registry case. |
| 7 | Object-level `Object_Name` / `Object_Type` reads | Names and types are readable for returned object identifiers. |
| 8 | Optional `ReadPropertyMultiple` metadata probe | Batch reads work where supported, and single-property fallback remains stable. |

### `phase0.risk_register`

| Risk / Baseline Item | Current Assumption | Why It Matters For YABE | Follow-up Phase | Acceptance |
|---|---|---|---|---|
| `Object_List array index semantics` | `Object_List` is a BACnet array and must honor index `0` as count and `1..n` as elements. | Explorers may enumerate object lists by indexed reads rather than only full-array reads. | Phase 1 | Deterministic test proves count, first element, and invalid-index behavior. |
| `full-array encoding` | Omitting array index should return a full array/list of object identifiers. | Explorers may request the full `Object_List` and decode all objects at once. | Phase 1 | Full `Object_List` response decodes and contains at least the Device object. |
| `empty-registry UX` | Empty user registry should still expose the mandatory Device object, but no synthetic sample points. | YABE may look sparse but should not look broken or nameless. | Phase 1, Phase 4 | Device name and Device object are visible; docs explain empty-registry behavior. |
| `ReadPropertyMultiple fallback` | Explorers may prefer `ReadPropertyMultiple`, but single-property reads must remain reliable. | Some clients batch metadata reads and fall back only after a stable error. | Phase 3 | Surrogate interop records batch-read behavior and single-read fallback. |
| `protocol metadata readability` | Device protocol and vendor metadata should be readable from default construction. | Explorers use these properties to decide what they can browse or display. | Phase 1 | Protocol services/object types supported and vendor/model/version fields decode. |
| `manual YABE reproducibility` | YABE stays manual/capture-only, not default CI automation. | The original GUI workflow still needs a reproducible maintainer runbook. | Phase 2 | Runbook and capture schema define exactly what evidence to collect. |

| Task | Description | Acceptance |
|---|---|---|
| `phase0.issue_summary` | Record the issue as a post-discovery metadata problem, not as a missing `Who-Is` response. | Maintainer-facing notes say `Who-Is` / `I-Am` is expected to be automatic. |
| `phase0.current_code_baseline` | Document that an empty user registry still produces the mandatory Device object. | Baseline points to `BACnetServer::new(...)`, Device object creation, and `WhoIsHandler` registration. |
| `phase0.yabe_sequence_contract` | Freeze the expected YABE-style read sequence listed above. | Later tests do not invent a different sequence. |
| `phase0.risk_register` | Track likely gaps: `Object_List` array indexes, full-array encoding, empty-registry UX, and `ReadPropertyMultiple` fallback behavior. | Each risk maps to a deterministic or interop task in later phases. |

## Phase 1. Deterministic Empty-Registry Regression

This phase belongs in the normal deterministic BACnet integration lane. It must
not require YABE or any external BACnet peer.

The initial deterministic regression is implemented by
`empty_registry_default_device_metadata_is_explorer_readable` in
`crates/mabi-bacnet/tests/profile_basic_property.rs`. It locks the empty-registry
Device metadata path and the BACnet `Object_List` array semantics that BACnet
explorers rely on.

| Task | Description | Acceptance |
|---|---|---|
| `phase1.empty_registry_whois` | Start a default server with an empty user registry and send `Who-Is`. | The server responds with `I-Am` for the configured device instance. |
| `phase1.device_object_name` | Read `ReadProperty(Device, Object_Name)` over the real UDP server path. | The response is a non-empty `CharacterString` matching server config. |
| `phase1.object_list_full` | Read `ReadProperty(Device, Object_List)` without array index. | The full response contains at least the Device object identifier. |
| `phase1.object_list_indexed` | Read `Object_List` with index `0`, index `1`, and an invalid index. | Index `0` returns count, index `1` returns Device object, invalid index returns a stable BACnet error. |
| `phase1.protocol_metadata` | Read explorer-relevant Device metadata. | `Protocol_Services_Supported`, `Protocol_Object_Types_Supported`, vendor, model, firmware, and application version are readable. |

Implementation notes:

- Prefer adding a new deterministic profile test or extending
  `profile_basic_property.rs` with an explicit `empty_registry` test.
- Keep assertions on decoded BACnet values, not just packet receipt.
- The generic array property path should keep BACnet array semantics correct for
  `Object_List` and future array properties: index `0` is count, and indexes
  `1..n` are 1-based element access.

## Phase 2. YABE-Style Manual Capture Lane

YABE is valuable as a human-facing compatibility check, but it remains a
manual/capture source rather than a CI dependency.

The Phase 2 capture seed is `yabe-empty-registry-device-metadata` under
`verification/bacnet/captures/yabe/empty-registry-device-metadata/`. It is
separate from the existing `yabe-discovery-property-io-loopback` property I/O
seed so the issue-specific empty-registry metadata evidence does not get mixed
with sample-object demo coverage.

| Task | Description | Acceptance |
|---|---|---|
| `phase2.yabe_runbook` | Add a manual runbook for scan, device selection, object tree expansion, and expected observations. | `runbook.md` tells a maintainer exactly how to verify `I-Am`, Device `Object_Name`, full `Object_List`, indexed `Object_List`, and the empty-registry object tree. |
| `phase2.capture_schema` | Define normalized artifact fields for YABE captures. | `manifest.toml` records tool version placeholder, OS placeholder, bind address, device instance, observed object tree, failed property, notes, and artifact paths. |
| `phase2.seed_capture` | Add a curated capture only after deterministic Phase 1 behavior is verified. | The `yabe-empty-registry-device-metadata` capture is listed in `verification/bacnet/captures/catalog.toml` and owns `manifest.toml`, `replay.json`, `packet-summary.json`, and `runbook.md`. |
| `phase2.no_gui_ci` | Keep GUI automation out of CI. | The capture catalog marks YABE artifacts as `ci_executable = false`, and static corpus tests assert that the YABE lane stays `capture_manual`. |

## Phase 3. Automated Surrogate Interop

This phase uses non-GUI peers to emulate the same property sequence YABE uses.
It improves confidence without adding GUI fragility.

The Phase 3 surrogate lane is implemented by two ignored interop profiles:
`bacpypes3_yabe_sequence_smoke_contract` is the exact single-property metadata
sequence peer, and `bac0_yabe_readmultiple_probe_smoke_contract` is the
high-level `ReadPropertyMultiple` style probe.

| Task | Description | Acceptance |
|---|---|---|
| `phase3.bacpypes3_yabe_sequence` | Add an ignored/containerized BACpypes3 probe that runs the expected sequence. | Transcript confirms discovery, Device `Object_Name`, full and indexed `Object_List`, and returned object name/type reads. |
| `phase3.bac0_readmultiple_probe` | Add a BAC0-style `readMultiple` probe for Device and object metadata. | Transcript records successful metadata `ReadPropertyMultiple` where supported and an explicit indexed `Object_List` fallback note where BAC0 cannot express the shape reliably. |
| `phase3.transcript_contract` | Normalize output around discovered name, object count, object names, fallback behavior, and failure category. | Transcripts expose `device_name_ok`, `object_list_full_ok`, `object_list_count_ok`, `object_list_first_ok`, `object_name_reads_ok`, `read_multiple_metadata_ok`, `unsupported_features`, and `failure_category`. |
| `phase3.regression_boundary` | Keep surrogate interop out of default workspace tests. | Tests remain `#[ignore]` and are only run through the BACnet interop lane or direct ignored profile commands. |

## Phase 4. CLI And README UX

This phase prevents user confusion around empty registries and demo visibility.
It is implemented as a user-facing policy: the core `BACnetServer::new(...)`
path and the default `mabi serve bacnet` path expose only the mandatory Device
object unless demo/sample objects are explicitly requested.

| Task | Description | Acceptance |
|---|---|---|
| `phase4.empty_registry_docs` | Document that an empty registry exposes the mandatory Device object only. | Users understand that YABE may show only the Device object unless sample points are registered. |
| `phase4.default_policy` | Preserve non-surprising default construction. | `BACnetServer::new(...)` and default `mabi serve bacnet` do not silently inject sample analog/binary points. |
| `phase4.demo_fixture_option` | Use explicit CLI demo object opt-in for users who want visible sample objects immediately. | `mabi serve bacnet --objects <N>` is documented as demo/sample data, while the default remains Device-only. |
| `phase4.issue_response_template` | Provide a friendly maintainer response. | The response explains automatic `Who-Is` / `I-Am`, Device-only empty registry behavior, and opt-in demo objects. |

Suggested issue response:

```text
Thanks for digging into this and for the follow-up.

The default server is expected to respond to Who-Is automatically, so you
should not need to manually build an I-Am response for normal discovery.

With an empty registry, the expected default view is intentionally sparse:
YABE should see the mandatory Device object, its Object_Name, Object_List, and
protocol metadata, but it should not see sample analog/binary points unless you
ask for demo data explicitly. For the CLI, that means:

    mabi serve bacnet --port 47808 --instance 1234 --objects 100

From what you described, this looks less like the core discovery path is
broken and more like a post-discovery metadata compatibility gap. We have been
tightening the default Device object / Object_Name / Object_List / registry
exposure behavior against deterministic tests and non-GUI BACpypes3/BAC0
surrogate interop flows.

Thanks for pointing out the improvement area. It is a useful compatibility
gap, and I would like to tighten this part up properly.
```

## Phase 5. Acceptance And Release Policy

Phase 5 is the release gate for this compatibility track. It makes the
completion criteria explicit enough that the issue cannot be closed by a single
unit test, and it keeps the existing BACnet verification boundaries intact.

| Task | Description | Acceptance |
|---|---|---|
| `phase5.acceptance_matrix` | Define completion criteria for deterministic regression, manual capture, optional interop, docs/UX, and release readiness. | A future patch must satisfy every lane below before the YABE compatibility issue is considered complete. |
| `phase5.release_notes` | Require a release note when the implementation ships. | Release notes include: "Improved BACnet explorer/YABE compatibility for empty-registry Device metadata discovery." |
| `phase5.no_default_heavy_tools` | Preserve verification lane boundaries. | Docker, GUI tools, external peers, and perf thresholds never enter `cargo test --workspace`. |

## Acceptance Matrix

| Lane | Required result | Gate command or artifact |
|---|---|---|
| Deterministic regression | Empty-registry default server responds to `Who-Is`, exposes readable Device `Object_Name`, full and indexed `Object_List`, protocol metadata, and stable BACnet errors for invalid array indexes. | `cargo test -p mabi-bacnet --test profile_basic_property` |
| Manual capture | YABE can discover the device, resolve the Device Object Name, and show at least the mandatory Device object for empty registries. | `verification/bacnet/captures/yabe/empty-registry-device-metadata/` with `ci_executable = false` |
| Optional interop | BACpypes3 and BAC0 surrogate profiles exercise the YABE-style metadata sequence without GUI automation. | `cargo test -p mabi-bacnet --test interop_profiles -- --ignored bacpypes3_yabe_sequence_smoke_contract` and `cargo test -p mabi-bacnet --test interop_profiles -- --ignored bac0_yabe_readmultiple_probe_smoke_contract` |
| Docs and UX | README and CLI docs explain that empty registry means mandatory Device object only, and demo/sample objects are opt-in through `--objects <N>`. | BACnet README, CLI README, and simulator docs use the same policy language. |
| Release readiness | The release note explicitly calls out improved BACnet explorer/YABE compatibility for empty-registry Device metadata discovery. | Release notes or changelog entry contains the required phrase below. |

## Release Note Requirement

When this compatibility work ships, the release note must include this wording
or an equivalent sentence with the same meaning:

```text
Improved BACnet explorer/YABE compatibility for empty-registry Device metadata discovery.
```

The note should mention that `Who-Is` / `I-Am` remains automatic, empty
registries expose the mandatory Device object by default, and demo objects are
available only through an explicit opt-in path such as `mabi serve bacnet
--objects <N>`.

## Default Lane Boundary

The default workspace lane is intentionally lightweight and deterministic.
These items are forbidden in `cargo test --workspace`:

- Docker or Docker Compose
- YABE or other GUI tools
- external BACnet peer processes
- Python, JVM, Node, or C peer harness requirements
- threshold-based perf assertions

Allowed default-lane coverage is limited to Rust deterministic regression,
static corpus validation, and policy checks that do not spawn external tools.

## Validation Commands

```bash
cargo test -p mabi-bacnet
cargo test -p mabi-bacnet --test profile_basic_property
cargo test -p mabi-bacnet --test capture_corpus
cargo test -p mabi-bacnet --test perf_contract
cargo test -p mabi-bacnet --test interop_profiles -- --ignored bacpypes3_yabe_sequence_smoke_contract
cargo test -p mabi-bacnet --test interop_profiles -- --ignored bac0_yabe_readmultiple_probe_smoke_contract
cargo test --workspace
```

The ignored interop commands are optional release-confidence checks. They must
not become prerequisites for `cargo test --workspace`.

## Static Policy Validation

The repository also keeps static regression checks for this policy:

```bash
git diff --check
rg -n "yabe-discovery-compatibility-plan" docs/bacnet-simulator/README.md
rg -n "phase5.acceptance_matrix|phase5.release_notes|phase5.no_default_heavy_tools" docs/bacnet-simulator/yabe-discovery-compatibility-plan.md
```

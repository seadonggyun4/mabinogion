# BACnet Verification

This directory defines the canonical BACnet verification contracts for
`mabi-bacnet`, including deterministic regression, ignored interop, manual
capture corpus, and release-only perf lanes.

The current Phase 5 operating boundary is:

- default regression stays deterministic and runs with `cargo test --workspace`
- YABE and VTS stay manual/capture-only with `ci_executable = false`
- BACpypes3 and BAC0 YABE metadata surrogates stay in the ignored interop lane
- perf stays release-only and ignored
- Docker, GUI tools, external peers, and perf thresholds stay out of the
  default workspace path

## Test Layers

- `cargo test --workspace`
  Runs deterministic regression, including the Phase 1 BACnet integration
  profiles. This remains the canonical developer path and must stay green
  without Docker, external peers, or perf thresholds.
- `cargo test -p mabi-bacnet --test interop_matrix -- --ignored`
  Runs the self-contained containerized BACnet interop matrix defined in
  [interop-matrix.toml](/Users/dgseo/Desktop/trap-project/mabinogion/verification/bacnet/interop-matrix.toml).
- `cargo test -p mabi-bacnet --test interop_profiles -- --ignored bacpypes3_canary_profile_smoke_contract`
  Runs the BACpypes3 programmable peer smoke contract directly.
- `cargo test -p mabi-bacnet --test interop_profiles -- --ignored bacpypes3_yabe_sequence_smoke_contract`
  Runs the BACpypes3 YABE-style Device metadata sequence directly.
- `cargo test -p mabi-bacnet --test interop_profiles -- --ignored bac0_canary_profile_smoke_contract`
  Runs the BAC0 controller-style peer smoke contract directly.
- `cargo test -p mabi-bacnet --test interop_profiles -- --ignored bac0_yabe_readmultiple_probe_smoke_contract`
  Runs the BAC0 ReadPropertyMultiple-style YABE metadata surrogate directly.
- `cargo test -p mabi-bacnet --test interop_profiles -- --ignored bacnet_stack_canary_profile_smoke_contract`
  Runs the bacnet-stack reference C peer smoke contract directly.
- `cargo test -p mabi-bacnet --test interop_profiles -- --ignored bacnet4j_canary_profile_smoke_contract`
  Runs the BACnet4J JVM peer smoke contract directly.
- `cargo test -p mabi-bacnet --release --test perf_contract -- --ignored`
  Runs the release-only BACnet perf contract lane. This is a policy gate, not a
  default workspace benchmark suite.

## Self-Contained Interop Matrix

The ignored interop matrix is repo-contained:

- target metadata lives in `interop-matrix.toml`
- container orchestration lives in
  [compose.yaml](/Users/dgseo/Desktop/trap-project/mabinogion/verification/bacnet/compose.yaml)
- target harnesses live under `verification/bacnet/harness/`

Each target is executed through the repo-local
[run-target.sh](/Users/dgseo/Desktop/trap-project/mabinogion/verification/bacnet/run-target.sh)
wrapper. No environment-provided runner commands are required.

The current matrix keeps a single-container loopback topology and activates the
canonical non-GUI peer set plus YABE-style surrogate targets:

- `bacpypes3-canary`
- `bacpypes3-yabe-sequence`
- `bac0-canary`
- `bac0-yabe-readmultiple`
- `bacnet-stack-canary`
- `bacnet4j-canary`

Each harness owns its Dockerfile, runtime script, and peer client implementation
inside `verification/bacnet/harness/`. The Rust test starts the SUT in-process,
then invokes the peer-specific script inside the same container.

The YABE surrogate targets are still non-GUI interop tests. They replay the
post-discovery metadata sequence that GUI explorers use while keeping YABE
itself in the manual capture lane.

## Capture Corpus Lane

GUI-oriented tools stay out of CI automation, but they now have a canonical
artifact lane under [captures/](/Users/dgseo/Desktop/trap-project/mabinogion/verification/bacnet/captures).

- [captures/catalog.toml](/Users/dgseo/Desktop/trap-project/mabinogion/verification/bacnet/captures/catalog.toml)
  is the machine-readable index
- `YABE` contributes seeded discovery and property I/O replay artifacts
- `VTS` contributes seeded negative-case and duplicate-request replay artifacts

This lane is intentionally:

- manual-only
- artifact-only
- excluded from the interop matrix and default workspace CI paths

## Local Prerequisites

- If Docker and Docker Compose are available, the ignored interop test will run
  the matrix.
- If Docker or the daemon is unavailable locally, the ignored interop test
  prints a skip summary and exits successfully.
- In CI, Docker availability is treated as required for the nightly/manual
  BACnet interop workflow.

## Perf Policy

BACnet perf policy is intentionally narrow in the current phase:

- perf remains a dedicated `--release -- --ignored` lane
- threshold-based perf assertions are forbidden in `cargo test --workspace`
- the current perf contract only enforces the execution boundary, not numeric
  budgets
- future perf benchmarks must stay outside the default developer and PR path

## Release Policy

YABE compatibility work is release-ready only when all of these hold:

- deterministic empty-registry metadata regression passes
- YABE manual capture corpus remains artifact-only and `ci_executable = false`
- BACpypes3 and BAC0 YABE surrogate profiles remain ignored interop checks
- README and CLI docs explain Device-only empty registries and opt-in demo
  objects
- release notes mention improved BACnet explorer/YABE compatibility for
  empty-registry Device metadata discovery

## Current Capability Coverage

The active peer harness lane upgrades three capabilities into active interop
coverage:

- `discovery`
- `property_io`
- `property_multiple`

The remaining capability matrix stays intentionally out of scope until later
interop expansion phases.

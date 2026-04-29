# BACnet Verification

This directory defines the canonical BACnet verification contracts for
`mabi-bacnet`, including deterministic regression, ignored interop, manual
capture corpus, and release-only perf lanes.

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
- `cargo test -p mabi-bacnet --test interop_profiles -- --ignored bac0_canary_profile_smoke_contract`
  Runs the BAC0 controller-style peer smoke contract directly.
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

The current matrix keeps a single-container loopback topology but now activates
four canonical non-GUI peers:

- `bacpypes3-canary`
- `bac0-canary`
- `bacnet-stack-canary`
- `bacnet4j-canary`

Each harness owns its Dockerfile, runtime script, and peer client implementation
inside `verification/bacnet/harness/`. The Rust test starts the SUT in-process,
then invokes the peer-specific script inside the same container.

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

## Current Capability Coverage

The active peer harness lane upgrades three capabilities into active interop
coverage:

- `discovery`
- `property_io`
- `property_multiple`

The remaining capability matrix stays intentionally out of scope until later
interop expansion phases.

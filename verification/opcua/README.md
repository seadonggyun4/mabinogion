# OPC UA Verification

This directory defines the canonical verification contracts for
`mabi-opcua`.

## Test Layers

- `cargo test --workspace`
  Runs deterministic fixture regression and the CTT-targeted suite. This is
  the canonical developer path and must stay green without Docker, external
  runners, or perf thresholds.
- `cargo test -p mabi-opcua --test interop_matrix -- --ignored`
  Runs the self-contained containerized interop matrix defined in
  [interop-matrix.toml](/Users/dgseo/Desktop/trap-project/mabinogion/verification/opcua/interop-matrix.toml).
- `cargo test -p mabi-opcua --release --test transport_perf_contract -- --ignored`
  Runs the release-only transport perf contract suite.

## Self-Contained Interop Matrix

The ignored interop matrix is repo-contained:

- target metadata lives in `interop-matrix.toml`
- container orchestration lives in
  [compose.yaml](/Users/dgseo/Desktop/trap-project/mabinogion/verification/opcua/compose.yaml)
- target harnesses live under `verification/opcua/harness/`

Each target is executed through the repo-local
[run-target.sh](/Users/dgseo/Desktop/trap-project/mabinogion/verification/opcua/run-target.sh)
wrapper. No environment-provided runner commands are required.

The current harnesses are self-contained target-profile smoke contracts for:

- `open62541`
- `milo`
- `async-opcua`

They are designed to be deterministic and runnable from repo assets alone,
while keeping the default workspace path lightweight. Push and PR automation
should stop at `cargo test --workspace`; nightly or manual automation may run
the interop matrix and the release-only perf suite.

## Local Prerequisites

- If Docker and Docker Compose are available, the ignored interop test will run
  the matrix.
- If Docker or the daemon is unavailable locally, the ignored interop test
  prints a skip summary and exits successfully.
- In CI, Docker availability is treated as required for the nightly/manual
  interop workflow.

## Perf Policy

Threshold-based perf assertions must never be part of the default workspace
suite. Perf contracts stay in release-only ignored tests or benches so
`cargo test --workspace` remains deterministic and green.

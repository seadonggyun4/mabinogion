# KNX Self-Contained Interop

This directory owns the KNX interop verification plane. It is intentionally
separate from the production crate and from the default deterministic test path.

## Lanes

- Default regression: `cargo test --workspace`
- Interop matrix: `cargo test -p mabi-knx --test interop_matrix -- --ignored`
- Direct peer profile: `cargo test -p mabi-knx --test interop_profiles -- --ignored <profile_test_name>`
- Static corpus replay: `cargo test -p mabi-knx --test capture_corpus` and
  `cargo test -p mabi-knx --test negative_fixtures`

The interop lane uses Docker Compose and repo-owned harness assets. If Docker is
missing on a local developer machine, the ignored matrix test prints a skip
summary and succeeds. In CI, or when `MABI_KNX_INTEROP_REQUIRED=1` is set,
Docker is required and missing Docker is a test failure.

## Current Matrix

| Target | Service | Tier | Coverage |
|---|---|---|---|
| `xknx-canary` | `xknx-canary` | `nightly` | discovery, description, tunneling connect, state observation, group value read/write |
| `calimero-tools` | `calimero-tools` | `nightly` | Discover, Description, ProcComm-style group read/write, NetworkMonitor transcript capture |
| `knxd` | `knxd` | `nightly` | gateway-style tunnel reconnect, state observation, group IO, routing unsupported-mode record |
| `knxjs` | `knxjs` | `nightly` | Node `knx@2.5.4` DPT/group telegram parity and routing unsupported-mode record |
| `openknx-thelsing` | `openknx-thelsing` | `manual` | optional device-stack/corpus replay, DPT/group IO, KNX Secure future marker |

The active nightly matrix runs the XKNX, Calimero Tools, knxd, and Node `knx`
targets. OpenKNX/thelsing is intentionally manual because it is primarily a
device-stack/corpus source and may require heavier Linux/device-stack context.

## Commands

```bash
cargo test -p mabi-knx --test interop_matrix -- --ignored
docker compose -f verification/knx/compose.yaml run --rm xknx-canary
docker compose -f verification/knx/compose.yaml run --rm calimero-tools
docker compose -f verification/knx/compose.yaml run --rm knxd
docker compose -f verification/knx/compose.yaml run --rm knxjs
MABI_KNX_INTEROP_INCLUDE_MANUAL=1 cargo test -p mabi-knx --test interop_matrix -- --ignored
docker compose -f verification/knx/compose.yaml run --rm openknx-thelsing
cargo test -p mabi-knx --test interop_profiles -- --ignored xknx_canary_profile_smoke_contract
```

## Contract

The matrix manifest is `interop-matrix.toml`. Each target maps to a repo-owned
Compose service; external runner commands are not accepted. The runner validates
that the requested target exists in the manifest and that it maps to the same
Compose service before invoking Docker.

Each peer writes the same JSON transcript shape. Rust assertions treat that
transcript as the source of truth rather than parsing stdout.

Required transcript fields:

- `schema_version`
- `target`
- `peer`
- `sut_addr`
- `capabilities`
- `steps`
- `failure_category`
- `errors`
- `artifacts`

Known failure categories are `tool_missing`, `build_failure`,
`protocol_failure`, `unsupported_feature`, and `timeout`. Unsupported protocol
modes, such as multicast routing in a single-container loopback topology, must
be recorded as `unsupported` capability results instead of disappearing from
the transcript.

## Capture Corpus

Phase 4 adds an artifact-only corpus under `captures/` and deterministic
negative fixtures under `fixtures/`.

- `captures/catalog.toml` records curated peer evidence from XKNX, Calimero
  Tools, knxd, and OpenKNX/thelsing.
- `fixtures/catalog.toml` records static malformed/HPAI/tunnel/sequence/service/DPT
  negative cases.
- Capture entries are manual-only and always `ci_executable=false`.
- Static replay tests may run in the default lane because they do not launch
  Docker, GUI tools, external peers, or physical KNX hardware.

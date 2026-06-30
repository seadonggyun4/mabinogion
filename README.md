<div align="center">
  <img width="500px" alt="mabinogion Banner" src="https://github.com/user-attachments/assets/bad251b6-b6dc-4452-ad43-d9345a32bd0b" />
</div>

<h1 align="center">Mabinogion</h1>

<p align="center">
  <strong>Protocol resilience engine for industrial protocol sessions</strong>
</p>

<p align="center">
  <em>"Spawn protocols at will"</em>
</p>

[![Crates.io](https://img.shields.io/crates/v/mabi-cli.svg)](https://crates.io/crates/mabi-cli)
[![License: Apache 2.0](https://img.shields.io/badge/License-Apache%202.0-orange.svg)](https://opensource.org/licenses/Apache-2.0)
[![Powered by Rust](https://img.shields.io/badge/Powered%20by-Rust-orange.svg)](https://www.rust-lang.org/)
[![Downloads](https://img.shields.io/crates/d/mabi-cli.svg?color=brightgreen)](https://crates.io/crates/mabi-cli)

---

## What is Mabinogion?

**Mabinogion** is a Rust protocol resilience engine for running local
industrial protocol sessions, managing runtime lifecycles, and exporting
execution evidence.

It can still be used directly as a local protocol simulator for Modbus,
OPC UA, BACnet/IP, and KNXnet/IP. `mabinogion` owns protocol/session
execution, runtime contracts, installed CLI surfaces, release metadata, and
evidence export.

`mabinogion` does **not** define external product responsibilities, score
results, publish third-party proof reports, issue certification, or replace
official certification programs. This repository provides the engine and
machine-readable evidence surface.

---

## Quick Start

```bash
cargo install mabi-cli
mabi doctor
mabi --format json version
```

Start a local protocol service:

```bash
mabi serve modbus --port 5020 --devices 10 --points 100
mabi serve opcua --config opcua.yaml --session default
mabi serve bacnet --port 47808 --instance 1234
mabi serve knx --port 3671 --address 1.1.1
```

`mabi doctor` verifies the installed binary and built-in protocol runtimes
without requiring Docker, Python, Java, Node, knxd, or external peer tools.
Optional interop and performance lanes remain source-tree, manual, nightly, or
release verification work.

---

## Mabinogion Role

<!--
README SCOPE GUARD:
Keep this README about mabinogion only. Do not reintroduce product-family role
tables, sibling repository ownership, external UI/backend ownership, scoring
ownership, proof-publication ownership, or certification-issuance ownership
here. Those boundaries belong in the owning repositories or dedicated contract
documents, not in the mabinogion README.
-->

`mabinogion` owns:

- Protocol/session execution.
- Runtime lifecycle and runtime contracts.
- Installed CLI runner surfaces.
- Evidence export.
- Release metadata for the engine.

`mabinogion` does not own external product roles, external user interfaces,
external job orchestration, result scoring, proof publication, or certification
issuance.

---

## Installed CLI Surface

| Command | Role |
| --- | --- |
| `mabi doctor` | Installed binary and runtime smoke check. |
| `mabi serve` | Local Modbus, OPC UA, BACnet/IP, or KNXnet/IP service execution. |
| `mabi inspect` | Runtime, protocol, schema, and config inspection. |
| `mabi validate` | Scenario and config validation with machine-readable output. |
| `mabi scenario` | Declarative local scenario execution. |
| `mabi chaos` | Fault orchestration over local protocol sessions. |
| `mabi version` | Engine, protocol capability, contract, release, and trial compatibility metadata. |

Runner-facing commands support `--format json`, `--format yaml`, and
`--format compact` envelopes for automation. Human table output remains
available for local operators.

---

## Contracts

| Contract | Purpose |
| --- | --- |
| `runtime-contract-v1` | Runtime lifecycle, error taxonomy, readiness, and snapshot metadata. |
| `unified-readiness-contract-v1` | Shared protocol capability/profile/lane mapping. |
| `local-runner-contract-v1` | Stable machine-readable CLI envelopes and exit categories. |
| `run-evidence-schema-v1` | Execution evidence exported for proof report inputs. |
| `trial-artifact-contract-v1` | Failure replay artifact metadata and visibility policy. |
| `version-metadata-contract-v1` | Engine release, protocol capability, and trial compatibility metadata. |

See [docs/README.md](./docs/README.md) for the documentation map.

---

## Installation For Library Users

```toml
[dependencies]
mabi-core = "1.7.1"
mabi-runtime = "1.7.1"
mabi-modbus = "1.7.1"
mabi-opcua = "1.7.1"
mabi-bacnet = "1.7.1"
mabi-knx = "1.7.1"
mabi-scenario = "1.7.1"
mabi-chaos = "1.7.1"
```

The Mabinogion release version is sourced from `[workspace.package].version` in
[Cargo.toml](./Cargo.toml). After changing the root version, run:

```bash
python3 scripts/release-version.py sync
python3 scripts/release-version.py check
```

---

## Documentation

| Area | Guide |
| --- | --- |
| Documentation map | [docs/README.md](./docs/README.md) |
| CLI and local runner surface | [docs/cli](./docs/cli/README.md) |
| Baseline and ownership | [docs/baseline](./docs/baseline/README.md) |
| Runtime contract | [docs/runtime](./docs/runtime/runtime-contract.md) |
| Protocol readiness | [docs/protocol-readiness](./docs/protocol-readiness/README.md) |
| Evidence export | [docs/evidence](./docs/evidence/README.md) |
| Release/version metadata | [docs/release](./docs/release/release-checklist.md) |
| Modbus | [docs/modbus-simulator](./docs/modbus-simulator/README.md) |
| OPC UA | [docs/opcua-simulator](./docs/opcua-simulator/README.md) |
| BACnet/IP | [docs/bacnet-simulator](./docs/bacnet-simulator/README.md) |
| KNXnet/IP | [docs/knx-simulator](./docs/knx-simulator/README.md) |

---

## Operational CI Position

Strategy documents treat CI as an operational gate, not as a list of every
developer check.

Deploy-blocking operational CI should prove that a release candidate can build
the release binary, run the installed CLI smoke path, emit structured version
metadata, and satisfy runner/evidence compatibility contracts.

Manual, nightly, or release verification is the right lane for external peer
interop, Docker matrices, long performance runs, and site replay captures.
Developer-only checks such as format, lint, and unit tests belong in
contributor guidance rather than the service-operation CI definition.

---

## Project Structure

```text
mabinogion/
├── crates/
│   ├── mabi-core/       # Shared domain kernel and release version surface
│   ├── mabi-runtime/    # Runtime/session contracts and evidence primitives
│   ├── mabi-modbus/     # Modbus runtime driver
│   ├── mabi-opcua/      # OPC UA runtime driver
│   ├── mabi-bacnet/     # BACnet/IP runtime driver
│   ├── mabi-knx/        # KNXnet/IP runtime driver
│   ├── mabi-scenario/   # Scenario parsing and execution
│   ├── mabi-chaos/      # Fault orchestration
│   └── mabi-cli/        # Installed mabi command
├── docs/                # Product-family and protocol documentation
└── scripts/             # Release/version guardrails
```

---

## License

Licensed under the Apache License, Version 2.0.

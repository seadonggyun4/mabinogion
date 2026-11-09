# Release Checklist

Release discipline keeps the installed `mabi` binary usable by local and
external runners.

Before a release candidate is accepted:

- Confirm the canonical source and public contracts remain publicly
  accessible and the release carries an OSI-approved license, as required by
  the [Open-Source Continuity Policy](../open-source-policy.md).
- `mabi --format json version` reports the workspace engine version,
  `version-metadata-contract-v2`, protocol capability versions, and all
  runner-facing contract versions.
- `docs/release/compatibility-matrix.yaml` has the same engine version as the
  workspace root `Cargo.toml`.
- Protocol capability changes are reflected in the compatibility matrix and
  the Unified Readiness matrix.
- The changelog classifies breaking changes against CLI output, config,
  runtime contract, readiness contract, run evidence schema, and version
  metadata contract surfaces.
- External consumers own runner admission decisions.
  Mabinogion only exports the metadata required for that decision.

Deploy-blocking operational checks should prove that the release artifact can
run and emit the required machine-readable contracts. Developer-only formatting,
lint, and unit checks belong in contributor docs, not in the strategy CI
definition.

## Canonical bounded soak

Build the release workspace, then exercise every built-in protocol lifecycle
ten times with a hard per-run bound:

```bash
cargo build --release --workspace
python3 scripts/soak.py \
  --binary target/release/mabi \
  --iterations 10 \
  --per-run-timeout 60
```

Each iteration runs the installed-style JSON `doctor` path and requires
successful launch, readiness, snapshot, and clean stop for Modbus, OPC UA,
BACnet, and KNX. The script fails on a timeout, a non-zero process exit, an
invalid output envelope, a missing protocol, or any failed lifecycle phase.
The explicit iteration count and per-run timeout make this the canonical
bounded soak; longer endurance campaigns may raise either value but do not
replace this release gate.

## Workspace publication order

All internal workspace dependencies require the same exact release line, so a
new version must become visible in the registry in dependency order. Publish
and verify each wave before starting the next one:

1. `mabi-core`
2. `mabi-runtime`
3. `mabi-modbus`, `mabi-opcua`, `mabi-bacnet`, `mabi-knx`, `mabi-scenario`,
   and `mabi-chaos` (independent after waves 1 and 2)
4. `mabi-cli`

For each crate, run `cargo package -p <crate>` and then the separately approved
`cargo publish -p <crate>`. Do not advance until the published version is
queryable from the registry index. A package failure that reports the current
workspace version as unavailable is evidence that the preceding wave has not
yet propagated; it is not a reason to weaken or remove the version constraint.

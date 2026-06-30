# Release Checklist

PHASE 5 release discipline keeps the installed `mabi` binary usable by
mabinogion-trials and imugi-back without moving trial decisions into
this repository.

Before a release candidate is accepted:

- `mabi --format json version` reports the workspace engine version,
  `version-metadata-contract-v1`, protocol capability versions, and all
  runner-facing contract versions.
- `docs/release/compatibility-matrix.yaml` has the same engine version as the
  workspace root `Cargo.toml`.
- Protocol capability changes are reflected in the compatibility matrix and
  the Unified Readiness matrix.
- The changelog classifies breaking changes against CLI output, config,
  runtime contract, readiness contract, run evidence schema, and version
  metadata contract surfaces.
- Imugi and Trials own allow/deny admission for engine and trial-suite pairs.
  Mabinogion only exports the metadata required for that decision.

Deploy-blocking operational checks should prove that the release artifact can
run and emit the required machine-readable contracts. Developer-only formatting,
lint, and unit checks belong in contributor docs, not in the strategy CI
definition.

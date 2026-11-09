# Changelog Policy

Every release note must describe changes in terms that external consumers can map
to runner compatibility.

Breaking-change categories:

- `cli`: machine-readable command output, exit code category, or envelope shape.
- `config`: accepted simulator configuration schema or defaults.
- `runtime_contract`: runtime error, readiness, session, or snapshot contract.
- `readiness_contract`: protocol capability/profile ids, lanes, coverage
  status, or profile mapping.
- `run_evidence_schema`: evidence fields, artifact visibility, or public/private
  summary behavior.
- `version_metadata_contract`: release metadata, compatibility matrix, or
  protocol capability version fields.

Protocol capability changes must include the affected protocol key,
capability version, profile ids, and whether external consumers should treat the
change as compatible, release-gated, or incompatible. The final allow/deny
decision remains outside `mabinogion`.

## Current contract migration

- `version-metadata-contract-v2` exposes `runner_metadata` with machine formats,
  supported commands, and external runner policy. Protocol `profile_ids` now
  match the native profile IDs in the readiness matrix.
- `unified-readiness-contract-v2` and `protocol-readiness-matrix-v2` use
  native protocol/profile identity.
- `run-evidence-schema-v2` exports engine/runtime facts.
  `RunEvidenceBuilder::new` now takes a run ID, protocol profile, and runtime
  snapshot.
- Replay metadata is described by `replay-artifact-contract-v1` and exported
  through `REPLAY_ARTIFACT_CONTRACT_VERSION`.
- `compatibility-matrix-v2` mirrors these contract versions. Consumers must
  check the advertised versions before parsing evidence or version payloads.

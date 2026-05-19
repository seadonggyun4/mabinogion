# Changelog Policy

Every release note must describe changes in terms that Forge and Trials can map
to runner compatibility.

Breaking-change categories:

- `cli`: machine-readable command output, exit code category, or envelope shape.
- `config`: accepted simulator configuration schema or defaults.
- `runtime_contract`: runtime error, readiness, session, or snapshot contract.
- `readiness_contract`: protocol capability/profile ids, lanes, coverage
  status, or trial profile mapping.
- `run_evidence_schema`: evidence fields, artifact visibility, or public/private
  summary behavior.
- `version_metadata_contract`: release metadata, compatibility matrix, or
  protocol capability version fields.

Protocol capability changes must include the affected protocol key,
capability version, trial profile ids, and whether Forge/Trials should treat the
change as compatible, release-gated, or incompatible. The final allow/deny
decision remains outside `mabinogion`.

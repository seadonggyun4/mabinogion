# Local Runner Contract

`local-runner-contract-v1` freezes the installed `mabi` CLI surfaces consumed by
local and external runners.

The contract is about reliable local invocation. It does not score results,
publish proof reports, or issue certifications.

## Machine Output Envelope

Runner-facing commands use the same envelope for `json`, `yaml`, and `compact`
output:

| Field | Meaning |
| --- | --- |
| `contract_version` | Local runner contract version. |
| `envelope_version` | CLI envelope schema version. |
| `command` | Stable command surface, such as `doctor` or `inspect protocols`. |
| `status` | `success` or `failure`. |
| `exit_code` | Process exit code that the runner will receive. |
| `exit_category` | Stable category for runner routing. |
| `generated_at` | UTC timestamp for this CLI response. |
| `engine_version` | Installed Mabinogion engine release version. |
| `data` | Command-specific payload. |
| `warnings` | Non-fatal warnings. |
| `errors` | Machine-readable error payloads. |

Table output remains human-facing and is not part of the parsing contract.

## Runner-Facing Commands

| Command | Contract role |
| --- | --- |
| `doctor` | Installed CLI and self-contained protocol runtime smoke. |
| `inspect protocols` | Protocol catalog and runner-compatible contract metadata. |
| `inspect schema` | Generic schema surfaces. |
| `inspect status` | Process-scoped runtime status. |
| `inspect modbus-config` | Compiled Modbus config/session summary. |
| `inspect opcua-config` | Compiled OPC UA config/session summary. |
| `validate scenario` | Scenario validation report. |
| `validate config` | Generic file validation report. |
| `validate modbus-config` | Typed Modbus config validation report. |
| `validate opcua-config` | Typed OPC UA config validation report. |
| `version` | Engine, protocol, feature flag, and contract version metadata. |

`version` reports `run-evidence-schema-v2`, `replay-artifact-contract-v1`, and
`version-metadata-contract-v2` so external consumers can reject incompatible
engines or evidence exporters before launching a runner.

Validation commands emit an envelope for both success and validation failure.
Validation failure exits with code `6`.

## Exit Codes

| Code | Category | Meaning |
| --- | --- | --- |
| `0` | `success` | Command completed successfully. |
| `2` | `input_contract_error` | Config, argument, or input contract error. |
| `6` | `validation_failure` | Validation completed and found invalid input. |
| `9` | `runtime_failure` | Runtime engine failed while operating a protocol service. |
| `124` | `timeout` | Bounded operation timed out. |
| `130` | `interrupted` | Runner or user interrupted execution. |
| `1` | `internal_failure` | Internal or unclassified failure. |

Existing CLI-specific detailed exit codes may remain, but runner envelopes expose
one of these categories.

# Run Evidence and Proof Export

The evidence contract defines the runtime facts that `mabinogion` exports
for local runners and external consumers.

`mabinogion` remains an execution evidence exporter. It does not own scoring
policy, proof report publication, or certification issuance.

## Contracts

| Artifact | Purpose |
| --- | --- |
| `run-evidence-schema.yaml` | Machine-readable source of truth for `run-evidence-schema-v2`. |
| `replay-artifact-contract.yaml` | Artifact metadata and public/private visibility policy. |
| `sample-run-evidence.json` | Example Proof Report input produced from the schema. |

## Evidence Boundary

Run evidence includes runtime/session facts that external consumers can trust:

- run id
- engine version
- protocol profile
- start and end timestamps
- feature flags
- failure replay artifact metadata
- public/private boundary metadata
- runtime session snapshot
- optional report-friendly metrics

Run evidence does not include a score, certification statement, or public proof
publication decision.

## Public and Private Artifacts

`public_summary()` exposes only public-safe metadata and public replay summaries.
Private raw logs, packet captures, private paths, and private digests remain
private artifact metadata referenced by the full evidence object.

The boundary is explicit so external consumer can publish a Proof Report without leaking raw
customer diagnostics, while private external-consumer workflows can still locate
failure replay material.

## Metrics Boundary

Prometheus metrics are runtime telemetry. Run evidence metrics are report
artifacts. PHASE 4 records report-friendly summaries such as latency, reconnect
count, error count, recovery events, and resource usage, but it does not replace
or scrape Prometheus.

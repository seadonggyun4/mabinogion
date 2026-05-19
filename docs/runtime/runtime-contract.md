# Runtime Contract

This document freezes the PHASE 1 runtime contract for `mabinogion`.

`mabinogion` owns runtime lifecycle, protocol/session execution, structured
runtime errors, and service snapshot metadata export. It does not own trial
definitions, scoring, certification issuance, or public proof publication.

The machine-readable source is `runtime-contract.yaml`.

## Lifecycle Contract

The public runtime lifecycle is centered on these types:

| Type | Role |
| --- | --- |
| `ManagedService` | Protocol service lifecycle trait. |
| `ServiceHandle` | Spawn, readiness, wait, stop, and snapshot wrapper. |
| `RuntimeSession` | Same-process multi-service session coordinator. |
| `ServiceStatus` | Current service state and readiness view. |
| `ServiceSnapshot` | Structured status plus metadata export. |
| `RuntimeSessionSnapshot` | Session-level snapshot envelope. |

The stable lifecycle is:

1. Build a `RuntimeSession` from `RuntimeSessionSpec`.
2. Spawn each `ManagedService`.
3. Wait for readiness with a bounded timeout.
4. Export service snapshots.
5. Stop services in reverse order.

## Error Taxonomy

Runtime consumers should use `RuntimeError::kind()` and `RuntimeError::info()`
instead of exhaustively matching enum variants.

| Kind | Meaning |
| --- | --- |
| `config_error` | Invalid launch config, runtime config, address, profile, or model input. |
| `bind_error` | Bind/listen/address allocation failure. |
| `protocol_error` | Accepted config failed during service start, run, or graceful stop. |
| `timeout` | Readiness or bounded runtime wait timeout. |
| `internal_error` | Serialization failure, task join failure, or impossible runtime state. |

The enum is non-exhaustive. Adding new variants is allowed when `kind()` and
`info()` remain stable.

## Snapshot Metadata

Every snapshot returned through `ServiceHandle::snapshot()` or
`RuntimeSession::snapshots()` includes runtime-owned metadata under `_runtime`.

Stable `_runtime` fields:

- `contract_version`
- `snapshot_metadata_version`
- `captured_at`
- `service_name`
- `protocol`
- `state`
- `ready`
- `started_at`
- `last_error`

Protocol-specific top-level metadata remains protocol-owned. Nested metrics
fields are experimental unless promoted in `runtime-contract.yaml`.

Human table output hides reserved metadata keys that start with `_`. JSON, YAML,
and compact machine output preserve those keys.

## Protocol Stable Metadata

| Protocol | Stable top-level keys |
| --- | --- |
| Modbus | `transport`, `devices`, `points`, `bind_address` or `rtu_transport`, `metrics` |
| OPC UA | `endpoint`, `transport_protocol`, `nodes`, `devices`, `namespaces`, `security_profile`, `durability_mode`, `restored_subscriptions`, `detached_restored_subscriptions`, `generated_types`, `stats` |
| BACnet | `bind_address`, `device_instance`, `objects`, `bbmd_enabled`, `metrics` |
| KNX | `bind_address`, `individual_address`, `group_objects`, `metrics` |

## Readiness And Session Reports

`ServiceReadinessReport` provides a structured readiness result for local
runner and health checks. `RuntimeSessionSnapshot` wraps normalized
`ServiceSnapshot` values into a session-level envelope.

Both include `runtime-contract-v1` so Forge and Trials can reject incompatible
future contracts deliberately.

## Breaking Change Policy

Breaking changes require a contract version bump:

- Removing a `RuntimeErrorKind`.
- Removing a stable `_runtime` field.
- Changing a stable field type or meaning.
- Removing a protocol stable top-level metadata key.

Non-breaking changes:

- Adding enum variants while preserving `kind()` and `info()`.
- Adding `_runtime` fields.
- Adding protocol top-level metadata keys.
- Adding nested metrics fields.

# Mabinogion Documentation

Mabinogion is a protocol resilience engine for Mabinogion trials and local
industrial protocol sessions. These docs are organized around the product
family boundary: `mabinogion` executes protocol sessions and exports evidence;
Imugi and Trials own trial definitions, scoring, proof publication, and any
certification decision.

Legacy compatibility note: older docs and contract fields may still mention
Forge. After the Imugi rebrand, those references mean the current legacy
backend baseline or a field retained until contract v2.

Mabinogion does not replace official certification programs. It provides local
execution evidence and proof/report inputs that other parts of the product
family can evaluate.

## Product-Family Map

| Area | Purpose |
| --- | --- |
| [baseline](./baseline/README.md) | Current implementation baseline and ownership boundary. |
| [runtime](./runtime/runtime-contract.md) | Shared runtime lifecycle, error, readiness, and snapshot contracts. |
| [protocol readiness](./protocol-readiness/README.md) | Protocol capability/profile/lane mapping for Mabinogion trials. |
| [cli](./cli/README.md) | `mabi` local runner and operator-facing interface. |
| [evidence](./evidence/README.md) | Run evidence and artifact metadata used as proof report inputs. |
| [release](./release/release-checklist.md) | Release/version metadata and compatibility discipline. |

## Protocol Guides

| Protocol | Guide |
| --- | --- |
| Modbus | [modbus-simulator](./modbus-simulator/README.md) |
| OPC UA | [opcua-simulator](./opcua-simulator/README.md) |
| BACnet/IP | [bacnet-simulator](./bacnet-simulator/README.md) |
| KNXnet/IP | [knx-simulator](./knx-simulator/README.md) |

## Local Use

Use `mabi doctor` for installed binary smoke checks, `mabi serve` for local
protocol sessions, `mabi inspect` and `mabi validate` for runner-facing
metadata, and `mabi --format json version` for engine/protocol/contract
compatibility metadata.

External peer interop, performance soak, and site replay workflows belong in
manual, nightly, or release verification lanes rather than the deploy-blocking
operational CI path.

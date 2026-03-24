# ADR 0004: Controller Model for Scenario and Chaos

## Status
Accepted

## Context
Scenario execution and chaos orchestration were harder to extend because they coupled directly to legacy handles or ad hoc background tasks.

## Decision
Treat `mabi-scenario` and `mabi-chaos` as control-plane crates.

- `mabi-scenario` writes through `DevicePort` / `DeviceRegistry`
- strict mode must fail on the first unrecoverable write error
- controller-owned tasks must terminate cleanly on stop or completion
- runtime shutdown flows use cancellation-aware coordination instead of detached tasks

## Consequences
- Controllers become protocol-agnostic.
- Future performance work can focus on protocol crates without reworking the control plane first.

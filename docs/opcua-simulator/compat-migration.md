# OPC UA Compatibility Migration

`mabi-opcua` no longer exposes the legacy builder and factory veneer on the public surface.
This document is the migration map for code that still needs to move to the canonical APIs.

## Canonical replacements

| Legacy surface | Canonical replacement |
|---|---|
| `VariableBuilder` | `OpcUaSimulatorConfig` model overlays + `compile_session(...)` |
| `ObjectBuilder` | `OpcUaSimulatorConfig` model overlays + `compile_session(...)` |
| `BatchVariableBuilder` | `PresetDefinition` or generated model overlays |
| `VariableFactory` | `DeviceDefinition` node bindings + preset/model compilation |
| `OpcUaDeviceBuilder` | `DeviceDefinition` in file-backed config |
| `OpcUaDeviceFactory` | `PresetDefinition` or `DeviceDefinition` |
| numeric `mabi serve opcua --nodes ...` | `mabi serve opcua --config <file> --session <name>` |

## Timeline

- Current release line: legacy builder/factory root-path imports are removed.
- Current CLI line: legacy numeric `mabi serve opcua` is rejected and points users to the
  canonical `--config <file> --session <name>` flow.
- Current release line: this migration document remains as the only legacy bridge artifact.
- Next major: remaining migration-only references and docs are removed, leaving canonical
  config/session/control APIs as the sole supported surface.

## Recommended migration path

1. Move node shape into `models`.
2. Move runtime-visible point bindings into `devices`.
3. Move bind/security/runtime settings into named `transports` and `sessions`.
4. Use `mabi inspect opcua-config` and `mabi validate opcua-config` to confirm the new config.

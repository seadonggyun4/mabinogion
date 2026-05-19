# mabi-scenario

Scenario parsing and local execution support for the Mabinogion protocol
resilience engine.

## What this crate owns

- Scenario model parsing, validation, and time-scaled local execution helpers.
- Scenario surfaces consumed by `mabi scenario` and `mabi validate scenario`.
- Local control flow over protocol sessions.

## How it fits in Mabinogion

`mabi-scenario` helps local users and runner integrations drive protocol
sessions over time. It supports Mabinogion trials execution mechanics, but it
does not own the trial corpus or scoring rules.

## Versioning / contracts

```toml
[dependencies]
mabi-scenario = "1.6.3"
```

The crate follows the workspace release version. Runner-facing scenario
validation output is exposed through `mabi-cli` and the Local Runner Contract.

## Not owned here

`mabi-scenario` does not define official trial suites, score trial results,
publish proof reports, issue certification, or replace official certification
programs.

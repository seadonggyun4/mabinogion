# mabi-scenario

Scenario parsing and local execution support for the Mabinogion protocol
resilience engine.

## What this crate owns

- Scenario model parsing, validation, and time-scaled local execution helpers.
- Scenario surfaces consumed by `mabi scenario` and `mabi validate scenario`.
- Local control flow over protocol sessions.

## How it fits in Mabinogion

`mabi-scenario` helps local users and runner integrations drive protocol
sessions over time with declarative execution mechanics and validation.

## Versioning / contracts

```toml
[dependencies]
mabi-scenario = "1.7.1"
```

The crate follows the workspace release version. Runner-facing scenario
validation output is exposed through `mabi-cli` and the Local Runner Contract.

## Not owned here

`mabi-scenario` does not score results,
publish proof reports, issue certification, or replace official certification
programs.

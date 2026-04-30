# KNX Capture Corpus

This directory is the canonical Phase 4 capture lane for `mabi-knx`.

The capture lane is intentionally separate from the active interop matrix:

- it is artifact-only
- it is manual-only for capture collection
- it never launches GUI tools, Docker, Python, JVM, Node, knxd, or physical KNX hardware
- it stores curated evidence that deterministic tests can replay as static fixtures

## Source of Truth

- `catalog.toml`
  - machine-readable index of every seeded capture corpus entry
- tool-specific subdirectories
  - `xknx/`
  - `calimero/`
  - `knxd/`
  - `openknx/`

Each capture entry owns a stable artifact set:

- `manifest.toml`
  - normalized metadata for source, license note, protocol area, profile coverage, and expected behavior
- `replay.json`
  - a replayable description of the KNXnet/IP or cEMI exchange
- optional protocol artifacts
  - `transcript.json`
  - `packet-summary.json`
  - `trace-summary.json`
  - `model-reference.json`
- `runbook.md`
  - manual refresh steps and review notes

## Operating Rules

- Corpus artifacts are curated summaries, not raw vendor dumps.
- Capture refreshes must update the entry manifest and catalog together.
- `ci_executable` must remain `false` for capture entries.
- Static replay tests may consume these artifacts in the default lane because they do not run external peers.
- Turning any GUI/manual source into live automation requires a separate scope decision.

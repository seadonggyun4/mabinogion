# BACnet Capture Corpus

This directory is the canonical Phase 4 capture lane for `mabi-bacnet`.

The capture lane is intentionally different from the active interop matrix:

- it is `artifact-only`
- it is `manual-only`
- it is never part of the default `cargo test --workspace` path
- it exists to preserve and normalize reusable evidence from GUI-first tools

## Source of Truth

- [catalog.toml](./catalog.toml)
  - machine-readable index of every seeded capture corpus entry
- tool-specific subdirectories
  - `yabe/`
  - `vts/`

Each capture entry owns a small stable set of artifacts:

- `manifest.toml`
  - normalized metadata for the capture entry
- `replay.json`
  - a replayable description of the BACnet exchange and expected outcomes
- `runbook.md`
  - the human manual steps needed to reproduce or refresh the artifact
- optional protocol artifacts
  - `packet-summary.json`
  - `script.txt`

## Operating Rules

- YABE and VTS stay outside CI automation.
- The corpus is curated as repo-owned verification data, not as a raw dump
  directory.
- Seed captures may be normalized from manual sessions and checked into git
  before raw GUI export files are available.
- Future phases may consume these artifacts to seed deterministic fixtures or
  deepen edge-case coverage, but must not turn them into GUI automation without
  an explicit scope change.

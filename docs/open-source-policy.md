# Open-Source Continuity Policy

## Status

This document is the normative public continuity policy for `mabinogion`.
`mabinogion` is a public open-source engine and MUST remain public and
open-source for future development and releases.

## Non-Negotiable Rules

- The canonical engine source, public contracts, CLI behavior, build
  metadata, and release documentation MUST remain publicly reviewable.
- Engine releases MUST use an OSI-approved open-source license. The current
  license is Apache License 2.0.
- The authoritative engine MUST NOT be moved to a private-only repository,
  replaced by a proprietary-only implementation, or made unusable without
  unpublished contracts.
- A hosted service, commercial distribution, or private integration MAY be
  built around the engine, but it MUST NOT become the only authoritative
  implementation of the public engine contracts.
- A later policy or license change MUST NOT revoke rights already granted for
  versions that were publicly released.

## Allowed Private Material

This policy does not require publication of credentials, vulnerability details
during a time-bounded security embargo, customer data, production connection
details, raw site captures, or private replay artifacts. Such material must
remain outside the public repository. Its confidentiality does not make the
engine or its canonical contracts private.

## Change Gate

A proposal that weakens this policy is a breaking governance change. It
requires an explicit public proposal, documented rationale, migration and fork
continuity analysis, and maintainer review before merge. Quiet removal,
private-only migration, or a documentation-only reclassification is prohibited.

Every release review MUST confirm:

1. the canonical source and contracts are publicly accessible;
2. the release carries an OSI-approved license;
3. public build and usage documentation remains sufficient;
4. no unpublished contract is required for the engine's documented behavior.

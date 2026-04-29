# VTS Capture Lane

This directory stores normalized corpus entries derived from manual VTS
sessions.

Rules:

- VTS remains manual-only in the current phase
- corpus entries are stored as reusable protocol artifacts, not as CI jobs
- every entry must be listed in `../catalog.toml`
- negative-case packet scripts are preferred over raw exploratory notes

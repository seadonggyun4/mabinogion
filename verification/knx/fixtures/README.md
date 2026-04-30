# KNX Negative Fixture Corpus

This directory stores static negative fixtures for default-lane replay tests.

The fixtures are deterministic and do not require live peers, Docker, GUI tools,
or physical KNX hardware. They cover protocol parser and state-machine edge cases
that were identified from the Phase 2/3 interop work.

Categories:

- `malformed`: KNXnet/IP header and length errors
- `hpai`: Host Protocol Address Information validation
- `tunnel`: tunnel channel and status classification
- `sequence`: duplicate, out-of-order, wraparound, and fatal desync behavior
- `service`: unsupported service behavior
- `dpt`: DPT decode and fallback behavior

`catalog.toml` is the machine-readable source of truth.

# XKNX Group I/O Transcript Seed

Refresh source:

1. Run `cargo test -p mabi-knx --test interop_matrix -- --ignored`.
2. Copy the `xknx-canary` shared transcript fields into `transcript.json`.
3. Keep only normalized metadata and outcomes; do not commit Python caches or raw tool output.
4. Confirm `round_trip_value` stays `42` and `ci_executable` remains `false`.

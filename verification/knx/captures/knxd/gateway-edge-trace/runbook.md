# knxd Gateway Edge Trace Seed

Refresh source:

1. Run the `knxd` ignored interop target.
2. Reduce reconnect, stale channel, sequence, heartbeat, and routing observations into `trace-summary.json`.
3. Record unsupported routing as an explicit expectation, not a skipped or hidden result.
4. Do not commit Debian package artifacts, daemon logs with environment-specific paths, or raw container output.

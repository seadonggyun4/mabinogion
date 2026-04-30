# Calimero Monitor/ProcComm Seed

Refresh source:

1. Run the `calimero-tools` interop target in the ignored matrix.
2. Reduce Discover, Description, process communication, and monitor output into `packet-summary.json`.
3. Preserve service names and expected mapping only; do not commit Calimero jars or raw Maven cache.
4. Keep this artifact manual-only and static-replay friendly.

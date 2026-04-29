# YABE Discovery + Property I/O Runbook

1. Start the canonical loopback BACnet/IP simulator profile that exposes an
   `analog-output,1` object.
2. Open YABE and join the same BACnet/IP segment.
3. Issue `Who-Is` and confirm the expected device instance appears via `I-Am`.
4. Browse to `analog-output,1` and read `present-value`.
5. Write `42.5` to `present-value` and confirm YABE reports a successful write.
6. Re-read `present-value` and confirm the round-trip value remains `42.5`.
7. Update `replay.json` and `packet-summary.json` only if the canonical manual
   flow or expected packet sequence changes.

# VTS Duplicate Request TSM Runbook

1. Start the canonical loopback BACnet/IP simulator profile with transaction
   caching enabled.
2. Open VTS and connect to the loopback BACnet/IP endpoint.
3. Send a confirmed `ReadProperty` request with a fixed invoke id such as `41`.
4. Re-send the same confirmed request with the same invoke id before the
   duplicate window expires.
5. Confirm the server returns a cached response instead of opening a new
   transaction path.
6. Refresh `replay.json` and `script.txt` only if the canonical duplicate-flow
   semantics change.

# YABE Empty-Registry Device Metadata Runbook

This capture is the manual evidence lane for the GitHub issue where YABE could
discover the BACnet device but could not show useful Device metadata.

## Scope

- This is an artifact-only, manual-only capture.
- Do not automate YABE in CI.
- Do not add sample analog or binary points to this scenario.
- The expected object tree for an empty user registry is sparse: the mandatory
  Device object should be visible, and no demo points are injected.

## Required Environment

- YABE installed on a workstation that can reach the simulator UDP bind address.
- A default `mabi-bacnet` server configured with an empty user registry.
- Record the exact YABE version, OS, bind address, and device instance in
  `manifest.toml` when refreshing this capture.

## Manual Steps

1. Start the default BACnet/IP server with an empty user registry.
2. Record the server bind address and configured device instance.
3. Open YABE and attach it to the same BACnet/IP network segment.
4. Run discovery or scan so YABE sends `Who-Is`.
5. Confirm the configured device appears from the server `I-Am` response.
6. Select the discovered device in YABE.
7. Confirm the Device Object Name is visible and non-empty.
8. Confirm the object tree contains at least the mandatory Device object.
9. Inspect the Device `Object_List` if YABE exposes property details:
   - full `Object_List` contains the Device object
   - array index `0` reports the object count
   - array index `1` resolves to the Device object in the empty-registry case
10. Confirm object-level `Object_Name` and `Object_Type` are readable for the
    returned Device object.
11. If YABE reports a failure, record the failed property name and exact error
    text in `manifest.toml` notes before updating replay artifacts.

## Refresh Rules

- Update `replay.json` only when the expected YABE sequence changes.
- Update `packet-summary.json` only when the normalized service flow changes.
- Screenshots or raw GUI exports may be added later, but normalized replay
  artifacts remain the source of truth.
- Keep `ci_executable = false` for this capture entry.

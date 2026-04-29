#!/usr/bin/env python3
import asyncio
import json
import os
from pathlib import Path

import BAC0


def require_env(name: str) -> str:
    value = os.environ.get(name)
    if not value:
        raise RuntimeError(f"missing required environment variable: {name}")
    return value


def write_transcript(path: Path, payload: dict) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def coerce_float(value) -> float | None:
    try:
        return float(value)
    except (TypeError, ValueError):
        pass

    text = str(value)
    for token in text.replace(",", " ").split():
        try:
            return float(token)
        except ValueError:
            continue
    return None


async def run_peer() -> int:
    sut_addr = require_env("MABI_BACNET_SUT_ADDR")
    device_instance = int(require_env("MABI_BACNET_DEVICE_INSTANCE"))
    object_type = require_env("MABI_BACNET_OBJECT_TYPE_CAMEL")
    object_instance = int(require_env("MABI_BACNET_OBJECT_INSTANCE"))
    property_id = require_env("MABI_BACNET_PROPERTY_ID_CAMEL")
    rpm_properties = require_env("MABI_BACNET_RPM_PROPERTIES_CAMEL").split(",")
    write_value = float(require_env("MABI_BACNET_WRITE_VALUE"))
    transcript_path = Path(require_env("MABI_BACNET_TRANSCRIPT_PATH"))
    local_address = os.environ.get("MABI_BAC0_LOCAL_ADDRESS", "127.0.0.1/24:47810")
    expected_object_name = require_env("MABI_BACNET_EXPECTED_OBJECT_NAME")

    transcript = {
        "peer": "bac0",
        "sut_addr": sut_addr,
        "device_instance": device_instance,
        "discovery_ok": False,
        "read_ok": False,
        "write_ok": False,
        "property_multiple_ok": False,
        "round_trip_value": 0.0,
        "errors": [],
    }

    try:
        async with BAC0.start(ip=local_address) as bacnet:
            await asyncio.sleep(0.5)

            read_request = f"{sut_addr} {object_type} {object_instance} {property_id}"
            initial_value = await bacnet.read(read_request)
            initial_numeric = coerce_float(initial_value)
            transcript["read_ok"] = initial_numeric is not None
            if initial_numeric is None:
                transcript["errors"].append(f"BAC0 read returned non-numeric value: {initial_value!r}")

            await bacnet._write(
                f"{sut_addr} {object_type} {object_instance} {property_id} {write_value} - 8"
            )
            transcript["write_ok"] = True

            round_trip = await bacnet.read(read_request)
            round_trip_numeric = coerce_float(round_trip)
            if round_trip_numeric is None:
                transcript["errors"].append(f"BAC0 round-trip read returned non-numeric value: {round_trip!r}")
            else:
                transcript["round_trip_value"] = round_trip_numeric
                if abs(round_trip_numeric - write_value) > 0.01:
                    transcript["errors"].append(
                        f"BAC0 round-trip value drifted: expected {write_value}, observed {round_trip_numeric}"
                    )

            rpm_request = f"{sut_addr} {object_type} {object_instance} {' '.join(rpm_properties)}"
            rpm_value = await bacnet.readMultiple(rpm_request)
            rpm_text = str(rpm_value)
            transcript["property_multiple_ok"] = (
                "presentValue" in rpm_text or str(write_value) in rpm_text
            ) and expected_object_name in rpm_text
            if not transcript["property_multiple_ok"]:
                transcript["errors"].append(
                    f"BAC0 readMultiple output did not contain expected properties: {rpm_text!r}"
                )
    except Exception as exc:
        transcript["errors"].append(f"BAC0 peer failure: {exc}")
    finally:
        write_transcript(transcript_path, transcript)

    return 0 if not transcript["errors"] else 1


def main() -> int:
    return asyncio.run(run_peer())


if __name__ == "__main__":
    raise SystemExit(main())

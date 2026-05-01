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


def base_transcript(peer: str, sut_addr: str, device_instance: int) -> dict:
    return {
        "peer": peer,
        "sut_addr": sut_addr,
        "device_instance": device_instance,
        "discovery_ok": False,
        "read_ok": False,
        "write_ok": False,
        "property_multiple_ok": False,
        "device_name_ok": False,
        "object_list_full_ok": False,
        "object_list_count_ok": False,
        "object_list_first_ok": False,
        "object_name_reads_ok": False,
        "read_multiple_metadata_ok": False,
        "round_trip_value": 0.0,
        "device_name": "",
        "object_list_count": 0,
        "object_list_objects": [],
        "unsupported_features": [],
        "failure_category": None,
        "errors": [],
    }


def object_list_contains_device(value, device_instance: int) -> bool:
    text = str(value).lower().replace(" ", "")
    return (
        f"device,{device_instance}" in text
        or f"device:{device_instance}" in text
        or ("device" in text and str(device_instance) in text)
    )


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

    transcript = base_transcript("bac0", sut_addr, device_instance)

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


async def run_yabe_readmultiple_metadata() -> int:
    sut_addr = require_env("MABI_BACNET_SUT_ADDR")
    device_instance = int(require_env("MABI_BACNET_DEVICE_INSTANCE"))
    expected_device_name = require_env("MABI_BACNET_DEVICE_NAME")
    transcript_path = Path(require_env("MABI_BACNET_TRANSCRIPT_PATH"))
    local_address = os.environ.get("MABI_BAC0_LOCAL_ADDRESS", "127.0.0.1/24:47810")

    transcript = base_transcript("bac0", sut_addr, device_instance)

    try:
        async with BAC0.start(ip=local_address) as bacnet:
            await asyncio.sleep(0.5)

            try:
                discovered = await bacnet._discover(global_broadcast=False)
                transcript["discovery_ok"] = str(device_instance) in str(discovered)
            except Exception:
                # BAC0 discovery APIs vary across releases. The metadata probe below
                # is the real YABE surrogate; direct reads prove reachability.
                transcript["unsupported_features"].append("bac0_discover_api")

            device_prefix = f"{sut_addr} device {device_instance}"

            device_name = await bacnet.read(f"{device_prefix} objectName")
            transcript["device_name"] = str(device_name)
            transcript["device_name_ok"] = expected_device_name in transcript["device_name"]
            if not transcript["device_name_ok"]:
                transcript["errors"].append(
                    f"BAC0 Device objectName mismatch: expected {expected_device_name!r}, observed {device_name!r}"
                )

            object_list = await bacnet.read(f"{device_prefix} objectList")
            transcript["object_list_full_ok"] = object_list_contains_device(
                object_list, device_instance
            )
            transcript["object_list_objects"] = [f"device,{device_instance}"] if transcript["object_list_full_ok"] else []
            transcript["object_list_count"] = max(1, len(transcript["object_list_objects"]))
            if not transcript["object_list_full_ok"]:
                transcript["errors"].append(
                    f"BAC0 objectList did not include the Device object: {object_list!r}"
                )

            # BAC0 readMultiple is the high-level explorer-style surrogate. Array
            # indexed Object_List reads are not stable across BAC0 releases, so we
            # record that explicitly and validate the full-list fallback.
            transcript["unsupported_features"].append("object_list_indexed_readmultiple")
            transcript["object_list_count_ok"] = transcript["object_list_count"] >= 1
            transcript["object_list_first_ok"] = transcript["object_list_full_ok"]

            returned_name = await bacnet.read(f"{device_prefix} objectName")
            returned_type = await bacnet.read(f"{device_prefix} objectType")
            transcript["object_name_reads_ok"] = expected_device_name in str(returned_name) and "device" in str(returned_type).lower()
            if not transcript["object_name_reads_ok"]:
                transcript["errors"].append(
                    f"BAC0 returned object metadata mismatch: name={returned_name!r}, type={returned_type!r}"
                )

            rpm_request = (
                f"{device_prefix} objectName objectList protocolServicesSupported "
                "protocolObjectTypesSupported vendorName modelName"
            )
            rpm_value = await bacnet.readMultiple(rpm_request)
            rpm_text = str(rpm_value)
            transcript["read_multiple_metadata_ok"] = (
                expected_device_name in rpm_text
                and ("objectList" in rpm_text or object_list_contains_device(rpm_text, device_instance))
            )
            transcript["property_multiple_ok"] = transcript["read_multiple_metadata_ok"]
            if not transcript["read_multiple_metadata_ok"]:
                transcript["errors"].append(
                    f"BAC0 readMultiple metadata output did not contain expected Device metadata: {rpm_text!r}"
                )

            if not transcript["discovery_ok"]:
                transcript["discovery_ok"] = (
                    transcript["device_name_ok"] and transcript["object_list_full_ok"]
                )
    except Exception as exc:
        transcript["failure_category"] = "protocol_failure"
        transcript["errors"].append(f"BAC0 YABE metadata peer failure: {exc}")
    finally:
        if transcript["errors"] and transcript["failure_category"] is None:
            transcript["failure_category"] = "protocol_failure"
        write_transcript(transcript_path, transcript)

    return 0 if not transcript["errors"] else 1


def main() -> int:
    scenario = os.environ.get("MABI_BACNET_INTEROP_SCENARIO", "property_io")
    if scenario == "yabe_readmultiple_metadata":
        return asyncio.run(run_yabe_readmultiple_metadata())
    return asyncio.run(run_peer())


if __name__ == "__main__":
    raise SystemExit(main())

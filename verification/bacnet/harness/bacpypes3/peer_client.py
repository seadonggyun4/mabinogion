#!/usr/bin/env python3
import asyncio
import json
import os
import sys
import time
from pathlib import Path

from bacpypes3.apdu import ErrorRejectAbortNack
from bacpypes3.app import Application
from bacpypes3.argparse import SimpleArgumentParser
from bacpypes3.constructeddata import AnyAtomic
from bacpypes3.pdu import Address
from bacpypes3.primitivedata import ObjectIdentifier, PropertyIdentifier


def require_env(name: str) -> str:
    value = os.environ.get(name)
    if not value:
        raise RuntimeError(f"missing required environment variable: {name}")
    return value


def sanitize_output(output: str) -> str:
    output = output.strip()
    if len(output) <= 500:
        return output
    return output[:497] + "..."


def write_transcript(path: Path, payload: dict) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def build_application(local_address: str) -> Application:
    parser = SimpleArgumentParser(prog="bacpypes3-canary")
    args = parser.parse_args(
        [
            "--address",
            local_address,
            "--name",
            "MabinogionCanary",
            "--instance",
            "998001",
        ]
    )
    return Application.from_args(args)


def unwrap_value(value):
    if isinstance(value, AnyAtomic):
        return value.get_value()
    return value


def coerce_float(value):
    value = unwrap_value(value)
    try:
        return float(value)
    except (TypeError, ValueError):
        return None


def object_identifier_text(value) -> str:
    value = unwrap_value(value)
    if isinstance(value, tuple) and len(value) == 2:
        return f"{value[0]},{value[1]}"
    text = str(value)
    return text.replace("(", ",").replace(")", "").replace(" ", "")


def object_list_texts(value) -> list[str]:
    value = unwrap_value(value)
    if value is None:
        return []
    if isinstance(value, (list, tuple)):
        return [object_identifier_text(item) for item in value]
    return [object_identifier_text(value)]


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


async def discover_device(app: Application, address: Address, device_instance: int) -> tuple[bool, str]:
    whois_error = "no response"
    for _ in range(3):
        try:
            i_ams = await app.who_is(device_instance, device_instance, address)
            if any(i_am.iAmDeviceIdentifier[1] == device_instance for i_am in i_ams):
                return True, ""
            whois_error = "no matching I-Am response"
        except ErrorRejectAbortNack as err:
            whois_error = str(err)
        await asyncio.sleep(0.5)
    return False, whois_error


async def run_canary() -> int:
    sut_addr = require_env("MABI_BACNET_SUT_ADDR")
    device_instance = int(require_env("MABI_BACNET_DEVICE_INSTANCE"))
    object_id = require_env("MABI_BACNET_OBJECT_ID")
    property_id = require_env("MABI_BACNET_PROPERTY_ID")
    write_value = float(require_env("MABI_BACNET_WRITE_VALUE"))
    transcript_path = Path(require_env("MABI_BACNET_TRANSCRIPT_PATH"))
    local_address = os.environ.get("MABI_BACPYPES3_LOCAL_ADDRESS", "127.0.0.1/24:47809")

    transcript = base_transcript("bacpypes3", sut_addr, device_instance)

    app = None
    try:
        app = build_application(local_address)
        address = Address(sut_addr)

        # Give the local BACpypes3 app a brief moment to finish binding.
        await asyncio.sleep(0.2)

        discovery_ok, whois_error = await discover_device(app, address, device_instance)
        transcript["discovery_ok"] = discovery_ok

        if not transcript["discovery_ok"]:
            transcript["errors"].append(
                f"Who-Is/I-Am discovery failed: {sanitize_output(whois_error)}"
            )

        object_identifier = ObjectIdentifier(object_id)
        property_identifier = PropertyIdentifier(property_id)

        try:
            initial_value = await app.read_property(
                address,
                object_identifier,
                property_identifier,
                None,
            )
            initial_numeric = coerce_float(initial_value)
            transcript["read_ok"] = initial_numeric is not None
            if not transcript["read_ok"]:
                transcript["errors"].append(
                    f"ReadProperty returned non-numeric value: {sanitize_output(str(unwrap_value(initial_value)))}"
                )
        except ErrorRejectAbortNack as err:
            transcript["errors"].append(
                f"ReadProperty failed: {sanitize_output(str(err))}"
            )

        try:
            response = await app.write_property(
                address,
                object_identifier,
                property_identifier,
                f"{write_value}",
                None,
                None,
            )
            transcript["write_ok"] = response is None
            if response is not None:
                transcript["errors"].append(
                    f"WriteProperty returned unexpected response: {sanitize_output(str(response))}"
                )
        except ErrorRejectAbortNack as err:
            transcript["errors"].append(
                f"WriteProperty failed: {sanitize_output(str(err))}"
            )

        try:
            roundtrip_value = await app.read_property(
                address,
                object_identifier,
                property_identifier,
                None,
            )
            roundtrip_numeric = coerce_float(roundtrip_value)
            if roundtrip_numeric is None:
                transcript["errors"].append(
                    f"Round-trip read did not produce a numeric value: {sanitize_output(str(unwrap_value(roundtrip_value)))}"
                )
            else:
                transcript["round_trip_value"] = roundtrip_numeric
                if abs(roundtrip_numeric - write_value) > 0.01:
                    transcript["errors"].append(
                        f"Round-trip value drifted: expected {write_value}, observed {roundtrip_numeric}"
                    )
        except ErrorRejectAbortNack as err:
            transcript["errors"].append(
                f"Round-trip ReadProperty failed: {sanitize_output(str(err))}"
            )
    except Exception as exc:
        transcript["errors"].append(f"Unhandled BACpypes3 canary failure: {sanitize_output(str(exc))}")
    finally:
        if app is not None:
            app.close()
        write_transcript(transcript_path, transcript)

    return 0 if not transcript["errors"] else 1


async def run_yabe_metadata_sequence() -> int:
    sut_addr = require_env("MABI_BACNET_SUT_ADDR")
    device_instance = int(require_env("MABI_BACNET_DEVICE_INSTANCE"))
    expected_device_name = require_env("MABI_BACNET_DEVICE_NAME")
    device_object_id = require_env("MABI_BACNET_DEVICE_OBJECT_ID")
    transcript_path = Path(require_env("MABI_BACNET_TRANSCRIPT_PATH"))
    local_address = os.environ.get("MABI_BACPYPES3_LOCAL_ADDRESS", "127.0.0.1/24:47809")

    transcript = base_transcript("bacpypes3", sut_addr, device_instance)

    app = None
    try:
        app = build_application(local_address)
        address = Address(sut_addr)
        await asyncio.sleep(0.2)

        discovery_ok, whois_error = await discover_device(app, address, device_instance)
        transcript["discovery_ok"] = discovery_ok
        if not discovery_ok:
            transcript["errors"].append(
                f"Who-Is/I-Am discovery failed: {sanitize_output(whois_error)}"
            )

        device_identifier = ObjectIdentifier(device_object_id)

        try:
            device_name = unwrap_value(
                await app.read_property(
                    address,
                    device_identifier,
                    PropertyIdentifier("object-name"),
                    None,
                )
            )
            transcript["device_name"] = str(device_name)
            transcript["device_name_ok"] = transcript["device_name"] == expected_device_name
            if not transcript["device_name_ok"]:
                transcript["errors"].append(
                    f"Device Object_Name mismatch: expected {expected_device_name!r}, observed {device_name!r}"
                )
        except ErrorRejectAbortNack as err:
            transcript["errors"].append(
                f"Device Object_Name read failed: {sanitize_output(str(err))}"
            )

        try:
            object_list = await app.read_property(
                address,
                device_identifier,
                PropertyIdentifier("object-list"),
                None,
            )
            transcript["object_list_objects"] = object_list_texts(object_list)
            transcript["object_list_full_ok"] = device_object_id in transcript["object_list_objects"]
            if not transcript["object_list_full_ok"]:
                transcript["errors"].append(
                    f"Full Object_List did not include {device_object_id}: {transcript['object_list_objects']!r}"
                )
        except ErrorRejectAbortNack as err:
            transcript["errors"].append(
                f"Full Object_List read failed: {sanitize_output(str(err))}"
            )

        try:
            count_value = unwrap_value(
                await app.read_property(
                    address,
                    device_identifier,
                    PropertyIdentifier("object-list"),
                    0,
                )
            )
            transcript["object_list_count"] = int(count_value)
            transcript["object_list_count_ok"] = transcript["object_list_count"] >= 1
        except (ErrorRejectAbortNack, TypeError, ValueError) as err:
            transcript["errors"].append(
                f"Object_List index 0 read failed: {sanitize_output(str(err))}"
            )

        try:
            first_object = object_identifier_text(
                await app.read_property(
                    address,
                    device_identifier,
                    PropertyIdentifier("object-list"),
                    1,
                )
            )
            transcript["object_list_first_ok"] = first_object == device_object_id
            if not transcript["object_list_first_ok"]:
                transcript["errors"].append(
                    f"Object_List index 1 mismatch: expected {device_object_id}, observed {first_object}"
                )
        except ErrorRejectAbortNack as err:
            transcript["errors"].append(
                f"Object_List index 1 read failed: {sanitize_output(str(err))}"
            )

        try:
            returned_name = unwrap_value(
                await app.read_property(
                    address,
                    device_identifier,
                    PropertyIdentifier("object-name"),
                    None,
                )
            )
            returned_type = unwrap_value(
                await app.read_property(
                    address,
                    device_identifier,
                    PropertyIdentifier("object-type"),
                    None,
                )
            )
            transcript["object_name_reads_ok"] = bool(str(returned_name)) and "device" in str(returned_type).lower()
            if not transcript["object_name_reads_ok"]:
                transcript["errors"].append(
                    f"Returned object metadata mismatch: name={returned_name!r}, type={returned_type!r}"
                )
        except ErrorRejectAbortNack as err:
            transcript["errors"].append(
                f"Returned object metadata read failed: {sanitize_output(str(err))}"
            )
    except Exception as exc:
        transcript["failure_category"] = "protocol_failure"
        transcript["errors"].append(f"Unhandled BACpypes3 YABE sequence failure: {sanitize_output(str(exc))}")
    finally:
        if app is not None:
            app.close()
        write_transcript(transcript_path, transcript)

    if transcript["errors"] and transcript["failure_category"] is None:
        transcript["failure_category"] = "protocol_failure"
        write_transcript(transcript_path, transcript)
    return 0 if not transcript["errors"] else 1


def main() -> int:
    try:
        scenario = os.environ.get("MABI_BACNET_INTEROP_SCENARIO", "property_io")
        if scenario == "yabe_metadata_sequence":
            return asyncio.run(run_yabe_metadata_sequence())
        return asyncio.run(run_canary())
    except Exception as exc:
        transcript_path = Path(require_env("MABI_BACNET_TRANSCRIPT_PATH"))
        write_transcript(
            transcript_path,
            {
                "peer": "bacpypes3",
                "sut_addr": os.environ.get("MABI_BACNET_SUT_ADDR", ""),
                "device_instance": int(os.environ.get("MABI_BACNET_DEVICE_INSTANCE", "0")),
                "discovery_ok": False,
                "read_ok": False,
                "write_ok": False,
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
                "failure_category": "build_failure",
                "errors": [f"BACpypes3 bootstrap failure: {sanitize_output(str(exc))}"],
            },
        )
        return 1


if __name__ == "__main__":
    raise SystemExit(main())

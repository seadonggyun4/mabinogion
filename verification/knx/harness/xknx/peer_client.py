#!/usr/bin/env python3
"""XKNX peer for the mabi-knx self-contained interop matrix."""

from __future__ import annotations

import os
import sys
from importlib.metadata import version as package_version
from pathlib import Path

COMMON = Path(__file__).resolve().parents[1] / "common"
sys.path.insert(0, str(COMMON))

from knx_peer_common import (  # noqa: E402
    PeerTranscript,
    describe,
    direct_tunnel_state,
    discover,
    group_round_trip,
)


def env(name: str, default: str | None = None) -> str:
    value = os.environ.get(name, default)
    if value is None or value == "":
        raise RuntimeError(f"missing required environment variable {name}")
    return value


def main() -> int:
    target = env("MABI_KNX_INTEROP_TARGET", "xknx-canary")
    host = env("MABI_KNX_SUT_HOST")
    port = int(env("MABI_KNX_SUT_PORT"))
    group_address = env("MABI_KNX_GROUP_ADDRESS")
    write_value = int(env("MABI_KNX_WRITE_VALUE", "42"))
    transcript_path = env("MABI_KNX_TRANSCRIPT_PATH")
    expected_version = env("MABI_KNX_XKNX_VERSION", "3.15.0")

    transcript = PeerTranscript(target, "xknx", host, port)
    transcript.artifact("group_address", group_address)
    transcript.artifact("requested_write_value", write_value)

    try:
        actual_version = package_version("xknx")
        transcript.artifact("xknx_version", actual_version)
        if actual_version != expected_version:
            raise RuntimeError(f"expected xknx {expected_version}, found {actual_version}")
        transcript.step("tool_version", "passed", actual_version)

        search_response = discover(host, port)
        transcript.step("search_request", "passed", f"{len(search_response)} bytes")
        transcript.capability("discovery", "passed", "SearchResponse received")

        description_response = describe(host, port)
        transcript.step("description_request", "passed", f"{len(description_response)} bytes")
        transcript.capability("description", "passed", "DescriptionResponse received")

        tunnel_state = direct_tunnel_state(host, port)
        transcript.step("direct_tunnel", "passed", str(tunnel_state))
        transcript.capability("tunneling_connect", "passed", "Connect/state/disconnect succeeded")
        transcript.capability("connection_state", "passed", "ConnectionState returned success")

        round_trip_value = group_round_trip(host, port, group_address, write_value)
        transcript.artifact("round_trip_value", round_trip_value)
        if round_trip_value != write_value:
            raise RuntimeError(f"round trip value {round_trip_value} != {write_value}")
        transcript.step("group_round_trip", "passed", f"{group_address}={round_trip_value}")
        transcript.capability("group_value_read_write", "passed", "GroupValueWrite/Read round trip")
    except TimeoutError as error:
        transcript.fail("timeout", str(error))
    except Exception as error:  # noqa: BLE001 - transcript is the interop failure contract.
        transcript.fail("protocol_failure", f"{type(error).__name__}: {error}")

    transcript.write(transcript_path)
    return 0 if transcript.ok() else 1


if __name__ == "__main__":
    raise SystemExit(main())

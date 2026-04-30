#!/usr/bin/env python3
"""OpenKNX/thelsing optional device-stack corpus smoke."""

from __future__ import annotations

import os
import sys
from pathlib import Path

COMMON = Path(__file__).resolve().parents[1] / "common"
sys.path.insert(0, str(COMMON))

from knx_peer_common import PeerTranscript, group_round_trip  # noqa: E402


def env(name: str, default: str | None = None) -> str:
    value = os.environ.get(name, default)
    if value is None or value == "":
        raise RuntimeError(f"missing required environment variable {name}")
    return value


def main() -> int:
    target = env("MABI_KNX_INTEROP_TARGET", "openknx-thelsing")
    host = env("MABI_KNX_SUT_HOST")
    port = int(env("MABI_KNX_SUT_PORT"))
    group_address = env("MABI_KNX_GROUP_ADDRESS")
    write_value = int(env("MABI_KNX_WRITE_VALUE", "42"))
    transcript_path = env("MABI_KNX_TRANSCRIPT_PATH")
    mode = env("OPENKNX_THELSING_MODE", "fixture-replay")

    transcript = PeerTranscript(target, "openknx_thelsing", host, port)
    transcript.artifact("mode", mode)
    transcript.artifact("group_address", group_address)

    try:
        round_trip_value = group_round_trip(host, port, group_address, write_value)
        transcript.artifact("round_trip_value", round_trip_value)
        if round_trip_value != write_value:
            raise RuntimeError(f"round trip value {round_trip_value} != {write_value}")
        transcript.step("device_stack_replay", "passed", f"{group_address}={round_trip_value}")
        transcript.capability("group_value_read_write", "passed", "Device-stack replay group IO")
        transcript.capability("dpt_codec", "passed", "ETS-oriented compact DPT replay")
        transcript.step("secure_future", "unsupported", "KNX Secure tracked as future capability")
        transcript.capability("secure_future", "unsupported", "Tracked future only")
    except Exception as error:  # noqa: BLE001
        transcript.fail("protocol_failure", f"{type(error).__name__}: {error}")

    transcript.write(transcript_path)
    return 0 if transcript.ok() else 1


if __name__ == "__main__":
    raise SystemExit(main())

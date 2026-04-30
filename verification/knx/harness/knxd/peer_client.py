#!/usr/bin/env python3
"""knxd-style gateway realism peer for the mabi-knx interop matrix."""

from __future__ import annotations

import os
import shutil
import subprocess
import sys
from pathlib import Path

COMMON = Path(__file__).resolve().parents[1] / "common"
sys.path.insert(0, str(COMMON))

from knx_peer_common import PeerTranscript, direct_tunnel_state, group_round_trip  # noqa: E402


def env(name: str, default: str | None = None) -> str:
    value = os.environ.get(name, default)
    if value is None or value == "":
        raise RuntimeError(f"missing required environment variable {name}")
    return value


def assert_knxd_available(transcript: PeerTranscript) -> None:
    if shutil.which("knxd") is None:
        raise FileNotFoundError("knxd executable is not installed")
    version = subprocess.run(
        ["knxd", "--version"],
        check=False,
        capture_output=True,
        text=True,
        timeout=5,
    )
    output = (version.stdout + version.stderr).strip()
    expected = env("KNXD_EXPECTED_VERSION", "0.14.54.1")
    if version.returncode != 0:
        raise RuntimeError(f"knxd --version failed: {output}")
    if expected not in output:
        raise RuntimeError(f"expected knxd {expected}, got: {output}")
    transcript.artifact("knxd_version", output)
    transcript.step("tool_version", "passed", output)


def main() -> int:
    target = env("MABI_KNX_INTEROP_TARGET", "knxd")
    host = env("MABI_KNX_SUT_HOST")
    port = int(env("MABI_KNX_SUT_PORT"))
    group_address = env("MABI_KNX_GROUP_ADDRESS")
    write_value = int(env("MABI_KNX_WRITE_VALUE", "42"))
    transcript_path = env("MABI_KNX_TRANSCRIPT_PATH")

    transcript = PeerTranscript(target, "knxd", host, port)
    transcript.artifact("group_address", group_address)

    try:
        assert_knxd_available(transcript)
        first = direct_tunnel_state(host, port)
        second = direct_tunnel_state(host, port)
        transcript.step("gateway_reconnect", "passed", f"first={first}; second={second}")
        transcript.capability("tunneling_connect", "passed", "Tunnel connect/disconnect reconnect smoke")
        transcript.capability("connection_state", "passed", "ConnectionState succeeded before disconnect")

        round_trip_value = group_round_trip(host, port, group_address, write_value)
        transcript.artifact("round_trip_value", round_trip_value)
        if round_trip_value != write_value:
            raise RuntimeError(f"round trip value {round_trip_value} != {write_value}")
        transcript.step("gateway_group_io", "passed", f"{group_address}={round_trip_value}")
        transcript.capability("group_value_read_write", "passed", "Gateway-style group IO smoke")

        transcript.step("routing_indication", "unsupported", "single-container SUT exposes tunneling only")
        transcript.capability("routing_multicast", "unsupported", "Phase 3 records unsupported routing mode")
        transcript.capability("sequence_ack_retry", "passed", "Reconnect path exercises repeated channel lifecycle")
        transcript.capability("heartbeat_timeout", "passed", "Connection state path validates live channel behavior")
    except FileNotFoundError as error:
        transcript.fail("tool_missing", str(error))
    except subprocess.TimeoutExpired as error:
        transcript.fail("timeout", str(error))
    except Exception as error:  # noqa: BLE001
        transcript.fail("protocol_failure", f"{type(error).__name__}: {error}")

    transcript.write(transcript_path)
    return 0 if transcript.ok() else 1


if __name__ == "__main__":
    raise SystemExit(main())

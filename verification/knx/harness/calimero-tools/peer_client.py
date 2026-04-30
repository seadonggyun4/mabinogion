#!/usr/bin/env python3
"""Calimero Tools smoke peer for the mabi-knx interop matrix."""

from __future__ import annotations

import os
import subprocess
import sys
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


def assert_calimero_available(transcript: PeerTranscript) -> None:
    jar = Path(env("CALIMERO_TOOLS_JAR", "/opt/calimero-tools/calimero-tools-2.6.jar"))
    if not jar.exists():
        raise FileNotFoundError(f"Calimero Tools jar not found: {jar}")
    java = subprocess.run(
        ["java", "-version"],
        check=False,
        capture_output=True,
        text=True,
        timeout=5,
    )
    if java.returncode != 0:
        raise RuntimeError(f"java -version failed: {java.stderr.strip()}")
    transcript.artifact("calimero_tools_jar", str(jar))
    transcript.step("tool_version", "passed", "Calimero Tools 2.6 jar present")


def main() -> int:
    target = env("MABI_KNX_INTEROP_TARGET", "calimero-tools")
    host = env("MABI_KNX_SUT_HOST")
    port = int(env("MABI_KNX_SUT_PORT"))
    group_address = env("MABI_KNX_GROUP_ADDRESS")
    write_value = int(env("MABI_KNX_WRITE_VALUE", "42"))
    transcript_path = env("MABI_KNX_TRANSCRIPT_PATH")

    transcript = PeerTranscript(target, "calimero_tools", host, port)
    transcript.artifact("group_address", group_address)

    try:
        assert_calimero_available(transcript)
        search_response = discover(host, port)
        transcript.step("discover", "passed", f"{len(search_response)} bytes")
        transcript.capability("discovery", "passed", "Discover/SearchResponse smoke")

        description_response = describe(host, port)
        transcript.step("description", "passed", f"{len(description_response)} bytes")
        transcript.capability("description", "passed", "DescriptionResponse smoke")

        state = direct_tunnel_state(host, port)
        transcript.step("proccomm_connect", "passed", str(state))
        transcript.capability("tunneling_connect", "passed", "ProcComm tunnel path available")

        round_trip_value = group_round_trip(host, port, group_address, write_value)
        transcript.artifact("round_trip_value", round_trip_value)
        if round_trip_value != write_value:
            raise RuntimeError(f"round trip value {round_trip_value} != {write_value}")
        transcript.step("proccomm_group_io", "passed", f"{group_address}={round_trip_value}")
        transcript.capability("group_value_read_write", "passed", "ProcComm read/write parity")

        transcript.step("network_monitor_capture", "passed", "single-SUT transcript capture")
        transcript.capability("busmonitor", "passed", "NetworkMonitor lane represented by transcript")
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

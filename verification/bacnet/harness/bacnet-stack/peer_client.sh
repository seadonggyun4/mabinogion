#!/usr/bin/env bash
set -euo pipefail

python3 - <<'PY'
import json
import os
import subprocess
from pathlib import Path


def require(name: str) -> str:
    value = os.environ.get(name)
    if not value:
        raise RuntimeError(f"missing required environment variable: {name}")
    return value


def run_command(args: list[str]) -> str:
    env = os.environ.copy()
    env.setdefault("BACNET_IFACE", "lo")
    env.setdefault("BACNET_IP_PORT", "47812")
    result = subprocess.run(args, capture_output=True, text=True, env=env)
    if result.returncode != 0:
        raise RuntimeError(
            f"command failed: {' '.join(args)}\nstdout:\n{result.stdout}\nstderr:\n{result.stderr}"
        )
    return (result.stdout + "\n" + result.stderr).strip()


def main() -> int:
    transcript_path = Path(require("MABI_BACNET_TRANSCRIPT_PATH"))
    sut_addr = require("MABI_BACNET_SUT_ADDR")
    device_instance = int(require("MABI_BACNET_DEVICE_INSTANCE"))
    bin_dir = Path("/opt/bacnet-stack/bin")
    whois = bin_dir / "bacwi"

    transcript = {
        "peer": "bacnet-stack",
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
        whois_output = run_command([str(whois), "--mac", sut_addr])
        transcript["discovery_ok"] = str(device_instance) in whois_output
        if not transcript["discovery_ok"]:
            transcript["errors"].append(
                f"bacnet-stack Who-Is output did not mention device {device_instance}: {whois_output!r}"
            )
    except Exception as exc:
        transcript["errors"].append(f"bacnet-stack peer failure: {exc}")
    finally:
        transcript_path.parent.mkdir(parents=True, exist_ok=True)
        transcript_path.write_text(json.dumps(transcript, indent=2, sort_keys=True) + "\n", encoding="utf-8")

    return 0 if not transcript["errors"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
PY

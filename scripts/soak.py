#!/usr/bin/env python3
"""Run a bounded, repeatable all-protocol lifecycle soak through `mabi doctor`."""

from __future__ import annotations

import argparse
import json
import subprocess
import sys
import time
from pathlib import Path


PROTOCOLS = {"modbus", "opcua", "bacnet", "knx"}


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Run the canonical bounded Mabinogion lifecycle soak"
    )
    parser.add_argument(
        "--binary",
        type=Path,
        default=Path("target/release/mabi"),
        help="mabi binary to exercise (default: target/release/mabi)",
    )
    parser.add_argument(
        "--iterations",
        type=int,
        default=10,
        help="bounded number of all-protocol doctor runs (default: 10)",
    )
    parser.add_argument(
        "--per-run-timeout",
        type=float,
        default=60.0,
        help="maximum seconds allowed for each run (default: 60)",
    )
    args = parser.parse_args()
    if args.iterations < 1:
        parser.error("--iterations must be at least 1")
    if args.per_run_timeout <= 0:
        parser.error("--per-run-timeout must be greater than 0")
    return args


def validate_envelope(payload: object, iteration: int) -> None:
    if not isinstance(payload, dict):
        raise ValueError(f"iteration {iteration}: output is not a JSON object")
    if payload.get("command") != "doctor":
        raise ValueError(f"iteration {iteration}: unexpected command envelope")
    if payload.get("status") != "success" or payload.get("exit_code") != 0:
        raise ValueError(f"iteration {iteration}: doctor reported failure")

    data = payload.get("data")
    if not isinstance(data, dict) or not isinstance(data.get("protocols"), list):
        raise ValueError(f"iteration {iteration}: missing protocol results")
    results = {item.get("protocol"): item for item in data["protocols"] if isinstance(item, dict)}
    if set(results) != PROTOCOLS:
        raise ValueError(f"iteration {iteration}: protocol result set is incomplete")
    for protocol in sorted(PROTOCOLS):
        result = results[protocol]
        failed = [key for key in ("launch_ok", "ready_ok", "snapshot_ok", "stop_ok") if result.get(key) is not True]
        if failed:
            raise ValueError(
                f"iteration {iteration}: {protocol} failed {', '.join(failed)}"
            )


def main() -> int:
    args = parse_args()
    binary = args.binary.resolve()
    if not binary.is_file():
        print(f"soak failed: binary not found: {binary}", file=sys.stderr)
        return 2

    started = time.monotonic()
    durations: list[float] = []
    for iteration in range(1, args.iterations + 1):
        run_started = time.monotonic()
        try:
            completed = subprocess.run(
                [str(binary), "--format", "json", "doctor"],
                capture_output=True,
                text=True,
                timeout=args.per_run_timeout,
                check=False,
            )
        except subprocess.TimeoutExpired:
            print(
                f"soak failed: iteration {iteration} exceeded {args.per_run_timeout:g}s",
                file=sys.stderr,
            )
            return 1

        duration = time.monotonic() - run_started
        durations.append(duration)
        try:
            payload = json.loads(completed.stdout)
            validate_envelope(payload, iteration)
            if completed.returncode != 0:
                raise ValueError(
                    f"iteration {iteration}: process exited {completed.returncode}"
                )
        except (json.JSONDecodeError, ValueError) as error:
            print(f"soak failed: {error}", file=sys.stderr)
            if completed.stderr:
                print(completed.stderr.rstrip(), file=sys.stderr)
            return 1
        print(f"iteration {iteration}/{args.iterations}: passed ({duration:.3f}s)")

    elapsed = time.monotonic() - started
    print(
        json.dumps(
            {
                "status": "passed",
                "iterations": args.iterations,
                "protocols_per_iteration": sorted(PROTOCOLS),
                "elapsed_seconds": round(elapsed, 3),
                "maximum_iteration_seconds": round(max(durations), 3),
                "per_run_timeout_seconds": args.per_run_timeout,
            },
            sort_keys=True,
        )
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

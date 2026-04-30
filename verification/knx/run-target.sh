#!/usr/bin/env bash
set -euo pipefail

target="${1:-}"
if [[ -z "$target" ]]; then
    echo "usage: $0 <target>" >&2
    exit 1
fi

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
manifest_file="$script_dir/interop-matrix.toml"
compose_file="$script_dir/compose.yaml"

python3 - "$manifest_file" "$target" <<'PY'
import sys
import tomllib

manifest_path = sys.argv[1]
target = sys.argv[2]

with open(manifest_path, "rb") as handle:
    manifest = tomllib.load(handle)

targets = manifest.get("targets", [])
matches = [
    entry for entry in targets
    if entry.get("name") == target or entry.get("compose_service") == target
]

if not matches:
    print(f"unknown KNX interop target: {target}", file=sys.stderr)
    sys.exit(1)

entry = matches[0]
if entry.get("name") != entry.get("compose_service"):
    print(
        "KNX interop runner requires target name and compose_service to match "
        f"for now: {entry.get('name')} != {entry.get('compose_service')}",
        file=sys.stderr,
    )
    sys.exit(1)

if entry.get("compose_service") != target:
    print(
        f"requested target {target} maps to service {entry.get('compose_service')}",
        file=sys.stderr,
    )
    sys.exit(1)

for required in ("peer", "profiles", "capabilities"):
    if not entry.get(required):
        print(f"KNX interop target {target} is missing required field: {required}", file=sys.stderr)
        sys.exit(1)
PY

if ! grep -Eq "^[[:space:]]{2}${target}:" "$compose_file"; then
    echo "compose service for KNX interop target not found: $target" >&2
    exit 1
fi

echo "running self-contained KNX interop target '$target' via docker compose"
docker compose -f "$compose_file" run --rm "$target"

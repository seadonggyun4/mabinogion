#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 1 ]]; then
  echo "usage: $0 <open62541|milo|async-opcua>" >&2
  exit 2
fi

target="$1"
case "$target" in
  open62541|milo|async-opcua)
    ;;
  *)
    echo "unknown interop target: $target" >&2
    exit 2
    ;;
esac

script_dir="$(cd "$(dirname "$0")" && pwd)"
repo_root="$(cd "$script_dir/../.." && pwd)"
compose_file="$script_dir/compose.yaml"

echo "running self-contained opcua interop target '$target' via docker compose"
docker compose -f "$compose_file" run --rm "$target"

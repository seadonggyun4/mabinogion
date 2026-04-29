#!/usr/bin/env bash
set -euo pipefail

target="${1:-}"
if [[ -z "$target" ]]; then
    echo "usage: $0 <target>" >&2
    exit 1
fi

case "$target" in
    bacpypes3-canary)
        ;;
    bac0-canary)
        ;;
    bacnet-stack-canary)
        ;;
    bacnet4j-canary)
        ;;
    *)
        echo "unknown BACnet interop target: $target" >&2
        exit 1
        ;;
esac

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
compose_file="$script_dir/compose.yaml"

echo "running self-contained BACnet interop target '$target' via docker compose"
docker compose -f "$compose_file" run --rm "$target"

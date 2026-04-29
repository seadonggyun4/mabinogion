#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$script_dir"

mvn -q -DskipTests compile exec:java -Dexec.mainClass=com.mabinogion.bacnet.PeerClient

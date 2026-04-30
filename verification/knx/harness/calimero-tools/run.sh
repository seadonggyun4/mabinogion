#!/usr/bin/env bash
set -euo pipefail

export CALIMERO_TOOLS_JAR="${CALIMERO_TOOLS_JAR:-/opt/calimero-tools/calimero-tools-2.6.jar}"
cargo test -p mabi-knx --test interop_profiles -- --ignored calimero_tools_profile_smoke_contract

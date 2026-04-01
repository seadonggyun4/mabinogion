#!/usr/bin/env bash
set -euo pipefail

cd /workspace
cargo test -p mabi-opcua --test interop_profiles -- --ignored async_opcua_profile_smoke_contract

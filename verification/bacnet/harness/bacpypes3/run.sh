#!/usr/bin/env bash
set -euo pipefail

cd /workspace
cargo test -p mabi-bacnet --test interop_profiles -- --ignored bacpypes3_canary_profile_smoke_contract

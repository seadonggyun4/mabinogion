#!/usr/bin/env bash
set -euo pipefail

cd /workspace

cargo test -p mabi-knx --test interop_profiles -- --ignored xknx_canary_profile_smoke_contract

#!/usr/bin/env bash
set -euo pipefail

cd /workspace
cargo test -p mabi-bacnet --test interop_profiles -- --ignored bac0_yabe_readmultiple_probe_smoke_contract

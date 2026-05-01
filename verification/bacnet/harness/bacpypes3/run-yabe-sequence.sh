#!/usr/bin/env bash
set -euo pipefail

cd /workspace
cargo test -p mabi-bacnet --test interop_profiles -- --ignored bacpypes3_yabe_sequence_smoke_contract

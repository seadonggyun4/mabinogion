#!/usr/bin/env bash
set -euo pipefail

cd /workspace
cargo test -p mabi-opcua --features https --test interop_profiles -- --ignored milo_profile_smoke_contract

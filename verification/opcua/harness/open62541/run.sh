#!/usr/bin/env bash
set -euo pipefail

cd /workspace
cargo test -p mabi-opcua --test interop_profiles -- --ignored open62541_profile_smoke_contract

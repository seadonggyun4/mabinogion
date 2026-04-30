#!/usr/bin/env bash
set -euo pipefail

export NODE_PATH="${NODE_PATH:-$(npm root -g)}"
cargo test -p mabi-knx --test interop_profiles -- --ignored knxjs_profile_smoke_contract

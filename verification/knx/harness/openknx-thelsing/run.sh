#!/usr/bin/env bash
set -euo pipefail

cargo test -p mabi-knx --test interop_profiles -- --ignored openknx_thelsing_profile_smoke_contract

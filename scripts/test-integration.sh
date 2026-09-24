#!/usr/bin/env bash
set -euo pipefail
export ATHENA_TEST_DATABASE_URL="${ATHENA_TEST_DATABASE_URL:-postgres://postgres@127.0.0.1:55432/athena_test}"
cargo test --locked --test storage_integration --test operations_integration --test scheduling_integration -- --ignored --nocapture

#!/bin/bash
set -euo pipefail
IFS=$'\n\t'

export MINICRATER_SHOW_OUTPUT=1
export MINICRATER_FAST_WORKSPACE_INIT=1

cargo run -- create-lists
cargo test -- --ignored --nocapture --test-threads 1

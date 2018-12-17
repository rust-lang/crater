#!/bin/bash
set -euo pipefail
IFS=$'\n\t'

cargo run -- create-lists
MINICRATER_SHOW_OUTPUT=1 cargo test -- --ignored --nocapture --test-threads 1

#!/bin/bash
set -euo pipefail
IFS=$'\n\t'

cargo run -- prepare-local --docker-env=mini
MINICRATER_SHOW_OUTPUT=1 cargo test -- --ignored --nocapture --test-threads 1

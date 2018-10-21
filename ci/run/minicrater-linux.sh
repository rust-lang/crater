#!/bin/bash
set -euo pipefail
IFS=$'\n\t'

cargo run -- prepare-local --docker-env=mini
cargo test -- --ignored --nocapture --test-threads 1

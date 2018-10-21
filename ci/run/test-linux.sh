#!/bin/bash
set -euo pipefail
IFS=$'\n\t'

cargo build

cargo run -- prepare-local --docker-env=mini
cargo test
cargo test -- --ignored

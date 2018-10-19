#!/bin/bash
set -euo pipefail
IFS=$'\n\t'

# Run lint tools
cargo fmt -- --check
cargo clippy

# Check if the configuration is OK
cargo run -- create-lists
cargo run -- check-config

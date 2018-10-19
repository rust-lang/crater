#!/bin/bash
set -euo pipefail
IFS=$'\n\t'

cargo build
cargo run -- create-lists
cargo test

#!/bin/bash
set -euo pipefail
IFS=$'\n\t'

cargo check --release
cargo check --release --tests

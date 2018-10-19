#!/bin/bash
set -euo pipefail
IFS=$'\n\t'

rustup component add rustfmt-preview
rustup component add clippy-preview

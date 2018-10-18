#!/bin/bash

export PATH="/cargo-home/bin:$PATH"
export CARGO_HOME=/cargo-home
export RUSTUP_HOME=/rustup-home
export SOURCE_DIR=/source
export CARGO_TARGET_DIR=/target

exec "$@"

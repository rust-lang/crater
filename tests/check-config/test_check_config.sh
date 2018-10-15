#!/bin/sh

set -e

echo -n "Testing good_config... "
cargo run -- check-config tests/check-config/good_config.toml >/dev/null 2>/dev/null || (echo "check-config of good_config FAILED! Unexpected exit status" && exit 1)
echo "OK"

echo -n "Testing bad_config1... "
(cargo run -- check-config tests/check-config/bad_config1.toml 2>/dev/null && (echo "check-config of bad_config1 FAILED! Unexpected exit status" && exit 1)) |
  ( grep -q 'got error parsing the config-file: duplicate key: `actix` for key `crates`' || (echo "check-config of bad_config1 FAILED! Expected error message missing" && exit 1))
echo "OK"

echo -n "Testing bad_config2... "
(cargo run -- check-config tests/check-config/bad_config2.toml 2>/dev/null && (echo "check-config of bad_config2 FAILED! Unexpected exit status" && exit 1)) |
  ( grep -q 'got error parsing the config-file: duplicate key: `rust-lang-nursery/crater` for key `github-repos`' || (echo "check-config of bad_config2 FAILED! Expected error message missing" && exit 1))
echo "OK"

echo -n "Testing bad_config3... "
(cargo run -- check-config tests/check-config/bad_config3.toml 2>/dev/null && (echo "check-config of bad_config3 FAILED! Unexpected exit status" && exit 1)) |
  ( grep -q 'check-config failed: crate `crater_missing_crate` is not available.' || (echo "check-config of bad_config3 FAILED! Expected error message missing" && exit 1))
echo "OK"

echo -n "Testing bad_config4... "
(cargo run -- check-config tests/check-config/bad_config4.toml 2>/dev/null && (echo "check-config of bad_config4 FAILED! Unexpected exit status" && exit 1)) |
  ( grep -q 'check-config failed: GitHub repo `rust-lang-nursery/no-such-repo` is missing' || (echo "check-config of bad_config4 FAILED! Expected error message missing" && exit 1))
echo "OK"

echo "All tests OK."

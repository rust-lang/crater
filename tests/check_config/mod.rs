use assert_cmd::prelude::*;
use common::CommandCraterExt;
use predicates::str::contains;
use std::process::Command;

#[test]
fn test_good_config() {
    Command::crater()
        .args(&["check-config", "tests/check_config/good.toml"])
        .assert()
        .success();
}

#[test]
fn test_bad_config_duplicate_crate() {
    Command::crater()
        .args(&[
            "check-config",
            "tests/check_config/bad-duplicate-crate.toml",
        ]).assert()
        .failure()
        .code(1)
        .stdout(contains("duplicate key: `lazy_static` for key `crates`"));
}

#[test]
fn test_bad_config_duplicate_repo() {
    Command::crater()
        .args(&["check-config", "tests/check_config/bad-duplicate-repo.toml"])
        .assert()
        .failure()
        .code(1)
        .stdout(contains(
            "duplicate key: `brson/hello-rs` for key `github-repos`",
        ));
}

#[test]
fn test_bad_config_missing_crate() {
    Command::crater()
        .args(&["check-config", "tests/check_config/bad-missing-crate.toml"])
        .assert()
        .failure()
        .code(1)
        .stdout(contains("crate `crater_missing_crate` is not available"));
}

#[test]
fn test_bad_config_missing_repo() {
    Command::crater()
        .args(&["check-config", "tests/check_config/bad-missing-repo.toml"])
        .assert()
        .failure()
        .code(1)
        .stdout(contains("GitHub repo `ghost/missing-repo` is missing"));
}

// We define some unused constants here, since we don't have compile time string concatination.
#![allow(dead_code)]


use std::path::PathBuf;

lazy_static! {
    pub static ref WORK_DIR: &'static str = "./work";
    pub static ref LOCAL_DIR: PathBuf = "./work/local".into();

    pub static ref CARGO_HOME: String = "./work/local/cargo-home".into();
    pub static ref RUSTUP_HOME: String = "./work/local/rustup-home".into();

    // Custom toolchains
    pub static ref TOOLCHAIN_DIR: PathBuf = "./work/local/rustup-home/toolchains".into();

    // Where cargo puts its output, when running outside a docker container,
    // CARGO_TARGET_DIR
    pub static ref TARGET_DIR: PathBuf = "./work/local/target-dirs".into();

    // The directory crates are unpacked to for running tests, mounted
    // in docker containers
    pub static ref TEST_SOURCE_DIR: PathBuf = "./work/local/test-source".into();

    // Where GitHub crate mirrors are stored
    pub static ref GH_MIRRORS_DIR: PathBuf = "./work/local/gh-mirrors".into();

    // Where crates.io sources are stores
    pub static ref CRATES_DIR: PathBuf = "./work/shared/crates".into();

    // Lists of crates
    pub static ref LIST_DIR: PathBuf = "./work/shared/lists".into();

    pub static ref EXPERIMENT_DIR: PathBuf = "./work/ex".into();
    pub static ref LOG_DIR: PathBuf = "./work/logs".into();
}

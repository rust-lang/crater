pub const WORK_DIR: &'static str = "./work";
pub const LOCAL_DIR: &'static str = "./work/local";

pub const CARGO_HOME: &'static str = "./work/local/cargo-home";
pub const RUSTUP_HOME: &'static str = "./work/local/rustup-home";

// Custom toolchains
pub const TOOLCHAIN_DIR: &'static str = "./work/local/custom-tc";

// Where cargo puts its output, when running outside a docker container,
// CARGO_TARGET_DIR
pub const TARGET_DIR: &'static str = "./work/local/target-dirs";

// The directory crates are unpacked to for running tests, mounted
// in docker containers
pub const TEST_SOURCE_DIR: &'static str = "./work/local/test-source";

// Where GitHub crate mirrors are stored
pub const GH_MIRRORS_DIR: &'static str = "./work/local/gh-mirrors";

// Where crates.io sources are stores
pub const CRATES_DIR: &'static str = "./work/shared/crates";

// Lists of crates
pub const LIST_DIR: &'static str = "./work/shared/lists";

// crates.io Cargo.toml files, modified to build correctly
pub const FROB_DIR: &'static str = "./work/shared/fromls";

pub const EXPERIMENT_DIR: &'static str = "./work/ex";
pub const LOG_DIR: &'static str = "./work/logs";

// State for asynchronous job management
pub const JOB_DIR: &'static str = "./work/jobs";

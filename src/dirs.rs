use std::path::{Path, PathBuf};

lazy_static! {
    pub static ref WORK_DIR: PathBuf = "./work".into();
    pub static ref LOCAL_DIR: PathBuf = WORK_DIR.join("local");

    pub static ref CARGO_HOME: String = LOCAL_DIR.join("cargo-home").to_string_lossy().into();
    pub static ref RUSTUP_HOME: String = LOCAL_DIR.join("rustup-home").to_string_lossy().into();

    // Custom toolchains
    pub static ref TOOLCHAIN_DIR: PathBuf = Path::new(&*RUSTUP_HOME).join("toolchains");

    // Where cargo puts its output, when running outside a docker container,
    // CARGO_TARGET_DIR
    pub static ref TARGET_DIR: PathBuf = LOCAL_DIR.join("target-dirs");

    // The directory crates are unpacked to for running tests, mounted
    // in docker containers
    pub static ref TEST_SOURCE_DIR: PathBuf = LOCAL_DIR.join("test-source");

    // Where GitHub crate mirrors are stored
    pub static ref GH_MIRRORS_DIR: PathBuf = LOCAL_DIR.join("gh-mirrors");

    // Where crates.io sources are stores
    pub static ref CRATES_DIR: PathBuf = WORK_DIR.join("shared/crates");

    // Lists of crates
    pub static ref LIST_DIR: PathBuf = WORK_DIR.join("shared/lists");

    pub static ref EXPERIMENT_DIR: PathBuf = WORK_DIR.join("ex");
    pub static ref LOG_DIR: PathBuf = WORK_DIR.join("logs");
}

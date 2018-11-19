use crates::Crate;
use experiments::Experiment;
use std::env;
use std::ffi::OsStr;
use std::path::PathBuf;
use toolchain::Toolchain;

lazy_static! {
    pub static ref WORK_DIR: PathBuf = {
        env::var_os("CRATER_WORK_DIR")
            .unwrap_or_else(|| OsStr::new("work").to_os_string())
            .into()
    };
    pub static ref LOCAL_DIR: PathBuf = WORK_DIR.join("local");

    pub static ref CARGO_HOME: String = LOCAL_DIR.join("cargo-home").to_string_lossy().into();
    pub static ref RUSTUP_HOME: String = LOCAL_DIR.join("rustup-home").to_string_lossy().into();

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

    pub static ref EXPERIMENT_DIR: PathBuf = WORK_DIR.join("ex");
    pub static ref LOG_DIR: PathBuf = WORK_DIR.join("logs");

    pub static ref LOCAL_CRATES_DIR: PathBuf = "local-crates".into();

    pub static ref SOURCE_CACHE_DIR: PathBuf = WORK_DIR.join("cache").join("sources");
}

pub(crate) fn crate_source_dir(ex: &Experiment, tc: &Toolchain, krate: &Crate) -> PathBuf {
    EXPERIMENT_DIR
        .join(&ex.name)
        .join("sources")
        .join(tc.to_string())
        .join(krate.id())
}

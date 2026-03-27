//! Filesystem layout for the crater work directory.

use crate::prelude::*;
use std::env;
use std::ffi::OsStr;
use std::path::PathBuf;

lazy_static! {
    /// Root working directory for crater data (overridable via `CRATER_WORK_DIR`).
    pub static ref WORK_DIR: PathBuf = {
        env::var_os("CRATER_WORK_DIR")
            .unwrap_or_else(|| OsStr::new("work").to_os_string())
            .into()
    };
    /// Directory containing locally-provided crate sources.
    pub static ref LOCAL_CRATES_DIR: PathBuf = "local-crates".into();
}

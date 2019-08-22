use crate::prelude::*;
use std::env;
use std::ffi::OsStr;
use std::path::PathBuf;

lazy_static! {
    pub static ref WORK_DIR: PathBuf = {
        env::var_os("CRATER_WORK_DIR")
            .unwrap_or_else(|| OsStr::new("work").to_os_string())
            .into()
    };
    pub static ref LOCAL_CRATES_DIR: PathBuf = "local-crates".into();
}

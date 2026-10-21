use std::env::{self, consts::EXE_SUFFIX};
use std::ffi::OsStr;
use std::path::PathBuf;
use std::process::Command;

static CRATER_BIN_NAME: &str = "crater";

fn bin_path() -> PathBuf {
    env::var_os("CARGO_BIN_PATH")
        .map(PathBuf::from)
        .or_else(|| {
            env::current_exe().ok().and_then(|mut path| {
                // Find target/<profile>, regardless whether Cargo uses the new or old build dir
                // layout
                while let Some(parent) = path.parent() {
                    if parent.file_name() == Some(OsStr::new("target")) {
                        return Some(path);
                    }
                    path = parent.to_path_buf();
                }
                None
            })
        })
        .unwrap_or_else(|| panic!("CARGO_BIN_PATH wasn't set. Cannot continue running test"))
}

pub(crate) trait CommandCraterExt {
    fn crater() -> Self;
}

impl CommandCraterExt for Command {
    fn crater() -> Self {
        Command::new(bin_path().join(format!("{CRATER_BIN_NAME}{EXE_SUFFIX}")))
    }
}

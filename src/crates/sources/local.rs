use crate::crates::{lists::List, Crate};
use crate::dirs::LOCAL_CRATES_DIR;
use crate::prelude::*;
use std::path::PathBuf;

pub(crate) struct LocalList {
    source: PathBuf,
}

impl Default for LocalList {
    fn default() -> Self {
        LocalList {
            source: LOCAL_CRATES_DIR.clone(),
        }
    }
}

impl List for LocalList {
    const NAME: &'static str = "local";

    fn fetch(&self) -> Fallible<Vec<Crate>> {
        if !self.source.is_dir() {
            return Ok(Vec::new());
        }

        let mut list = Vec::new();
        for entry in ::std::fs::read_dir(&self.source)? {
            let entry = entry?;

            if entry.path().join("Cargo.toml").is_file() {
                let name = entry
                    .file_name()
                    .to_str()
                    .ok_or_else(|| {
                        err_msg(format!(
                            "invalid UTF-8 in local crate name: {}",
                            entry.file_name().to_string_lossy()
                        ))
                    })?
                    .to_string();

                list.push(Crate::Local(name));
            }
        }

        Ok(list)
    }
}

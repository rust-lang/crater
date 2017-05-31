use crates_index;
use dirs::LOCAL_DIR;
use errors::*;
use std::path::{Path, PathBuf};

fn repo_path() -> PathBuf {
    Path::new(LOCAL_DIR).join("crates.io-index")
}

pub fn crates_index_registry() -> Result<crates_index::Index> {
    let index = crates_index::Index::new(repo_path());
    if index.exists() {
        info!("Fetching latest 'crates.io-index' repository commits");
        index.update()?;
    } else {
        info!("Cloning 'crates.io-index' repository");
        index.retrieve()?;
    }
    Ok(index)
}

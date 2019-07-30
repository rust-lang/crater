use failure::{Error, ResultExt};
use std::path::{Path, PathBuf};

/// Directory on the filesystem containing rustwide's state and caches.
pub struct Workspace {
    _path: PathBuf,
}

impl Workspace {
    /// Open a workspace on disk.
    ///
    /// If the workspace path doesn't exist it will be created.
    pub fn open(path: &Path) -> Result<Workspace, Error> {
        std::fs::create_dir_all(path).with_context(|_| {
            format!("failed to create workspace directory: {}", path.display())
        })?;
        Ok(Workspace { _path: path.into() })
    }
}

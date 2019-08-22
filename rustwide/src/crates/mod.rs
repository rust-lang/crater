mod cratesio;
mod git;
mod local;

use crate::Workspace;
use failure::Error;
use log::info;
use std::path::Path;

trait CrateTrait: std::fmt::Display {
    fn fetch(&self, workspace: &Workspace) -> Result<(), Error>;
    fn copy_source_to(&self, workspace: &Workspace, dest: &Path) -> Result<(), Error>;
}

enum CrateType {
    CratesIO(cratesio::CratesIOCrate),
    Git(git::GitRepo),
    Local(local::Local),
}

/// A Rust crate that can be used with rustwide.
pub struct Crate(CrateType);

impl Crate {
    /// Load a crate from the [crates.io registry](https://crates.io).
    pub fn crates_io(name: &str, version: &str) -> Self {
        Crate(CrateType::CratesIO(cratesio::CratesIOCrate::new(
            name, version,
        )))
    }

    /// Load a crate from a git repository. The full URL needed to clone the repo has to be
    /// provided.
    pub fn git(url: &str) -> Self {
        Crate(CrateType::Git(git::GitRepo::new(url)))
    }

    /// Load a crate from a directory in the local filesystem.
    pub fn local(path: &Path) -> Self {
        Crate(CrateType::Local(local::Local::new(path)))
    }

    /// Fetch the crate's source code and cache it in the workspace. This method will reach out to
    /// the network for some crate types.
    pub fn fetch(&self, workspace: &Workspace) -> Result<(), Error> {
        self.as_trait().fetch(workspace)
    }

    /// Get this crate's git commit. This method is best-effort, and currently works just for git
    /// crates. If the commit can't be retrieved `None` will be returned.
    pub fn git_commit(&self, workspace: &Workspace) -> Option<String> {
        if let CrateType::Git(repo) = &self.0 {
            repo.git_commit(workspace)
        } else {
            None
        }
    }

    pub(crate) fn copy_source_to(&self, workspace: &Workspace, dest: &Path) -> Result<(), Error> {
        if dest.exists() {
            info!(
                "crate source directory {} already exists, cleaning it up",
                dest.display()
            );
            std::fs::remove_dir_all(dest)?;
        }
        self.as_trait().copy_source_to(workspace, dest)
    }

    fn as_trait(&self) -> &dyn CrateTrait {
        match &self.0 {
            CrateType::CratesIO(krate) => krate,
            CrateType::Git(repo) => repo,
            CrateType::Local(local) => local,
        }
    }
}

impl std::fmt::Display for Crate {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{}", self.as_trait())
    }
}

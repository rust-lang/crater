use super::CrateTrait;
use crate::cmd::Command;
use crate::Workspace;
use failure::{Error, ResultExt};
use log::info;
use percent_encoding::{percent_encode, AsciiSet, CONTROLS};
use std::path::{Path, PathBuf};

const ENCODE_SET: AsciiSet = CONTROLS
    .add(b'/')
    .add(b'\\')
    .add(b'<')
    .add(b'>')
    .add(b':')
    .add(b'"')
    .add(b'|')
    .add(b'?')
    .add(b'*')
    .add(b' ');

pub(super) struct GitRepo {
    pub url: String,
}

impl GitRepo {
    pub(super) fn new(url: &str) -> Self {
        Self { url: url.into() }
    }

    fn cached_path(&self, workspace: &Workspace) -> PathBuf {
        workspace
            .cache_dir()
            .join("git-repos")
            .join(percent_encode(self.url.as_bytes(), &ENCODE_SET).to_string())
    }
}

impl CrateTrait for GitRepo {
    fn fetch(&self, workspace: &Workspace) -> Result<(), Error> {
        let path = self.cached_path(workspace);
        if path.join("HEAD").is_file() {
            info!("updating cached repository {}", self.url);
            Command::new(workspace, "git")
                .args(&["fetch", "--all"])
                .cd(&path)
                .run()
                .with_context(|_| format!("failed to update {}", self.url))?;
        } else {
            info!("cloning repository {}", self.url);
            Command::new(workspace, "git")
                .args(&["clone", "--bare", &self.url])
                .args(&[&path])
                .run()
                .with_context(|_| format!("failed to clone {}", self.url))?;
        }
        Ok(())
    }

    fn copy_source_to(&self, workspace: &Workspace, dest: &Path) -> Result<(), Error> {
        Command::new(workspace, "git")
            .args(&["clone"])
            .args(&[self.cached_path(workspace).as_path(), dest])
            .run()
            .with_context(|_| format!("failed to checkout {}", self.url))?;
        Ok(())
    }
}

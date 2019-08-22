use super::CrateTrait;
use crate::Workspace;
use failure::Error;
use flate2::read::GzDecoder;
use log::info;
use std::fs::File;
use std::io::{BufReader, BufWriter, Read};
use std::path::{Path, PathBuf};
use tar::Archive;

static CRATES_ROOT: &str = "https://static.crates.io/crates";

impl CratesIOCrate {
    pub(super) fn new(name: &str, version: &str) -> Self {
        CratesIOCrate {
            name: name.into(),
            version: version.into(),
        }
    }

    fn cache_path(&self, workspace: &Workspace) -> PathBuf {
        workspace
            .cache_dir()
            .join("cratesio-sources")
            .join(&self.name)
            .join(format!("{}-{}.crate", self.name, self.version))
    }
}

pub(super) struct CratesIOCrate {
    name: String,
    version: String,
}

impl CrateTrait for CratesIOCrate {
    fn fetch(&self, workspace: &Workspace) -> Result<(), Error> {
        let local = self.cache_path(workspace);
        if local.exists() {
            info!("crate {} {} is already in cache", self.name, self.version);
            return Ok(());
        }

        info!("fetching crate {} {}...", self.name, self.version);
        if let Some(parent) = local.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let remote = format!(
            "{0}/{1}/{1}-{2}.crate",
            CRATES_ROOT, self.name, self.version
        );
        let mut resp = workspace
            .http_client()
            .get(&remote)
            .send()?
            .error_for_status()?;
        resp.copy_to(&mut BufWriter::new(File::create(&local)?))?;

        Ok(())
    }

    fn copy_source_to(&self, workspace: &Workspace, dest: &Path) -> Result<(), Error> {
        let cached = self.cache_path(workspace);
        let mut file = File::open(cached)?;
        let mut tar = Archive::new(GzDecoder::new(BufReader::new(&mut file)));

        info!(
            "extracting crate {} {} into {}",
            self.name,
            self.version,
            dest.display()
        );
        if let Err(err) = unpack_without_first_dir(&mut tar, dest) {
            let _ = std::fs::remove_dir_all(dest);
            Err(err
                .context(format!(
                    "unable to download {} version {}",
                    self.name, self.version
                ))
                .into())
        } else {
            Ok(())
        }
    }
}

impl std::fmt::Display for CratesIOCrate {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "crates.io crate {} {}", self.name, self.version)
    }
}

fn unpack_without_first_dir<R: Read>(archive: &mut Archive<R>, path: &Path) -> Result<(), Error> {
    let entries = archive.entries()?;
    for entry in entries {
        let mut entry = entry?;
        let relpath = {
            let path = entry.path();
            let path = path?;
            path.into_owned()
        };
        let mut components = relpath.components();
        // Throw away the first path component
        components.next();
        let full_path = path.join(&components.as_path());
        if let Some(parent) = full_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        entry.unpack(&full_path)?;
    }

    Ok(())
}

use crates::{lists::List, Crate};
use crates_index::Index;
use dirs::LOCAL_DIR;
use flate2::read::GzDecoder;
use prelude::*;
use std::collections::HashMap;
use std::fs;
use std::io::Read;
use std::path::Path;
use tar::Archive;

static CRATES_ROOT: &str = "https://crates-io.s3-us-west-1.amazonaws.com/crates";

pub(crate) struct RegistryList;

impl List for RegistryList {
    const NAME: &'static str = "registry";

    fn fetch(&self) -> Fallible<Vec<Crate>> {
        let mut list = Vec::new();
        let mut counts = HashMap::new();

        let index = Index::new(LOCAL_DIR.join("crates.io-index"));
        index.retrieve_or_update().to_failure()?;

        for krate in index.crates() {
            // The versions() method returns the list of published versions starting from the
            // first one, so its output is reversed to check the latest first
            for version in krate.versions().iter().rev() {
                // Try every version until we find a non-yanked one. If all the versions are
                // yanked the crate is automatically skipped
                if !version.is_yanked() {
                    // Increment the counters of this crate's dependencies
                    for dependency in version.dependencies() {
                        let count = counts.entry(dependency.name().to_string()).or_insert(0);
                        *count += 1;
                    }

                    list.push(Crate::Registry(RegistryCrate {
                        name: krate.name().to_string(),
                        version: version.version().to_string(),
                    }));
                    break;
                }
            }
        }

        // Ensure the list is sorted by popularity
        list.sort_by(|a, b| {
            if let (Crate::Registry(ref a), Crate::Registry(ref b)) = (a, b) {
                let count_a = counts.get(&a.name).cloned().unwrap_or(0);
                let count_b = counts.get(&b.name).cloned().unwrap_or(0);
                count_b.cmp(&count_a)
            } else {
                panic!("non-registry crate produced in the registry list");
            }
        });

        Ok(list)
    }
}

#[derive(Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize, Clone)]
pub struct RegistryCrate {
    pub name: String,
    pub version: String,
}

impl RegistryCrate {
    pub(in crates) fn prepare(&self, dest: &Path) -> Fallible<()> {
        dl_registry(&self.name, &self.version, &dest).with_context(|_| {
            format!("unable to download {} version {}", self.name, self.version)
        })?;
        Ok(())
    }
}

fn dl_registry(name: &str, vers: &str, dir: &Path) -> Fallible<()> {
    if dir.exists() {
        info!(
            "crate {}-{} exists at {}. skipping",
            name,
            vers,
            dir.display()
        );
        return Ok(());
    }
    info!("downloading crate {}-{} to {}", name, vers, dir.display());
    let url = format!("{0}/{1}/{1}-{2}.crate", CRATES_ROOT, name, vers);
    let bin =
        ::utils::http::get_sync(&url).with_context(|_| format!("unable to download {}", url))?;

    fs::create_dir_all(&dir)?;

    let mut tar = Archive::new(GzDecoder::new(bin));
    let r =
        unpack_without_first_dir(&mut tar, dir).with_context(|_| "unable to unpack crate tarball");

    if r.is_err() {
        let _ = ::utils::fs::remove_dir_all(dir);
    }

    r.map_err(|e| e.into())
}

fn unpack_without_first_dir<R: Read>(archive: &mut Archive<R>, path: &Path) -> Fallible<()> {
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
            fs::create_dir_all(parent)?;
        }
        entry.unpack(&full_path)?;
    }

    Ok(())
}

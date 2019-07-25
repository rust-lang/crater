use crate::crates::{lists::List, Crate};
use crate::dirs::{LOCAL_DIR, SOURCE_CACHE_DIR};
use crate::prelude::*;
use crates_index::Index;
use flate2::read::GzDecoder;
use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{BufReader, BufWriter, Read};
use std::path::{Path, PathBuf};
use tar::Archive;

static CRATES_ROOT: &str = "https://crates-io.s3-us-west-1.amazonaws.com/crates";

pub(crate) struct RegistryList;

impl List for RegistryList {
    const NAME: &'static str = "registry";

    fn fetch(&self) -> Fallible<Vec<Crate>> {
        let mut list = Vec::new();
        let mut counts = HashMap::new();

        fs::create_dir_all(&*LOCAL_DIR)?;
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
    fn cached_path(&self) -> PathBuf {
        SOURCE_CACHE_DIR
            .join("reg")
            .join(&self.name)
            .join(format!("{}-{}.crate", self.name, self.version))
    }

    pub(in crate::crates) fn fetch(&self) -> Fallible<()> {
        let local = self.cached_path();
        if local.exists() {
            info!("crate {} {} is already in cache", self.name, self.version);
            return Ok(());
        }

        info!("fetching crate {} {}...", self.name, self.version);
        if let Some(parent) = local.parent() {
            fs::create_dir_all(parent)?;
        }
        let remote = format!(
            "{0}/{1}/{1}-{2}.crate",
            CRATES_ROOT, self.name, self.version
        );
        let mut resp = crate::utils::http::get_sync(&remote)?;
        resp.copy_to(&mut BufWriter::new(File::create(&local)?))?;

        Ok(())
    }

    pub(in crate::crates) fn copy_to(&self, dest: &Path) -> Fallible<()> {
        let cached = self.cached_path();
        let mut file = File::open(cached)?;
        let mut tar = Archive::new(GzDecoder::new(BufReader::new(&mut file)));

        info!(
            "extracting crate {} {} into {}",
            self.name,
            self.version,
            dest.display()
        );
        if let Err(err) = unpack_without_first_dir(&mut tar, dest) {
            let _ = crate::utils::fs::remove_dir_all(dest);
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

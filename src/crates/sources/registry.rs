use crate::crates::{lists::List, Crate};
use crate::dirs::WORK_DIR;
use crate::prelude::*;
use crates_index::GitIndex;
use rayon::iter::ParallelIterator;
use std::collections::HashMap;
use std::fs::{self};
use std::sync::Mutex;

pub(crate) struct RegistryList;

impl List for RegistryList {
    const NAME: &'static str = "registry";

    fn fetch(&self) -> Fallible<Vec<Crate>> {
        let counts = Mutex::new(HashMap::new());

        debug!("updating git index");
        fs::create_dir_all(&*WORK_DIR)?;
        let mut index = GitIndex::with_path(
            WORK_DIR.join("crates.io-index"),
            "https://github.com/rust-lang/crates.io-index",
        )?;
        index.update()?;
        debug!("collecting crate information");

        let mut list: Vec<_> = index
            .crates_parallel()
            .filter_map(|krate| {
                let krate = krate.as_ref().unwrap();
                // The versions() method returns the list of published versions starting from the
                // first one, so its output is reversed to check the latest first
                krate
                    .versions()
                    .iter()
                    .rev()
                    // Don't include yanked versions. If all versions are
                    // yanked, then the crate is skipped.
                    .filter(|version| !version.is_yanked())
                    .map(|version| {
                        // Increment the counters of this crate's dependencies
                        let mut counts = counts.lock().unwrap();
                        for dependency in version.dependencies() {
                            let count = counts.entry(dependency.name().to_string()).or_insert(0);
                            *count += 1;
                        }
                        Crate::Registry(RegistryCrate {
                            name: krate.name().to_string(),
                            version: version.version().to_string(),
                        })
                    })
                    .next()
            })
            .collect();

        // Ensure the list is sorted by popularity
        let counts = counts.lock().unwrap();
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

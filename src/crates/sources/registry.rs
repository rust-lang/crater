use crate::crates::{lists::List, Crate};
use crate::dirs::WORK_DIR;
use crate::prelude::*;
use crates_index::Index;
use std::collections::HashMap;
use std::fs::{self};

pub(crate) struct RegistryList;

impl List for RegistryList {
    const NAME: &'static str = "registry";

    fn fetch(&self) -> Fallible<Vec<Crate>> {
        let mut list = Vec::new();
        let mut counts = HashMap::new();

        fs::create_dir_all(&*WORK_DIR)?;
        let index = Index::new(WORK_DIR.join("crates.io-index"));
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

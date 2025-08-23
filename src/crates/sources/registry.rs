use crate::crates::{lists::List, Crate};
use crate::dirs::WORK_DIR;
use crate::prelude::*;
use crates_index::GitIndex;
use smol_str::SmolStr;
use std::collections::HashMap;
use std::fs;

pub(crate) struct RegistryList;

impl List for RegistryList {
    const NAME: &'static str = "registry";

    fn fetch(&self) -> Fallible<Vec<Crate>> {
        let arena = bumpalo::Bump::new();
        let mut counts = HashMap::new();

        debug!("updating git index");
        fs::create_dir_all(&*WORK_DIR)?;
        let mut index = GitIndex::with_path(
            WORK_DIR.join("crates.io-index"),
            "https://github.com/rust-lang/crates.io-index",
        )?;
        index.update()?;
        debug!("collecting crate information");

        let mut list: Vec<_> = index
            .crates()
            .filter_map(|krate| {
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
                        for dependency in version.dependencies() {
                            if let Some(count) = counts.get_mut(dependency.name()) {
                                *count += 1;
                            } else {
                                let allocated = arena.alloc_str(dependency.name());
                                counts.insert(&*allocated, 1);
                            }
                        }
                        Crate::Registry(RegistryCrate {
                            name: SmolStr::from(krate.name()),
                            version: SmolStr::from(version.version()),
                        })
                    })
                    .next()
            })
            .collect();

        // Ensure the list is sorted by popularity
        list.sort_unstable_by_key(|a| {
            if let Crate::Registry(ref a) = a {
                counts.get(a.name.as_str()).cloned().unwrap_or(0)
            } else {
                panic!("non-registry crate produced in the registry list");
            }
        });

        Ok(list)
    }
}

#[derive(Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize, Clone)]
pub struct RegistryCrate {
    pub name: SmolStr,
    pub version: SmolStr,
}

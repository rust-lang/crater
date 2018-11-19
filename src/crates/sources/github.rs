use crates::{lists::List, Crate};
use dirs::SOURCE_CACHE_DIR;
use prelude::*;
use run::RunCommand;
use std::borrow::Cow;
use std::path::{Path, PathBuf};
use std::str::FromStr;

static CACHED_LIST: &'static str =
    "https://raw.githubusercontent.com/rust-ops/rust-repos/master/data/github.csv";

#[derive(Deserialize)]
struct ListRepo {
    name: String,
    has_cargo_toml: bool,
    has_cargo_lock: bool,
}

pub(crate) struct GitHubList {
    source: Cow<'static, str>,
}

impl Default for GitHubList {
    fn default() -> Self {
        GitHubList {
            source: CACHED_LIST.into(),
        }
    }
}

impl List for GitHubList {
    const NAME: &'static str = "github-oss";

    fn fetch(&self) -> Fallible<Vec<Crate>> {
        info!("loading cached GitHub list from {}", self.source);

        let mut resp = ::utils::http::get_sync(&self.source)
            .with_context(|_| format!("failed to fetch GitHub crates list from {}", self.source))?;
        let mut reader = ::csv::Reader::from_reader(&mut resp);

        let mut list = Vec::new();
        for line in reader.deserialize() {
            let line: ListRepo = line?;

            // Only import repos with a Cargo.toml or Cargo.lock
            if !line.has_cargo_toml || !line.has_cargo_lock {
                continue;
            }

            let mut name_parts = line.name.split('/');
            let org = name_parts.next();
            let name = name_parts.next();
            let trailing = name_parts.next();

            if let (Some(org), Some(name), None) = (org, name, trailing) {
                list.push(Crate::GitHub(GitHubRepo {
                    org: org.to_string(),
                    name: name.to_string(),
                }));
            } else {
                warn!("skipping malformed repo name: {}", line.name);
            }
        }

        Ok(list)
    }
}

#[derive(Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize, Clone)]
pub struct GitHubRepo {
    pub org: String,
    pub name: String,
}

impl GitHubRepo {
    pub(crate) fn cached_path(&self) -> PathBuf {
        SOURCE_CACHE_DIR.join("gh").join(&self.org).join(&self.name)
    }

    pub(crate) fn slug(&self) -> String {
        format!("{}/{}", self.org, self.name)
    }

    pub(in crates) fn fetch(&self) -> Fallible<()> {
        let path = self.cached_path();
        if path.join("HEAD").is_file() {
            info!("updating cached repository {}", self.slug());
            RunCommand::new("git")
                .args(&["fetch", "--all"])
                .cd(&path)
                .run()
                .with_context(|_| format!("failed to update {}", self.slug()))?;
        } else {
            info!("cloning repository {}", self.slug());
            RunCommand::new("git")
                .args(&[
                    "clone",
                    "--bare",
                    &format!("git://github.com/{}/{}.git", self.org, self.name),
                ])
                .args(&[&path])
                .run()
                .with_context(|_| format!("failed to clone {}", self.slug()))?;
        }
        Ok(())
    }

    pub(in crates) fn copy_to(&self, dest: &Path) -> Fallible<()> {
        if dest.exists() {
            ::utils::fs::remove_dir_all(dest)?;
        }
        RunCommand::new("git")
            .args(&["clone"])
            .args(&[self.cached_path().as_path(), dest])
            .run()
            .with_context(|_| format!("failed to checkout {}", self.slug()))?;
        Ok(())
    }
}

impl FromStr for GitHubRepo {
    type Err = ::failure::Error;

    fn from_str(input: &str) -> Fallible<Self> {
        let mut components = input.split('/').collect::<Vec<_>>();
        let name = components.pop();
        let org = components.pop();

        if let (Some(org), Some(name)) = (org, name) {
            Ok(GitHubRepo {
                org: org.to_string(),
                name: name.to_string(),
            })
        } else {
            bail!("malformed repo url: {}", input);
        }
    }
}

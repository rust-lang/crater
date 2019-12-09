use crate::crates::{lists::List, Crate};
use crate::prelude::*;
use std::borrow::Cow;
use std::str::FromStr;

static CACHED_LIST: &str =
    "https://raw.githubusercontent.com/rust-lang/rust-repos/master/data/github.csv";
const DUMMY_ORG: &str = "ghost";
const DUMMY_NAME: &str = "missing";

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

        let mut resp = crate::utils::http::get_sync(&self.source)
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
    pub(crate) fn slug(&self) -> String {
        format!("{}/{}", self.org, self.name)
    }

    pub(crate) fn dummy() -> GitHubRepo {
        GitHubRepo {
            org: DUMMY_ORG.to_string(),
            name: DUMMY_NAME.to_string(),
        }
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

use crates::{lists::List, Crate, GitHubRepo};
use errors::*;
use std::borrow::Cow;

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

    fn fetch(&self) -> Result<Vec<Crate>> {
        info!("loading cached GitHub list from {}", self.source);

        let mut resp = ::utils::http::get(&self.source)
            .chain_err(|| format!("failed to fetch GitHub crates list from {}", self.source))?;
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

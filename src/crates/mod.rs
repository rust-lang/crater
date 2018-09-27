pub(crate) mod lists;
mod sources;

use dirs::{CRATES_DIR, LOCAL_CRATES_DIR};
use errors::*;
use std::fmt;
use std::path::PathBuf;
use std::str::FromStr;

pub(crate) use crates::sources::github::GitHubRepo;
pub(crate) use crates::sources::registry::RegistryCrate;

#[derive(Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize, Clone)]
pub enum Crate {
    Registry(RegistryCrate),
    GitHub(GitHubRepo),
    Local(String),
}

impl Crate {
    pub(crate) fn dir(&self) -> PathBuf {
        match *self {
            Crate::Registry(ref details) => CRATES_DIR
                .join("reg")
                .join(format!("{}-{}", details.name, details.version)),
            Crate::GitHub(ref repo) => CRATES_DIR
                .join("gh")
                .join(format!("{}.{}", repo.org, repo.name)),
            Crate::Local(ref name) => CRATES_DIR.join("local").join(name),
        }
    }

    pub(crate) fn id(&self) -> String {
        match *self {
            Crate::Registry(ref details) => format!("reg/{}/{}", details.name, details.version),
            Crate::GitHub(ref repo) => format!("gh/{}/{}", repo.org, repo.name),
            Crate::Local(ref name) => format!("local/{}", name),
        }
    }

    pub(crate) fn prepare(&self) -> Result<()> {
        let dir = self.dir();
        match *self {
            Crate::Registry(ref details) => details.prepare(&dir)?,
            Crate::GitHub(ref repo) => repo.prepare(&dir)?,
            Crate::Local(ref name) => {
                ::utils::fs::copy_dir(&LOCAL_CRATES_DIR.join(name), &dir)?;
            }
        }

        Ok(())
    }
}

impl fmt::Display for Crate {
    fn fmt(&self, f: &mut fmt::Formatter) -> ::std::result::Result<(), fmt::Error> {
        write!(
            f,
            "{}",
            match *self {
                Crate::Registry(ref krate) => format!("{}-{}", krate.name, krate.version),
                Crate::GitHub(ref repo) => repo.slug(),
                Crate::Local(ref name) => format!("{} (local)", name),
            }
        )
    }
}

impl FromStr for Crate {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self> {
        if s.starts_with("https://github.com/") {
            Ok(Crate::GitHub(s.parse()?))
        } else if let Some(dash_idx) = s.rfind('-') {
            let name = &s[..dash_idx];
            let version = &s[dash_idx + 1..];
            Ok(Crate::Registry(RegistryCrate {
                name: name.to_string(),
                version: version.to_string(),
            }))
        } else {
            bail!("no version for crate");
        }
    }
}

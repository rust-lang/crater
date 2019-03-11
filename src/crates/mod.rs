pub(crate) mod lists;
mod sources;

use crate::dirs::LOCAL_CRATES_DIR;
use crate::prelude::*;
use std::fmt;
use std::path::Path;
use std::str::FromStr;

pub(crate) use crate::crates::sources::github::GitHubRepo;
pub(crate) use crate::crates::sources::registry::RegistryCrate;

#[derive(Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize, Clone)]
pub enum Crate {
    Registry(RegistryCrate),
    GitHub(GitHubRepo),
    Local(String),
}

impl Crate {
    pub(crate) fn id(&self) -> String {
        match *self {
            Crate::Registry(ref details) => format!("reg/{}/{}", details.name, details.version),
            Crate::GitHub(ref repo) => format!("gh/{}/{}", repo.org, repo.name),
            Crate::Local(ref name) => format!("local/{}", name),
        }
    }

    pub(crate) fn fetch(&self) -> Fallible<()> {
        match *self {
            Crate::Registry(ref krate) => krate.fetch(),
            Crate::GitHub(ref repo) => repo.fetch(),
            Crate::Local(_) => Ok(()),
        }
    }

    pub(crate) fn copy_to(&self, dest: &Path) -> Fallible<()> {
        if dest.exists() {
            info!(
                "crate source directory {} already exists, cleaning it up",
                dest.display()
            );
            crate::utils::fs::remove_dir_all(dest)?;
        }
        match *self {
            Crate::Registry(ref details) => details.copy_to(&dest)?,
            Crate::GitHub(ref repo) => repo.copy_to(&dest)?,
            Crate::Local(ref name) => {
                info!("copying local crate {} to {}", name, dest.display());
                crate::utils::fs::copy_dir(&LOCAL_CRATES_DIR.join(name), &dest)?;
            }
        }

        Ok(())
    }
}

impl fmt::Display for Crate {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
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
    type Err = ::failure::Error;

    fn from_str(s: &str) -> Fallible<Self> {
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
            bail!("crate not found");
        }
    }
}

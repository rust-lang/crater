pub(crate) mod lists;
mod sources;

use crate::dirs::LOCAL_CRATES_DIR;
use crate::prelude::*;
use rustwide::Crate as RustwideCrate;
use std::fmt;
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
            Crate::GitHub(ref repo) => {
                if let Some(ref sha) = repo.sha {
                    format!("gh/{}/{}/{}", repo.org, repo.name, sha)
                } else {
                    format!("gh/{}/{}", repo.org, repo.name)
                }
            }
            Crate::Local(ref name) => format!("local/{}", name),
        }
    }

    pub(crate) fn to_rustwide(&self) -> RustwideCrate {
        match self {
            Self::Registry(krate) => RustwideCrate::crates_io(&krate.name, &krate.version),
            Self::GitHub(repo) => {
                RustwideCrate::git(&format!("https://github.com/{}/{}", repo.org, repo.name))
            }
            Self::Local(name) => RustwideCrate::local(&LOCAL_CRATES_DIR.join(name)),
        }
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

    // matches with `Crate::id'
    fn from_str(s: &str) -> Fallible<Self> {
        match s.split('/').collect::<Vec<_>>()[..] {
            ["reg", name, version] => Ok(Crate::Registry(RegistryCrate {
                name: name.to_string(),
                version: version.to_string(),
            })),
            ["gh", org, name, sha] => Ok(Crate::GitHub(GitHubRepo {
                org: org.to_string(),
                name: name.to_string(),
                sha: Some(sha.to_string()),
            })),
            ["gh", org, name] => Ok(Crate::GitHub(GitHubRepo {
                org: org.to_string(),
                name: name.to_string(),
                sha: None,
            })),
            ["local", name] => Ok(Crate::Local(name.to_string())),
            _ => bail!("unexpected crate value"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Crate, GitHubRepo, RegistryCrate};
    use std::str::FromStr;

    #[test]
    fn test_parse() {
        macro_rules! test_from_str {
            ($($str:expr => $rust:expr,)*) => {
                $(
                    // Test parsing from string to rust
                    assert_eq!(Crate::from_str($str).unwrap(), $rust);

                    // Test dumping from rust to string
                    assert_eq!(&$rust.id(), $str);

                    // Test dumping from rust to string to rust
                    assert_eq!(Crate::from_str($rust.id().as_ref()).unwrap(), $rust);
                )*
            };
        }

        test_from_str! {
            "local/build-fail" => Crate::Local("build-fail".to_string()),
            "gh/org/user" => Crate::GitHub(GitHubRepo{org: "org".to_string(), name: "user".to_string(), sha: None}),
            "gh/org/user/sha" => Crate::GitHub(GitHubRepo{org: "org".to_string(), name: "user".to_string(), sha: Some("sha".to_string())}),
            "reg/name/version" => Crate::Registry(RegistryCrate{name: "name".to_string(), version: "version".to_string()}),
        }
    }
}

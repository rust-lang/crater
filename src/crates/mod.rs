pub(crate) mod lists;
mod sources;

use crate::dirs::LOCAL_CRATES_DIR;
use crate::prelude::*;
use cargo_metadata::PackageId;
use percent_encoding::{percent_decode_str, utf8_percent_encode, NON_ALPHANUMERIC};
use rustwide::Crate as RustwideCrate;
use std::fmt;
use std::path::Path;
use std::str::FromStr;

pub(crate) use crate::crates::sources::github::GitHubRepo;
pub(crate) use crate::crates::sources::registry::RegistryCrate;

#[derive(Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize, Clone)]
pub struct GitRepo {
    pub url: String,
    pub sha: Option<String>,
}

#[derive(Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize, Clone)]
pub enum Crate {
    Registry(RegistryCrate),
    GitHub(GitHubRepo),
    Local(String),
    Path(String),
    Git(GitRepo),
}

impl Crate {
    pub(crate) fn id(&self) -> String {
        match *self {
            Crate::Registry(ref details) => format!("reg/{}/{}", details.name, details.version),
            Crate::GitHub(ref repo) => {
                if let Some(ref sha) = repo.sha {
                    format!("gh/{}/{}/{sha}", repo.org, repo.name)
                } else {
                    format!("gh/{}/{}", repo.org, repo.name)
                }
            }
            Crate::Local(ref name) => format!("local/{name}"),
            Crate::Path(ref path) => {
                format!("path/{}", utf8_percent_encode(path, NON_ALPHANUMERIC))
            }
            Crate::Git(ref repo) => {
                if let Some(ref sha) = repo.sha {
                    format!(
                        "git/{}/{}",
                        utf8_percent_encode(&repo.url, NON_ALPHANUMERIC),
                        sha
                    )
                } else {
                    format!("git/{}", utf8_percent_encode(&repo.url, NON_ALPHANUMERIC),)
                }
            }
        }
    }

    pub(crate) fn to_rustwide(&self) -> RustwideCrate {
        match self {
            Self::Registry(krate) => RustwideCrate::crates_io(&krate.name, &krate.version),
            Self::GitHub(repo) => {
                RustwideCrate::git(&format!("https://github.com/{}/{}", repo.org, repo.name))
            }
            Self::Local(name) => RustwideCrate::local(&LOCAL_CRATES_DIR.join(name)),
            Self::Path(path) => RustwideCrate::local(Path::new(&path)),
            Self::Git(repo) => RustwideCrate::git(&repo.url),
        }
    }
}

impl TryFrom<&'_ PackageId> for Crate {
    type Error = anyhow::Error;

    fn try_from(pkgid: &PackageId) -> Fallible<Crate> {
        if pkgid.repr.contains(|c: char| c.is_ascii_whitespace()) {
            let parts = &pkgid
                .repr
                .split_ascii_whitespace()
                .flat_map(|s| {
                    // remove ()
                    s.trim_matches(|c: char| c.is_ascii_punctuation())
                        // split resource and protocol
                        .split('+')
                })
                .collect::<Vec<_>>();

            match parts[..] {
                [name, version, "registry", _] => Ok(Crate::Registry(RegistryCrate {
                    name: name.to_string(),
                    version: version.to_string(),
                })),
                [_, _, "path", path] => Ok(Crate::Path(path.to_string())),
                [_, _, "git", repo] => {
                    if repo.starts_with("https://github.com") {
                        Ok(Crate::GitHub(repo.replace('#', "/").parse()?))
                    } else {
                        let mut parts = repo.split('#').rev().collect::<Vec<_>>();
                        let url = parts.pop();
                        let sha = parts.pop();

                        match (url, sha) {
                            (Some(url), None) => Ok(Crate::Git(GitRepo {
                                url: url.to_string(),
                                sha: None,
                            })),
                            (Some(url), Some(sha)) => Ok(Crate::Git(GitRepo {
                                // remove additional queries if the sha is present
                                // as the crate version is already uniquely determined
                                url: url.split('?').next().unwrap().to_string(),
                                sha: Some(sha.to_string()),
                            })),
                            _ => bail!("malformed git repo: {}", repo),
                        }
                    }
                }
                _ => bail!(
                    "malformed pkgid format: {}\n maybe the representation has changed?",
                    pkgid.repr
                ),
            }
        } else {
            use cargo_util_schemas::core::*;

            let package_id = PackageIdSpec::parse(&pkgid.repr)?;

            match package_id.kind() {
                Some(SourceKind::LocalRegistry) => Ok(Crate::Local(package_id.name().to_string())),
                Some(SourceKind::Git(rev)) => {
                    if let Some(url) = package_id.url() {
                        if url.domain() == Some("github.com") {
                            if let Some(mut path) = url.path_segments() {
                                let Some(org) = path.next() else {
                                    bail!("Github URL path is too short")
                                };

                                let Some(repo_name) = path.next() else {
                                    bail!("Github URL path is too short")
                                };

                                Ok(Crate::GitHub(GitHubRepo {
                                    org: org.to_string(),
                                    name: repo_name.to_string(),
                                    sha: match rev {
                                        GitReference::Rev(rev)
                                            if rev.chars().all(|c| c.is_ascii_hexdigit()) =>
                                        {
                                            Some(rev.to_string())
                                        }
                                        _ => None,
                                    },
                                }))
                            } else {
                                bail!("Github Git URL doesn't have a valid path")
                            }
                        } else {
                            Ok(Crate::Git(GitRepo {
                                url: url.to_string(),
                                sha: match rev {
                                    GitReference::Rev(rev)
                                        if rev.chars().all(|c| c.is_ascii_hexdigit()) =>
                                    {
                                        Some(rev.to_string())
                                    }
                                    _ => None,
                                },
                            }))
                        }
                    } else {
                        bail!("Package Id with SourceKind Git should have a URL")
                    }
                }
                Some(SourceKind::Path) => {
                    if let Some(url) = package_id.url() {
                        Ok(Crate::Path(url.path().to_string()))
                    } else {
                        bail!("Package Id with SourceKind Path should have a URL")
                    }
                }
                Some(SourceKind::Registry | SourceKind::SparseRegistry) => {
                    Ok(Crate::Registry(RegistryCrate {
                        name: package_id.name().to_string(),
                        version: package_id
                            .version()
                            .ok_or_else(|| anyhow!("missing version for registry crate"))?
                            .to_string(),
                    }))
                }
                Some(SourceKind::Directory) => {
                    bail!("Unsupported SourceKind Directory")
                }
                None => match package_id.url() {
                    None => Ok(Crate::Registry(RegistryCrate {
                        name: package_id.name().to_string(),
                        version: package_id
                            .version()
                            .ok_or_else(|| anyhow!("missing version for registry crate"))?
                            .to_string(),
                    })),
                    Some(url) => match url.scheme() {
                        "http" | "https" | "git" | "ssh" => {
                            if url.domain() == Some("github.com") {
                                if let Some(mut path) = url.path_segments() {
                                    let Some(org) = path.next() else {
                                        bail!("Github URL path is too short")
                                    };

                                    let Some(repo_name) = path.next() else {
                                        bail!("Github URL path is too short")
                                    };

                                    Ok(Crate::GitHub(GitHubRepo {
                                        org: org.to_string(),
                                        name: repo_name.to_string(),
                                        sha: None,
                                    }))
                                } else {
                                    bail!("Github Git URL doesn't have a valid path")
                                }
                            } else {
                                Ok(Crate::Git(GitRepo {
                                    url: url.to_string(),
                                    sha: None,
                                }))
                            }
                        }
                        "file" => Ok(Crate::Path(url.path().to_string())),
                        other => {
                            bail!(format!("Unsuported Protocol: {other}"))
                        }
                    },
                },
            }
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
                Crate::GitHub(ref repo) =>
                    if let Some(ref sha) = repo.sha {
                        format!("{}/{}/{sha}", repo.org, repo.name)
                    } else {
                        format!("{}/{}", repo.org, repo.name)
                    },
                Crate::Local(ref name) => format!("{name} (local)"),
                Crate::Path(ref path) => format!("{}", utf8_percent_encode(path, NON_ALPHANUMERIC)),
                Crate::Git(ref repo) =>
                    if let Some(ref sha) = repo.sha {
                        format!(
                            "{}/{}",
                            utf8_percent_encode(&repo.url, NON_ALPHANUMERIC),
                            sha
                        )
                    } else {
                        utf8_percent_encode(&repo.url, NON_ALPHANUMERIC).to_string()
                    },
            }
        )
    }
}

impl FromStr for Crate {
    type Err = ::anyhow::Error;

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
            ["git", repo, sha] => Ok(Crate::Git(GitRepo {
                url: percent_decode_str(repo).decode_utf8()?.to_string(),
                sha: Some(sha.to_string()),
            })),
            ["git", repo] => Ok(Crate::Git(GitRepo {
                url: percent_decode_str(repo).decode_utf8()?.to_string(),
                sha: None,
            })),
            ["local", name] => Ok(Crate::Local(name.to_string())),
            ["path", path] => Ok(Crate::Path(
                percent_decode_str(path).decode_utf8()?.to_string(),
            )),
            _ => bail!("unexpected crate value"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Crate, GitHubRepo, GitRepo, RegistryCrate};
    use cargo_metadata::PackageId;
    use percent_encoding::{utf8_percent_encode, NON_ALPHANUMERIC};
    use std::convert::TryFrom;
    use std::str::FromStr;

    macro_rules! test_from_pkgid {
        ($($str:expr => $rust:expr,)*) => {
            $(
                let pkgid = PackageId {
                    repr: $str.to_string(),
                };

                assert_eq!(Crate::try_from(&pkgid).unwrap(), $rust);
            )*
        };
    }

    #[test]
    fn test_parse_from_pkgid() {
        test_from_pkgid! {
            "dummy 0.1.0 (path+file:///opt/rustwide/workdir)" => Crate::Path("file:///opt/rustwide/workdir".to_string()),
            "dummy 0.1.0 (registry+https://github.com/rust-lang/crates.io-index)" => Crate::Registry(RegistryCrate {
                name: "dummy".to_string(),
                version: "0.1.0".to_string()
            }),
            "dummy 0.1.0 (git+https://github.com/dummy_org/dummy#9823f01cf4948a41279f6a3febcf793130cab4f6)" => Crate::GitHub(GitHubRepo {
                org: "dummy_org".to_string(),
                name: "dummy".to_string(),
                sha: Some("9823f01cf4948a41279f6a3febcf793130cab4f6".to_string())
            }),
            "dummy 0.1.0 (git+https://github.com/dummy_org/dummy?rev=dummyrev#9823f01cf4948a41279f6a3febcf793130cab4f6)" => Crate::GitHub(GitHubRepo {
                org: "dummy_org".to_string(),
                name: "dummy".to_string(),
                sha: Some("9823f01cf4948a41279f6a3febcf793130cab4f6".to_string())
            }),
            "dummy 0.1.0 (git+https://github.com/dummy_org/dummy)" => Crate::GitHub(GitHubRepo {
                org: "dummy_org".to_string(),
                name: "dummy".to_string(),
                sha: None
            }),
            "dummy 0.1.0 (git+https://gitlab.com/dummy_org/dummy#9823f01cf4948a41279f6a3febcf793130cab4f6)" => Crate::Git(GitRepo {
                url: "https://gitlab.com/dummy_org/dummy"
                    .to_string(),
                sha: Some("9823f01cf4948a41279f6a3febcf793130cab4f6".to_string())
            }),
            "dummy 0.1.0 (git+https://gitlab.com/dummy_org/dummy?branch=dummybranch#9823f01cf4948a41279f6a3febcf793130cab4f6)" => Crate::Git(GitRepo {
                url: "https://gitlab.com/dummy_org/dummy"
                    .to_string(),
                sha: Some("9823f01cf4948a41279f6a3febcf793130cab4f6".to_string())
            }),
            "dummy 0.1.0 (git+https://gitlab.com/dummy_org/dummy)" => Crate::Git(GitRepo {
                url: "https://gitlab.com/dummy_org/dummy"
                    .to_string(),
                sha: None
            }),
            "dummy 0.1.0 (git+https://gitlab.com/dummy_org/dummy?branch=dummybranch)" => Crate::Git(GitRepo {
                url: "https://gitlab.com/dummy_org/dummy?branch=dummybranch"
                    .to_string(),
                sha: None
            }),

            // package id spec <https://doc.rust-lang.org/cargo/reference/pkgid-spec.html>

            "registry+https://github.com/rust-lang/crates.io-index#cookie@0.15.0" => Crate::Registry(RegistryCrate {
                name: "cookie".to_string(),
                version: "0.15.0".to_string(),
            }),
            "sparse+https://github.com/rust-lang/crates.io-index#cookie@0.15.0" => Crate::Registry(RegistryCrate {
                name: "cookie".to_string(),
                version: "0.15.0".to_string(),
            }),
            "regex@1.4.3" => Crate::Registry(RegistryCrate {
                name: "regex".to_string(),
                version: "1.4.3".to_string(),
            }),

            "https://github.com/rust-lang/cargo#0.52.0" => Crate::GitHub(GitHubRepo {
                org: "rust-lang".to_string(),
                name: "cargo".to_string(),
                sha: None
            }),
            "https://github.com/rust-lang/cargo#cargo-platform@0.1.2" => Crate::GitHub(GitHubRepo {
                org: "rust-lang".to_string(),
                name: "cargo".to_string(), // repo name not crate name
                sha: None
            }),
            "ssh://git@github.com/rust-lang/regex.git#regex@1.4.3" => Crate::GitHub(GitHubRepo {
                org: "rust-lang".to_string(),
                name: "regex".to_string(),
                sha: None
            }),
            "git+ssh://git@github.com/rust-lang/regex.git#regex@1.4.3" => Crate::GitHub(GitHubRepo {
                org: "rust-lang".to_string(),
                name: "regex".to_string(),
                sha: None
            }),
            "git+ssh://git@github.com/rust-lang/regex.git?branch=dev#regex@1.4.3" => Crate::GitHub(GitHubRepo {
                org: "rust-lang".to_string(),
                name: "regex".to_string(),
                sha: None
            }),

            "git+https://gitlab.com/dummy_org/dummy?rev=9823f01cf4948a41279f6a3febcf793130cab4f6" => Crate::Git(GitRepo {
                url: "https://gitlab.com/dummy_org/dummy"
                    .to_string(),
                sha: Some("9823f01cf4948a41279f6a3febcf793130cab4f6".to_string())
            }),

            "file:///path/to/my/project/foo" => Crate::Path("/path/to/my/project/foo".to_string()),
            "file:///path/to/my/project/foo#1.1.8" => Crate::Path("/path/to/my/project/foo".to_string()),
            "path+file:///path/to/my/project/foo#1.1.8" => Crate::Path("/path/to/my/project/foo".to_string()),
        }

        // while `invalid` is a valid package name it is missing a version
        assert!(Crate::try_from(&PackageId {
            repr: "invalid".to_string()
        })
        .is_err());
    }

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
            "path/pathtofile" => Crate::Path("pathtofile".to_string()),
            &format!("path/{}", utf8_percent_encode("path/with:stange?characters", NON_ALPHANUMERIC)) => Crate::Path("path/with:stange?characters".to_string()),
            "gh/org/user" => Crate::GitHub(GitHubRepo{org: "org".to_string(), name: "user".to_string(), sha: None}),
            "gh/org/user/sha" => Crate::GitHub(GitHubRepo{org: "org".to_string(), name: "user".to_string(), sha: Some("sha".to_string())}),
            "git/url" => Crate::Git(GitRepo{url: "url".to_string(), sha: None}),
            &format!("git/{}", utf8_percent_encode("url/with:stange?characters", NON_ALPHANUMERIC)) => Crate::Git(GitRepo{url: "url/with:stange?characters".to_string(), sha: None}),
            "git/url/sha" => Crate::Git(GitRepo{url: "url".to_string(), sha: Some("sha".to_string())}),
            "reg/name/version" => Crate::Registry(RegistryCrate{name: "name".to_string(), version: "version".to_string()}),
        }
    }
}

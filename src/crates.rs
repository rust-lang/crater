use dl;
use errors::*;
use ex::ExCrate;
use flate2::read::GzDecoder;
use gh_mirrors;
use std::fmt;
use std::fs;
use std::io::Read;
use std::path::Path;
use std::str::FromStr;
use std::thread;
use std::time::Duration;
use tar::Archive;
use util;

const CRATES_ROOT: &str = "https://crates-io.s3-us-west-1.amazonaws.com/crates";

#[derive(Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize, Clone)]
pub struct GitHubRepo {
    pub org: String,
    pub name: String,
}

impl GitHubRepo {
    pub fn slug(&self) -> String {
        format!("{}/{}", self.org, self.name)
    }

    pub fn url(&self) -> String {
        format!("https://github.com/{}/{}", self.org, self.name)
    }
}

impl FromStr for GitHubRepo {
    type Err = Error;

    fn from_str(input: &str) -> Result<Self> {
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

#[derive(Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize, Clone)]
pub struct RegistryCrate {
    pub name: String,
    pub version: String,
}

#[derive(Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize, Clone)]
pub enum Crate {
    Registry(RegistryCrate),
    GitHub(GitHubRepo),
}

impl Crate {
    pub fn registry(&self) -> Option<&RegistryCrate> {
        if let Crate::Registry(ref krate) = *self {
            Some(krate)
        } else {
            None
        }
    }

    pub fn github(&self) -> Option<&GitHubRepo> {
        if let Crate::GitHub(ref repo) = *self {
            Some(repo)
        } else {
            None
        }
    }

    pub fn into_ex_crate(self, ex: &::ex::Experiment) -> Result<::ex::ExCrate> {
        match self {
            Crate::Registry(krate) => Ok(::ex::ExCrate::Version {
                name: krate.name,
                version: krate.version,
            }),
            Crate::GitHub(repo) => if let Some(sha) = ex.shas.lock().unwrap().get(&repo.url()) {
                Ok(::ex::ExCrate::Repo {
                    org: repo.org,
                    name: repo.name,
                    sha: sha.to_string(),
                })
            } else {
                bail!("missing sha for GitHub repo {}", repo.slug());
            },
        }
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
            }
        )
    }
}

pub fn prepare(list: &[ExCrate]) -> Result<()> {
    info!("preparing {} crates", list.len());
    let mut successes = 0;
    for crate_ in list {
        let dir = crate_.dir();
        match *crate_ {
            ExCrate::Version {
                ref name,
                ref version,
            } => {
                let r = dl_registry(name, &version.to_string(), &dir)
                    .chain_err(|| format!("unable to download {}-{}", name, version));
                if let Err(e) = r {
                    util::report_error(&e);
                } else {
                    successes += 1;
                }
                // crates.io doesn't rate limit. Go fast
            }
            ExCrate::Repo {
                ref org,
                ref name,
                ref sha,
            } => {
                let url = format!("https://github.com/{}/{}", org, name);
                let r =
                    dl_repo(&url, &dir, sha).chain_err(|| format!("unable to download {}", url));
                if let Err(e) = r {
                    util::report_error(&e);
                } else {
                    successes += 1;
                }
                // delay to be nice to github
                thread::sleep(Duration::from_millis(100));
            }
        }
    }

    if successes < list.len() / 2 {
        bail!("unable to download a suspiciously-large number of crates");
    }

    Ok(())
}

fn dl_registry(name: &str, vers: &str, dir: &Path) -> Result<()> {
    if dir.exists() {
        info!(
            "crate {}-{} exists at {}. skipping",
            name,
            vers,
            dir.display()
        );
        return Ok(());
    }
    info!("downloading crate {}-{} to {}", name, vers, dir.display());
    let url = format!("{0}/{1}/{1}-{2}.crate", CRATES_ROOT, name, vers);
    let bin = dl::download(&url).chain_err(|| format!("unable to download {}", url))?;

    fs::create_dir_all(&dir)?;

    let mut tar = Archive::new(GzDecoder::new(bin));
    let r = unpack_without_first_dir(&mut tar, dir).chain_err(|| "unable to unpack crate tarball");

    if r.is_err() {
        let _ = util::remove_dir_all(dir);
    }

    r
}

fn dl_repo(url: &str, dir: &Path, sha: &str) -> Result<()> {
    info!("downloading repo {} to {}", url, dir.display());
    gh_mirrors::reset_to_sha(url, sha)?;
    let src_dir = gh_mirrors::repo_dir(url)?;
    util::copy_dir(&src_dir, dir)
}

fn unpack_without_first_dir<R: Read>(archive: &mut Archive<R>, path: &Path) -> Result<()> {
    let entries = archive.entries()?;
    for entry in entries {
        let mut entry = entry?;
        let relpath = {
            let path = entry.path();
            let path = path?;
            path.into_owned()
        };
        let mut components = relpath.components();
        // Throw away the first path component
        components.next();
        let full_path = path.join(&components.as_path());
        if let Some(parent) = full_path.parent() {
            fs::create_dir_all(parent)?;
        }
        entry.unpack(&full_path)?;
    }

    Ok(())
}

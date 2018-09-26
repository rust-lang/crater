pub(crate) mod lists;
mod sources;

use dirs::{CRATES_DIR, GH_MIRRORS_DIR};
use errors::*;
use flate2::read::GzDecoder;
use std::fmt;
use std::fs;
use std::io::Read;
use std::path::Path;
use std::path::PathBuf;
use std::str::FromStr;
use tar::Archive;

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

    pub fn mirror_dir(&self) -> PathBuf {
        GH_MIRRORS_DIR.join(format!("{}.{}", self.org, self.name))
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

    pub fn dir(&self) -> PathBuf {
        match *self {
            Crate::Registry(ref details) => CRATES_DIR
                .join("reg")
                .join(format!("{}-{}", details.name, details.version)),
            Crate::GitHub(ref repo) => CRATES_DIR
                .join("gh")
                .join(format!("{}.{}", repo.org, repo.name)),
        }
    }

    pub fn id(&self) -> String {
        match *self {
            Crate::Registry(ref details) => format!("reg/{}/{}", details.name, details.version),
            Crate::GitHub(ref repo) => format!("gh/{}/{}", repo.org, repo.name),
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

pub fn prepare_crate(krate: &Crate) -> Result<()> {
    let dir = krate.dir();
    match *krate {
        Crate::Registry(ref details) => {
            // crates.io doesn't rate limit. Go fast
            dl_registry(&details.name, &details.version, &dir)
                .chain_err(|| format!("unable to download {}", krate))?;
        }
        Crate::GitHub(ref repo) => {
            info!(
                "cloning GitHub repo {} to {}...",
                repo.slug(),
                dir.display()
            );
            ::utils::fs::copy_dir(&repo.mirror_dir(), &dir)?;
        }
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
    let bin = ::utils::http::get(&url).chain_err(|| format!("unable to download {}", url))?;

    fs::create_dir_all(&dir)?;

    let mut tar = Archive::new(GzDecoder::new(bin));
    let r = unpack_without_first_dir(&mut tar, dir).chain_err(|| "unable to unpack crate tarball");

    if r.is_err() {
        let _ = ::utils::fs::remove_dir_all(dir);
    }

    r
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

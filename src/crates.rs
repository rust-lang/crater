use std::path::{Path, PathBuf};
use CRATES_DIR;
use ex::ExCrate;
use std::thread;
use std::time::Duration;
use semver::Version;
use run;
use util;
use dl;
use std::fs;
use errors::*;
use git;
use tar::Archive;
use flate2::read::GzDecoder;
use std::io::Read;
use gh_mirrors;

const CRATES_ROOT: &'static str = "https://crates-io.s3-us-west-1.amazonaws.com/crates";

pub fn prepare(list: &[(ExCrate, PathBuf)]) -> Result<()> {
    log!("preparing {} crates", list.len());
    let mut successes = 0;
    for &(ref crate_, ref dir) in list {
        match *crate_ {
            ExCrate::Version { ref name, ref version } => {
                let r = dl_registry(name, &version.to_string(), dir)
                    .chain_err(|| format!("unable to download {}-{}", name, version));
                if let Err(e) = r {
                    util::report_error(&e);
                } else {
                    successes += 1;
                }
                // crates.io doesn't rate limit. Go fast
            }
            ExCrate::Repo { ref url, ref sha } => {
                let r = dl_repo(url, dir, sha)
                    .chain_err(|| format!("unable to download {}", url));
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
        log!("crate {}-{} exists at {}. skipping", name, vers, dir.display());
        return Ok(());
    }
    log!("downloading crate {}-{} to {}", name, vers, dir.display());
    let url = format!("{0}/{1}/{1}-{2}.crate", CRATES_ROOT, name, vers);
    let bin = dl::download(&url)
        .chain_err(|| format!("unable to download {}", url))?;

    fs::create_dir_all(&dir)?;

    let mut tar = Archive::new(GzDecoder::new(&*bin)?);
    let r = unpack_without_first_dir(&mut tar, dir)
        .chain_err(|| "unable to unpack crate tarball");

    if r.is_err() {
        let _ = util::remove_dir_all(dir);
    }

    r
}

fn dl_repo(url: &str, dir: &Path, sha: &str) -> Result<()> {
    log!("downloading repo {} to {}", url, dir.display());
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


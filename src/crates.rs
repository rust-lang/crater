use std::path::{Path, PathBuf};
use CRATES_DIR;
use lists::{self, Crate};
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
use TEST_DIR;

const CRATES_ROOT: &'static str = "https://crates-io.s3-us-west-1.amazonaws.com/crates";

fn gh_dir() -> PathBuf {
    Path::new(CRATES_DIR).join("gh")
}

fn registry_dir() -> PathBuf {
    Path::new(CRATES_DIR).join("reg")
}

pub fn prepare() -> Result<()> {
    let list = crates_and_dirs()?;
    prepare_(&list)
}

pub fn prepare_(list: &[(Crate, PathBuf)]) -> Result<()> {
    log!("preparing {} crates", list.len());
    let mut successes = 0;
    for &(ref crate_, ref dir) in list {
        match *crate_ {
            Crate::Version(ref name, ref vers) => {
                let r = dl_registry(name, &vers.to_string(), dir)
                    .chain_err(|| format!("unable to download {}-{}", name, vers));
                if let Err(e) = r {
                    util::report_error(&e);
                } else {
                    successes += 1;
                }
                // crates.io doesn't rate limit. Go fast
            }
            Crate::Repo(ref url) => {
                let r = dl_repo(url, dir)
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
        return Err("unable to download a suspiciously-large number of crates".into());
    }

    Ok(())
}

pub fn crate_dir(c: &Crate) -> Result<PathBuf> {
    match *c {
        Crate::Version(ref name, ref vers) => {
            Ok(registry_dir().join(format!("{}-{}", name, vers)))
        }
        Crate::Repo(ref url) => {
            let (org, name) = gh_url_to_org_and_name(url)?;
            Ok(gh_dir().join(format!("{}.{}", org, name)))
        }
    }
}

fn gh_url_to_org_and_name(url: &str) -> Result<(String, String)> {
    let mut components = url.split("/").collect::<Vec<_>>();
    let name = components.pop();
    let org = components.pop();
    let (org, name) = if let (Some(org), Some(name)) = (org, name) {
        (org, name)
    } else {
        let e = format!("malformed repo url: {}", url);
        return Err(e.into());
    };

    Ok((org.to_string(), name.to_string()))
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
    let r = unpack_without_first_dir(&mut tar, &dir)
        .chain_err(|| "unable to unpack crate tarball");

    if r.is_err() {
        let _ = util::remove_dir_all(&dir);
    }

    r
}

fn dl_repo(url: &str, dir: &Path) -> Result<()> {
    let (org, name) = gh_url_to_org_and_name(url)?;
    fs::create_dir_all(&gh_dir())?;
    log!("downloading repo {} to {}", url, dir.display());
    let r = git::shallow_clone_or_pull(url, &dir);

    if r.is_err() {
        let _ = util::remove_dir_all(&dir);
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

pub fn crates_and_dirs() -> Result<Vec<(Crate, PathBuf)>> {
    let cd = lists::read_all_lists()?.into_iter().filter_map(|c| {
        let dir = crate_dir(&c)
            .chain_err(|| format!("unable to get crate_dir for {}", c));
        if let Err(e) = dir {
            util::report_error(&e);
            None
        } else {
            Some((c, dir.expect("")))
        }
    });

    Ok(cd.collect())
}

pub fn with_work_crate<F, R>(crate_: &Crate, f: F) -> Result<R>
    where F: Fn(&Path) -> Result<R>
{
    let src_dir = crate_dir(crate_)?;
    let dest_dir = Path::new(TEST_DIR);
    log!("creating temporary build dir for {} in {}", crate_, dest_dir.display());

    copy_dir(&src_dir, &dest_dir)?;
    let r = f(&dest_dir);
    util::remove_dir_all(dest_dir)?;
    r
}

fn copy_dir(src_dir: &Path, dest_dir: &Path) -> Result<()> {
    use walkdir::*;

    if dest_dir.exists() {
        util::remove_dir_all(dest_dir)
            .chain_err(|| "unable to remove test dir")?;
    }
    fs::create_dir_all(dest_dir)
        .chain_err(|| "unable to create test dir")?;

    fn is_hidden(entry: &DirEntry) -> bool {
        entry.file_name()
            .to_str()
            .map(|s| s.starts_with("."))
            .unwrap_or(false)
    }

    let mut partial_dest_dir = PathBuf::from("./");
    let mut depth = 0;
    for entry in WalkDir::new(src_dir)
        .into_iter()
        .filter_entry(|e| !is_hidden(e))
    {
        let entry = entry.chain_err(|| "walk dir")?;
        while entry.depth() <= depth && depth > 0 {
            assert!(partial_dest_dir.pop());
            depth -= 1;
        }
        let path = dest_dir.join(&partial_dest_dir).join(entry.file_name());
        if entry.file_type().is_dir() && entry.depth() > 0 {
            fs::create_dir_all(&path)?;
            assert!(entry.depth() == depth + 1);
            partial_dest_dir.push(entry.file_name());
            depth += 1;
        }
        if entry.file_type().is_file() {
            fs::copy(&entry.path(), path)?;
        }
    }

    Ok(())
}

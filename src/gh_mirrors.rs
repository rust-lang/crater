use dirs::GH_MIRRORS_DIR;
use errors::*;
use git;
use std::path::PathBuf;

pub fn repo_dir(url: &str) -> Result<PathBuf> {
    let (org, name) = gh_url_to_org_and_name(url)?;
    Ok(GH_MIRRORS_DIR.join(format!("{}.{}", org, name)))
}

fn gh_url_to_org_and_name(url: &str) -> Result<(&str, &str)> {
    let mut components = url.split('/').collect::<Vec<_>>();
    let name = components.pop();
    let org = components.pop();

    if let (Some(org), Some(name)) = (org, name) {
        Ok((org, name))
    } else {
        bail!("malformed repo url: {}", url);
    }
}

pub fn fetch(url: &str) -> Result<()> {
    let dir = repo_dir(url)?;
    git::shallow_clone_or_pull(url, &dir)
}

pub fn reset_to_sha(url: &str, sha: &str) -> Result<()> {
    let dir = &repo_dir(url)?;
    git::shallow_fetch_sha(url, dir, sha)?;
    git::reset_to_sha(dir, sha)
}

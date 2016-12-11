use git;
use errors::*;
use std::path::{Path, PathBuf};
use GH_MIRRORS_DIR;
use crates;

pub fn repo_dir(url: &str) -> Result<PathBuf> {
    let (org, name) = gh_url_to_org_and_name(url)?;
    Ok(Path::new(GH_MIRRORS_DIR).join(format!("{}.{}", org, name)))
}

pub fn gh_url_to_org_and_name(url: &str) -> Result<(String, String)> {
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

pub fn fetch(url: &str) -> Result<()> {
    let dir = repo_dir(url)?;
    git::shallow_clone_or_pull(url, &dir)
}

use LOCAL_DIR;
use errors::*;
use git;
use serde_json;
use std::fs::{self, File};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

const REGISTRY: &'static str = "https://github.com/rust-lang/crates.io-index.git";

pub struct Crate {
    pub name: String,
    pub versions: Vec<(String, Vec<Dep>)>,
}

pub type Dep = (String, String);

pub fn find_registry_crates() -> Result<Vec<Crate>> {
    fs::create_dir_all(LOCAL_DIR)?;
    update_registry()?;
    info!("loading registry");
    let r = read_registry()?;
    info!("registry loaded");
    Ok(r)
}

fn update_registry() -> Result<()> {
    git::shallow_clone_or_pull(REGISTRY, &repo_path()).chain_err(|| "unable to update registry")
}

fn repo_path() -> PathBuf {
    Path::new(LOCAL_DIR).join("crates.io-index")
}

fn read_registry() -> Result<Vec<Crate>> {
    use walkdir::*;

    fn is_hidden(entry: &DirEntry) -> bool {
        entry
            .file_name()
            .to_str()
            .map(|s| s.starts_with('.'))
            .unwrap_or(false)
    }

    let mut crates = Vec::new();

    for entry in WalkDir::new(&repo_path())
            .into_iter()
            .filter_entry(|e| !is_hidden(e)) {
        let entry = entry.chain_err(|| "walk dir")?;
        if !entry.file_type().is_file() {
            continue;
        }
        if entry.file_name() == "config.json" {
            continue;
        }

        crates.push(read_crate(entry.path())?);
    }

    Ok(crates)
}

/// Structure of a crate in https://github.com/rust-lang/crates.io-index
#[derive(Deserialize)]
pub struct IndexCrate {
    name: String,
    vers: String,
    deps: Vec<IndexCrateDependency>,
}

/// Structure of a crate dependency in https://github.com/rust-lang/crates.io-index
#[derive(Deserialize)]
pub struct IndexCrateDependency {
    name: String,
    req: String,
}

fn read_crate(path: &Path) -> Result<Crate> {
    let mut crate_name = String::new();
    let mut crate_versions = Vec::new();
    let file = BufReader::new(File::open(path)?);
    for line in file.lines() {
        let line = &line?;
        let crate_: IndexCrate = serde_json::from_str(line).chain_err(|| "parsing json")?;
        let deps = crate_
            .deps
            .into_iter()
            .map(|d| (d.name, d.req))
            .collect::<Vec<_>>();
        crate_name = crate_.name;
        crate_versions.push((crate_.vers, deps));
    }

    Ok(Crate {
           name: crate_name,
           versions: crate_versions,
       })
}

use errors::*;
use DISCO_DIR;
use std::fs::{self, File};
use std::thread;
use std::process::Command;
use std::path::{Path, PathBuf};
use std::time::Duration;
use std::io::{BufReader, Read, BufRead};
use util;

const REGISTRY: &'static str = "https://github.com/rust-lang/crates.io-index.git";

pub struct Crate {
    pub name: String,
    pub versions: Vec<(String, Vec<Dep>)>
}

pub type Dep = (String, String);

pub fn find_registry_crates() -> Result<Vec<Crate>> {
    fs::create_dir_all(DISCO_DIR)?;
    update_registry()?;
    log!("loading registry");
    let r = read_registry()?;
    log!("registry loaded");
    Ok(r)
}

fn update_registry() -> Result<()> {
    util::try_hard(|| {
        let repo_path = repo_path();
        if !repo_path.exists() {
            log!("cloning registry");
            let status = Command::new("git")
                .arg("clone")
                .arg("--depth").arg("1")
                .arg(REGISTRY)
                .arg(&*repo_path.to_string_lossy())
                .status()
                .chain_err(|| "unable to run git clone")?;

            if !status.success() {
                return Err("unable to clone registry".into())
            }
        } else {
            log!("updating registry");
            let status = Command::new("git")
                .arg("pull")
                .current_dir(&repo_path)
                .status()
                .chain_err(|| "unable to run git pull")?;

            if !status.success() {
                return Err("unable to update registry".into())
            }
        }

        Ok(())
    })
}

fn repo_path() -> PathBuf {
    Path::new(DISCO_DIR).join("crates.io-index")
}

fn read_registry() -> Result<Vec<Crate>> {
    use walkdir::*;

    fn is_hidden(entry: &DirEntry) -> bool {
        entry.file_name()
            .to_str()
            .map(|s| s.starts_with("."))
            .unwrap_or(false)
    }

    let mut crates = Vec::new();

    for entry in WalkDir::new(&repo_path())
        .into_iter()
        .filter_entry(|e| !is_hidden(e))
    {
        let entry = entry.chain_err(|| "walk dir")?;
        if !entry.file_type().is_file() { continue }
        if entry.file_name() == "config.json" { continue }

        crates.push(read_crate(entry.path())?);
    }

    Ok(crates)
}

fn read_crate(path: &Path) -> Result<Crate> {
    use json;
    use json::*;

    let mut crate_name = String::new();
    let mut crate_versions = Vec::new();
    let mut file = BufReader::new(File::open(path)?);
    for line in file.lines() {
        let ref line = line?;
        let json = json::parse(line).chain_err(|| "parsing json")?;
        let mut deps = Vec::new();
        let name = json["name"].as_str();
        let vers = json["vers"].as_str();
        for json in json["deps"].members() {
            let dep_name = json["name"].as_str();
            let dep_req = json["req"].as_str();
            match (dep_name, dep_req) {
                (Some(n), Some(r)) => {
                    deps.push((n.to_string(), r.to_string()));
                }
                _ => ()
            }
        }
        match (name, vers) {
            (Some(n), Some(v)) => {
                crate_name = n.to_string();
                crate_versions.push((v.to_string(), deps));
            }
            _ => ()
        }
    }

    Ok(Crate {
        name: crate_name,
        versions: crate_versions,
    })
}

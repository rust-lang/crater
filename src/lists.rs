use std::fs;
use errors::*;
use registry;
use LIST_DIR;
use file;
use std::path::{Path, PathBuf};
use std::collections::{HashMap, HashSet};
use gh;
use std::thread;
use std::time::Duration;
use semver::{Version, VersionReq};
use std::fmt::{self, Formatter, Display};

fn recent_path() -> PathBuf {
    Path::new(LIST_DIR).join("recent-crates.txt")
}

pub fn create_recent_list() -> Result<()> {
    log!("creating recent list");
    fs::create_dir_all(LIST_DIR)?;

    let crates = registry::find_registry_crates()?;
    let crates: Vec<_> = crates.into_iter().map(|mut crate_| {
        (crate_.name, crate_.versions.pop().expect("").0)
    }).collect();
    write_crate_list(&recent_path(), &crates)?;
    log!("recent crates written to {}", recent_path().display());
    Ok(())
}

pub fn read_recent_list() -> Result<Vec<(String, String)>> {
    let lines = file::read_lines(&recent_path())
        .chain_err(|| "unable to read recent list. run `cargobomb create-recent-list`?")?;
    split_crate_lines(&lines)
}

fn second_path() -> PathBuf {
    Path::new(LIST_DIR).join("second-crates.txt")
}

pub fn create_second_list() -> Result<()> {
    log!("creating second list");
    fs::create_dir_all(LIST_DIR)?;

    let crates = registry::find_registry_crates()?;
    let crates: Vec<_> = crates.into_iter().filter_map(|mut crate_| {
        crate_.versions.pop();
        crate_.versions.pop().map(move |v| (crate_.name, v.0))
    }).collect();
    write_crate_list(&second_path(), &crates)?;
    log!("second crates written to {}", second_path().display());
    Ok(())
}

pub fn read_second_list() -> Result<Vec<(String, String)>> {
    let lines = file::read_lines(&second_path())
        .chain_err(|| "unable to read second list. run `cargobomb create-second-list`?")?;
    split_crate_lines(&lines)
}

fn write_crate_list(path: &Path, crates: &[(String, String)]) -> Result<()> {
    let strings = crates.iter()
        .map(|&(ref name, ref version)| format!("{}:{}", name, version))
        .collect::<Vec<_>>();
    file::write_lines(path, &strings)
}

fn split_crate_lines(lines: &[String]) -> Result<Vec<(String, String)>> {
    Ok(lines.iter().filter_map(|line| {
        line.find(":").map(|i| {
            (line[..i].to_string(), line[i + 1..].to_string())
        })
    }).collect())
}

fn hot_path() -> PathBuf {
    Path::new(LIST_DIR).join("hot-crates.txt")
}

pub fn create_hot_list() -> Result<()> {
    log!("creating hot list");
    fs::create_dir_all(LIST_DIR)?;

    let crates = registry::find_registry_crates()?;

    // We're going to map reverse dependency counts of all crate versions.

    // Create a map from name to versions, recent to oldest
    let mut crate_map = HashMap::new();
    for crate_ in &crates {
        let name = &crate_.name;
        let versions = &crate_.versions;
        let versions: Vec<_> = versions.iter().rev().take(10)
            .map(|v| (v.0.to_string(), 0)).collect();
        crate_map.insert(name.to_string(), versions);
    }

    log!("mapping reverse deps");
    // For each crate's dependency mark which revisions of the dep satisfy
    // semver
    for crate_ in &crates {
        for &(_, ref deps) in &crate_.versions {
            for &(ref name, ref req) in deps {
                if let Some(ref mut dep_versions) = crate_map.get_mut(&*name) {
                    let semver_req = VersionReq::parse(req);
                    for &mut (ref rev, ref mut count) in dep_versions.iter_mut() {
                        let semver_rev = Version::parse(rev);
                        match (&semver_req, semver_rev) {
                            (&Ok(ref req), Ok(ref rev)) => {
                                if req.matches(&rev) {
                                    *count += 1;
                                }
                            }
                            _ => ()
                        }
                    }
                }
            }
        }
    }

    log!("calculating most popular crate versions");
    // Take the version of each crate that satisfies the most rev deps
    let mut hot_crates = Vec::new();
    for crate_ in &crates {
        if let Some(dep_versions) = crate_map.get(&crate_.name) {
            let mut best_version = String::new();
            let mut max_rev_deps = 0;
            for version in dep_versions {
                // Only pick versions that have more than 0 rev deps,
                // and prefer newer revisions (earlier in the list).
                if version.1 > max_rev_deps {
                    best_version = version.0.to_string();
                    max_rev_deps = version.1;
                }
            }
            if !best_version.is_empty() {
                hot_crates.push((crate_.name.to_string(), best_version));
            }
        }
    }

    write_crate_list(&hot_path(), &hot_crates)?;
    log!("hot crates written to {}", hot_path().display());
    Ok(())
}

pub fn read_hot_list() -> Result<Vec<(String, String)>> {
    let lines = file::read_lines(&hot_path())
        .chain_err(|| "unable to read hot list. run `cargobomb create-hot-list`?")?;
    split_crate_lines(&lines)
}

fn gh_candidate_path() -> PathBuf {
    Path::new(LIST_DIR).join("gh-candidates.txt")
}

pub fn create_gh_candidate_list() -> Result<()> {
    log!("creating recent list");
    fs::create_dir_all(LIST_DIR)?;

    let candidates = gh::get_candidate_repos()?;
    file::write_lines(&gh_candidate_path(), &candidates)?;
    log!("candidate repos written to {}", gh_candidate_path().display());
    Ok(())
}

pub fn read_gh_candidates_list() -> Result<Vec<String>> {
    file::read_lines(&gh_candidate_path())
        .chain_err(|| "unable to read gh-candidates list. run `cargobomb create-gh-candidates-list`?")
}

fn gh_app_path() -> PathBuf {
    Path::new(LIST_DIR).join("gh-apps.txt")
}

pub fn create_gh_app_list() -> Result<()> {
    let repos = read_gh_candidates_list()?;
    let delay = 100;

    log!("testing {} repos. {}ms+", repos.len(), repos.len() * delay);

    // Look for Cargo.lock files in the Rust repos we're aware of
    let mut apps = Vec::new();
    for repo in repos {
        if gh::is_rust_app(&repo)? {
            apps.push(format!("https://github.com/{}", repo));
        }
        thread::sleep(Duration::from_millis(delay as u64));
    }

    file::write_lines(&gh_app_path(), &apps)?;
    log!("rust apps written to {}", gh_app_path().display());
    Ok(())
}

pub fn read_gh_app_list() -> Result<Vec<String>> {
    file::read_lines(&gh_app_path())
        .chain_err(|| "unable to read gh-app list. run `cargobomb create-gh-app-list`?")
}

#[derive(Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum Crate {
    Version(String, Version),
    Repo(String)
}

impl Display for Crate {
    fn fmt(&self, f: &mut Formatter) -> ::std::result::Result<(), fmt::Error> {
        let s = match *self {
            Crate::Version(ref n, ref v) => format!("{}-{}", n, v),
            Crate::Repo(ref u) => u.to_string(),
        };
        s.fmt(f)
    }
}

pub fn read_all_lists() -> Result<Vec<Crate>> {
    let mut all = HashSet::new();
    let recent = read_recent_list();
    let second = read_second_list();
    let hot = read_hot_list();
    let gh_apps = read_gh_app_list();

    fn semverify(v: Result<Vec<(String, String)>>) -> Result<Vec<(String, Version)>> {
        v.map(|r| r.into_iter().map(|(c, v)| {
            let ref bogus = format!("bogus version {} for {}", v, c);
            let v = Version::parse(&v).expect(bogus);
            (c, v)
        }).collect())
    }
    
    let recent = semverify(recent);
    let second = semverify(second);
    let hot = semverify(hot);

    if let Ok(recent) = recent {
        all.extend(recent.into_iter().map(|(c, v)| Crate::Version(c, v)));
    } else {
        log!("failed to load recent list. ignoring");
    }
    if let Ok(second) = second { 
       all.extend(second.into_iter().map(|(c, v)| Crate::Version(c, v)));
    } else {
        log!("failed to load second list. ignoring");
    }
    if let Ok(hot) = hot {
        all.extend(hot.into_iter().map(|(c, v)| Crate::Version(c, v)));
    } else {
        log!("failed to load hot list. ignoring");
    }
    if let Ok(gh_apps) = gh_apps {
        all.extend(gh_apps.into_iter().map(|c| Crate::Repo(c)));
    } else {
        log!("failed to load gh-app list. ignoring");
    }

    if all.is_empty() {
        return Err("no crates loaded. run `cargobomb prepare-lists`?".into());
    }

    let mut all: Vec<_> = all.drain().collect();
    all.sort();
    Ok(all)
}

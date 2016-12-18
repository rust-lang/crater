use gh_mirrors;
use std::time::Instant;
use RUSTUP_HOME;
use CARGO_HOME;
use std::env;
use std::fs;
use errors::*;
use EXPERIMENT_DIR;
use std::path::{Path, PathBuf};
use crates;
use lists::{self, Crate};
use run;
use std::collections::{HashMap, HashSet};
use serde_json;
use file;
use toolchain::{self, Toolchain};
use util;
use std::fmt::{self, Formatter, Display};
use log;
use toml_frobber;
use TEST_SOURCE_DIR;

pub fn ex_dir(ex_name: &str) -> PathBuf {
    Path::new(EXPERIMENT_DIR).join(ex_name)
}

fn shafile(ex_name: &str) -> PathBuf {
    Path::new(EXPERIMENT_DIR).join(ex_name).join("shas.json")
}

fn config_file(ex_name: &str) -> PathBuf {
    Path::new(EXPERIMENT_DIR).join(ex_name).join("config.json")
}

fn froml_dir(ex_name: &str) -> PathBuf {
    Path::new(EXPERIMENT_DIR).join(ex_name).join("fromls")
}

fn froml_path(ex_name: &str, name: &str, vers: &str) -> PathBuf {
    froml_dir(ex_name).join(format!("{}-{}.Cargo.toml", name, vers))
}

#[derive(Serialize, Deserialize)]
pub struct Experiment {
    pub name: String,
    pub crates: Vec<Crate>,
    pub toolchains: Vec<Toolchain>,
    pub mode: ExMode,
}

#[derive(Serialize, Deserialize)]
pub enum ExMode {
    BuildAndTest,
    BuildOnly,
    CheckOnly,
    UnstableFeatures
}

pub struct ExOpts {
    pub name: String,
    pub toolchains: Vec<Toolchain>,
    pub mode: ExMode,
    pub crates: ExCrateSelect
}

pub enum ExCrateSelect {
    Default,
    Demo,
}

pub fn define(opts: ExOpts) -> Result<()> {
    let crates = match opts.crates {
        ExCrateSelect::Default => lists::read_all_lists()?,
        ExCrateSelect::Demo => demo_list()?,
    };
    define_(&opts.name, opts.toolchains, crates, opts.mode)
}

fn demo_list() -> Result<Vec<Crate>> {
    let demo_crate = "lazy_static";
    let demo_gh_app = "brson/hello-rs";
    let mut found_demo_crate = false;
    let crates = lists::read_all_lists()?.into_iter().filter(|c| {
        match *c {
            Crate::Version(ref c, _) => {
                if c == demo_crate && !found_demo_crate {
                    found_demo_crate = true;
                    true
                } else {
                    false
                }
            }
            Crate::Repo(ref r) => {
                r.contains(demo_gh_app)
            }
        }
    }).collect::<Vec<_>>();
    assert!(crates.len() == 2);

    Ok(crates)
}

pub fn define_(ex_name: &str, tcs: Vec<Toolchain>,
               crates: Vec<Crate>, mode: ExMode) -> Result<()> {
    log!("defining experiment {} for {} crates", ex_name, crates.len());
    let ex = Experiment {
        name: ex_name.to_string(),
        crates: crates,
        toolchains: tcs,
        mode: mode,
    };
    fs::create_dir_all(&ex_dir(ex_name))?;
    let json = serde_json::to_string(&ex)?;
    log!("writing ex config to {}", config_file(ex_name).display());
    file::write_string(&config_file(ex_name), &json)?;
    Ok(())
}

pub fn load_config(ex_name: &str) -> Result<Experiment> {
    let config = file::read_string(&config_file(ex_name))?;
    Ok(serde_json::from_str(&config)?)
}

pub fn fetch_gh_mirrors(ex_name: &str) -> Result<()> {
    let config = load_config(ex_name)?;
    for c in &config.crates {
        match *c {
            Crate::Repo(ref url) => {
                if let Err(e) = gh_mirrors::fetch(url) {
                    util::report_error(&e);
                }
            }
            _ => ()
        }
    }

    Ok(())
}

pub fn capture_shas(ex_name: &str) -> Result<()> {
    let mut shas: HashMap<String, String> = HashMap::new();
    let config = load_config(ex_name)?;
    for krate in config.crates {
        match krate {
            Crate::Repo(url) => {
                let dir = gh_mirrors::repo_dir(&url)?;
                let r = run::run_capture(Some(&dir),
                                         "git",
                                         &["log", "-n1", "--pretty=%H"],
                                         &[]);

                match r {
                    Ok((stdout, stderr)) => {
                        if let Some(shaline) = stdout.get(0) {
                            if shaline.len() > 0 {
                                log!("sha for {}: {}", url, shaline);
                                shas.insert(url, shaline.to_string());
                            } else {
                                log_err!("bogus output from git log for {}", dir.display());
                            }
                        } else {
                            log_err!("bogus output from git log for {}", dir.display());
                        }
                    }
                    Err(e) => {
                        log_err!("unable to capture sha for {}: {}", dir.display(), e);
                    }
                }
            }
            _ => ()
        }
    }

    fs::create_dir_all(&ex_dir(ex_name))?;
    let shajson = serde_json::to_string(&shas)?;
    log!("writing shas to {}", shafile(ex_name).display());
    file::write_string(&shafile(ex_name), &shajson)?;

    Ok(())
}

fn load_shas(ex_name: &str) -> Result<HashMap<String, String>> {
    let shas = file::read_string(&shafile(ex_name))?;
    let shas = serde_json::from_str(&shas)?;
    Ok(shas)
}

#[derive(Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize, Clone)]
pub enum ExCrate {
    Version(String, String), // name, vers
    Repo(String, String) // url, sha
}

impl Display for ExCrate {
    fn fmt(&self, f: &mut Formatter) -> ::std::result::Result<(), fmt::Error> {
        let s = match *self {
            ExCrate::Version(ref n, ref v) => format!("{}-{}", n, v),
            ExCrate::Repo(ref u, ref s) => format!("{}#{}", u, s)
        };
        s.fmt(f)
    }
}

fn crate_to_ex_crate(c: Crate, shas: &HashMap<String, String>) -> Result<ExCrate> {
    match c {
        Crate::Version(n, v) => Ok(ExCrate::Version(n, v)),
        Crate::Repo(u) => {
            if let Some(sha) = shas.get(&u) {
                Ok(ExCrate::Repo(u, sha.to_string()))
            } else {
                Err(format!("missing sha for {}", u).into())
            }
        }
    }
}

fn ex_crate_to_crate(c: ExCrate) -> Result<Crate> {
    match c {
        ExCrate::Version(n, v) => Ok(Crate::Version(n, v)),
        ExCrate::Repo(u, _) => Ok(Crate::Repo(u))
    }
}

pub fn ex_crates_and_dirs(ex_name: &str) -> Result<Vec<(ExCrate, PathBuf)>> {
    let config = load_config(ex_name)?;
    let shas = load_shas(ex_name)?;
    let crates = config.crates.clone().into_iter().filter_map(|c| {
        let c = crate_to_ex_crate(c, &shas);
        if let Err(e) = c {
            util::report_error(&e);
            return None;
        }
        let c = c.expect("");
        let dir = crates::crate_dir(&c);
        if let Err(e) = dir {
            util::report_error(&e);
            return None;
        }
        let dir = dir.expect("");
        Some((c, dir))
    });
    Ok(crates.collect())
}

pub fn download_crates(ex_name: &str) -> Result<()> {
    crates::prepare(&ex_crates_and_dirs(ex_name)?)
}

pub fn frob_tomls(ex_name: &str) -> Result<()> {
    for (krate, dir) in ex_crates_and_dirs(ex_name)? {
        match krate {
            ExCrate::Version(ref name, ref vers) => {
                let out = froml_path(ex_name, name, vers);
                let r = toml_frobber::frob_toml(&dir, name, vers, &out);
                if let Err(e) = r {
                    log!("couldn't frob: {}", e);
                    util::report_error(&e);
                }
            }
            _ => ()
        }
    }

    Ok(())
}

pub fn with_frobbed_toml(ex_name: &str, crate_: &ExCrate, path: &Path) -> Result<()> {
    let (crate_name, crate_vers) = match *crate_ {
        ExCrate::Version(ref n, ref v) => (n.to_string(), v.to_string()),
        _ => return Ok(())
    };
    let ref src_froml = froml_path(ex_name, &crate_name, &crate_vers);
    let ref dst_froml = path.join("Cargo.toml");
    if src_froml.exists() {
        log!("using frobbed toml {}", src_froml.display());
        fs::copy(src_froml, dst_froml)
            .chain_err(|| format!("unable to copy frobbed toml from {} to {}",
                                  src_froml.display(), dst_froml.display()))?;
    }

    Ok(())
}

fn lockfile_dir(ex_name: &str) -> PathBuf {
    Path::new(EXPERIMENT_DIR).join(ex_name).join("lockfiles")
}

fn lockfile(ex_name: &str, crate_: &ExCrate) -> Result<PathBuf> {
    let (crate_name, crate_vers) = match *crate_ {
        ExCrate::Version(ref n, ref v) => (n.to_string(), v.to_string()),
        _ => bail!("unimplemented crate type in `lockfile`"),
    };
    Ok(lockfile_dir(ex_name).join(format!("{}-{}.lock", crate_name, crate_vers)))
}

fn crate_work_dir(ex_name: &str, toolchain: &str) -> PathBuf {
    Path::new(TEST_SOURCE_DIR).join(ex_name).join(toolchain)
}

pub fn with_work_crate<F, R>(ex_name: &str, toolchain: &str, crate_: &ExCrate, f: F) -> Result<R>
    where F: Fn(&Path) -> Result<R>
{
    let src_dir = crates::crate_dir(crate_)?;
    let dest_dir = crate_work_dir(ex_name, toolchain);
    log!("creating temporary build dir for {} in {}", crate_, dest_dir.display());

    util::copy_dir(&src_dir, &dest_dir)?;
    let r = f(&dest_dir);
    util::remove_dir_all(&dest_dir)?;
    r
}

pub fn capture_lockfiles(ex_name: &str, toolchain: &str, recapture_existing: bool) -> Result<()> {
    fs::create_dir_all(&lockfile_dir(ex_name))?;

    let crates = ex_crates_and_dirs(ex_name)?;

    for (ref c, ref dir) in crates {
        if dir.join("Cargo.lock").exists() {
            log!("crate {} has a lockfile. skipping", c);
            continue;
        }
        let captured_lockfile = lockfile(ex_name, c);
        if let Err(e) = captured_lockfile {
            util::report_error(&e);
            continue;
        }
        let captured_lockfile = captured_lockfile.expect("");
        if captured_lockfile.exists() && !recapture_existing {
            log!("skipping existing lockfile for {}", c);
            continue;
        }
        let r = with_work_crate(ex_name, toolchain, c, |path| {
            with_frobbed_toml(ex_name, c, path)?;
            capture_lockfile(ex_name, c, path, toolchain)
        }).chain_err(|| format!("failed to generate lockfile for {}", c));
        if let Err(e) = r {
            util::report_error(&e);
        }
    }

    Ok(())
}

fn capture_lockfile(ex_name: &str, crate_: &ExCrate, path: &Path, toolchain: &str) -> Result<()> {
    let manifest_path = path.join("Cargo.toml").to_string_lossy().to_string();
    let args = &["generate-lockfile",
                 "--manifest-path",
                 &*manifest_path];
    toolchain::run_cargo(toolchain, ex_name, args)
        .chain_err(|| format!("unable to generate lockfile for {}", crate_))?;

    let ref src_lockfile = path.join("Cargo.lock");
    let ref dst_lockfile = lockfile(ex_name, crate_)?;
    fs::copy(src_lockfile, dst_lockfile)
        .chain_err(|| format!("unable to copy lockfile from {} to {}",
                              src_lockfile.display(), dst_lockfile.display()))?;

    log!("generated lockfile for {} at {}", crate_, dst_lockfile.display());
    
    Ok(())
}

pub fn with_captured_lockfile(ex_name: &str, crate_: &ExCrate, path: &Path) -> Result<()> {
    let ref dst_lockfile = path.join("Cargo.lock");
    if dst_lockfile.exists() {
        return Ok(());
    }
    let ref src_lockfile = lockfile(ex_name, crate_)?;
    if src_lockfile.exists() {
        log!("using lockfile {}", src_lockfile.display());
        fs::copy(src_lockfile, dst_lockfile)
            .chain_err(|| format!("unable to copy lockfile from {} to {}",
                                  src_lockfile.display(), dst_lockfile.display()))?;
    }

    Ok(())
}

pub fn fetch_deps(ex_name: &str, toolchain: &str) -> Result<()> {
    let crates = ex_crates_and_dirs(ex_name)?;
    for (ref c, ref dir) in crates {
        let r = with_work_crate(ex_name, toolchain, c, |path| {
            with_frobbed_toml(ex_name, c, path)?;
            with_captured_lockfile(ex_name, c, path)?;

            let manifest_path = path.join("Cargo.toml").to_string_lossy().to_string();
            let args = &["fetch",
                         "--locked",
                         "--manifest-path",
                         &*manifest_path];
            toolchain::run_cargo(toolchain, ex_name, args)
                .chain_err(|| format!("unable to fetch deps for {}", c))?;

            Ok(())
        });
        if let Err(e) = r {
            util::report_error(&e);
        }
    }

    Ok(())

}

pub fn prepare_all_toolchains(ex_name: &str) -> Result<()> {
    let config = load_config(ex_name)?;
    for tc in &config.toolchains {
        toolchain::prepare_toolchain_(tc)?;
    }

    Ok(())
}

pub fn copy(ex1_name: &str, ex2_name: &str) -> Result<()> {
    let ref ex1_dir = ex_dir(ex1_name);
    let ref ex2_dir = ex_dir(ex2_name);

    if !ex1_dir.exists() {
        bail!("experiment {} is not defined", ex1_name);
    }

    if ex2_dir.exists() {
        bail!("experiment {} is already defined", ex2_name);
    }

    util::copy_dir(ex1_dir, ex2_dir)
}

pub fn delete_all_target_dirs(ex_name: &str) -> Result<()> {
    let ref target_dir = toolchain::ex_target_dir(ex_name);
    if target_dir.exists() {
        util::remove_dir_all(target_dir)?;
    }

    Ok(())
}

pub fn delete(ex_name: &str) -> Result<()> {
    let ref ex_dir = ex_dir(ex_name);
    if ex_dir.exists() {
        util::remove_dir_all(ex_dir)?;
    }

    Ok(())
}


use CARGO_HOME;
use CRATES_DIR;
use EXPERIMENT_DIR;
use RUSTUP_HOME;
use TEST_SOURCE_DIR;
use crates;
use errors::*;
use file;
use gh_mirrors;
use lists::{self, Crate};
use log;
use model::{ExCrateSelect, ExMode};
use run;
use serde_json;
use std::collections::{HashMap, HashSet};
use std::env;
use std::fmt::{self, Display, Formatter};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;
use toml_frobber;
use toolchain::{self, Toolchain};
use util;

pub fn ex_dir(ex_name: &str) -> PathBuf {
    Path::new(EXPERIMENT_DIR).join(ex_name)
}

fn gh_dir() -> PathBuf {
    Path::new(CRATES_DIR).join("gh")
}

fn registry_dir() -> PathBuf {
    Path::new(CRATES_DIR).join("reg")
}

fn shafile(ex_name: &str) -> PathBuf {
    Path::new(EXPERIMENT_DIR).join(ex_name).join("shas.json")
}

fn config_file(ex_name: &str) -> PathBuf {
    Path::new(EXPERIMENT_DIR)
        .join(ex_name)
        .join("config.json")
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

pub struct ExOpts {
    pub name: String,
    pub toolchains: Vec<Toolchain>,
    pub mode: ExMode,
    pub crates: ExCrateSelect,
}

pub fn define(opts: ExOpts) -> Result<()> {
    delete(&opts.name)?;
    let crates = match opts.crates {
        ExCrateSelect::Full => lists::read_all_lists()?,
        ExCrateSelect::Demo => demo_list()?,
        ExCrateSelect::SmallRandom => small_random()?,
        ExCrateSelect::Top100 => top_100()?,
    };
    define_(&opts.name, opts.toolchains, crates, opts.mode)
}

fn demo_list() -> Result<Vec<Crate>> {
    let demo_crate = "lazy_static";
    let demo_gh_app = "brson/hello-rs";
    let mut found_demo_crate = false;
    let crates = lists::read_all_lists()?
        .into_iter()
        .filter(|c| match *c {
                    Crate::Version { ref name, .. } => {
                        if name == demo_crate && !found_demo_crate {
                            found_demo_crate = true;
                            true
                        } else {
                            false
                        }
                    }
                    Crate::Repo { ref url } => url.contains(demo_gh_app),
                })
        .collect::<Vec<_>>();
    assert_eq!(crates.len(), 2);

    Ok(crates)
}

fn small_random() -> Result<Vec<Crate>> {
    use rand::{Rng, thread_rng};

    const COUNT: usize = 20;

    let mut crates = lists::read_all_lists()?;
    let mut rng = thread_rng();
    rng.shuffle(&mut crates);

    crates.truncate(COUNT);
    crates.sort();

    Ok(crates)
}

fn top_100() -> Result<Vec<Crate>> {
    let mut crates = lists::read_pop_list()?;
    crates.truncate(100);

    let crates = crates
        .into_iter()
        .map(|(c, v)| {
                 Crate::Version {
                     name: c,
                     version: v,
                 }
             })
        .collect();

    Ok(crates)
}

pub fn define_(ex_name: &str, tcs: Vec<Toolchain>, crates: Vec<Crate>, mode: ExMode) -> Result<()> {
    log!("defining experiment {} for {} crates",
         ex_name,
         crates.len());
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
        if let Crate::Repo { ref url } = *c {
            if let Err(e) = gh_mirrors::fetch(url) {
                util::report_error(&e);
            }
        }
    }

    Ok(())
}

pub fn capture_shas(ex_name: &str) -> Result<()> {
    let mut shas: HashMap<String, String> = HashMap::new();
    let config = load_config(ex_name)?;
    for krate in config.crates {
        if let Crate::Repo { url } = krate {
            let dir = gh_mirrors::repo_dir(&url)?;
            let r = run::run_capture(Some(&dir), "git", &["log", "-n1", "--pretty=%H"], &[]);

            match r {
                Ok((stdout, stderr)) => {
                    if let Some(shaline) = stdout.get(0) {
                        if !shaline.is_empty() {
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
    Version { name: String, version: String },
    Repo { url: String, sha: String },
}

impl ExCrate {
    fn dir(&self) -> Result<PathBuf> {
        match *self {
            ExCrate::Version {
                ref name,
                ref version,
            } => Ok(registry_dir().join(format!("{}-{}", name, version))),
            ExCrate::Repo { ref url, ref sha } => {
                let (org, name) = gh_mirrors::gh_url_to_org_and_name(url)?;
                Ok(gh_dir().join(format!("{}.{}.{}", org, name, sha)))
            }
        }
    }
}

impl Display for ExCrate {
    fn fmt(&self, f: &mut Formatter) -> ::std::result::Result<(), fmt::Error> {
        let s = match *self {
            ExCrate::Version {
                ref name,
                ref version,
            } => format!("{}-{}", name, version),
            ExCrate::Repo { ref url, ref sha } => format!("{}#{}", url, sha),
        };
        s.fmt(f)
    }
}

pub fn ex_crates_and_dirs(ex_name: &str) -> Result<Vec<(ExCrate, PathBuf)>> {
    let config = load_config(ex_name)?;
    let shas = load_shas(ex_name)?;
    let crates = config
        .crates
        .clone()
        .into_iter()
        .filter_map(|c| {
            let c = c.into_ex_crate(&shas);
            if let Err(e) = c {
                util::report_error(&e);
                return None;
            }
            let c = c.expect("");
            let dir = c.dir();
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
        if let ExCrate::Version {
                   ref name,
                   ref version,
               } = krate {
            fs::create_dir_all(&froml_dir(ex_name))?;
            let out = froml_path(ex_name, name, version);
            let r = toml_frobber::frob_toml(&dir, name, version, &out);
            if let Err(e) = r {
                log!("couldn't frob: {}", e);
                util::report_error(&e);
            }
        }
    }

    Ok(())
}

pub fn with_frobbed_toml(ex_name: &str, crate_: &ExCrate, path: &Path) -> Result<()> {
    let (crate_name, crate_vers) = match *crate_ {
        ExCrate::Version {
            ref name,
            ref version,
        } => (name.to_string(), version.to_string()),
        _ => return Ok(()),
    };
    let src_froml = &froml_path(ex_name, &crate_name, &crate_vers);
    let dst_froml = &path.join("Cargo.toml");
    if src_froml.exists() {
        log!("using frobbed toml {}", src_froml.display());
        fs::copy(src_froml, dst_froml)
            .chain_err(|| {
                           format!("unable to copy frobbed toml from {} to {}",
                                   src_froml.display(),
                                   dst_froml.display())
                       })?;
    }

    Ok(())
}

fn lockfile_dir(ex_name: &str) -> PathBuf {
    Path::new(EXPERIMENT_DIR).join(ex_name).join("lockfiles")
}

fn lockfile(ex_name: &str, crate_: &ExCrate) -> Result<PathBuf> {
    let (crate_name, crate_vers) = match *crate_ {
        ExCrate::Version {
            ref name,
            ref version,
        } => (name.to_string(), version.to_string()),
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
    let src_dir = crate_.dir()?;
    let dest_dir = crate_work_dir(ex_name, toolchain);
    log!("creating temporary build dir for {} in {}",
         crate_,
         dest_dir.display());

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
        })
                .chain_err(|| format!("failed to generate lockfile for {}", c));
        if let Err(e) = r {
            util::report_error(&e);
        }
    }

    Ok(())
}

fn capture_lockfile(ex_name: &str, crate_: &ExCrate, path: &Path, toolchain: &str) -> Result<()> {
    let manifest_path = path.join("Cargo.toml").to_string_lossy().to_string();
    let args = &["generate-lockfile", "--manifest-path", &*manifest_path];
    toolchain::run_cargo(toolchain, ex_name, args)
        .chain_err(|| format!("unable to generate lockfile for {}", crate_))?;

    let src_lockfile = &path.join("Cargo.lock");
    let dst_lockfile = &lockfile(ex_name, crate_)?;
    fs::copy(src_lockfile, dst_lockfile)
        .chain_err(|| {
                       format!("unable to copy lockfile from {} to {}",
                               src_lockfile.display(),
                               dst_lockfile.display())
                   })?;

    log!("generated lockfile for {} at {}",
         crate_,
         dst_lockfile.display());

    Ok(())
}

pub fn with_captured_lockfile(ex_name: &str, crate_: &ExCrate, path: &Path) -> Result<()> {
    let dst_lockfile = &path.join("Cargo.lock");
    if dst_lockfile.exists() {
        return Ok(());
    }
    let src_lockfile = &lockfile(ex_name, crate_)?;
    if src_lockfile.exists() {
        log!("using lockfile {}", src_lockfile.display());
        fs::copy(src_lockfile, dst_lockfile)
            .chain_err(|| {
                           format!("unable to copy lockfile from {} to {}",
                                   src_lockfile.display(),
                                   dst_lockfile.display())
                       })?;
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
            let args = &["fetch", "--locked", "--manifest-path", &*manifest_path];
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
        tc.prepare()?;
    }

    Ok(())
}

pub fn copy(ex1_name: &str, ex2_name: &str) -> Result<()> {
    let ex1_dir = &ex_dir(ex1_name);
    let ex2_dir = &ex_dir(ex2_name);

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
    let ex_dir = ex_dir(ex_name);
    if ex_dir.exists() {
        util::remove_dir_all(&ex_dir)?;
    }

    Ok(())
}

use config::Config;
use crates;
use dirs::{CRATES_DIR, EXPERIMENT_DIR, TEST_SOURCE_DIR};
use errors::*;
use ex_run;
use file;
use gh_mirrors;
use lists::{self, Crate, List};
use run::RunCommand;
use serde_json;
use std::collections::{HashMap, HashSet};
use std::fmt::{self, Display, Formatter};
use std::fs;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::Mutex;
use toml_frobber;
use toolchain::{self, CargoState, Toolchain};
use util;

macro_rules! string_enum {
    (pub enum $name:ident { $($item:ident => $str:expr,)* }) => {
        #[derive(Serialize, Deserialize, Debug, Clone)]
        pub enum $name {
            $($item,)*
        }

        impl FromStr for $name {
            type Err = Error;

            fn from_str(s: &str) -> Result<$name> {
                Ok(match s {
                    $($str => $name::$item,)*
                    s => bail!("invalid {}: {}", stringify!($name), s),
                })
            }
        }

        impl $name {
            pub fn to_str(&self) -> &'static str {
                match *self {
                    $($name::$item => $str,)*
                }
            }

            pub fn possible_values() -> &'static [&'static str] {
                &[$($str,)*]
            }
        }
    }
}

string_enum!(pub enum ExMode {
    BuildAndTest => "build-and-test",
    BuildOnly => "build-only",
    CheckOnly => "check-only",
    UnstableFeatures => "unstable-features",
});

string_enum!(pub enum ExCrateSelect {
    Full => "full",
    Demo => "demo",
    SmallRandom => "small-random",
    Top100 => "top-100",
});

string_enum!(pub enum ExCapLints {
    Allow => "allow",
    Warn => "warn",
    Deny => "deny",
    Forbid => "forbid",
});

pub fn ex_dir(ex_name: &str) -> PathBuf {
    EXPERIMENT_DIR.join(ex_name)
}

fn gh_dir() -> PathBuf {
    CRATES_DIR.join("gh")
}

fn registry_dir() -> PathBuf {
    CRATES_DIR.join("reg")
}

fn shafile(ex_name: &str) -> PathBuf {
    EXPERIMENT_DIR.join(ex_name).join("shas.json")
}

fn config_file(ex_name: &str) -> PathBuf {
    EXPERIMENT_DIR.join(ex_name).join("config.json")
}

fn froml_dir(ex_name: &str) -> PathBuf {
    EXPERIMENT_DIR.join(ex_name).join("fromls")
}

fn froml_path(ex_name: &str, name: &str, vers: &str) -> PathBuf {
    froml_dir(ex_name).join(format!("{}-{}.Cargo.toml", name, vers))
}

#[derive(Serialize, Deserialize)]
pub struct SerializableExperiment {
    pub name: String,
    pub crates: Vec<Crate>,
    pub toolchains: Vec<Toolchain>,
    pub mode: ExMode,
    pub cap_lints: ExCapLints,
}

pub struct Experiment {
    pub name: String,
    pub crates: Vec<Crate>,
    pub toolchains: Vec<Toolchain>,
    pub shas: Mutex<ShasMap>,
    pub mode: ExMode,
    pub cap_lints: ExCapLints,
}

impl Experiment {
    pub fn serializable(&self) -> SerializableExperiment {
        SerializableExperiment {
            name: self.name.clone(),
            crates: self.crates.clone(),
            toolchains: self.toolchains.clone(),
            mode: self.mode.clone(),
            cap_lints: self.cap_lints.clone(),
        }
    }
}

pub struct ExOpts {
    pub name: String,
    pub toolchains: Vec<Toolchain>,
    pub mode: ExMode,
    pub crates: ExCrateSelect,
    pub cap_lints: ExCapLints,
}

pub fn define(opts: ExOpts, config: &Config) -> Result<()> {
    delete(&opts.name)?;
    let crates = match opts.crates {
        ExCrateSelect::Full => lists::read_all_lists()?,
        ExCrateSelect::Demo => demo_list(config)?,
        ExCrateSelect::SmallRandom => small_random()?,
        ExCrateSelect::Top100 => top_100()?,
    };
    define_(
        &opts.name,
        opts.toolchains,
        crates,
        opts.mode,
        opts.cap_lints,
    )
}

fn demo_list(config: &Config) -> Result<Vec<Crate>> {
    let mut crates = config.demo_crates().crates.iter().collect::<HashSet<_>>();
    let repos = &config.demo_crates().github_repos;
    let expected_len = crates.len() + repos.len();

    let result = lists::read_all_lists()?
        .into_iter()
        .filter(|c| match *c {
            Crate::Version { ref name, .. } => crates.remove(name),
            Crate::Repo { ref url } => {
                let mut found = false;
                for repo in repos {
                    if url.ends_with(repo) {
                        found = true;
                        break;
                    }
                }

                found
            }
        })
        .collect::<Vec<_>>();

    assert_eq!(result.len(), expected_len);
    Ok(result)
}

fn small_random() -> Result<Vec<Crate>> {
    use rand::{thread_rng, Rng};

    const COUNT: usize = 20;

    let mut crates = lists::read_all_lists()?;
    let mut rng = thread_rng();
    rng.shuffle(&mut crates);

    crates.truncate(COUNT);
    crates.sort();

    Ok(crates)
}

fn top_100() -> Result<Vec<Crate>> {
    let mut crates = lists::PopList::read()?;
    crates.truncate(100);
    Ok(crates)
}

pub fn define_(
    ex_name: &str,
    toolchains: Vec<Toolchain>,
    crates: Vec<Crate>,
    mode: ExMode,
    cap_lints: ExCapLints,
) -> Result<()> {
    info!(
        "defining experiment {} for {} crates",
        ex_name,
        crates.len()
    );
    let ex = Experiment {
        name: ex_name.to_string(),
        crates,
        shas: Mutex::new(ShasMap::new(shafile(ex_name))?),
        toolchains,
        mode,
        cap_lints,
    };
    fs::create_dir_all(&ex_dir(&ex.name))?;
    let json = serde_json::to_string(&ex.serializable())?;
    info!("writing ex config to {}", config_file(ex_name).display());
    file::write_string(&config_file(ex_name), &json)?;
    Ok(())
}

pub struct ShasMap {
    shas: HashMap<String, String>,
    path: PathBuf,
}

impl ShasMap {
    pub fn new(path: PathBuf) -> Result<Self> {
        let shas = if path.exists() {
            serde_json::from_str(&file::read_string(&path)?)?
        } else {
            HashMap::new()
        };

        Ok(ShasMap { shas, path })
    }

    pub fn capture<'a, I: Iterator<Item = &'a str>>(&mut self, urls: I) -> Result<()> {
        let mut changed = false;

        for url in urls {
            let dir = gh_mirrors::repo_dir(url)?;
            let r = RunCommand::new("git", &["log", "-n1", "--pretty=%H"])
                .cd(&dir)
                .run_capture();

            match r {
                Ok((stdout, _)) => if let Some(shaline) = stdout.get(0) {
                    if !shaline.is_empty() {
                        info!("sha for {}: {}", url, shaline);
                        self.shas.insert(url.to_string(), shaline.to_string());
                        changed = true;
                    } else {
                        error!("bogus output from git log for {}", dir.display());
                    }
                } else {
                    error!("bogus output from git log for {}", dir.display());
                },
                Err(e) => {
                    error!("unable to capture sha for {}: {}", dir.display(), e);
                }
            }
        }

        if changed {
            if let Some(parent) = self.path.parent() {
                fs::create_dir_all(parent)?;
            }

            let shajson = serde_json::to_string(&self.shas)?;
            info!("writing shas to {:?}", self.path);
            file::write_string(&self.path, &shajson)?;
        }

        Ok(())
    }

    pub fn get(&self, url: &str) -> Option<&str> {
        self.shas.get(url).map(|u| u.as_ref())
    }

    pub fn inner(&self) -> &HashMap<String, String> {
        &self.shas
    }
}

impl Experiment {
    fn repo_crate_urls(&self) -> Vec<String> {
        self.crates
            .iter()
            .filter_map(|crate_| crate_.repo_url().map(|u| u.to_owned()))
            .collect()
    }

    pub fn fetch_repo_crates(&self) -> Result<()> {
        for url in self.repo_crate_urls() {
            if let Err(e) = gh_mirrors::fetch(&url) {
                util::report_error(&e);
            }
        }
        Ok(())
    }

    pub fn prepare_shared(&mut self) -> Result<()> {
        self.fetch_repo_crates()?;
        self.shas
            .lock()
            .unwrap()
            .capture(self.crates.iter().filter_map(|c| c.repo_url()))?;
        download_crates(self)?;

        let crates = self.crates()?;
        frob_tomls(self, &crates)?;
        capture_lockfiles(self, &crates, &Toolchain::Dist("stable".into()), false)?;
        Ok(())
    }

    pub fn prepare_local(&self) -> Result<()> {
        // Local experiment prep
        delete_all_target_dirs(&self.name)?;
        ex_run::delete_all_results(&self.name)?;
        fetch_deps(self, &self.crates()?, &Toolchain::Dist("stable".into()))?;
        prepare_all_toolchains(self)?;

        Ok(())
    }
}

impl Experiment {
    pub fn load(ex_name: &str) -> Result<Self> {
        let config = file::read_string(&config_file(ex_name))?;
        let data: SerializableExperiment = serde_json::from_str(&config)?;

        Ok(Experiment {
            shas: Mutex::new(ShasMap::new(shafile(&data.name))?),

            name: data.name,
            crates: data.crates,
            toolchains: data.toolchains,
            mode: data.mode,
            cap_lints: data.cap_lints,
        })
    }

    pub fn crates(&self) -> Result<Vec<ExCrate>> {
        let (oks, fails): (Vec<_>, Vec<_>) = self.crates
            .clone()
            .into_iter()
            .map(|c| c.into_ex_crate(self))
            .partition(Result::is_ok);
        if !fails.is_empty() {
            let fails = fails
                .into_iter()
                .map(Result::unwrap_err)
                .map(|e| e.to_string())
                .collect::<Vec<_>>();
            Err(fails.join(", ").into())
        } else {
            oks.into_iter().collect()
        }
    }
}

#[derive(Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize, Clone)]
pub enum ExCrate {
    Version {
        name: String,
        version: String,
    },
    Repo {
        org: String,
        name: String,
        sha: String,
    },
}

impl ExCrate {
    pub fn dir(&self) -> PathBuf {
        match *self {
            ExCrate::Version {
                ref name,
                ref version,
            } => registry_dir().join(format!("{}-{}", name, version)),
            ExCrate::Repo {
                ref org,
                ref name,
                ref sha,
            } => gh_dir().join(format!("{}.{}.{}", org, name, sha)),
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
            ExCrate::Repo {
                ref org,
                ref name,
                ref sha,
            } => format!("https://github.com/{}/{}#{}", org, name, sha),
        };
        s.fmt(f)
    }
}

impl FromStr for ExCrate {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self> {
        if s.starts_with("https://") {
            if let Some(hash_idx) = s.find('#') {
                let repo = &s[..hash_idx];
                let sha = &s[hash_idx + 1..];
                let (org, name) = gh_mirrors::gh_url_to_org_and_name(repo)?;
                Ok(ExCrate::Repo {
                    org: org.to_string(),
                    name: name.to_string(),
                    sha: sha.to_string(),
                })
            } else {
                Err("no sha for git crate".into())
            }
        } else if let Some(dash_idx) = s.rfind('-') {
            let name = &s[..dash_idx];
            let version = &s[dash_idx + 1..];
            Ok(ExCrate::Version {
                name: name.to_string(),
                version: version.to_string(),
            })
        } else {
            Err("no version for crate".into())
        }
    }
}

fn download_crates(ex: &Experiment) -> Result<()> {
    crates::prepare(&ex.crates()?)
}

#[cfg_attr(feature = "cargo-clippy", allow(match_ref_pats))]
pub fn frob_tomls(ex: &Experiment, crates: &[ExCrate]) -> Result<()> {
    for krate in crates {
        if let &ExCrate::Version {
            ref name,
            ref version,
        } = krate
        {
            fs::create_dir_all(&froml_dir(&ex.name))?;
            let out = froml_path(&ex.name, name, version);
            let r = toml_frobber::frob_toml(&krate.dir(), name, version, &out);
            if let Err(e) = r {
                info!("couldn't frob: {}", e);
                util::report_error(&e);
            }
        }
    }

    Ok(())
}

pub fn with_frobbed_toml(ex: &Experiment, crate_: &ExCrate, path: &Path) -> Result<()> {
    let (crate_name, crate_vers) = match *crate_ {
        ExCrate::Version {
            ref name,
            ref version,
        } => (name.to_string(), version.to_string()),
        _ => return Ok(()),
    };
    let src_froml = &froml_path(&ex.name, &crate_name, &crate_vers);
    let dst_froml = &path.join("Cargo.toml");
    if src_froml.exists() {
        info!("using frobbed toml {}", src_froml.display());
        fs::copy(src_froml, dst_froml).chain_err(|| {
            format!(
                "unable to copy frobbed toml from {} to {}",
                src_froml.display(),
                dst_froml.display()
            )
        })?;
    }

    Ok(())
}

fn lockfile_dir(ex_name: &str) -> PathBuf {
    EXPERIMENT_DIR.join(ex_name).join("lockfiles")
}

fn lockfile(ex_name: &str, crate_: &ExCrate) -> Result<PathBuf> {
    let name = match *crate_ {
        ExCrate::Version {
            ref name,
            ref version,
        } => format!("registry-{}-{}.lock", name, version),
        ExCrate::Repo {
            ref org, ref name, ..
        } => format!("repo-{}.{}.lock", org, name),
    };
    Ok(lockfile_dir(ex_name).join(name))
}

fn crate_work_dir(ex_name: &str, toolchain: &Toolchain) -> PathBuf {
    let mut dir = TEST_SOURCE_DIR.clone();
    if let Some(thread) = ::std::thread::current().name() {
        dir = dir.join(thread);
    }
    dir.join(ex_name).join(toolchain.to_string())
}

pub fn with_work_crate<F, R>(
    ex: &Experiment,
    toolchain: &Toolchain,
    crate_: &ExCrate,
    f: F,
) -> Result<R>
where
    F: Fn(&Path) -> Result<R>,
{
    let src_dir = crate_.dir();
    let dest_dir = crate_work_dir(&ex.name, toolchain);
    info!(
        "creating temporary build dir for {} in {}",
        crate_,
        dest_dir.display()
    );

    util::copy_dir(&src_dir, &dest_dir)?;
    let r = f(&dest_dir);
    util::remove_dir_all(&dest_dir)?;
    r
}

pub fn capture_lockfiles(
    ex: &Experiment,
    crates: &[ExCrate],
    toolchain: &Toolchain,
    recapture_existing: bool,
) -> Result<()> {
    fs::create_dir_all(&lockfile_dir(&ex.name))?;

    for c in crates {
        if c.dir().join("Cargo.lock").exists() {
            info!("crate {} has a lockfile. skipping", c);
            continue;
        }
        let captured_lockfile = lockfile(&ex.name, c);
        if let Err(e) = captured_lockfile {
            util::report_error(&e);
            continue;
        }
        let captured_lockfile = captured_lockfile.expect("");
        if captured_lockfile.exists() && !recapture_existing {
            info!("skipping existing lockfile for {}", c);
            continue;
        }
        let r = with_work_crate(ex, toolchain, c, |path| {
            with_frobbed_toml(ex, c, path)?;
            capture_lockfile(ex, c, path, toolchain)
        }).chain_err(|| format!("failed to generate lockfile for {}", c));
        if let Err(e) = r {
            util::report_error(&e);
        }
    }

    Ok(())
}

fn capture_lockfile(
    ex: &Experiment,
    crate_: &ExCrate,
    path: &Path,
    toolchain: &Toolchain,
) -> Result<()> {
    let args = &["generate-lockfile", "--manifest-path", "Cargo.toml"];
    toolchain
        .run_cargo(ex, path, args, CargoState::Unlocked, false)
        .chain_err(|| format!("unable to generate lockfile for {}", crate_))?;

    let src_lockfile = &path.join("Cargo.lock");
    let dst_lockfile = &lockfile(&ex.name, crate_)?;
    fs::copy(src_lockfile, dst_lockfile).chain_err(|| {
        format!(
            "unable to copy lockfile from {} to {}",
            src_lockfile.display(),
            dst_lockfile.display()
        )
    })?;

    info!(
        "generated lockfile for {} at {}",
        crate_,
        dst_lockfile.display()
    );

    Ok(())
}

pub fn with_captured_lockfile(ex: &Experiment, crate_: &ExCrate, path: &Path) -> Result<()> {
    let dst_lockfile = &path.join("Cargo.lock");
    if dst_lockfile.exists() {
        return Ok(());
    }
    let src_lockfile = &lockfile(&ex.name, crate_)?;
    if src_lockfile.exists() {
        info!("using lockfile {}", src_lockfile.display());
        fs::copy(src_lockfile, dst_lockfile).chain_err(|| {
            format!(
                "unable to copy lockfile from {} to {}",
                src_lockfile.display(),
                dst_lockfile.display()
            )
        })?;
    }

    Ok(())
}

pub fn fetch_deps(ex: &Experiment, crates: &[ExCrate], toolchain: &Toolchain) -> Result<()> {
    for c in crates {
        let r = with_work_crate(ex, toolchain, c, |path| {
            with_frobbed_toml(ex, c, path)?;
            with_captured_lockfile(ex, c, path)?;

            let args = &["fetch", "--locked", "--manifest-path", "Cargo.toml"];
            toolchain
                .run_cargo(ex, path, args, CargoState::Unlocked, false)
                .chain_err(|| format!("unable to fetch deps for {}", c))?;

            Ok(())
        });
        if let Err(e) = r {
            util::report_error(&e);
        }
    }

    Ok(())
}

pub fn prepare_all_toolchains(ex: &Experiment) -> Result<()> {
    for tc in &ex.toolchains {
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
    let target_dir = &toolchain::ex_target_dir(ex_name);
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

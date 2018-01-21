use crates;
use dirs::{CRATES_DIR, EXPERIMENT_DIR, TEST_SOURCE_DIR};
use errors::*;
use ex_run;
use file;
use gh_mirrors;
use lists::{self, Crate, List};
use run;
use serde_json;
use std::collections::HashMap;
use std::fmt::{self, Display, Formatter};
use std::fs;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use toml_frobber;
use toolchain::{self, CargoState, Toolchain};
use util;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum ExMode {
    BuildAndTest,
    BuildOnly,
    CheckOnly,
    UnstableFeatures,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum ExCrateSelect {
    Full,
    Demo,
    SmallRandom,
    Top100,
}

pub fn ex_dir(ex_name: &str) -> PathBuf {
    EXPERIMENT_DIR.join(ex_name)
}

fn gh_dir() -> PathBuf {
    CRATES_DIR.join("gh")
}

fn registry_dir() -> PathBuf {
    CRATES_DIR.join("reg")
}

fn shafile(ex: &Experiment) -> PathBuf {
    EXPERIMENT_DIR.join(&ex.name).join("shas.json")
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
            Crate::Version { ref name, .. } => if name == demo_crate && !found_demo_crate {
                found_demo_crate = true;
                true
            } else {
                false
            },
            Crate::Repo { ref url } => url.contains(demo_gh_app),
        })
        .collect::<Vec<_>>();
    assert_eq!(crates.len(), 2);

    Ok(crates)
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

pub fn define_(ex_name: &str, tcs: Vec<Toolchain>, crates: Vec<Crate>, mode: ExMode) -> Result<()> {
    info!(
        "defining experiment {} for {} crates",
        ex_name,
        crates.len()
    );
    let ex = Experiment {
        name: ex_name.to_string(),
        crates: crates,
        toolchains: tcs,
        mode: mode,
    };
    fs::create_dir_all(&ex_dir(&ex.name))?;
    let json = serde_json::to_string(&ex)?;
    info!("writing ex config to {}", config_file(ex_name).display());
    file::write_string(&config_file(ex_name), &json)?;
    Ok(())
}

impl Experiment {
    pub fn load(ex_name: &str) -> Result<Self> {
        let config = file::read_string(&config_file(ex_name))?;
        Ok(serde_json::from_str(&config)?)
    }

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

    pub fn prepare_shared(&self) -> Result<()> {
        self.fetch_repo_crates()?;
        capture_shas(self)?;
        download_crates(self)?;
        frob_tomls(self)?;
        capture_lockfiles(self, &Toolchain::Dist("stable".into()), false)?;
        Ok(())
    }

    pub fn prepare_local(&self) -> Result<()> {
        // Local experiment prep
        delete_all_target_dirs(&self.name)?;
        ex_run::delete_all_results(&self.name)?;
        fetch_deps(self, &Toolchain::Dist("stable".into()))?;
        prepare_all_toolchains(self)?;

        Ok(())
    }
}

fn capture_shas(ex: &Experiment) -> Result<()> {
    let mut shas: HashMap<String, String> = HashMap::new();
    for krate in &ex.crates {
        if let Crate::Repo { ref url } = *krate {
            let dir = gh_mirrors::repo_dir(url)?;
            let r = run::run_capture(Some(&dir), "git", &["log", "-n1", "--pretty=%H"], &[]);

            match r {
                Ok((stdout, _)) => if let Some(shaline) = stdout.get(0) {
                    if !shaline.is_empty() {
                        info!("sha for {}: {}", url, shaline);
                        shas.insert(url.to_string(), shaline.to_string());
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
    }

    fs::create_dir_all(&ex_dir(&ex.name))?;
    let shajson = serde_json::to_string(&shas)?;
    info!("writing shas to {}", shafile(ex).display());
    file::write_string(&shafile(ex), &shajson)?;

    Ok(())
}

impl Experiment {
    pub fn load_shas(&self) -> Result<HashMap<String, String>> {
        let shas = file::read_string(&shafile(self))?;
        let shas = serde_json::from_str(&shas)?;
        Ok(shas)
    }

    pub fn crates(&self) -> Result<Vec<ExCrate>> {
        let shas = self.load_shas()?;
        let (oks, fails): (Vec<_>, Vec<_>) = self.crates
            .clone()
            .into_iter()
            .map(|c| c.into_ex_crate(&shas))
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
                    org,
                    name,
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

fn frob_tomls(ex: &Experiment) -> Result<()> {
    for krate in ex.crates()? {
        if let ExCrate::Version {
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
    let (crate_name, crate_vers) = match *crate_ {
        ExCrate::Version {
            ref name,
            ref version,
        } => (name.to_string(), version.to_string()),
        _ => bail!("unimplemented crate type in `lockfile`"),
    };
    Ok(lockfile_dir(ex_name).join(format!("{}-{}.lock", crate_name, crate_vers)))
}

fn crate_work_dir(ex_name: &str, toolchain: &Toolchain) -> PathBuf {
    TEST_SOURCE_DIR.join(ex_name).join(toolchain.to_string())
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

fn capture_lockfiles(
    ex: &Experiment,
    toolchain: &Toolchain,
    recapture_existing: bool,
) -> Result<()> {
    fs::create_dir_all(&lockfile_dir(&ex.name))?;

    for c in &ex.crates()? {
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
        .run_cargo(&ex.name, path, args, CargoState::Unlocked)
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

fn fetch_deps(ex: &Experiment, toolchain: &Toolchain) -> Result<()> {
    for c in &ex.crates()? {
        let r = with_work_crate(ex, toolchain, c, |path| {
            with_frobbed_toml(ex, c, path)?;
            with_captured_lockfile(ex, c, path)?;

            let args = &["fetch", "--locked", "--manifest-path", "Cargo.toml"];
            toolchain
                .run_cargo(&ex.name, path, args, CargoState::Unlocked)
                .chain_err(|| format!("unable to fetch deps for {}", c))?;

            Ok(())
        });
        if let Err(e) = r {
            util::report_error(&e);
        }
    }

    Ok(())
}

fn prepare_all_toolchains(ex: &Experiment) -> Result<()> {
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

impl FromStr for ExMode {
    type Err = Error;

    fn from_str(s: &str) -> Result<ExMode> {
        Ok(match s {
            "build-and-test" => ExMode::BuildAndTest,
            "build-only" => ExMode::BuildOnly,
            "check-only" => ExMode::CheckOnly,
            "unstable-features" => ExMode::UnstableFeatures,
            s => bail!("invalid ex-mode: {}", s),
        })
    }
}

impl ExMode {
    pub fn to_str(&self) -> &'static str {
        match *self {
            ExMode::BuildAndTest => "build-and-test",
            ExMode::BuildOnly => "build-only",
            ExMode::CheckOnly => "check-only",
            ExMode::UnstableFeatures => "unstable-features",
        }
    }
}

impl FromStr for ExCrateSelect {
    type Err = Error;

    fn from_str(s: &str) -> Result<ExCrateSelect> {
        Ok(match s {
            "full" => ExCrateSelect::Full,
            "demo" => ExCrateSelect::Demo,
            "small-random" => ExCrateSelect::SmallRandom,
            "top-100" => ExCrateSelect::Top100,
            s => bail!("invalid crate-select: {}", s),
        })
    }
}

impl ExCrateSelect {
    pub fn to_str(&self) -> &'static str {
        match *self {
            ExCrateSelect::Full => "full",
            ExCrateSelect::Demo => "demo",
            ExCrateSelect::SmallRandom => "small-random",
            ExCrateSelect::Top100 => "top-100",
        }
    }
}

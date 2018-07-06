use config::Config;
use crates::{self, Crate, RegistryCrate};
use dirs::{EXPERIMENT_DIR, TEST_SOURCE_DIR};
use errors::*;
use ex_run;
use file;
use git;
use lists::{self, List};
use results::WriteResults;
use run::RunCommand;
use serde_json;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use toml_frobber;
use toolchain::{self, CargoState, Toolchain, MAIN_TOOLCHAIN};
use util;

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

pub fn config_file(ex_name: &str) -> PathBuf {
    EXPERIMENT_DIR.join(ex_name).join("config.json")
}

fn froml_dir(ex_name: &str) -> PathBuf {
    EXPERIMENT_DIR.join(ex_name).join("fromls")
}

fn froml_path(ex_name: &str, name: &str, vers: &str) -> PathBuf {
    froml_dir(ex_name).join(format!("{}-{}.Cargo.toml", name, vers))
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Experiment {
    pub name: String,
    pub crates: Vec<Crate>,
    pub toolchains: Vec<Toolchain>,
    pub mode: ExMode,
    pub cap_lints: ExCapLints,
}

pub struct ExOpts {
    pub name: String,
    pub toolchains: Vec<Toolchain>,
    pub mode: ExMode,
    pub crates: ExCrateSelect,
    pub cap_lints: ExCapLints,
}

pub fn get_crates(crates: ExCrateSelect, config: &Config) -> Result<Vec<Crate>> {
    match crates {
        ExCrateSelect::Full => lists::read_all_lists(),
        ExCrateSelect::Demo => demo_list(config),
        ExCrateSelect::SmallRandom => small_random(),
        ExCrateSelect::Top100 => top_100(),
    }
}

pub fn define(opts: ExOpts, config: &Config) -> Result<()> {
    delete(&opts.name)?;
    define_(
        &opts.name,
        opts.toolchains,
        get_crates(opts.crates, config)?,
        opts.mode,
        opts.cap_lints,
    )
}

pub fn demo_list(config: &Config) -> Result<Vec<Crate>> {
    let mut crates = config.demo_crates().crates.iter().collect::<HashSet<_>>();
    let repos = &config.demo_crates().github_repos;
    let expected_len = crates.len() + repos.len();

    let result = lists::read_all_lists()?
        .into_iter()
        .filter(|c| match *c {
            Crate::Registry(RegistryCrate { ref name, .. }) => crates.remove(name),
            Crate::GitHub(ref repo) => {
                let url = repo.url();

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
        toolchains,
        mode,
        cap_lints,
    };

    ex.validate()?;

    fs::create_dir_all(&ex_dir(&ex.name))?;
    let json = serde_json::to_string(&ex)?;
    info!("writing ex config to {}", config_file(ex_name).display());
    file::write_string(&config_file(ex_name), &json)?;
    Ok(())
}

impl Experiment {
    pub fn validate(&self) -> Result<()> {
        if self.toolchains[0] == self.toolchains[1] {
            bail!("reusing the same toolchain isn't supported");
        }

        Ok(())
    }

    pub fn fetch_repo_crates(&self) -> Result<()> {
        for repo in self.crates.iter().filter_map(|krate| krate.github()) {
            if let Err(e) = git::shallow_clone_or_pull(&repo.url(), &repo.mirror_dir()) {
                util::report_error(&e);
            }
        }
        Ok(())
    }

    pub fn prepare_shared<DB: WriteResults>(&self, config: &Config, db: &DB) -> Result<()> {
        self.fetch_repo_crates()?;
        capture_shas(self, &self.crates, db)?;
        crates::prepare(&self.crates)?;

        frob_tomls(self, &self.crates)?;
        capture_lockfiles(config, self, &self.crates, &MAIN_TOOLCHAIN)?;
        Ok(())
    }

    pub fn prepare_local(&self, config: &Config) -> Result<()> {
        // Local experiment prep
        delete_all_target_dirs(&self.name)?;
        ex_run::delete_all_results(&self.name)?;
        fetch_deps(config, self, &self.crates, &MAIN_TOOLCHAIN)?;
        prepare_all_toolchains(self)?;

        Ok(())
    }
}

impl Experiment {
    pub fn load(ex_name: &str) -> Result<Self> {
        let config = file::read_string(&config_file(ex_name))?;
        Ok(serde_json::from_str(&config)?)
    }
}

pub fn frob_tomls(ex: &Experiment, crates: &[Crate]) -> Result<()> {
    for krate in crates {
        if let Err(e) = frob_toml(ex, krate) {
            info!("couldn't frob: {}", e);
            util::report_error(&e);
        }
    }

    Ok(())
}

#[cfg_attr(feature = "cargo-clippy", allow(match_ref_pats))]
pub fn frob_toml(ex: &Experiment, krate: &Crate) -> Result<()> {
    if let Crate::Registry(ref details) = *krate {
        fs::create_dir_all(&froml_dir(&ex.name))?;
        let out = froml_path(&ex.name, &details.name, &details.version);
        toml_frobber::frob_toml(&krate.dir(), &details.name, &details.version, &out)?;
    }

    Ok(())
}

pub fn capture_shas<DB: WriteResults>(ex: &Experiment, crates: &[Crate], db: &DB) -> Result<()> {
    for krate in crates {
        if let Crate::GitHub(ref repo) = *krate {
            let dir = repo.mirror_dir();
            let r = RunCommand::new("git", &["rev-parse", "HEAD"])
                .cd(&dir)
                .run_capture();

            let sha = match r {
                Ok((stdout, _)) => if let Some(shaline) = stdout.get(0) {
                    if !shaline.is_empty() {
                        info!("sha for GitHub repo {}: {}", repo.slug(), shaline);
                        shaline.to_string()
                    } else {
                        bail!("bogus output from git log for {}", dir.display());
                    }
                } else {
                    bail!("bogus output from git log for {}", dir.display());
                },
                Err(e) => {
                    bail!("unable to capture sha for {}: {}", dir.display(), e);
                }
            };

            db.record_sha(ex, repo, &sha)
                .chain_err(|| format!("failed to record the sha of GitHub repo {}", repo.slug()))?;
        }
    }

    Ok(())
}

pub fn with_frobbed_toml(ex: &Experiment, krate: &Crate, path: &Path) -> Result<()> {
    let (crate_name, crate_vers) = match *krate {
        Crate::Registry(ref details) => (details.name.clone(), details.version.clone()),
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

fn lockfile(ex_name: &str, krate: &Crate) -> Result<PathBuf> {
    let name = match *krate {
        Crate::Registry(ref details) => format!("reg-{}-{}.lock", details.name, details.version),
        Crate::GitHub(ref repo) => format!("reg-{}-{}.lock", repo.org, repo.name),
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
    krate: &Crate,
    f: F,
) -> Result<R>
where
    F: Fn(&Path) -> Result<R>,
{
    let src_dir = krate.dir();
    let dest_dir = crate_work_dir(&ex.name, toolchain);
    info!(
        "creating temporary build dir for {} in {}",
        krate,
        dest_dir.display()
    );

    util::copy_dir(&src_dir, &dest_dir)?;
    let r = f(&dest_dir);
    util::remove_dir_all(&dest_dir)?;
    r
}

pub fn capture_lockfiles(
    config: &Config,
    ex: &Experiment,
    crates: &[Crate],
    toolchain: &Toolchain,
) -> Result<()> {
    for c in crates {
        if let Err(e) = capture_lockfile(config, ex, c, toolchain) {
            util::report_error(&e);
        }
    }

    Ok(())
}

pub fn capture_lockfile(
    config: &Config,
    ex: &Experiment,
    krate: &Crate,
    toolchain: &Toolchain,
) -> Result<()> {
    fs::create_dir_all(&lockfile_dir(&ex.name))?;

    if !config.should_update_lockfile(krate) && krate.dir().join("Cargo.lock").exists() {
        info!("crate {} has a lockfile. skipping", krate);
        return Ok(());
    }

    with_work_crate(ex, toolchain, krate, |path| {
        with_frobbed_toml(ex, krate, path)?;
        capture_lockfile_inner(ex, krate, path, toolchain)
    }).chain_err(|| format!("failed to generate lockfile for {}", krate))?;

    Ok(())
}

fn capture_lockfile_inner(
    ex: &Experiment,
    krate: &Crate,
    path: &Path,
    toolchain: &Toolchain,
) -> Result<()> {
    let args = &[
        "generate-lockfile",
        "--manifest-path",
        "Cargo.toml",
        "-Zno-index-update",
    ];
    toolchain
        .run_cargo(ex, path, args, CargoState::Unlocked, false, false)
        .chain_err(|| format!("unable to generate lockfile for {}", krate))?;

    let src_lockfile = &path.join("Cargo.lock");
    let dst_lockfile = &lockfile(&ex.name, krate)?;
    fs::copy(src_lockfile, dst_lockfile).chain_err(|| {
        format!(
            "unable to copy lockfile from {} to {}",
            src_lockfile.display(),
            dst_lockfile.display()
        )
    })?;

    info!(
        "generated lockfile for {} at {}",
        krate,
        dst_lockfile.display()
    );

    Ok(())
}

pub fn with_captured_lockfile(
    config: &Config,
    ex: &Experiment,
    krate: &Crate,
    path: &Path,
) -> Result<()> {
    let src_lockfile = &lockfile(&ex.name, krate)?;
    let dst_lockfile = &path.join("Cargo.lock");

    // Only use the local lockfile if it wasn't overridden
    if !config.should_update_lockfile(krate) && dst_lockfile.exists() {
        return Ok(());
    }

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

pub fn fetch_deps(
    config: &Config,
    ex: &Experiment,
    crates: &[Crate],
    toolchain: &Toolchain,
) -> Result<()> {
    for c in crates {
        if let Err(e) = fetch_crate_deps(config, ex, c, toolchain) {
            util::report_error(&e);
        }
    }

    Ok(())
}

pub fn fetch_crate_deps(
    config: &Config,
    ex: &Experiment,
    krate: &Crate,
    toolchain: &Toolchain,
) -> Result<()> {
    with_work_crate(ex, toolchain, krate, |path| {
        with_frobbed_toml(ex, krate, path)?;
        with_captured_lockfile(config, ex, krate, path)?;

        let args = &["fetch", "--locked", "--manifest-path", "Cargo.toml"];
        toolchain
            .run_cargo(ex, path, args, CargoState::Unlocked, false, true)
            .chain_err(|| format!("unable to fetch deps for {}", krate))?;

        Ok(())
    })
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

#[cfg(test)]
mod tests {
    use super::{ExCapLints, ExMode, Experiment};

    #[test]
    fn test_validate_experiment() {
        // Correct experiment
        assert!(
            Experiment {
                name: "foo".to_string(),
                crates: vec![],
                toolchains: vec!["stable".parse().unwrap(), "beta".parse().unwrap()],
                mode: ExMode::BuildAndTest,
                cap_lints: ExCapLints::Forbid,
            }.validate()
                .is_ok()
        );

        // Experiment with the same toolchain
        assert!(
            Experiment {
                name: "foo".to_string(),
                crates: vec![],
                toolchains: vec!["stable".parse().unwrap(), "stable".parse().unwrap()],
                mode: ExMode::BuildAndTest,
                cap_lints: ExCapLints::Forbid,
            }.validate()
                .is_err()
        );
    }
}

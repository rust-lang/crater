use config::Config;
use crates::Crate;
use dirs::{EXPERIMENT_DIR, TEST_SOURCE_DIR};
use errors::*;
use experiments::Experiment;
use git;
use results::WriteResults;
use run::RunCommand;
use std::fs;
use std::path::{Path, PathBuf};
use toml_frobber;
use toolchain::{CargoState, Toolchain};

fn froml_dir(ex_name: &str) -> PathBuf {
    EXPERIMENT_DIR.join(ex_name).join("fromls")
}

fn froml_path(ex_name: &str, name: &str, vers: &str) -> PathBuf {
    froml_dir(ex_name).join(format!("{}-{}.Cargo.toml", name, vers))
}

impl Experiment {
    pub fn fetch_repo_crates(&self) -> Result<()> {
        for repo in self.crates.iter().filter_map(|krate| krate.github()) {
            if let Err(e) = git::shallow_clone_or_pull(&repo.url(), &repo.mirror_dir()) {
                ::utils::report_error(&e);
            }
        }
        Ok(())
    }
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
        Crate::GitHub(ref repo) => format!("gh-{}-{}.lock", repo.org, repo.name),
        Crate::Local(ref name) => format!("local-{}.lock", name),
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

    ::utils::fs::copy_dir(&src_dir, &dest_dir)?;
    let r = f(&dest_dir);
    ::utils::fs::remove_dir_all(&dest_dir)?;
    r
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
        capture_lockfile_inner(config, ex, krate, path, toolchain)
    }).chain_err(|| format!("failed to generate lockfile for {}", krate))?;

    Ok(())
}

fn capture_lockfile_inner(
    config: &Config,
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
        .run_cargo(config, ex, path, args, CargoState::Unlocked, false, false)
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
            .run_cargo(config, ex, path, args, CargoState::Unlocked, false, true)
            .chain_err(|| format!("unable to fetch deps for {}", krate))?;

        Ok(())
    })
}

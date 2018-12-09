use config::Config;
use crates::Crate;
use dirs::{EXPERIMENT_DIR, TEST_SOURCE_DIR};
use experiments::Experiment;
use prelude::*;
use results::{FailureReason, TestResult, WriteResults};
use run::RunCommand;
use runner::toml_frobber::TomlFrobber;
use runner::OverrideResult;
use std::fs;
use std::path::{Path, PathBuf};
use toolchain::Toolchain;
use tools::CARGO;

fn froml_dir(ex_name: &str) -> PathBuf {
    EXPERIMENT_DIR.join(ex_name).join("fromls")
}

fn froml_path(ex_name: &str, name: &str, vers: &str) -> PathBuf {
    froml_dir(ex_name).join(format!("{}-{}.Cargo.toml", name, vers))
}

#[allow(clippy::match_ref_pats)]
pub(super) fn frob_toml(ex: &Experiment, krate: &Crate) -> Fallible<()> {
    if let Crate::Registry(ref details) = *krate {
        fs::create_dir_all(&froml_dir(&ex.name))?;
        let source = krate.dir();
        let out = froml_path(&ex.name, &details.name, &details.version);

        let mut frobber = TomlFrobber::new(krate, &source)?;
        frobber.frob();
        frobber.save(&out)?;
    }

    Ok(())
}

pub(super) fn capture_shas<DB: WriteResults>(
    ex: &Experiment,
    crates: &[Crate],
    db: &DB,
) -> Fallible<()> {
    for krate in crates {
        if let Crate::GitHub(ref repo) = *krate {
            let dir = repo.mirror_dir();
            let r = RunCommand::new("git")
                .args(&["rev-parse", "HEAD"])
                .cd(&dir)
                .run_capture();

            let sha = match r {
                Ok((stdout, _)) => {
                    if let Some(shaline) = stdout.get(0) {
                        if !shaline.is_empty() {
                            info!("sha for GitHub repo {}: {}", repo.slug(), shaline);
                            shaline.to_string()
                        } else {
                            bail!("bogus output from git log for {}", dir.to_string_lossy());
                        }
                    } else {
                        bail!("bogus output from git log for {}", dir.to_string_lossy());
                    }
                }
                Err(e) => {
                    bail!("unable to capture sha for {}: {}", dir.to_string_lossy(), e);
                }
            };

            db.record_sha(ex, repo, &sha).with_context(|_| {
                format!("failed to record the sha of GitHub repo {}", repo.slug())
            })?;
        }
    }

    Ok(())
}

pub(super) fn with_frobbed_toml(ex: &Experiment, krate: &Crate, path: &Path) -> Fallible<()> {
    let (crate_name, crate_vers) = match *krate {
        Crate::Registry(ref details) => (details.name.clone(), details.version.clone()),
        _ => return Ok(()),
    };
    let src_froml = &froml_path(&ex.name, &crate_name, &crate_vers);
    let dst_froml = &path.join("Cargo.toml");
    if src_froml.exists() {
        info!("using frobbed toml {}", src_froml.display());
        fs::copy(src_froml, dst_froml).with_context(|_| {
            format!(
                "unable to copy frobbed toml from {} to {}",
                src_froml.display(),
                dst_froml.display()
            )
        })?;
    }

    Ok(())
}

pub(super) fn validate_manifest(ex: &Experiment, krate: &Crate, tc: &Toolchain) -> Fallible<()> {
    info!("validating manifest of {} on toolchain {}", krate, tc);
    with_work_crate(ex, tc, krate, |path| {
        // Skip crates missing a Cargo.toml
        if !path.join("Cargo.toml").is_file() {
            Err(err_msg(format!("missing Cargo.toml for {}", krate)))
                .with_context(|_| OverrideResult(TestResult::BuildFail(FailureReason::Broken)))?;
        }

        RunCommand::new(CARGO.toolchain(tc))
            .args(&["read-manifest", "--manifest-path", "Cargo.toml"])
            .cd(path)
            .hide_output(true)
            .run()
            .with_context(|_| format!("invalid syntax in {}'s Cargo.toml", krate))
            .with_context(|_| OverrideResult(TestResult::BuildFail(FailureReason::Broken)))?;

        Ok(())
    })
}

fn lockfile_dir(ex_name: &str) -> PathBuf {
    EXPERIMENT_DIR.join(ex_name).join("lockfiles")
}

fn lockfile(ex_name: &str, krate: &Crate) -> Fallible<PathBuf> {
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

pub(super) fn with_work_crate<F, R>(
    ex: &Experiment,
    toolchain: &Toolchain,
    krate: &Crate,
    f: F,
) -> Fallible<R>
where
    F: Fn(&Path) -> Fallible<R>,
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

pub(super) fn capture_lockfile(
    config: &Config,
    ex: &Experiment,
    krate: &Crate,
    toolchain: &Toolchain,
) -> Fallible<()> {
    fs::create_dir_all(&lockfile_dir(&ex.name))?;

    if !config.should_update_lockfile(krate)
        && krate.is_repo()
        && krate.dir().join("Cargo.lock").exists()
    {
        info!("crate {} has a lockfile. skipping", krate);
        return Ok(());
    }

    with_work_crate(ex, toolchain, krate, |path| {
        with_frobbed_toml(ex, krate, path)?;
        capture_lockfile_inner(ex, krate, path, toolchain)
    })
    .with_context(|_| format!("failed to generate lockfile for {}", krate))?;

    Ok(())
}

fn capture_lockfile_inner(
    ex: &Experiment,
    krate: &Crate,
    path: &Path,
    toolchain: &Toolchain,
) -> Fallible<()> {
    RunCommand::new(CARGO.toolchain(toolchain).unstable_features(true))
        .args(&[
            "generate-lockfile",
            "--manifest-path",
            "Cargo.toml",
            "-Zno-index-update",
        ])
        .cd(path)
        .run()?;

    let src_lockfile = &path.join("Cargo.lock");
    let dst_lockfile = &lockfile(&ex.name, krate)?;
    fs::copy(src_lockfile, dst_lockfile).with_context(|_| {
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

pub(super) fn with_captured_lockfile(
    config: &Config,
    ex: &Experiment,
    krate: &Crate,
    path: &Path,
) -> Fallible<()> {
    let src_lockfile = &lockfile(&ex.name, krate)?;
    let dst_lockfile = &path.join("Cargo.lock");

    // Only use the local lockfile if it wasn't overridden
    if !config.should_update_lockfile(krate) && krate.is_repo() && dst_lockfile.exists() {
        return Ok(());
    }

    if src_lockfile.exists() {
        info!("using lockfile {}", src_lockfile.display());
        fs::copy(src_lockfile, dst_lockfile).with_context(|_| {
            format!(
                "unable to copy lockfile from {} to {}",
                src_lockfile.display(),
                dst_lockfile.display()
            )
        })?;
    }

    Ok(())
}

pub(super) fn fetch_crate_deps(
    config: &Config,
    ex: &Experiment,
    krate: &Crate,
    toolchain: &Toolchain,
) -> Fallible<()> {
    with_work_crate(ex, toolchain, krate, |path| {
        with_frobbed_toml(ex, krate, path)?;
        with_captured_lockfile(config, ex, krate, path)?;

        RunCommand::new(CARGO.toolchain(toolchain))
            .args(&["fetch", "--locked", "--manifest-path", "Cargo.toml"])
            .cd(path)
            .run()?;

        Ok(())
    })
}

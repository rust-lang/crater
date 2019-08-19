use crate::crates::Crate;
use crate::dirs::crate_source_dir;
use crate::experiments::Experiment;
use crate::prelude::*;
use crate::results::{BrokenReason, TestResult, WriteResults};

use crate::runner::toml_frobber::TomlFrobber;
use crate::runner::OverrideResult;
use crate::toolchain::Toolchain;
use crate::tools::CARGO;
use rustwide::{cmd::Command, Workspace};
use std::path::PathBuf;

pub(super) struct PrepareCrate<'a, DB: WriteResults + 'a> {
    workspace: &'a Workspace,
    experiment: &'a Experiment,
    krate: &'a Crate,
    db: &'a DB,
    source_dirs: Vec<(&'a Toolchain, PathBuf)>,
    lockfile_captured: bool,
}

impl<'a, DB: WriteResults + 'a> PrepareCrate<'a, DB> {
    pub(super) fn new(
        workspace: &'a Workspace,
        experiment: &'a Experiment,
        krate: &'a Crate,
        db: &'a DB,
    ) -> Self {
        let source_dirs = experiment
            .toolchains
            .iter()
            .map(|tc| (tc, crate_source_dir(experiment, tc, krate)))
            .collect();

        PrepareCrate {
            workspace,
            experiment,
            krate,
            db,
            source_dirs,
            lockfile_captured: false,
        }
    }

    pub(super) fn prepare(&mut self) -> Fallible<()> {
        self.krate.fetch(self.workspace)?;
        for (_, source_dir) in &self.source_dirs {
            self.krate.copy_to(source_dir, self.workspace)?;
        }
        self.capture_sha()?;
        self.validate_manifest()?;
        self.frob_toml()?;
        self.capture_lockfile(false)?;
        self.fetch_deps()?;
        Ok(())
    }

    fn capture_sha(&self) -> Fallible<()> {
        if let Crate::GitHub(ref repo) = self.krate {
            let dir = repo.cached_path();
            let r = Command::new(self.workspace, "git")
                .args(&["rev-parse", "HEAD"])
                .cd(&dir)
                .run_capture();

            let sha = match r {
                Ok(out) => {
                    if let Some(shaline) = out.stdout_lines().get(0) {
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

            self.db
                .record_sha(self.experiment, repo, &sha)
                .with_context(|_| {
                    format!("failed to record the sha of GitHub repo {}", repo.slug())
                })?;
        }
        Ok(())
    }

    fn validate_manifest(&self) -> Fallible<()> {
        for (tc, source_dir) in &self.source_dirs {
            info!("validating manifest of {} on toolchain {}", self.krate, tc);

            // Skip crates missing a Cargo.toml
            if !source_dir.join("Cargo.toml").is_file() {
                Err(err_msg(format!("missing Cargo.toml for {}", self.krate))).with_context(
                    |_| OverrideResult(TestResult::BrokenCrate(BrokenReason::CargoToml)),
                )?;
            }

            Command::new(self.workspace, CARGO.toolchain(tc))
                .args(&["read-manifest", "--manifest-path", "Cargo.toml"])
                .cd(source_dir)
                .log_output(false)
                .run()
                .with_context(|_| format!("invalid syntax in {}'s Cargo.toml", self.krate))
                .with_context(|_| {
                    OverrideResult(TestResult::BrokenCrate(BrokenReason::CargoToml))
                })?;
        }
        Ok(())
    }

    fn frob_toml(&self) -> Fallible<()> {
        for (_, source_dir) in &self.source_dirs {
            let path = source_dir.join("Cargo.toml");
            let mut frobber = TomlFrobber::new(&self.krate, &path)?;
            frobber.frob();
            frobber.save(&path)?;
        }
        Ok(())
    }

    fn capture_lockfile(&mut self, force: bool) -> Fallible<()> {
        for (toolchain, source_dir) in &self.source_dirs {
            if !force && source_dir.join("Cargo.lock").exists() {
                info!(
                    "crate {} already has a lockfile, it will not be regenerated",
                    self.krate
                );
                return Ok(());
            }

            let mut yanked_deps = false;
            let res = Command::new(
                self.workspace,
                CARGO.toolchain(toolchain).unstable_features(true),
            )
            .args(&[
                "generate-lockfile",
                "--manifest-path",
                "Cargo.toml",
                "-Zno-index-update",
            ])
            .cd(source_dir)
            .process_lines(&mut |line| {
                if line.contains("failed to select a version for the requirement") {
                    yanked_deps = true;
                }
            })
            .run();
            match res {
                Err(_) if yanked_deps => {
                    return Err(
                        err_msg(format!("crate {} depends on yanked crates", self.krate))
                            .context(OverrideResult(TestResult::BrokenCrate(
                                BrokenReason::Yanked,
                            )))
                            .into(),
                    );
                }
                other => other?,
            }
            self.lockfile_captured = true;
        }
        Ok(())
    }

    fn fetch_deps(&mut self) -> Fallible<()> {
        for (toolchain, source_dir) in &self.source_dirs {
            let mut outdated_lockfile = false;
            let res = Command::new(self.workspace, CARGO.toolchain(toolchain))
                .args(&["fetch", "--locked", "--manifest-path", "Cargo.toml"])
                .cd(source_dir)
                .process_lines(&mut |line| {
                    if line.ends_with(
                        "Cargo.lock needs to be updated but --locked was passed to prevent this",
                    ) {
                        outdated_lockfile = true;
                    }
                })
                .run();
            match res {
                Ok(_) => {}
                Err(_) if outdated_lockfile && !self.lockfile_captured => {
                    info!("the lockfile is outdated, regenerating it");
                    // Force-update the lockfile and recursively call this function to fetch
                    // dependencies again.
                    self.capture_lockfile(true)?;
                    return self.fetch_deps();
                }
                err => return err,
            }
        }
        Ok(())
    }
}

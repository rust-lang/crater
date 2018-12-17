use config::Config;
use crates::Crate;
use dirs::crate_source_dir;
use experiments::Experiment;
use prelude::*;
use results::{FailureReason, TestResult, WriteResults};
use run::RunCommand;
use runner::toml_frobber::TomlFrobber;
use runner::OverrideResult;
use std::path::PathBuf;
use toolchain::Toolchain;
use tools::CARGO;

pub(super) struct PrepareCrate<'a, DB: WriteResults + 'a> {
    experiment: &'a Experiment,
    krate: &'a Crate,
    config: &'a Config,
    db: &'a DB,
    source_dirs: Vec<(&'a Toolchain, PathBuf)>,
}

impl<'a, DB: WriteResults + 'a> PrepareCrate<'a, DB> {
    pub(super) fn new(
        experiment: &'a Experiment,
        krate: &'a Crate,
        config: &'a Config,
        db: &'a DB,
    ) -> Self {
        let source_dirs = experiment
            .toolchains
            .iter()
            .map(|tc| (tc, crate_source_dir(experiment, tc, krate)))
            .collect();

        PrepareCrate {
            experiment,
            krate,
            config,
            db,
            source_dirs,
        }
    }

    pub(super) fn prepare(&self) -> Fallible<()> {
        self.krate.fetch()?;
        for (_, source_dir) in &self.source_dirs {
            self.krate.copy_to(source_dir)?;
        }
        self.capture_sha()?;
        self.validate_manifest()?;
        self.frob_toml()?;
        self.capture_lockfile()?;
        self.fetch_deps()?;
        Ok(())
    }

    fn capture_sha(&self) -> Fallible<()> {
        if let Crate::GitHub(ref repo) = self.krate {
            let dir = repo.cached_path();
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
                    |_| OverrideResult(TestResult::BuildFail(FailureReason::Broken)),
                )?;
            }

            RunCommand::new(CARGO.toolchain(tc))
                .args(&["read-manifest", "--manifest-path", "Cargo.toml"])
                .cd(source_dir)
                .hide_output(true)
                .run()
                .with_context(|_| format!("invalid syntax in {}'s Cargo.toml", self.krate))
                .with_context(|_| OverrideResult(TestResult::BuildFail(FailureReason::Broken)))?;
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

    fn capture_lockfile(&self) -> Fallible<()> {
        for (toolchain, source_dir) in &self.source_dirs {
            if !self.config.should_update_lockfile(&self.krate)
                && self.krate.is_repo()
                && source_dir.join("Cargo.lock").exists()
            {
                info!("crate {} has a lockfile. skipping", self.krate);
                return Ok(());
            }

            RunCommand::new(CARGO.toolchain(toolchain).unstable_features(true))
                .args(&[
                    "generate-lockfile",
                    "--manifest-path",
                    "Cargo.toml",
                    "-Zno-index-update",
                ])
                .cd(source_dir)
                .run()?;
        }
        Ok(())
    }

    fn fetch_deps(&self) -> Fallible<()> {
        for (toolchain, source_dir) in &self.source_dirs {
            RunCommand::new(CARGO.toolchain(toolchain))
                .args(&["fetch", "--locked", "--manifest-path", "Cargo.toml"])
                .cd(source_dir)
                .run()?;
        }
        Ok(())
    }
}

use crate::config::Config;
use crate::crates::{Crate, GitHubRepo};
use crate::experiments::Experiment;
use crate::prelude::*;
use crate::results::{EncodingType, TestResult, WriteResults};
use crate::runner::test::detect_broken;
use crate::runner::{test, RunnerState};
use crate::toolchain::Toolchain;
use crate::utils;
use rustwide::{Build, BuildDirectory, Workspace};
use std::sync::Mutex;

use rustwide::logging::{self, LogStorage};
use std::fmt;

pub(super) struct TaskCtx<'ctx, DB: WriteResults + 'ctx> {
    pub(super) build_dir: &'ctx Mutex<BuildDirectory>,
    pub(super) config: &'ctx Config,
    pub(super) db: &'ctx DB,
    pub(super) experiment: &'ctx Experiment,
    pub(super) toolchain: &'ctx Toolchain,
    pub(super) krate: &'ctx Crate,
    pub(super) state: &'ctx RunnerState,
    pub(super) quiet: bool,
}

impl<'ctx, DB: WriteResults + 'ctx> TaskCtx<'ctx, DB> {
    fn new(
        build_dir: &'ctx Mutex<BuildDirectory>,
        config: &'ctx Config,
        db: &'ctx DB,
        experiment: &'ctx Experiment,
        toolchain: &'ctx Toolchain,
        krate: &'ctx Crate,
        state: &'ctx RunnerState,
        quiet: bool,
    ) -> Self {
        TaskCtx {
            build_dir,
            config,
            db,
            experiment,
            toolchain,
            krate,
            state,
            quiet,
        }
    }
}

pub(super) enum TaskStep {
    Prepare,
    Cleanup,
    Skip { tc: Toolchain },
    BuildAndTest { tc: Toolchain, quiet: bool },
    BuildOnly { tc: Toolchain, quiet: bool },
    CheckOnly { tc: Toolchain, quiet: bool },
    Clippy { tc: Toolchain, quiet: bool },
    Rustdoc { tc: Toolchain, quiet: bool },
    UnstableFeatures { tc: Toolchain },
}

impl fmt::Debug for TaskStep {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let (name, quiet, tc) = match *self {
            TaskStep::Prepare => ("prepare", false, None),
            TaskStep::Cleanup => ("cleanup", false, None),
            TaskStep::Skip { ref tc } => ("skip", false, Some(tc)),
            TaskStep::BuildAndTest { ref tc, quiet } => ("build and test", quiet, Some(tc)),
            TaskStep::BuildOnly { ref tc, quiet } => ("build", quiet, Some(tc)),
            TaskStep::CheckOnly { ref tc, quiet } => ("check", quiet, Some(tc)),
            TaskStep::Clippy { ref tc, quiet } => ("clippy", quiet, Some(tc)),
            TaskStep::Rustdoc { ref tc, quiet } => ("doc", quiet, Some(tc)),
            TaskStep::UnstableFeatures { ref tc } => ("find unstable features on", false, Some(tc)),
        };

        write!(f, "{}", name)?;
        if let Some(tc) = tc {
            write!(f, " {}", tc)?;
        }
        if quiet {
            write!(f, " (quiet)")?;
        }
        Ok(())
    }
}

pub(super) struct Task {
    pub(super) krate: Crate,
    pub(super) step: TaskStep,
}

impl fmt::Debug for Task {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:?} of crate {}", self.step, self.krate)
    }
}

impl Task {
    pub(super) fn mark_as_failed<DB: WriteResults>(
        &self,
        ex: &Experiment,
        db: &DB,
        state: &RunnerState,
        config: &Config,
        err: &failure::Error,
        result: &TestResult,
    ) -> Fallible<()> {
        match self.step {
            TaskStep::Prepare | TaskStep::Cleanup => {}
            TaskStep::Skip { ref tc }
            | TaskStep::BuildAndTest { ref tc, .. }
            | TaskStep::BuildOnly { ref tc, .. }
            | TaskStep::CheckOnly { ref tc, .. }
            | TaskStep::Clippy { ref tc, .. }
            | TaskStep::Rustdoc { ref tc, .. }
            | TaskStep::UnstableFeatures { ref tc } => {
                let log_storage = state
                    .lock()
                    .prepare_logs
                    .get(&self.krate)
                    .map(|s| s.duplicate());
                db.record_result(
                    ex,
                    tc,
                    &self.krate,
                    log_storage,
                    config,
                    EncodingType::Plain,
                    || {
                        error!("this task or one of its parent failed!");
                        utils::report_failure(err);
                        Ok(result.clone())
                    },
                )?;
            }
        }

        Ok(())
    }

    pub(super) fn run<'ctx, 's: 'ctx, DB: WriteResults>(
        &'s self,
        config: &'ctx Config,
        workspace: &Workspace,
        build_dir: &'ctx Mutex<BuildDirectory>,
        ex: &'ctx Experiment,
        db: &'ctx DB,
        state: &'ctx RunnerState,
    ) -> Fallible<()> {
        let (action, test, toolchain, quiet): (_, fn(&TaskCtx<_>, &Build, &_) -> _, _, _) =
            match self.step {
                TaskStep::BuildAndTest { ref tc, quiet } => {
                    ("testing", test::test_build_and_test, tc, quiet)
                }
                TaskStep::BuildOnly { ref tc, quiet } => {
                    ("building", test::test_build_only, tc, quiet)
                }
                TaskStep::CheckOnly { ref tc, quiet } => {
                    ("checking", test::test_check_only, tc, quiet)
                }
                TaskStep::Clippy { ref tc, quiet } => {
                    ("linting", test::test_clippy_only, tc, quiet)
                }
                TaskStep::Rustdoc { ref tc, quiet } => {
                    ("documenting", test::test_rustdoc, tc, quiet)
                }
                TaskStep::UnstableFeatures { ref tc } => (
                    "checking unstable",
                    crate::runner::unstable_features::find_unstable_features,
                    tc,
                    false,
                ),
                TaskStep::Cleanup => {
                    // Remove stored logs
                    state.lock().prepare_logs.remove(&self.krate);
                    return Ok(());
                }
                TaskStep::Prepare => {
                    let storage = LogStorage::from(config);
                    state
                        .lock()
                        .prepare_logs
                        .insert(self.krate.clone(), storage.clone());
                    logging::capture(&storage, || {
                        let rustwide_crate = self.krate.to_rustwide();
                        for attempt in 1..=15 {
                            match detect_broken(rustwide_crate.fetch(workspace)) {
                                Ok(()) => {}
                                Err(e) => {
                                    if storage.to_string().contains("No space left on device") {
                                        if attempt == 15 {
                                            // If we've failed 15 times, then
                                            // just give up. It's been at least
                                            // 45 seconds, which is enough that
                                            // our disk space check should
                                            // have run at least once in this
                                            // time. If that's not helped, then
                                            // maybe this git repository *is*
                                            // actually too big.
                                            //
                                            // Ideally we'd have some kind of
                                            // per-worker counter and if we hit
                                            // this too often we'd replace the
                                            // machine, but it's not very clear
                                            // what "too often" means here.
                                            return Err(e);
                                        } else {
                                            log::warn!(
                                                "Retrying crate fetch in 3 seconds (attempt {})",
                                                attempt
                                            );
                                            std::thread::sleep(std::time::Duration::from_secs(3));
                                        }
                                    } else {
                                        return Err(e);
                                    }
                                }
                            }
                        }

                        if let Crate::GitHub(repo) = &self.krate {
                            if let Some(sha) = rustwide_crate.git_commit(workspace) {
                                let updated = GitHubRepo {
                                    sha: Some(sha),
                                    ..repo.clone()
                                };
                                db.update_crate_version(
                                    ex,
                                    &Crate::GitHub(repo.clone()),
                                    &Crate::GitHub(updated),
                                )
                                .with_context(|_| {
                                    format!(
                                        "failed to record the sha of GitHub repo {}",
                                        repo.slug()
                                    )
                                })?;
                            } else {
                                bail!("unable to capture sha for {}", repo.slug());
                            }
                        }
                        Ok(())
                    })?;
                    return Ok(());
                }
                TaskStep::Skip { ref tc } => {
                    // If a skipped crate is somehow sent to the agent (for example, when a crate was
                    // added to the experiment and *then* blacklisted) report the crate as skipped
                    // instead of silently ignoring it.
                    db.record_result(
                        ex,
                        tc,
                        &self.krate,
                        None,
                        config,
                        EncodingType::Plain,
                        || {
                            warn!("crate skipped");
                            Ok(TestResult::Skipped)
                        },
                    )?;
                    return Ok(());
                }
            };

        let ctx = TaskCtx::new(
            build_dir,
            config,
            db,
            ex,
            toolchain,
            &self.krate,
            state,
            quiet,
        );
        test::run_test(action, &ctx, test)?;

        Ok(())
    }
}

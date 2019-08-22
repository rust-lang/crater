use crate::config::Config;
use crate::crates::Crate;
use crate::experiments::Experiment;
use crate::prelude::*;
use crate::results::{EncodingType, TestResult, WriteResults};
use crate::runner::{test, RunnerState};
use crate::toolchain::Toolchain;
use crate::utils;
use failure::AsFail;
use rustwide::{BuildDirectory, Workspace};
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
            TaskStep::BuildAndTest { ref tc, quiet } => ("build and test", quiet, Some(tc)),
            TaskStep::BuildOnly { ref tc, quiet } => ("build", quiet, Some(tc)),
            TaskStep::CheckOnly { ref tc, quiet } => ("check", quiet, Some(tc)),
            TaskStep::Clippy { ref tc, quiet } => ("clippy", quiet, Some(tc)),
            TaskStep::Rustdoc { ref tc, quiet } => ("doc", quiet, Some(tc)),
            TaskStep::UnstableFeatures { ref tc } => ("find unstable features on", false, Some(tc)),
        };

        write!(f, "{}", name)?;
        if let Some(tc) = tc {
            write!(f, " {}", tc.to_string())?;
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
    pub(super) fn needs_exec<DB: WriteResults>(&self, ex: &Experiment, db: &DB) -> bool {
        // If an error happens while checking if the task should be executed, the error is ignored
        // and the function returns true.
        match self.step {
            TaskStep::Cleanup => true,
            // The prepare step should always be executed.
            // It will not be executed if all the dependent tasks are already executed, since the
            // runner will not reach the prepare task in that case.
            TaskStep::Prepare => true,
            // Build tasks should only be executed if there are no results for them
            TaskStep::BuildAndTest { ref tc, .. }
            | TaskStep::BuildOnly { ref tc, .. }
            | TaskStep::CheckOnly { ref tc, .. }
            | TaskStep::Clippy { ref tc, .. }
            | TaskStep::Rustdoc { ref tc, .. }
            | TaskStep::UnstableFeatures { ref tc } => {
                db.get_result(ex, tc, &self.krate).unwrap_or(None).is_none()
            }
        }
    }

    pub(super) fn mark_as_failed<DB: WriteResults, F: AsFail>(
        &self,
        ex: &Experiment,
        db: &DB,
        state: &RunnerState,
        config: &Config,
        err: &F,
        result: TestResult,
    ) -> Fallible<()> {
        match self.step {
            TaskStep::Prepare | TaskStep::Cleanup => {}
            TaskStep::BuildAndTest { ref tc, .. }
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
                        Ok(result)
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
        match self.step {
            TaskStep::Cleanup => {
                // Remove stored logs
                state.lock().prepare_logs.remove(&self.krate);
            }
            TaskStep::Prepare => {
                let storage = LogStorage::from(config);
                state
                    .lock()
                    .prepare_logs
                    .insert(self.krate.clone(), storage.clone());
                logging::capture(&storage, || {
                    let rustwide_crate = self.krate.to_rustwide();
                    rustwide_crate.fetch(workspace)?;

                    if let Crate::GitHub(repo) = &self.krate {
                        if let Some(sha) = rustwide_crate.git_commit(workspace) {
                            db.record_sha(ex, repo, &sha).with_context(|_| {
                                format!("failed to record the sha of GitHub repo {}", repo.slug())
                            })?;
                        } else {
                            bail!("unable to capture sha for {}", repo.slug());
                        }
                    }
                    Ok(())
                })?;
            }
            TaskStep::BuildAndTest { ref tc, quiet } => {
                let ctx = TaskCtx::new(build_dir, config, db, ex, tc, &self.krate, state, quiet);
                test::run_test("testing", &ctx, test::test_build_and_test)?;
            }
            TaskStep::BuildOnly { ref tc, quiet } => {
                let ctx = TaskCtx::new(build_dir, config, db, ex, tc, &self.krate, state, quiet);
                test::run_test("building", &ctx, test::test_build_only)?;
            }
            TaskStep::CheckOnly { ref tc, quiet } => {
                let ctx = TaskCtx::new(build_dir, config, db, ex, tc, &self.krate, state, quiet);
                test::run_test("checking", &ctx, test::test_check_only)?;
            }
            TaskStep::Clippy { ref tc, quiet } => {
                let ctx = TaskCtx::new(build_dir, config, db, ex, tc, &self.krate, state, quiet);
                test::run_test("linting", &ctx, test::test_clippy_only)?;
            }
            TaskStep::Rustdoc { ref tc, quiet } => {
                let ctx = TaskCtx::new(build_dir, config, db, ex, tc, &self.krate, state, quiet);
                test::run_test("documenting", &ctx, test::test_rustdoc)?;
            }
            TaskStep::UnstableFeatures { ref tc } => {
                let ctx = TaskCtx::new(build_dir, config, db, ex, tc, &self.krate, state, false);
                test::run_test(
                    "checking unstable",
                    &ctx,
                    crate::runner::unstable_features::find_unstable_features,
                )?;
            }
        }

        Ok(())
    }
}

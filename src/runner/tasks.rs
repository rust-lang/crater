use config::Config;
use crates::Crate;
use dirs;
use docker::DockerEnv;
use experiments::Experiment;
use failure::AsFail;
use prelude::*;
use results::{TestResult, WriteResults};
use runner::prepare::PrepareCrate;
use runner::test;
use std::fmt;
use toolchain::Toolchain;
use utils;

pub(super) struct TaskCtx<'ctx, DB: WriteResults + 'ctx> {
    pub(super) config: &'ctx Config,
    pub(super) db: &'ctx DB,
    pub(super) experiment: &'ctx Experiment,
    pub(super) toolchain: &'ctx Toolchain,
    pub(super) krate: &'ctx Crate,
    pub(super) docker_env: &'ctx DockerEnv,
    pub(super) quiet: bool,
}

impl<'ctx, DB: WriteResults + 'ctx> TaskCtx<'ctx, DB> {
    fn new(
        config: &'ctx Config,
        db: &'ctx DB,
        experiment: &'ctx Experiment,
        toolchain: &'ctx Toolchain,
        krate: &'ctx Crate,
        docker_env: &'ctx DockerEnv,
        quiet: bool,
    ) -> Self {
        TaskCtx {
            config,
            db,
            experiment,
            toolchain,
            krate,
            docker_env,
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
    Rustdoc { tc: Toolchain, quiet: bool },
    UnstableFeatures { tc: Toolchain },
}

impl fmt::Debug for TaskStep {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            TaskStep::Prepare => write!(f, "prepare")?,
            TaskStep::Cleanup => write!(f, "cleanup")?,
            TaskStep::BuildAndTest { ref tc, quiet } => {
                write!(f, "build and test {}", tc.to_string())?;
                if quiet {
                    write!(f, " (quiet)")?;
                }
            }
            TaskStep::BuildOnly { ref tc, quiet } => {
                write!(f, "build {}", tc.to_string())?;
                if quiet {
                    write!(f, " (quiet)")?;
                }
            }
            TaskStep::CheckOnly { ref tc, quiet } => {
                write!(f, "check {}", tc.to_string())?;
                if quiet {
                    write!(f, " (quiet)")?;
                }
            }
            TaskStep::Rustdoc { ref tc, quiet } => {
                write!(f, "doc {}", tc.to_string())?;
                if quiet {
                    write!(f, " (quiet)")?;
                }
            }
            TaskStep::UnstableFeatures { ref tc } => {
                write!(f, "find unstable features on {}", tc.to_string())?;
            }
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
        err: &F,
        result: TestResult,
    ) -> Fallible<()> {
        match self.step {
            TaskStep::Prepare | TaskStep::Cleanup => {}
            TaskStep::BuildAndTest { ref tc, .. }
            | TaskStep::BuildOnly { ref tc, .. }
            | TaskStep::CheckOnly { ref tc, .. }
            | TaskStep::Rustdoc { ref tc, .. }
            | TaskStep::UnstableFeatures { ref tc } => {
                db.record_result(ex, tc, &self.krate, || {
                    error!("this task or one of its parent failed!");
                    utils::report_failure(err);
                    Ok(result)
                })?;
            }
        }

        Ok(())
    }

    pub(super) fn run<DB: WriteResults>(
        &self,
        config: &Config,
        ex: &Experiment,
        db: &DB,
        docker_env: &DockerEnv,
    ) -> Fallible<()> {
        match self.step {
            TaskStep::Cleanup => {
                // Ensure source directories are cleaned up
                for tc in &ex.toolchains {
                    let _ = utils::fs::remove_dir_all(&dirs::crate_source_dir(ex, tc, &self.krate));
                }
            }
            TaskStep::Prepare => {
                let prepare = PrepareCrate::new(ex, &self.krate, config, db);
                prepare.prepare()?;
            }
            TaskStep::BuildAndTest { ref tc, quiet } => {
                let ctx = TaskCtx::new(config, db, ex, tc, &self.krate, docker_env, quiet);
                test::run_test("testing", &ctx, test::test_build_and_test)?;
            }
            TaskStep::BuildOnly { ref tc, quiet } => {
                let ctx = TaskCtx::new(config, db, ex, tc, &self.krate, docker_env, quiet);
                test::run_test("building", &ctx, test::test_build_only)?;
            }
            TaskStep::CheckOnly { ref tc, quiet } => {
                let ctx = TaskCtx::new(config, db, ex, tc, &self.krate, docker_env, quiet);
                test::run_test("checking", &ctx, test::test_check_only)?;
            }
            TaskStep::Rustdoc { ref tc, quiet } => {
                let ctx = TaskCtx::new(config, db, ex, tc, &self.krate, docker_env, quiet);
                test::run_test("documenting", &ctx, test::test_rustdoc)?;
            }
            TaskStep::UnstableFeatures { ref tc } => {
                let ctx = TaskCtx::new(config, db, ex, tc, &self.krate, docker_env, false);
                test::run_test(
                    "checking unstable",
                    &ctx,
                    ::runner::unstable_features::find_unstable_features,
                )?;
            }
        }

        Ok(())
    }
}

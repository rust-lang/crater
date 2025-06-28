use crate::config::Config;
use crate::crates::Crate;
use crate::experiments::Experiment;
use crate::prelude::*;
use crate::results::TestResult;
use crate::runner::test;
use crate::toolchain::Toolchain;
use rustwide::{Build, BuildDirectory};
use std::collections::HashMap;
use std::sync::Mutex;

use rustwide::logging::LogStorage;
use std::fmt;

pub(super) struct TaskCtx<'ctx> {
    pub(super) build_dir: &'ctx Mutex<BuildDirectory>,
    pub(super) config: &'ctx Config,
    pub(super) experiment: &'ctx Experiment,
    pub(super) toolchain: &'ctx Toolchain,
    pub(super) krate: &'ctx Crate,
    pub(super) quiet: bool,
}

impl<'ctx> TaskCtx<'ctx> {
    fn new(
        build_dir: &'ctx Mutex<BuildDirectory>,
        config: &'ctx Config,
        experiment: &'ctx Experiment,
        toolchain: &'ctx Toolchain,
        krate: &'ctx Crate,
        quiet: bool,
    ) -> Self {
        TaskCtx {
            build_dir,
            config,
            experiment,
            toolchain,
            krate,
            quiet,
        }
    }
}

pub(super) enum TaskStep {
    BuildAndTest { tc: Toolchain, quiet: bool },
    BuildOnly { tc: Toolchain, quiet: bool },
    CheckOnly { tc: Toolchain, quiet: bool },
    Clippy { tc: Toolchain, quiet: bool },
    Rustdoc { tc: Toolchain, quiet: bool },
    UnstableFeatures { tc: Toolchain },
    Fix { tc: Toolchain, quiet: bool },
}

impl fmt::Debug for TaskStep {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let (name, quiet, tc) = match *self {
            TaskStep::BuildAndTest { ref tc, quiet } => ("build and test", quiet, Some(tc)),
            TaskStep::BuildOnly { ref tc, quiet } => ("build", quiet, Some(tc)),
            TaskStep::CheckOnly { ref tc, quiet } => ("check", quiet, Some(tc)),
            TaskStep::Clippy { ref tc, quiet } => ("clippy", quiet, Some(tc)),
            TaskStep::Rustdoc { ref tc, quiet } => ("doc", quiet, Some(tc)),
            TaskStep::UnstableFeatures { ref tc } => ("find unstable features on", false, Some(tc)),
            TaskStep::Fix { ref tc, quiet } => ("fix", quiet, Some(tc)),
        };

        write!(f, "{name}")?;
        if let Some(tc) = tc {
            write!(f, " {tc}")?;
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
    pub(super) fn run<'ctx, 's: 'ctx>(
        &'s self,
        config: &'ctx Config,
        build_dir: &'ctx HashMap<&'ctx crate::toolchain::Toolchain, Mutex<BuildDirectory>>,
        ex: &'ctx Experiment,
        logs: &LogStorage,
    ) -> Fallible<TestResult> {
        let (build_dir, action, test, toolchain, quiet): (
            _,
            _,
            fn(&TaskCtx, &Build, &_) -> _,
            _,
            _,
        ) = match self.step {
            TaskStep::BuildAndTest { ref tc, quiet } => (
                &build_dir[tc],
                "testing",
                test::test_build_and_test,
                tc,
                quiet,
            ),
            TaskStep::BuildOnly { ref tc, quiet } => {
                (&build_dir[tc], "building", test::test_build_only, tc, quiet)
            }
            TaskStep::CheckOnly { ref tc, quiet } => {
                (&build_dir[tc], "checking", test::test_check_only, tc, quiet)
            }
            TaskStep::Clippy { ref tc, quiet } => {
                (&build_dir[tc], "linting", test::test_clippy_only, tc, quiet)
            }
            TaskStep::Rustdoc { ref tc, quiet } => {
                (&build_dir[tc], "documenting", test::test_rustdoc, tc, quiet)
            }
            TaskStep::UnstableFeatures { ref tc } => (
                &build_dir[tc],
                "checking unstable",
                crate::runner::unstable_features::find_unstable_features,
                tc,
                false,
            ),
            TaskStep::Fix { ref tc, quiet } => (&build_dir[tc], "fixing", test::fix, tc, quiet),
        };

        let ctx = TaskCtx::new(build_dir, config, ex, toolchain, &self.krate, quiet);
        test::run_test(action, &ctx, test, logs)
    }
}

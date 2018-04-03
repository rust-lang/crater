use crates::{self, Crate};
use errors::*;
use ex::{self, Experiment};
use ex_run;
use gh_mirrors;
use results::ExperimentResultDB;
use std::fmt;
use toolchain::Toolchain;
use util;

pub enum TaskStep {
    Prepare,
    BuildAndTest { tc: Toolchain, quiet: bool },
    BuildOnly { tc: Toolchain, quiet: bool },
    CheckOnly { tc: Toolchain, quiet: bool },
    UnstableFeatures { tc: Toolchain },
}

impl fmt::Debug for TaskStep {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            TaskStep::Prepare => write!(f, "prepare")?,
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
            TaskStep::UnstableFeatures { ref tc } => {
                write!(f, "find unstable features on {}", tc.to_string())?;
            }
        }
        Ok(())
    }
}

pub struct Task {
    pub krate: Crate,
    pub step: TaskStep,
}

impl fmt::Debug for Task {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:?} of crate {}", self.step, self.krate)
    }
}

impl Task {
    pub fn run<DB: ExperimentResultDB>(&self, ex: &Experiment, db: &DB) -> Result<()> {
        match self.step {
            TaskStep::Prepare => self.run_prepare(ex),
            TaskStep::BuildAndTest { ref tc, quiet } => self.run_build_and_test(ex, tc, db, quiet),
            TaskStep::BuildOnly { ref tc, quiet } => self.run_build_only(ex, tc, db, quiet),
            TaskStep::CheckOnly { ref tc, quiet } => self.run_check_only(ex, tc, db, quiet),
            TaskStep::UnstableFeatures { ref tc } => self.run_unstable_features(ex, db, tc),
        }
    }

    fn run_prepare(&self, ex: &Experiment) -> Result<()> {
        // Fetch repository data if it's a git repo
        if let Some(url) = self.krate.github().map(|repo| repo.url()) {
            if let Err(e) = gh_mirrors::fetch(&url) {
                util::report_error(&e);
            }

            ex.shas
                .lock()
                .unwrap()
                .capture(::std::iter::once(url.as_str()))?;
        }

        let ex_crate = [self.krate.clone().into_ex_crate(ex)?];
        let stable = Toolchain::Dist("stable".into());

        crates::prepare(&ex_crate)?;
        ex::frob_tomls(ex, &ex_crate)?;
        ex::capture_lockfiles(ex, &ex_crate, &stable, false)?;
        ex::fetch_deps(ex, &ex_crate, &stable)?;

        Ok(())
    }

    fn run_build_and_test<DB: ExperimentResultDB>(
        &self,
        ex: &Experiment,
        tc: &Toolchain,
        db: &DB,
        quiet: bool,
    ) -> Result<()> {
        let krate = self.krate.clone().into_ex_crate(ex)?;
        ex_run::run_test(
            "testing",
            ex,
            tc,
            &krate,
            db,
            quiet,
            ex_run::test_build_and_test,
        ).map(|_| ())
    }

    fn run_build_only<DB: ExperimentResultDB>(
        &self,
        ex: &Experiment,
        tc: &Toolchain,
        db: &DB,
        quiet: bool,
    ) -> Result<()> {
        let krate = self.krate.clone().into_ex_crate(ex)?;
        ex_run::run_test(
            "testing",
            ex,
            tc,
            &krate,
            db,
            quiet,
            ex_run::test_build_only,
        ).map(|_| ())
    }

    fn run_check_only<DB: ExperimentResultDB>(
        &self,
        ex: &Experiment,
        tc: &Toolchain,
        db: &DB,
        quiet: bool,
    ) -> Result<()> {
        let krate = self.krate.clone().into_ex_crate(ex)?;
        ex_run::run_test(
            "checking",
            ex,
            tc,
            &krate,
            db,
            quiet,
            ex_run::test_check_only,
        ).map(|_| ())
    }

    fn run_unstable_features<DB: ExperimentResultDB>(
        &self,
        ex: &Experiment,
        db: &DB,
        tc: &Toolchain,
    ) -> Result<()> {
        let krate = self.krate.clone().into_ex_crate(ex)?;
        ex_run::run_test(
            "checking",
            ex,
            tc,
            &krate,
            db,
            false,
            ex_run::test_find_unstable_features,
        ).map(|_| ())
    }
}

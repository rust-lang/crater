use crates;
use errors::*;
use ex::{self, Experiment};
use ex_run;
use gh_mirrors;
use lists::Crate;
use toolchain::Toolchain;
use util;
use std::fmt;

pub enum TaskStep {
    Prepare,
    BuildAndTest { tc: Toolchain, quiet: bool, },
    BuildOnly { tc: Toolchain, quiet: bool, }
}

impl fmt::Debug for TaskStep {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            TaskStep::Prepare => write!(f, "prepare")?,
            TaskStep::BuildAndTest {
                ref tc,
                quiet,
            } => {
                write!(f, "build and test {}", tc.to_string())?;
                if quiet {
                    write!(f, " (quiet)")?;
                }
            }
            TaskStep::BuildOnly {
                ref tc,
                quiet,
            } => {
                write!(f, "build {}", tc.to_string())?;
                if quiet {
                    write!(f, " (quiet)")?;
                }
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
    pub fn run(&self, ex: &mut Experiment) -> Result<()> {
        match &self.step {
            &TaskStep::Prepare => self.run_prepare(ex),
            &TaskStep::BuildAndTest { ref tc, quiet, } => self.run_build_and_test(ex, tc, quiet),
            &TaskStep::BuildOnly { ref tc, quiet, } => self.run_build_only(ex, tc, quiet),
        }
    }

    fn run_prepare(&self, ex: &mut Experiment) -> Result<()> {
        // Fetch repository data if it's a git repo
        if let Some(url) = self.krate.repo_url() {
            if let Err(e) = gh_mirrors::fetch(url) {
                util::report_error(&e);
            }

            ex.shas.capture(::std::iter::once(url))?;
        }

        let ex_crate = [self.krate.clone().into_ex_crate(ex)?];
        let stable = Toolchain::Dist("stable".into());

        crates::prepare(&ex_crate)?;
        ex::frob_tomls(ex, &ex_crate)?;
        ex::capture_lockfiles(ex, &ex_crate, &stable, false)?;
        ex::fetch_deps(ex, &ex_crate, &stable)?;

        Ok(())
    }

    fn run_build_and_test(&self, ex: &mut Experiment, tc: &Toolchain, quiet: bool) -> Result<()> {
        let krate = self.krate.clone().into_ex_crate(ex)?;
        ex_run::run_test("testing", ex, tc, &krate, quiet, ex_run::test_build_and_test).map(|_| ())
    }

    fn run_build_only(&self, ex: &mut Experiment, tc: &Toolchain, quiet: bool) -> Result<()> {
        let krate = self.krate.clone().into_ex_crate(ex)?;
        ex_run::run_test("testing", ex, tc, &krate, quiet, ex_run::test_build_only).map(|_| ())
    }
}

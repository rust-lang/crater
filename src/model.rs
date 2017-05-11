/*!

Cargobomb works by serially processing a queue of commands, each of
which transforms the application state in some discrete way, and
designed to be resilient to I/O errors. The application state is
backed by a directory in the filesystem, and optionally synchronized
with s3.

These command queues may be created dynamically and executed in
parallel jobs, either locally, or distributed on e.g. AWS. The
application state employs ownership techniques to ensure that
parallel access is consistent and race-free.

NB: The design of this module is SERIOUSLY MESSED UP, with lots of
duplication, the result of a deep yak shave that failed. It needs a
rewrite.

*/

use errors::*;
use toolchain::Toolchain;

// An experiment name
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Ex(String);

pub trait Cmd {
    fn run(&self) -> Result<()>;
}

struct PrepareLocal;
struct DefineEx(Ex, Toolchain, Toolchain, ExMode, ExCrateSelect);
struct PrepareEx(Ex);
struct Run(Ex);
struct RunTc(Ex, Toolchain);
struct GenReport(Ex);
struct DeleteAllTargetDirs(Ex);

struct CreateListsFull;

struct CopyEx(Ex, Ex);
struct DeleteEx(Ex);

struct DeleteAllResults(Ex);

#[derive(Serialize, Deserialize)]
#[derive(Debug, Clone)]
pub enum ExMode {
    BuildAndTest,
    BuildOnly,
    CheckOnly,
    UnstableFeatures,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum ExCrateSelect {
    Full,
    Demo,
    SmallRandom,
    Top100,
}

use docker;
use ex;
use ex_run;
use lists;
use report;

// Local prep
impl Cmd for PrepareLocal {
    fn run(&self) -> Result<()> {
        let stable_tc = Toolchain::Dist("stable".into());
        stable_tc.prepare()?;
        docker::build_container()?;
        lists::create_all_lists(false)
    }
}

// List creation
impl Cmd for CreateListsFull {
    fn run(&self) -> Result<()> {
        lists::create_all_lists(true)
    }
}

// Experiment prep
impl Cmd for DefineEx {
    fn run(&self) -> Result<()> {
        let &DefineEx(ref ex, ref tc1, ref tc2, ref mode, ref crates) = self;
        ex::define(ex::ExOpts {
                       name: ex.0.clone(),
                       toolchains: vec![tc1.clone(), tc2.clone()],
                       mode: mode.clone(),
                       crates: crates.clone(),
                   })
    }
}
impl Cmd for PrepareEx {
    fn run(&self) -> Result<()> {
        let &PrepareEx(ref ex) = self;
        // Shared emperiment prep
        ex::fetch_gh_mirrors(&ex.0)?;
        ex::capture_shas(&ex.0)?;
        ex::download_crates(&ex.0)?;
        ex::frob_tomls(&ex.0)?;
        ex::capture_lockfiles(&ex.0, &Toolchain::Dist("stable".into()), false)?;

        // Local experiment prep
        ex::delete_all_target_dirs(&ex.0);
        ex::delete_all_target_dirs(&ex.0);
        ex::fetch_deps(&ex.0, &Toolchain::Dist("stable".into()))?;
        ex::prepare_all_toolchains(&ex.0)?;

        Ok(())
    }
}
impl Cmd for CopyEx {
    fn run(&self) -> Result<()> {
        let &CopyEx(ref ex1, ref ex2) = self;
        ex::copy(&ex1.0, &ex2.0)
    }
}
impl Cmd for DeleteEx {
    fn run(&self) -> Result<()> {
        let &DeleteEx(ref ex) = self;
        ex::delete(&ex.0)
    }
}

impl Cmd for DeleteAllTargetDirs {
    fn run(&self) -> Result<()> {
        let &DeleteAllTargetDirs(ref ex) = self;
        ex::delete_all_target_dirs(&ex.0)
    }
}
impl Cmd for DeleteAllResults {
    fn run(&self) -> Result<()> {
        let &DeleteAllResults(ref ex) = self;
        ex_run::delete_all_results(&ex.0)
    }
}

// Experimenting
impl Cmd for Run {
    fn run(&self) -> Result<()> {
        let &Run(ref ex) = self;
        ex_run::run_ex_all_tcs(&ex.0)
    }
}
impl Cmd for RunTc {
    fn run(&self) -> Result<()> {
        let &RunTc(ref ex, ref tc) = self;
        ex_run::run_ex(&ex.0, tc.clone())
    }
}

// Reporting
impl Cmd for GenReport {
    fn run(&self) -> Result<()> {
        let &GenReport(ref ex) = self;
        report::gen(&ex.0)
    }
}

// Boilerplate conversions on the model. Ideally all this would be generated.
pub mod conv {
    use super::*;

    use clap::{App, Arg, ArgMatches, SubCommand};
    use std::str::FromStr;

    pub fn clap_cmds() -> Vec<App<'static, 'static>> {
        // Types of arguments
        let ex = || opt("ex", "default");
        let ex1 = || req("ex-1");
        let ex2 = || req("ex-2");
        let req_tc = || req("tc");
        let tc1 = || req("tc-1");
        let tc2 = || req("tc-2");
        let mode = || {
            Arg::with_name("mode")
                .required(false)
                .long("mode")
                .default_value(ExMode::BuildAndTest.to_str())
                .possible_values(&[
                    ExMode::BuildAndTest.to_str(),
                    ExMode::BuildOnly.to_str(),
                    ExMode::CheckOnly.to_str(),
                    ExMode::UnstableFeatures.to_str(),
                ])
        };
        let crate_select = || {
            Arg::with_name("crate-select")
                .required(false)
                .long("crate-select")
                .default_value(ExCrateSelect::Demo.to_str())
                .possible_values(&[
                    ExCrateSelect::Demo.to_str(),
                    ExCrateSelect::Full.to_str(),
                    ExCrateSelect::SmallRandom.to_str(),
                    ExCrateSelect::Top100.to_str(),
                ])
        };

        fn opt(n: &'static str, def: &'static str) -> Arg<'static, 'static> {
            Arg::with_name(n).required(false).long(n).default_value(def)
        }

        fn req(n: &'static str) -> Arg<'static, 'static> {
            Arg::with_name(n).required(true)
        }

        fn cmd(n: &'static str, desc: &'static str) -> App<'static, 'static> {
            SubCommand::with_name(n).about(desc)
        }

        vec![
            // Local prep
            cmd("prepare-local",
                "acquire toolchains, build containers, build crate lists"),

            // List creation
            cmd("create-lists-full", "create all the lists of crates"),

            // Master experiment prep
            cmd("define-ex", "define an experiment")
                .arg(ex())
                .arg(tc1())
                .arg(tc2())
                .arg(mode())
                .arg(crate_select()),
            cmd("prepare-ex", "prepare shared and local data for experiment").arg(ex()),
            cmd("copy-ex", "copy all data from one experiment to another")
                .arg(ex1())
                .arg(ex2()),
            cmd("delete-ex", "delete shared data for experiment").arg(ex()),

            cmd("delete-all-target-dirs",
                "delete the cargo target dirs for an experiment")
                    .arg(ex()),
            cmd("delete-all-results", "delete all results for an experiment").arg(ex()),

            // Experimenting
            cmd("run", "run an experiment, with all toolchains").arg(ex()),
            cmd("run-tc", "run an experiment, with a single toolchain")
                .arg(ex())
                .arg(req_tc()),

            // Reporting
            cmd("gen-report", "generate the experiment report").arg(ex()),
        ]
    }

    pub fn clap_args_to_cmd(m: &ArgMatches) -> Result<Box<Cmd>> {

        fn ex(m: &ArgMatches) -> Result<Ex> {
            m.value_of("ex").expect("").parse::<Ex>()
        }

        fn ex1(m: &ArgMatches) -> Result<Ex> {
            m.value_of("ex-1").expect("").parse::<Ex>()
        }

        fn ex2(m: &ArgMatches) -> Result<Ex> {
            m.value_of("ex-2").expect("").parse::<Ex>()
        }

        fn tc(m: &ArgMatches) -> Result<Toolchain> {
            m.value_of("tc").expect("").parse()
        }

        fn tc1(m: &ArgMatches) -> Result<Toolchain> {
            m.value_of("tc-1").expect("").parse()
        }

        fn tc2(m: &ArgMatches) -> Result<Toolchain> {
            m.value_of("tc-2").expect("").parse()
        }

        fn mode(m: &ArgMatches) -> Result<ExMode> {
            m.value_of("mode").expect("").parse::<ExMode>()
        }

        fn crate_select(m: &ArgMatches) -> Result<ExCrateSelect> {
            m.value_of("crate-select")
                .expect("")
                .parse::<ExCrateSelect>()
        }

        Ok(match m.subcommand() {
               // Local prep
               ("prepare-local", _) => Box::new(PrepareLocal),
               ("create-list-full", _) => Box::new(CreateListsFull),

               // Master experiment prep
               ("define-ex", Some(m)) => {
                   Box::new(DefineEx(ex(m)?, tc1(m)?, tc2(m)?, mode(m)?, crate_select(m)?))
               }
               ("prepare-ex", Some(m)) => Box::new(PrepareEx(ex(m)?)),
               ("copy-ex", Some(m)) => Box::new(CopyEx(ex1(m)?, ex2(m)?)),
               ("delete-ex", Some(m)) => Box::new(DeleteEx(ex(m)?)),

               // Local experiment prep
               ("delete-all-target-dirs", Some(m)) => Box::new(DeleteAllTargetDirs(ex(m)?)),
               ("delete-all-results", Some(m)) => Box::new(DeleteAllResults(ex(m)?)),

               // Experimenting
               ("run", Some(m)) => Box::new(Run(ex(m)?)),
               ("run-tc", Some(m)) => Box::new(RunTc(ex(m)?, tc(m)?)),

               // Reporting
               ("gen-report", Some(m)) => Box::new(GenReport(ex(m)?)),

               (s, _) => panic!("unimplemented args_to_cmd {}", s),
           })
    }

    impl FromStr for ExMode {
        type Err = Error;

        fn from_str(s: &str) -> Result<ExMode> {
            Ok(match s {
                   "build-and-test" => ExMode::BuildAndTest,
                   "build-only" => ExMode::BuildOnly,
                   "check-only" => ExMode::CheckOnly,
                   "unstable-features" => ExMode::UnstableFeatures,
                   s => bail!("invalid ex-mode: {}", s),
               })
        }
    }

    impl ExMode {
        pub fn to_str(&self) -> &'static str {
            match *self {
                ExMode::BuildAndTest => "build-and-test",
                ExMode::BuildOnly => "build-only",
                ExMode::CheckOnly => "check-only",
                ExMode::UnstableFeatures => "unstable-features",
            }
        }
    }

    impl FromStr for ExCrateSelect {
        type Err = Error;

        fn from_str(s: &str) -> Result<ExCrateSelect> {
            Ok(match s {
                   "full" => ExCrateSelect::Full,
                   "demo" => ExCrateSelect::Demo,
                   "small-random" => ExCrateSelect::SmallRandom,
                   "top-100" => ExCrateSelect::Top100,
                   s => bail!("invalid crate-select: {}", s),
               })
        }
    }

    impl ExCrateSelect {
        pub fn to_str(&self) -> &'static str {
            match *self {
                ExCrateSelect::Full => "full",
                ExCrateSelect::Demo => "demo",
                ExCrateSelect::SmallRandom => "small-random",
                ExCrateSelect::Top100 => "top-100",
            }
        }
    }

    impl FromStr for Ex {
        type Err = Error;

        fn from_str(ex: &str) -> Result<Ex> {
            Ok(Ex(ex.to_string()))
        }
    }
}

/*!

Crater works by serially processing a queue of commands, each of
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

use crater::docker;
use crater::errors::*;
use crater::ex;
use crater::ex::{ExCrate, ExCrateSelect, ExMode};
use crater::ex_run;
use crater::lists;
use crater::report;
use crater::server;
use crater::toolchain::Toolchain;
use std::env;
use std::path::PathBuf;
use std::str::FromStr;

// An experiment name
#[derive(Debug, Clone)]
pub struct Ex(String);

#[derive(Debug, Clone)]
pub struct Dest(PathBuf);

pub trait Cmd {
    fn run(&self) -> Result<()>;
}

struct PrepareLocal;
struct DefineEx(Ex, Toolchain, Toolchain, ExMode, ExCrateSelect);
#[derive(StructOpt)]
#[structopt(name = "prepare-ex", about = "prepare shared and local data for experiment")]
struct PrepareEx {
    #[structopt(name = "experiment", long = "ex", default_value = "default")]
    ex: Ex,
}
#[derive(StructOpt)]
#[structopt(name = "run", about = "run an experiment, with all toolchains")]
struct Run {
    #[structopt(name = "experiment", long = "ex", default_value = "default")]
    ex: Ex,
}
#[derive(StructOpt)]
#[structopt(name = "run-tc", about = "run an experiment, with a single toolchain")]
struct RunTc {
    #[structopt(name = "experiment", long = "ex", default_value = "default")]
    ex: Ex,
    #[structopt(name = "toolchain")]
    tc: Toolchain,
}
#[derive(StructOpt)]
#[structopt(name = "gen-report", about = "generate the experiment report")]
struct GenReport {
    #[structopt(name = "experiment", long = "ex", default_value = "default")]
    ex: Ex,
    #[structopt(name = "destination")]
    dest: Dest,
}
#[derive(StructOpt)]
#[structopt(name = "publish-report", about = "publish the experiment report to S3")]
struct PublishReport {
    #[structopt(name = "experiment", long = "ex", default_value = "default",
                help = "The experiment to publish a report for.")]
    ex: Ex,
    #[structopt(name = "S3 URI",
                help = "The S3 URI to put the report at. \
                        [default: $CARGOBOMB_REPORT_S3_PREFIX/<experiment>")]
    s3_prefix: Option<report::S3Prefix>,
}
struct DeleteAllTargetDirs(Ex);

struct CreateLists;

struct CopyEx(Ex, Ex);
struct DeleteEx(Ex);

struct DeleteAllResults(Ex);
struct DeleteResult(Ex, Option<Toolchain>, ExCrate);
struct Serve;


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
impl Cmd for CreateLists {
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
        let ex = ex::Experiment::load(&self.ex.0)?;
        ex.prepare_shared()?;
        ex.prepare_local()?;

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

impl Cmd for DeleteResult {
    fn run(&self) -> Result<()> {
        let &DeleteResult(ref ex, ref tc, ref crate_) = self;
        ex_run::delete_result(&ex.0, tc.as_ref(), crate_)
    }
}

// Experimenting
impl Cmd for Run {
    fn run(&self) -> Result<()> {
        ex_run::run_ex_all_tcs(&self.ex.0)
    }
}
impl Cmd for RunTc {
    fn run(&self) -> Result<()> {
        ex_run::run_ex(&self.ex.0, self.tc.clone())
    }
}

// Reporting
impl Cmd for GenReport {
    fn run(&self) -> Result<()> {
        report::gen(
            &self.ex.0,
            &report::FileWriter::create(self.dest.0.clone())?,
        )
    }
}

impl PublishReport {
    fn s3_prefix(&self) -> Result<report::S3Prefix> {
        match self.s3_prefix {
            Some(ref prefix) => Ok(prefix.clone()),
            None => {
                let mut prefix: report::S3Prefix = get_env("CARGOBOMB_REPORT_S3_PREFIX")?;
                prefix.prefix.push(&self.ex.0);
                Ok(prefix)
            }
        }
    }
}

impl Cmd for PublishReport {
    fn run(&self) -> Result<()> {
        report::gen(&self.ex.0, &report::S3Writer::create(self.s3_prefix()?)?)
    }
}

impl Cmd for Serve {
    fn run(&self) -> Result<()> {
        server::start(server::Data);
        Ok(())
    }
}

/// Load and parse and environment variable.
fn get_env<T>(name: &str) -> Result<T>
where
    T: FromStr,
    T::Err: ::std::error::Error + Send + 'static,
{
    env::var(name)
        .chain_err(|| {
            format!{"Need to specify {:?} in environment or `.env`.", name}
        })?
        .parse()
        .chain_err(|| format!{"Couldn't parse {:?}.", name})
}


// Boilerplate conversions on the model. Ideally all this would be generated.
pub mod conv {
    use super::*;

    use clap::{App, Arg, ArgMatches, SubCommand};
    use std::str::FromStr;
    use structopt::StructOpt;

    pub fn clap_cmds() -> Vec<App<'static, 'static>> {
        // Types of arguments
        let ex = || opt("ex", "default");
        let ex1 = || req("ex-1");
        let ex2 = || req("ex-2");
        let tc1 = || req("tc-1");
        let tc2 = || req("tc-2");
        let mode = || {
            Arg::with_name("mode")
                .required(false)
                .long("mode")
                .default_value(ExMode::BuildAndTest.to_str())
                .possible_values(
                    &[
                        ExMode::BuildAndTest.to_str(),
                        ExMode::BuildOnly.to_str(),
                        ExMode::CheckOnly.to_str(),
                        ExMode::UnstableFeatures.to_str(),
                    ],
                )
        };
        let crate_select = || {
            Arg::with_name("crate-select")
                .required(false)
                .long("crate-select")
                .default_value(ExCrateSelect::Demo.to_str())
                .possible_values(
                    &[
                        ExCrateSelect::Demo.to_str(),
                        ExCrateSelect::Full.to_str(),
                        ExCrateSelect::SmallRandom.to_str(),
                        ExCrateSelect::Top100.to_str(),
                    ],
                )
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
            cmd(
                "prepare-local",
                "acquire toolchains, build containers, build crate lists"
            ),

            // List creation
            cmd("create-lists", "create all the lists of crates"),

            // Master experiment prep
            cmd("define-ex", "define an experiment")
                .arg(ex())
                .arg(tc1())
                .arg(tc2())
                .arg(mode())
                .arg(crate_select()),
            PrepareEx::clap(),
            cmd("copy-ex", "copy all data from one experiment to another")
                .arg(ex1())
                .arg(ex2()),
            cmd("delete-ex", "delete shared data for experiment").arg(ex()),

            cmd(
                "delete-all-target-dirs",
                "delete the cargo target dirs for an experiment"
            ).arg(ex()),
            cmd("delete-all-results", "delete all results for an experiment").arg(ex()),
            cmd(
                "delete-result",
                "delete results for a crate from an experiment"
            ).arg(ex())
                .arg(
                    Arg::with_name("toolchain")
                        .long("toolchain")
                        .short("t")
                        .takes_value(true)
                        .required(false)
                )
                .arg(Arg::with_name("crate").required(true)),

            // Experimenting
            Run::clap(),
            RunTc::clap(),

            // Reporting
            GenReport::clap(),
            PublishReport::clap(),

            cmd("serve-report", "serve report"),
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
            ("create-lists", _) => Box::new(CreateLists),

            // Master experiment prep
            ("define-ex", Some(m)) => {
                Box::new(DefineEx(
                    ex(m)?,
                    tc1(m)?,
                    tc2(m)?,
                    mode(m)?,
                    crate_select(m)?,
                ))
            }
            ("prepare-ex", Some(m)) => Box::new(PrepareEx::from_clap(m.clone())),
            ("copy-ex", Some(m)) => Box::new(CopyEx(ex1(m)?, ex2(m)?)),
            ("delete-ex", Some(m)) => Box::new(DeleteEx(ex(m)?)),

            // Local experiment prep
            ("delete-all-target-dirs", Some(m)) => Box::new(DeleteAllTargetDirs(ex(m)?)),
            ("delete-all-results", Some(m)) => Box::new(DeleteAllResults(ex(m)?)),
            ("delete-result", Some(m)) => {
                use result::OptionResultExt;
                Box::new(DeleteResult(
                    ex(m)?,
                    m.value_of("tc").map(str::parse).invert()?,
                    m.value_of("crate").map(str::parse).expect("")?,
                ))
            }

            // Experimenting
            ("run", Some(m)) => Box::new(Run::from_clap(m.clone())),
            ("run-tc", Some(m)) => Box::new(RunTc::from_clap(m.clone())),

            // Reporting
            ("gen-report", Some(m)) => Box::new(GenReport::from_clap(m.clone())),
            ("publish-report", Some(m)) => Box::new(PublishReport::from_clap(m.clone())),

            ("serve-report", _) => Box::new(Serve),

            (s, _) => panic!("unimplemented args_to_cmd {}", s),
        })
    }

    impl FromStr for Ex {
        type Err = Error;

        fn from_str(ex: &str) -> Result<Ex> {
            Ok(Ex(ex.to_string()))
        }
    }

    impl FromStr for Dest {
        type Err = Error;

        fn from_str(ex: &str) -> Result<Dest> {
            Ok(Dest(ex.into()))
        }
    }
}

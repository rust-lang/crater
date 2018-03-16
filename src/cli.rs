//! Crater works by serially processing a queue of commands, each of
//! which transforms the application state in some discrete way, and
//! designed to be resilient to I/O errors. The application state is
//! backed by a directory in the filesystem, and optionally synchronized
//! with s3.
//!
//! These command queues may be created dynamically and executed in
//! parallel jobs, either locally, or distributed on e.g. AWS. The
//! application state employs ownership techniques to ensure that
//! parallel access is consistent and race-free.

use crater::config::Config;
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
use structopt::clap::AppSettings;

// An experiment name
#[derive(Debug, Clone)]
pub struct Ex(String);

#[derive(Debug, Clone)]
pub struct DockerEnv(String);
impl FromStr for Ex {
    type Err = Error;

    fn from_str(ex: &str) -> Result<Ex> {
        Ok(Ex(ex.to_string()))
    }
}

impl FromStr for DockerEnv {
    type Err = Error;

    fn from_str(env: &str) -> Result<DockerEnv> {
        Ok(DockerEnv(env.to_string()))
    }
}

#[derive(Debug, Clone)]
pub struct Dest(PathBuf);

impl FromStr for Dest {
    type Err = Error;

    fn from_str(env: &str) -> Result<Dest> {
        Ok(Dest(env.into()))
    }
}

#[derive(StructOpt)]
#[structopt(name = "crater", about = "Kaboom!",
            setting_raw = "AppSettings::VersionlessSubcommands",
            setting_raw = "AppSettings::DeriveDisplayOrder",
            setting_raw = "AppSettings::SubcommandRequiredElseHelp")]
pub enum Crater {
    #[structopt(name = "prepare-local",
                about = "acquire toolchains, build containers, build crate lists")]
    PrepareLocal {
        #[structopt(name = "docker env", long = "docker-env", default_value = "full")]
        env: DockerEnv,
    },

    #[structopt(name = "create-lists", about = "create all the lists of crates")] CreateLists,

    #[structopt(name = "define-ex", about = "define an experiment")]
    DefineEx {
        #[structopt(name = "experiment", long = "ex", default_value = "default")]
        ex: Ex,
        #[structopt(name = "tc-1")]
        tc1: Toolchain,
        #[structopt(name = "tc-2")]
        tc2: Toolchain,
        #[structopt(name = "mode", long = "mode",
                    default_value_raw = "ExMode::BuildAndTest.to_str()",
                    possible_values_raw = "&[
                ExMode::BuildAndTest.to_str(),
                ExMode::BuildOnly.to_str(),
                ExMode::CheckOnly.to_str(),
                ExMode::UnstableFeatures.to_str(),
            ]")]
        mode: ExMode,
        #[structopt(name = "crate-select", long = "crate-select",
                    default_value_raw = "ExCrateSelect::Demo.to_str()",
                    possible_values_raw = "&[
                ExCrateSelect::Demo.to_str(),
                ExCrateSelect::Full.to_str(),
                ExCrateSelect::SmallRandom.to_str(),
                ExCrateSelect::Top100.to_str(),
            ]")]
        crates: ExCrateSelect,
    },

    #[structopt(name = "prepare-ex", about = "prepare shared and local data for experiment")]
    PrepareEx {
        #[structopt(name = "experiment", long = "ex", default_value = "default")]
        ex: Ex,
    },

    #[structopt(name = "copy-ex", about = "copy all data from one experiment to another")]
    CopyEx {
        ex1: Ex,
        ex2: Ex,
    },

    #[structopt(name = "delete-ex", about = "delete shared data for experiment")]
    DeleteEx {
        #[structopt(long = "ex", default_value = "default")]
        ex: Ex,
    },

    #[structopt(name = "delete-all-target-dirs",
                about = "delete the cargo target dirs for an experiment")]
    DeleteAllTargetDirs {
        #[structopt(long = "ex", default_value = "default")]
        ex: Ex,
    },

    #[structopt(name = "delete-all-results", about = "delete all results for an experiment")]
    DeleteAllResults {
        #[structopt(name = "experiment", long = "ex", default_value = "default")]
        ex: Ex,
    },

    #[structopt(name = "delete-result", about = "delete results for a crate from an experiment")]
    DeleteResult {
        #[structopt(name = "experiment", long = "ex", default_value = "default")]
        ex: Ex,
        #[structopt(name = "toolchain", long = "toolchain", short = "t")]
        tc: Option<Toolchain>,
        #[structopt(name = "crate")]
        krate: ExCrate,
    },

    #[structopt(name = "run", about = "run an experiment, with all toolchains")]
    Run {
        #[structopt(name = "experiment", long = "ex", default_value = "default")]
        ex: Ex,
    },

    #[structopt(name = "run-tc", about = "run an experiment, with a single toolchain")]
    RunTc {
        #[structopt(name = "experiment", long = "ex", default_value = "default")]
        ex: Ex,
        #[structopt(name = "toolchain")]
        tc: Toolchain,
    },

    #[structopt(name = "gen-report", about = "generate the experiment report")]
    GenReport {
        #[structopt(name = "experiment", long = "ex", default_value = "default")]
        ex: Ex,
        #[structopt(name = "destination")]
        dest: Dest,
    },

    #[structopt(name = "publish-report", about = "publish the experiment report to S3")]
    PublishReport {
        #[structopt(name = "experiment", long = "ex", default_value = "default",
                    help = "The experiment to publish a report for.")]
        ex: Ex,
        #[structopt(name = "S3 URI",
                    help = "The S3 URI to put the report at. \
                            [default: $CARGOBOMB_REPORT_S3_PREFIX/<experiment>")]
        s3_prefix: Option<report::S3Prefix>,
    },

    #[structopt(name = "serve-report", about = "serve report")] Serve,
}

impl Crater {
    pub fn run(&self) -> Result<()> {
        let config = Config::load()?;

        match *self {
            Crater::CreateLists => lists::create_all_lists(true)?,
            Crater::PrepareLocal { ref env } => {
                let docker_env = &env.0;
                let stable_tc = Toolchain::Dist("stable".into());
                stable_tc.prepare()?;
                docker::build_container(docker_env)?;
                lists::create_all_lists(false)?;
            }
            Crater::DefineEx {
                ref ex,
                ref tc1,
                ref tc2,
                ref mode,
                ref crates,
            } => {
                ex::define(
                    ex::ExOpts {
                        name: ex.0.clone(),
                        toolchains: vec![tc1.clone(), tc2.clone()],
                        mode: mode.clone(),
                        crates: crates.clone(),
                    },
                    &config,
                )?;
            }
            Crater::PrepareEx { ref ex } => {
                let ex = ex::Experiment::load(&ex.0)?;
                ex.prepare_shared()?;
                ex.prepare_local()?;
            }
            Crater::CopyEx { ref ex1, ref ex2 } => {
                ex::copy(&ex1.0, &ex2.0)?;
            }
            Crater::DeleteEx { ref ex } => {
                ex::delete(&ex.0)?;
            }
            Crater::DeleteAllTargetDirs { ref ex } => {
                ex::delete_all_target_dirs(&ex.0)?;
            }
            Crater::DeleteAllResults { ref ex } => {
                ex_run::delete_all_results(&ex.0)?;
            }
            Crater::DeleteResult {
                ref ex,
                ref tc,
                ref krate,
            } => ex_run::delete_result(&ex.0, tc.as_ref(), krate)?,
            Crater::Run { ref ex } => {
                ex_run::run_ex_all_tcs(&ex.0, &config)?;
            }
            Crater::RunTc { ref ex, ref tc } => {
                ex_run::run_ex(&ex.0, tc.clone(), &config)?;
            }
            Crater::GenReport { ref ex, ref dest } => {
                report::gen(&ex.0, &report::FileWriter::create(dest.0.clone())?, &config)?;
            }
            Crater::PublishReport {
                ref ex,
                ref s3_prefix,
            } => {
                let s3_prefix = match *s3_prefix {
                    Some(ref prefix) => prefix.clone(),
                    None => {
                        let mut prefix: report::S3Prefix = get_env("CARGOBOMB_REPORT_S3_PREFIX")?;
                        prefix.prefix.push(&ex.0);
                        prefix
                    }
                };
                report::gen(&ex.0, &report::S3Writer::create(s3_prefix)?, &config)?;
            }
            Crater::Serve => {
                server::start(server::Data { config });
            }
        }

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

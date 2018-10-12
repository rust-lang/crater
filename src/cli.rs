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

use crater::actions;
use crater::agent;
use crater::config::Config;
use crater::crates::Crate;
use crater::db::Database;
use crater::docker;
use crater::errors::*;
use crater::experiments::{Assignee, CapLints, CrateSelect, Experiment, Mode, Status};
use crater::report;
use crater::results::{DatabaseDB, DeleteResults};
use crater::run_graph;
use crater::server;
use crater::toolchain::Toolchain;
use crater::tools;
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
#[structopt(
    name = "crater",
    about = "Kaboom!",
    raw(
        setting = "AppSettings::VersionlessSubcommands",
        setting = "AppSettings::DeriveDisplayOrder",
        setting = "AppSettings::SubcommandRequiredElseHelp"
    )
)]
pub enum Crater {
    #[structopt(
        name = "prepare-local",
        about = "acquire toolchains, build containers, build crate lists"
    )]
    PrepareLocal {
        #[structopt(
            name = "docker env",
            long = "docker-env",
            default_value = "full"
        )]
        env: DockerEnv,
    },

    #[structopt(
        name = "create-lists",
        about = "create all the lists of crates"
    )]
    CreateLists,

    #[structopt(name = "define-ex", about = "define an experiment")]
    DefineEx {
        #[structopt(name = "experiment", long = "ex", default_value = "default")]
        ex: Ex,
        #[structopt(name = "tc-1")]
        tc1: Toolchain,
        #[structopt(name = "tc-2")]
        tc2: Toolchain,
        #[structopt(
            name = "mode",
            long = "mode",
            raw(
                default_value = "Mode::BuildAndTest.to_str()",
                possible_values = "Mode::possible_values()"
            )
        )]
        mode: Mode,
        #[structopt(
            name = "crate-select",
            long = "crate-select",
            raw(
                default_value = "CrateSelect::Demo.to_str()",
                possible_values = "CrateSelect::possible_values()"
            )
        )]
        crates: CrateSelect,
        #[structopt(
            name = "level",
            long = "cap-lints",
            raw(
                default_value = "CapLints::Forbid.to_str()",
                possible_values = "CapLints::possible_values()"
            )
        )]
        cap_lints: CapLints,
        #[structopt(
            name = "priority",
            long = "priority",
            short = "p",
            default_value = "0"
        )]
        priority: i32,
    },

    #[structopt(name = "edit", about = "edit an experiment configuration")]
    Edit {
        #[structopt(name = "name")]
        name: String,
        #[structopt(name = "toolchain-start", long = "start")]
        tc1: Option<Toolchain>,
        #[structopt(name = "toolchain-end", long = "end")]
        tc2: Option<Toolchain>,
        #[structopt(
            name = "mode",
            long = "mode",
            raw(possible_values = "Mode::possible_values()")
        )]
        mode: Option<Mode>,
        #[structopt(
            name = "crates",
            long = "crates",
            raw(possible_values = "CrateSelect::possible_values()")
        )]
        crates: Option<CrateSelect>,
        #[structopt(
            name = "cap-lints",
            long = "cap-lints",
            raw(possible_values = "CapLints::possible_values()")
        )]
        cap_lints: Option<CapLints>,
        #[structopt(name = "priority", long = "priority", short = "p",)]
        priority: Option<i32>,
    },

    #[structopt(
        name = "delete-ex",
        about = "delete shared data for experiment"
    )]
    DeleteEx {
        #[structopt(long = "ex", default_value = "default")]
        ex: Ex,
    },

    #[structopt(
        name = "delete-all-results",
        about = "delete all results for an experiment"
    )]
    DeleteAllResults {
        #[structopt(name = "experiment", long = "ex", default_value = "default")]
        ex: Ex,
    },

    #[structopt(
        name = "delete-result",
        about = "delete results for a crate from an experiment"
    )]
    DeleteResult {
        #[structopt(name = "experiment", long = "ex", default_value = "default")]
        ex: Ex,
        #[structopt(name = "toolchain", long = "toolchain", short = "t")]
        tc: Option<Toolchain>,
        #[structopt(name = "crate")]
        krate: Crate,
    },

    #[structopt(name = "run-graph", about = "run a parallelized experiment")]
    RunGraph {
        #[structopt(name = "experiment", long = "ex", default_value = "default")]
        ex: Ex,
        #[structopt(
            name = "threads",
            short = "t",
            long = "threads",
            default_value = "1"
        )]
        threads: usize,
    },

    #[structopt(
        name = "gen-report",
        about = "generate the experiment report"
    )]
    GenReport {
        #[structopt(name = "experiment", long = "ex", default_value = "default")]
        ex: Ex,
        #[structopt(name = "destination")]
        dest: Dest,
        #[structopt(name = "force", long = "force")]
        force: bool,
    },

    #[structopt(
        name = "publish-report",
        about = "publish the experiment report to S3"
    )]
    PublishReport {
        #[structopt(
            name = "experiment",
            long = "ex",
            default_value = "default",
            help = "The experiment to publish a report for."
        )]
        ex: Ex,
        #[structopt(
            name = "S3 URI",
            help = "The S3 URI to put the report at. \
                    [default: $CARGOBOMB_REPORT_S3_PREFIX/<experiment>"
        )]
        s3_prefix: Option<report::S3Prefix>,
        #[structopt(name = "force", long = "force")]
        force: bool,
    },

    #[structopt(name = "server")]
    Server,

    #[structopt(name = "agent")]
    Agent {
        #[structopt(name = "url")]
        url: String,
        #[structopt(name = "token")]
        token: String,
        #[structopt(
            name = "threads",
            short = "t",
            long = "threads",
            default_value = "1"
        )]
        threads: usize,
    },

    #[structopt(
        name = "dump-tasks-graph",
        about = "dump the internal tasks graph in .dot format"
    )]
    DumpTasksGraph {
        #[structopt(name = "dest", parse(from_os_str))]
        dest: PathBuf,
        #[structopt(name = "experiment", long = "ex", default_value = "default")]
        ex: Ex,
    },
}

impl Crater {
    pub fn run(&self) -> Result<()> {
        match *self {
            Crater::CreateLists => {
                let config = Config::load()?;
                let db = Database::open()?;

                actions::UpdateLists::default().apply(&db, &config)?;
            }
            Crater::PrepareLocal { ref env } => {
                let config = Config::load()?;
                let db = Database::open()?;

                let docker_env = &env.0;
                tools::install()?;
                docker::build_container(docker_env)?;
                actions::UpdateLists::default().apply(&db, &config)?;
            }
            Crater::DefineEx {
                ref ex,
                ref tc1,
                ref tc2,
                ref mode,
                ref crates,
                ref cap_lints,
                ref priority,
            } => {
                let config = Config::load()?;
                let db = Database::open()?;

                actions::CreateExperiment {
                    name: ex.0.clone(),
                    toolchains: [tc1.clone(), tc2.clone()],
                    mode: *mode,
                    crates: *crates,
                    cap_lints: *cap_lints,
                    priority: *priority,
                    github_issue: None,
                }.apply(&db, &config)?;
            }
            Crater::Edit {
                ref name,
                ref tc1,
                ref tc2,
                ref mode,
                ref crates,
                ref cap_lints,
                ref priority,
            } => {
                let config = Config::load()?;
                let db = Database::open()?;

                actions::EditExperiment {
                    name: name.clone(),
                    toolchains: [tc1.clone(), tc2.clone()],
                    mode: *mode,
                    crates: *crates,
                    cap_lints: *cap_lints,
                    priority: *priority,
                }.apply(&db, &config)?;
            }
            Crater::DeleteEx { ref ex } => {
                let config = Config::load()?;
                let db = Database::open()?;

                actions::DeleteExperiment { name: ex.0.clone() }.apply(&db, &config)?;
            }
            Crater::DeleteAllResults { ref ex } => {
                let db = Database::open()?;
                let result_db = DatabaseDB::new(&db);

                if let Some(mut experiment) = Experiment::get(&db, &ex.0)? {
                    result_db.delete_all_results(&experiment)?;
                    experiment.set_status(&db, Status::Queued)?;
                } else {
                    bail!("missing experiment {}", ex.0);
                }
            }
            Crater::DeleteResult {
                ref ex,
                ref tc,
                ref krate,
            } => {
                let db = Database::open()?;
                let result_db = DatabaseDB::new(&db);

                if let Some(mut experiment) = Experiment::get(&db, &ex.0)? {
                    if let Some(tc) = tc {
                        result_db.delete_result(&experiment, tc, krate)?;
                    } else {
                        for tc in &experiment.toolchains {
                            result_db.delete_result(&experiment, tc, krate)?;
                        }
                    }

                    experiment.set_status(&db, Status::Queued)?;
                } else {
                    bail!("missing experiment {}", ex.0);
                }
            }
            Crater::RunGraph { ref ex, threads } => {
                let config = Config::load()?;
                let db = Database::open()?;

                if let Some(mut experiment) = Experiment::get(&db, &ex.0)? {
                    // Ensure the experiment is properly assigned
                    match experiment.assigned_to {
                        None => experiment.set_assigned_to(&db, Some(&Assignee::CLI))?,
                        Some(Assignee::CLI) => {}
                        Some(a) => bail!("experiment {} is assigned to {}", ex.0, a),
                    }

                    // Update the status
                    match experiment.status {
                        Status::Queued => experiment.set_status(&db, Status::Running)?,
                        Status::Running => {}
                        other => bail!("can't run an experiment with status {}", other.to_str()),
                    }

                    let result_db = DatabaseDB::new(&db);
                    run_graph::run_ex(&experiment, &result_db, threads, &config)?;
                    experiment.set_status(&db, Status::NeedsReport)?;
                } else {
                    bail!("missing experiment {}", ex.0);
                }
            }
            Crater::GenReport {
                ref ex,
                ref dest,
                force,
            } => {
                let config = Config::load()?;
                let db = Database::open()?;

                if let Some(mut experiment) = Experiment::get(&db, &ex.0)? {
                    // Update the status
                    match (experiment.status, force) {
                        (Status::NeedsReport, _) | (Status::ReportFailed, _) | (_, true) => {
                            experiment.set_status(&db, Status::GeneratingReport)?;
                        }
                        (other, false) => bail!(
                            "can't generate the report of an experiment with status {} \
                             (use --force to override)",
                            other
                        ),
                    }

                    let result_db = DatabaseDB::new(&db);
                    let res = report::gen(
                        &result_db,
                        &experiment,
                        &report::FileWriter::create(dest.0.clone())?,
                        &config,
                    );

                    if let Err(err) = res {
                        experiment.set_status(&db, Status::ReportFailed)?;
                        return Err(err)?;
                    } else {
                        experiment.set_status(&db, Status::Completed)?;
                    }
                } else {
                    bail!("missing experiment: {}", ex.0);
                }
            }
            Crater::PublishReport {
                ref ex,
                ref s3_prefix,
                force,
            } => {
                let config = Config::load()?;
                let db = Database::open()?;

                let s3_prefix = match *s3_prefix {
                    Some(ref prefix) => prefix.clone(),
                    None => {
                        let mut prefix: report::S3Prefix = get_env("CARGOBOMB_REPORT_S3_PREFIX")?;
                        prefix.prefix.push(&ex.0);
                        prefix
                    }
                };

                if let Some(mut experiment) = Experiment::get(&db, &ex.0)? {
                    // Update the status
                    match (experiment.status, force) {
                        (Status::NeedsReport, _) | (Status::ReportFailed, _) | (_, true) => {
                            experiment.set_status(&db, Status::GeneratingReport)?;
                        }
                        (other, false) => bail!(
                            "can't publish the report of an experiment with status {} \
                             (use --force to override)",
                            other
                        ),
                    }

                    let result_db = DatabaseDB::new(&db);
                    let client = report::get_client_for_bucket(&s3_prefix.bucket)?;

                    let res = report::gen(
                        &result_db,
                        &experiment,
                        &report::S3Writer::create(client, s3_prefix)?,
                        &config,
                    );

                    if let Err(err) = res {
                        experiment.set_status(&db, Status::ReportFailed)?;
                        return Err(err)?;
                    } else {
                        experiment.set_status(&db, Status::Completed)?;
                    }
                } else {
                    bail!("missing experiment: {}", ex.0);
                }
            }
            Crater::Server => {
                let config = Config::load()?;
                server::run(config)?;
            }
            Crater::Agent {
                ref url,
                ref token,
                threads,
            } => {
                agent::run(url, token, threads)?;
            }
            Crater::DumpTasksGraph { ref dest, ref ex } => {
                let config = Config::load()?;
                let db = Database::open()?;

                if let Some(experiment) = Experiment::get(&db, &ex.0)? {
                    run_graph::dump_dot(&experiment, &config, dest)?;
                } else {
                    bail!("missing experiment: {}", ex.0);
                }
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
        })?.parse()
        .chain_err(|| format!{"Couldn't parse {:?}.", name})
}

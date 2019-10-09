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

use crater::actions::{self, Action, ActionsCtx};
use crater::agent::{self, Capabilities};
use crater::config::Config;
use crater::crates::Crate;
use crater::db::Database;
use crater::experiments::{Assignee, CapLints, DeferredCrateSelect, Experiment, Mode, Status};
use crater::report;
use crater::results::{DatabaseDB, DeleteResults};
use crater::runner;
use crater::server;
use crater::toolchain::Toolchain;
use failure::{bail, Error, Fallible};
use rustwide::{cmd::SandboxImage, Workspace, WorkspaceBuilder};
use std::collections::HashSet;
use std::path::PathBuf;
use std::str::FromStr;
use std::time::Duration;
use structopt::clap::AppSettings;

// An experiment name
#[derive(Debug, Clone)]
pub struct Ex(String);

#[derive(Debug, Clone)]
pub struct DockerEnv(String);
impl FromStr for Ex {
    type Err = Error;

    fn from_str(ex: &str) -> Fallible<Ex> {
        Ok(Ex(ex.to_string()))
    }
}

impl FromStr for DockerEnv {
    type Err = Error;

    fn from_str(env: &str) -> Fallible<DockerEnv> {
        Ok(DockerEnv(env.to_string()))
    }
}

#[derive(Debug, Clone)]
pub struct Dest(PathBuf);

impl FromStr for Dest {
    type Err = Error;

    fn from_str(env: &str) -> Fallible<Dest> {
        Ok(Dest(env.into()))
    }
}

/// The default capabilities for the machine that `crater` has been compiled on.
fn default_capabilities_for_target() -> Capabilities {
    let caps: &[_] = if cfg!(target_os = "windows") {
        &["windows"]
    } else if cfg!(target_os = "linux") {
        &["linux"]
    } else {
        &[]
    };

    Capabilities::new(caps)
}

#[derive(structopt_derive::StructOpt)]
#[allow(clippy::large_enum_variant)]
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
    PrepareLocal,

    #[structopt(name = "create-lists", about = "create all the lists of crates")]
    CreateLists {
        #[structopt(name = "lists")]
        lists: Vec<String>,
    },

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
            help = "The set of crates on which the experiment will run.",
            long_help = "The set of crates on which the experiment will run.\n\n\
                         This can be one of (full, demo, random-{d}, top-{d}, local) \
                         where {d} is a positive integer, or \"list:\" followed \
                         by a comma-separated list of crates.",
            raw(default_value = "\"demo\"",)
        )]
        crates: DeferredCrateSelect,
        #[structopt(
            name = "level",
            long = "cap-lints",
            raw(
                default_value = "CapLints::Forbid.to_str()",
                possible_values = "CapLints::possible_values()"
            )
        )]
        cap_lints: CapLints,
        #[structopt(name = "priority", long = "priority", short = "p", default_value = "0")]
        priority: i32,
        #[structopt(name = "ignore-blacklist", long = "ignore-blacklist")]
        ignore_blacklist: bool,
        #[structopt(name = "assign", long = "assign")]
        assign: Option<Assignee>,
        #[structopt(name = "requirement", long = "requirement")]
        requirement: Option<String>,
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
            help = "The set of crates on which the experiment will run.",
            long_help = "The set of crates on which the experiment will run.\n\n\
                         This can be one of (full, demo, random-{d}, top-{d}, local) \
                         where {d} is a positive integer, or \"list:\" followed \
                         by a comma-separated list of crates."
        )]
        crates: Option<DeferredCrateSelect>,
        #[structopt(
            name = "cap-lints",
            long = "cap-lints",
            raw(possible_values = "CapLints::possible_values()")
        )]
        cap_lints: Option<CapLints>,
        #[structopt(name = "priority", long = "priority", short = "p")]
        priority: Option<i32>,
        #[structopt(
            name = "ignore-blacklist",
            long = "ignore-blacklist",
            conflicts_with = "no-ignore-blacklist"
        )]
        ignore_blacklist: bool,
        #[structopt(
            name = "no-ignore-blacklist",
            long = "no-ignore-blacklist",
            conflicts_with = "ignore-blacklist"
        )]
        no_ignore_blacklist: bool,
        #[structopt(name = "assign", long = "assign")]
        assign: Option<Assignee>,
        #[structopt(name = "requirement", long = "requirement")]
        requirement: Option<String>,
    },

    #[structopt(name = "delete-ex", about = "delete shared data for experiment")]
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
        #[structopt(name = "threads", short = "t", long = "threads", default_value = "1")]
        threads: usize,
        #[structopt(name = "docker-env", long = "docker-env")]
        docker_env: Option<String>,
        #[structopt(name = "fast-workspace-init", long = "fast-workspace-init")]
        fast_workspace_init: bool,
    },

    #[structopt(name = "gen-report", about = "generate the experiment report")]
    GenReport {
        #[structopt(name = "experiment", long = "ex", default_value = "default")]
        ex: Ex,
        #[structopt(name = "destination")]
        dest: Dest,
        #[structopt(name = "force", long = "force")]
        force: bool,
    },

    #[structopt(name = "publish-report", about = "publish the experiment report to S3")]
    PublishReport {
        #[structopt(
            name = "experiment",
            long = "ex",
            default_value = "default",
            help = "The experiment to publish a report for."
        )]
        ex: Ex,
        #[structopt(name = "S3 URI", help = "The S3 URI to put the report at.")]
        s3_prefix: report::S3Prefix,
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
        #[structopt(name = "threads", short = "t", long = "threads", default_value = "1")]
        threads: usize,
        #[structopt(name = "docker-env", long = "docker-env")]
        docker_env: Option<String>,
        #[structopt(name = "fast-workspace-init", long = "fast-workspace-init")]
        fast_workspace_init: bool,
        #[structopt(
            name = "capabilities",
            help = "Registers additional capabilities for this agent.",
            long_help = "Registers additional capabilities for this agent.\n\n \
                         These will be appended to the defaults for this platform, unless those \
                         have been disabled via `--no-default-capabilities`.",
            long,
            raw(use_delimiter = "true")
        )]
        capabilities: Vec<String>,
        #[structopt(
            name = "no-default-capabilities",
            long,
            help = "Disables the default capabilities for this platform."
        )]
        no_default_capabilities: bool,
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

    #[structopt(
        name = "check-config",
        about = "check if the config.toml file is valid"
    )]
    CheckConfig {
        #[structopt(name = "file")]
        filename: Option<String>,
    },
}

impl Crater {
    pub fn run(&self) -> Fallible<()> {
        match *self {
            Crater::CreateLists { ref lists } => {
                let mut lists: HashSet<_> = lists.iter().map(|s| s.as_str()).collect();

                let config = Config::load()?;
                let db = Database::open()?;
                let ctx = ActionsCtx::new(&db, &config);

                let action = if lists.is_empty() {
                    actions::UpdateLists::default()
                } else {
                    actions::UpdateLists {
                        github: lists.remove("github"),
                        registry: lists.remove("registry"),
                        local: lists.remove("local"),
                    }
                };

                if let Some(unknown) = lists.iter().next() {
                    bail!("unknown list: {}", unknown);
                } else {
                    action.apply(&ctx)?;
                }
            }
            Crater::PrepareLocal => {
                let config = Config::load()?;
                let db = Database::open()?;
                let ctx = ActionsCtx::new(&db, &config);
                actions::UpdateLists::default().apply(&ctx)?;
            }
            Crater::DefineEx {
                ref ex,
                ref tc1,
                ref tc2,
                ref mode,
                ref crates,
                ref cap_lints,
                ref priority,
                ref ignore_blacklist,
                ref assign,
                ref requirement,
            } => {
                let config = Config::load()?;
                let db = Database::open()?;
                let ctx = ActionsCtx::new(&db, &config);

                actions::CreateExperiment {
                    name: ex.0.clone(),
                    toolchains: [tc1.clone(), tc2.clone()],
                    mode: *mode,
                    crates: crates.clone().resolve()?,
                    cap_lints: *cap_lints,
                    priority: *priority,
                    github_issue: None,
                    ignore_blacklist: *ignore_blacklist,
                    assign: assign.clone(),
                    requirement: requirement.clone(),
                }
                .apply(&ctx)?;
            }
            Crater::Edit {
                ref name,
                ref tc1,
                ref tc2,
                ref mode,
                ref crates,
                ref cap_lints,
                ref priority,
                ref ignore_blacklist,
                ref no_ignore_blacklist,
                ref assign,
                ref requirement,
            } => {
                let config = Config::load()?;
                let db = Database::open()?;
                let ctx = ActionsCtx::new(&db, &config);

                let ignore_blacklist = if *ignore_blacklist {
                    Some(true)
                } else if *no_ignore_blacklist {
                    Some(false)
                } else {
                    None
                };

                actions::EditExperiment {
                    name: name.clone(),
                    toolchains: [tc1.clone(), tc2.clone()],
                    mode: *mode,
                    crates: crates.clone().map(|cs| cs.resolve()).transpose()?,
                    cap_lints: *cap_lints,
                    priority: *priority,
                    ignore_blacklist,
                    assign: assign.clone(),
                    requirement: requirement.clone(),
                }
                .apply(&ctx)?;
            }
            Crater::DeleteEx { ref ex } => {
                let config = Config::load()?;
                let db = Database::open()?;
                let ctx = ActionsCtx::new(&db, &config);

                actions::DeleteExperiment { name: ex.0.clone() }.apply(&ctx)?;
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
            Crater::RunGraph {
                ref ex,
                threads,
                ref docker_env,
                fast_workspace_init,
            } => {
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
                    runner::run_ex(
                        &experiment,
                        &self.workspace(
                            docker_env.as_ref().map(|s| s.as_str()),
                            fast_workspace_init,
                        )?,
                        &experiment.get_uncompleted_crates(&db, &config, &Assignee::CLI)?,
                        &result_db,
                        threads,
                        &config,
                    )?;
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
                        &experiment.get_crates(&db)?,
                        &report::FileWriter::create(dest.0.clone())?,
                        &config,
                    );

                    if let Err(err) = res {
                        experiment.set_status(&db, Status::ReportFailed)?;
                        return Err(err);
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
                        &experiment.get_crates(&db)?,
                        &report::S3Writer::create(client, s3_prefix.clone())?,
                        &config,
                    );

                    if let Err(err) = res {
                        experiment.set_status(&db, Status::ReportFailed)?;
                        return Err(err);
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
                ref docker_env,
                fast_workspace_init,
                ref capabilities,
                no_default_capabilities,
            } => {
                let mut caps = if no_default_capabilities {
                    Capabilities::default()
                } else {
                    default_capabilities_for_target()
                };
                caps.extend(capabilities.clone().into_iter());

                agent::run(
                    url,
                    token,
                    threads,
                    &caps,
                    &self
                        .workspace(docker_env.as_ref().map(|s| s.as_str()), fast_workspace_init)?,
                )?;
            }
            Crater::DumpTasksGraph { ref dest, ref ex } => {
                let config = Config::load()?;
                let db = Database::open()?;

                if let Some(experiment) = Experiment::get(&db, &ex.0)? {
                    runner::dump_dot(&experiment, &experiment.get_crates(&db)?, &config, dest)?;
                } else {
                    bail!("missing experiment: {}", ex.0);
                }
            }
            Crater::CheckConfig { ref filename } => {
                if let Err(ref e) = Config::check(filename) {
                    bail!("check-config failed: {}", e);
                }
            }
        }

        Ok(())
    }

    fn workspace(&self, docker_env: Option<&str>, fast_init: bool) -> Result<Workspace, Error> {
        let mut builder = WorkspaceBuilder::new(&crater::dirs::WORK_DIR, &crater::USER_AGENT)
            .fast_init(fast_init)
            .fetch_registry_index_during_builds(false)
            .command_timeout(Some(Duration::from_secs(15 * 60)))
            .command_no_output_timeout(Some(Duration::from_secs(5 * 60)))
            .running_inside_docker(std::env::var("CRATER_INSIDE_DOCKER").is_ok());
        if let Some(env) = docker_env {
            builder = builder.sandbox_image(if env.contains('/') {
                SandboxImage::remote(env)?
            } else {
                SandboxImage::local(env)?
            });
        }
        Ok(builder.init()?)
    }
}

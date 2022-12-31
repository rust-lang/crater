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

use clap::Parser;
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
use std::net::SocketAddr;
use std::path::PathBuf;
use std::str::FromStr;
use std::time::Duration;

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

#[derive(Parser)]
#[allow(clippy::large_enum_variant)]
#[clap(name = "crater", about = "Kaboom!")]
pub enum Crater {
    #[clap(
        name = "prepare-local",
        about = "acquire toolchains, build containers, build crate lists"
    )]
    PrepareLocal,

    #[clap(name = "create-lists", about = "create all the lists of crates")]
    CreateLists {
        #[clap(name = "lists")]
        lists: Vec<String>,
    },

    #[clap(name = "define-ex", about = "define an experiment")]
    DefineEx {
        #[clap(name = "experiment", long = "ex", default_value = "default")]
        ex: Ex,
        #[clap(name = "tc-1")]
        tc1: Toolchain,
        #[clap(name = "tc-2")]
        tc2: Toolchain,
        #[clap(name = "mode", long = "mode", default_value_t = Mode::BuildAndTest)]
        mode: Mode,
        #[clap(
            name = "crate-select",
            long = "crate-select",
            help = "The set of crates on which the experiment will run.",
            long_help = "The set of crates on which the experiment will run.\n\n\
                         This can be one of (full, demo, random-{d}, top-{d}, local) \
                         where {d} is a positive integer, or \"list:\" followed \
                         by a comma-separated list of crates.",
            default_value = "demo"
        )]
        crates: DeferredCrateSelect,
        #[clap(
            name = "level",
            long = "cap-lints",
            default_value_t = CapLints::Forbid
        )]
        cap_lints: CapLints,
        #[clap(name = "priority", long = "priority", short = 'p', default_value = "0")]
        priority: i32,
        #[clap(name = "ignore-blacklist", long = "ignore-blacklist")]
        ignore_blacklist: bool,
        #[clap(name = "assign", long = "assign")]
        assign: Option<Assignee>,
        #[clap(name = "requirement", long = "requirement")]
        requirement: Option<String>,
    },

    #[clap(name = "edit", about = "edit an experiment configuration")]
    Edit {
        #[clap(name = "name")]
        name: String,
        #[clap(name = "toolchain-start", long = "start")]
        tc1: Option<Toolchain>,
        #[clap(name = "toolchain-end", long = "end")]
        tc2: Option<Toolchain>,
        #[clap(name = "mode", long = "mode")]
        mode: Option<Mode>,
        #[clap(
            name = "crates",
            long = "crates",
            help = "The set of crates on which the experiment will run.",
            long_help = "The set of crates on which the experiment will run.\n\n\
                         This can be one of (full, demo, random-{d}, top-{d}, local) \
                         where {d} is a positive integer, or \"list:\" followed \
                         by a comma-separated list of crates."
        )]
        crates: Option<DeferredCrateSelect>,
        #[clap(name = "cap-lints", long = "cap-lints")]
        cap_lints: Option<CapLints>,
        #[clap(name = "priority", long = "priority", short = 'p')]
        priority: Option<i32>,
        #[clap(
            name = "ignore-blacklist",
            long = "ignore-blacklist",
            conflicts_with = "no-ignore-blacklist"
        )]
        ignore_blacklist: bool,
        #[clap(
            name = "no-ignore-blacklist",
            long = "no-ignore-blacklist",
            conflicts_with = "ignore-blacklist"
        )]
        no_ignore_blacklist: bool,
        #[clap(name = "assign", long = "assign")]
        assign: Option<Assignee>,
        #[clap(name = "requirement", long = "requirement")]
        requirement: Option<String>,
    },

    #[clap(name = "delete-ex", about = "delete shared data for experiment")]
    DeleteEx {
        #[clap(long = "ex", default_value = "default")]
        ex: Ex,
    },

    #[clap(
        name = "delete-all-results",
        about = "delete all results for an experiment"
    )]
    DeleteAllResults {
        #[clap(name = "experiment", long = "ex", default_value = "default")]
        ex: Ex,
    },

    #[clap(
        name = "delete-result",
        about = "delete results for a crate from an experiment"
    )]
    DeleteResult {
        #[clap(name = "experiment", long = "ex", default_value = "default")]
        ex: Ex,
        #[clap(name = "toolchain", long = "toolchain", short = 't')]
        tc: Option<Toolchain>,
        #[clap(name = "crate")]
        krate: Crate,
    },

    #[clap(name = "run-graph", about = "run a parallelized experiment")]
    RunGraph {
        #[clap(name = "experiment", long = "ex", default_value = "default")]
        ex: Ex,
        #[clap(name = "threads", short = 't', long = "threads", default_value = "1")]
        threads: usize,
        #[clap(name = "docker-env", long = "docker-env")]
        docker_env: Option<String>,
        #[clap(name = "fast-workspace-init", long = "fast-workspace-init")]
        fast_workspace_init: bool,
    },

    #[clap(name = "gen-report", about = "generate the experiment report")]
    GenReport {
        #[clap(name = "experiment", long = "ex", default_value = "default")]
        ex: Ex,
        #[clap(name = "destination")]
        dest: Dest,
        #[clap(name = "force", long = "force")]
        force: bool,
        #[clap(name = "output-templates", long = "output-templates")]
        output_templates: bool,
    },

    #[clap(name = "server")]
    Server {
        #[clap(
            name = "bind",
            long = "bind",
            short = 'b',
            help = "The address and port to bind to."
        )]
        bind: Option<SocketAddr>,
    },

    #[clap(name = "agent")]
    Agent {
        #[clap(name = "url")]
        url: String,
        #[clap(name = "token")]
        token: String,
        #[clap(name = "threads", short = 't', long = "threads", default_value = "1")]
        threads: usize,
        #[clap(name = "docker-env", long = "docker-env")]
        docker_env: Option<String>,
        #[clap(name = "fast-workspace-init", long = "fast-workspace-init")]
        fast_workspace_init: bool,
        #[clap(
            name = "capabilities",
            help = "Registers additional capabilities for this agent.",
            long_help = "Registers additional capabilities for this agent.\n\n \
                         These will be appended to the defaults for this platform, unless those \
                         have been disabled via `--no-default-capabilities`.",
            long
        )]
        capabilities: Vec<String>,
        #[clap(
            name = "no-default-capabilities",
            long,
            help = "Disables the default capabilities for this platform."
        )]
        no_default_capabilities: bool,
    },

    #[clap(
        name = "check-config",
        about = "check if the config.toml file is valid"
    )]
    CheckConfig {
        #[clap(name = "file")]
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

                    let workspace = self
                        .workspace(docker_env.as_ref().map(|s| s.as_str()), fast_workspace_init)?;
                    workspace.purge_all_build_dirs()?;

                    let crates =
                        std::sync::Mutex::new(experiment.get_uncompleted_crates(&db, None)?);
                    let res = runner::run_ex(
                        &experiment,
                        &workspace,
                        &result_db,
                        threads,
                        &config,
                        &|| Ok(crates.lock().unwrap().pop()),
                    );
                    workspace.purge_all_build_dirs()?;
                    res?;
                } else {
                    bail!("missing experiment {}", ex.0);
                }
            }
            Crater::GenReport {
                ref ex,
                ref dest,
                force,
                output_templates,
            } => {
                let config = Config::load()?;
                let db = Database::open()?;

                if let Some(mut experiment) = Experiment::get(&db, &ex.0)? {
                    let (completed, all) = experiment.raw_progress(&db)?;
                    if !force && completed != all {
                        bail!(
                            "can't generate the report of an incomplete experiment: {}/{} results \
                             (use --force to override)",
                            completed,
                            all,
                        );
                    }

                    experiment.set_status(&db, Status::GeneratingReport)?;

                    let result_db = DatabaseDB::new(&db);
                    let res = report::gen(
                        &result_db,
                        &experiment,
                        &experiment.get_crates(&db)?,
                        &report::FileWriter::create(dest.0.clone())?,
                        &config,
                        output_templates,
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
            Crater::Server { bind } => {
                let config = Config::load()?;
                server::run(
                    config,
                    bind.unwrap_or_else(|| ([127, 0, 0, 1], 8000).into()),
                )?;
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

mod api;
mod results;

use crate::agent::api::AgentApi;
use crate::agent::results::ResultsUploader;
use crate::config::Config;
use crate::crates::Crate;
use crate::db::{Database, QueryUtils};
use crate::experiments::Experiment;
use crate::prelude::*;
use crate::utils;
use crate::utils::disk_usage::DiskUsage;
use failure::Error;
use rustwide::Workspace;
use std::collections::BTreeSet;
use std::ops;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

// Purge all the caches if the disk is more than 50% full.
const PURGE_CACHES_THRESHOLD: f32 = 0.5;

#[derive(Default, Serialize, Deserialize)]
pub struct Capabilities {
    #[serde(default)]
    capabilities: BTreeSet<String>,
}

impl ops::Deref for Capabilities {
    type Target = BTreeSet<String>;

    fn deref(&self) -> &Self::Target {
        &self.capabilities
    }
}

impl ops::DerefMut for Capabilities {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.capabilities
    }
}

impl Capabilities {
    pub fn new(caps: &[&str]) -> Self {
        let capabilities = caps.iter().map(|s| (*s).to_string()).collect();
        Capabilities { capabilities }
    }

    pub fn for_agent(db: &Database, agent: &str) -> Fallible<Self> {
        let caps = db.query(
            "SELECT capability FROM agent_capabilities WHERE agent_name = ?1",
            [&agent],
            |r| r.get::<_, String>(0),
        )?;

        Ok(caps.into_iter().collect())
    }
}

impl FromIterator<String> for Capabilities {
    fn from_iter<T>(iter: T) -> Self
    where
        T: IntoIterator<Item = String>,
    {
        let capabilities = iter.into_iter().collect();
        Capabilities { capabilities }
    }
}

pub struct Agent {
    api: AgentApi,
    pub config: Config,
}

impl Agent {
    fn new(url: &str, token: &str, caps: &Capabilities) -> Fallible<Self> {
        info!("connecting to crater server {}...", url);

        let api = AgentApi::new(url, token);
        let config = api.config(caps)?;

        info!("connected to the crater server!");
        info!("assigned agent name: {}", config.agent_name);

        Ok(Agent {
            api,
            config: config.crater_config,
        })
    }

    fn experiment(&self) -> Fallible<Experiment> {
        info!("asking the server for a new experiment...");
        Ok(self.api.next_experiment()?)
    }

    pub fn next_crate(&self, ex: &str) -> Fallible<Option<Crate>> {
        self.api.next_crate(ex)
    }
}

static HEALTH_CHECK: AtomicBool = AtomicBool::new(false);

// Should be called at least once every 5 minutes, otherwise instance is
// replaced.
pub fn set_healthy() {
    HEALTH_CHECK.store(true, Ordering::SeqCst);
}

fn health_thread() {
    std::thread::spawn(move || {
        let mut last_check = Instant::now();

        let listener = std::net::TcpListener::bind("0.0.0.0:4343").unwrap();
        loop {
            // Accept a connection...
            drop(listener.accept());

            // Then check whether we should still be healthy. If not, we simply
            // drop the listening socket by breaking out of the loop, meaning
            // that we'll stop responding as healthy to future connects.
            //
            // A build has a maximum timeout of 15 minutes in rustwide, so we
            // currently expect checkpoints at least that often. It likely makes
            // sense for us to be more eager, but ultimately crater runtimes are
            // long enough that 15 minutes on one builder hopefully won't matter
            // too much.
            if last_check.elapsed() > Duration::from_secs(15 * 60) {
                last_check = Instant::now();
                if !HEALTH_CHECK.swap(false, Ordering::SeqCst) {
                    break;
                }
            }
        }
    });
}

fn run_heartbeat(url: &str, token: &str) {
    let api = AgentApi::new(url, token);

    thread::spawn(move || loop {
        if let Err(e) = api.heartbeat().with_context(|_| "failed to send heartbeat") {
            utils::report_failure(&e);
        }
        thread::sleep(Duration::from_secs(60));
    });
}

fn run_experiment(
    agent: &Agent,
    workspace: &Workspace,
    db: &ResultsUploader,
    threads_count: usize,
    past_experiment: &mut Option<String>,
) -> Result<(), (Option<Box<Experiment>>, Error)> {
    let ex = agent.experiment().map_err(|e| (None, e))?;

    if Some(&ex.name) != past_experiment.as_ref() {
        debug!("purging build directories...");
        workspace.purge_all_build_dirs().map_err(|e| (None, e))?;
    }
    *past_experiment = Some(ex.name.clone());

    match DiskUsage::fetch() {
        Ok(usage) => {
            if usage.is_threshold_reached(PURGE_CACHES_THRESHOLD) {
                warn!("purging all caches");
                workspace.purge_all_caches().map_err(|err| (None, err))?;
            }
        }
        Err(err) => {
            warn!("failed to check the disk usage: {}", err);
        }
    }

    crate::runner::run_ex(&ex, workspace, db, threads_count, &agent.config, &|| {
        agent.next_crate(&ex.name)
    })
    .map_err(|err| (Some(Box::new(ex)), err))?;
    Ok(())
}

pub fn run(
    url: &str,
    token: &str,
    threads_count: usize,
    caps: &Capabilities,
    workspace: &Workspace,
) -> Fallible<()> {
    let agent = Agent::new(url, token, caps)?;
    let db = results::ResultsUploader::new(&agent.api);

    run_heartbeat(url, token);
    health_thread();

    let mut past_experiment = None;
    loop {
        if let Err((ex, err)) =
            run_experiment(&agent, workspace, &db, threads_count, &mut past_experiment)
        {
            utils::report_failure(&err);
            if let Some(ex) = ex {
                if let Err(e) = agent
                    .api
                    .report_error(&ex, format!("{}", err.find_root_cause()))
                    .with_context(|_| "error encountered")
                {
                    utils::report_failure(&e);
                }
            }
        }
    }
}

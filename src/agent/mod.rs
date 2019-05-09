mod api;
mod results;

use crate::agent::api::AgentApi;
use crate::agent::results::ResultsUploader;
use crate::config::Config;
use crate::crates::Crate;
use crate::experiments::Experiment;
use crate::prelude::*;
use crate::utils;
use std::thread;
use std::time::Duration;

struct Agent {
    api: AgentApi,
    config: Config,
}

impl Agent {
    fn new(url: &str, token: &str) -> Fallible<Self> {
        info!("connecting to crater server {}...", url);

        let api = AgentApi::new(url, token);
        let config = api.config()?;

        info!("connected to the crater server!");
        info!("assigned agent name: {}", config.agent_name);

        Ok(Agent {
            api,
            config: config.crater_config,
        })
    }

    fn experiment(&self) -> Fallible<(Experiment, Vec<Crate>)> {
        info!("asking the server for a new experiment...");
        Ok(self.api.next_experiment()?)
    }
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
    db: &ResultsUploader,
    threads_count: usize,
    docker_env: &str,
) -> Fallible<()> {
    let (ex, crates) = agent.experiment()?;
    crate::runner::run_ex(&ex, &crates, db, threads_count, &agent.config, docker_env)?;
    Ok(())
}

pub fn run(url: &str, token: &str, threads_count: usize, docker_env: &str) -> Fallible<()> {
    let agent = Agent::new(url, token)?;
    let db = results::ResultsUploader::new(&agent.api);

    run_heartbeat(url, token);

    loop {
        if let Err(err) = run_experiment(&agent, &db, threads_count, docker_env) {
            utils::report_failure(&err);
            if let Err(e) = agent
                .api
                .report_error(format!("{}", err.find_root_cause()))
                .with_context(|_| "error encountered")
            {
                utils::report_failure(&e);
            }
        }
    }
}

mod api;
mod results;

use agent::api::AgentApi;
use config::Config;
use errors::*;
use ex::{self, Experiment};
use run_graph;
use std::thread;
use std::time::Duration;
use util;

struct Agent {
    api: AgentApi,
    config: Config,
}

impl Agent {
    fn new(url: &str, token: &str) -> Result<Self> {
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

    fn experiment(&self) -> Result<Experiment> {
        info!("asking the server for a new experiment...");
        Ok(self.api.next_experiment()?)
    }
}

fn run_heartbeat(url: &str, token: &str) {
    let api = AgentApi::new(url, token);

    thread::spawn(move || loop {
        if let Err(e) = api.heartbeat().chain_err(|| "failed to send heartbeat") {
            util::report_error(&e);
        }
        thread::sleep(Duration::from_secs(60));
    });
}

pub fn run(url: &str, token: &str, threads_count: usize) -> Result<()> {
    let agent = Agent::new(url, token)?;
    let db = results::ResultsUploader::new(&agent.api);

    run_heartbeat(url, token);

    loop {
        let ex = agent.experiment()?;

        let result = run_graph::run_ex(&ex, &db, threads_count, &agent.config);

        // Ensure local data is cleaned up even if the run crashed
        ex::delete_all_target_dirs(&ex.name)?;
        ex::delete(&ex.name)?;

        result?;

        agent.api.complete_experiment()?;
    }
}

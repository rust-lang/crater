#[macro_use]
mod http;

mod agents;
pub mod api_types;
mod auth;
mod db;
mod experiments;
mod github;
mod messages;
mod reports;
mod results;
mod routes;
mod tokens;

use config::Config;
use errors::*;
use hyper::Method;
use server::agents::Agents;
use server::auth::auth_agent;
use server::experiments::Experiments;
use server::github::GitHubApi;
use server::http::Server;
use server::tokens::Tokens;

#[derive(Clone)]
pub struct Data {
    pub bot_username: String,
    pub config: Config,
    pub github: GitHubApi,
    pub tokens: Tokens,
    pub agents: Agents,
    pub experiments: Experiments,
    pub db: db::Database,
    pub reports_worker: reports::ReportsWorker,
}

pub fn run(config: Config) -> Result<()> {
    let db = db::Database::open()?;
    let tokens = tokens::Tokens::load()?;
    let github = GitHubApi::new(&tokens);
    let agents = Agents::new(db.clone(), &tokens)?;
    let bot_username = github.username()?;

    info!("bot username: {}", bot_username);

    let data = Data {
        bot_username,
        config,
        github,
        tokens,
        agents,
        experiments: Experiments::new(db.clone()),
        db: db.clone(),
        reports_worker: reports::ReportsWorker::new(),
    };

    data.reports_worker.spawn(data.clone());
    let mut server = Server::new(data)?;

    server.add_route(
        Method::Get,
        "/agent-api/config",
        auth_agent(routes::agent::config),
    );
    server.add_route(
        Method::Get,
        "/agent-api/next-experiment",
        auth_agent(routes::agent::next_ex),
    );
    server.add_route(
        Method::Post,
        "/agent-api/complete-experiment",
        auth_agent(routes::agent::complete_ex),
    );
    server.add_route(
        Method::Post,
        "/agent-api/record-progress",
        auth_agent(routes::agent::record_progress),
    );
    server.add_route(
        Method::Post,
        "/agent-api/heartbeat",
        auth_agent(routes::agent::heartbeat),
    );

    server.add_route(Method::Post, "/webhooks", routes::webhooks::handle);

    info!("running server...");
    server.run()?;
    Ok(())
}

#[macro_use]
mod http;

mod agent;
mod auth;
mod db;
mod experiments;
mod github;
mod messages;
mod results;
mod tokens;
mod agents;
mod webhooks;
pub mod api_types;

use config::Config;
use errors::*;
use hyper::Method;
use server::agents::Agents;
use server::auth::auth_agent;
use server::experiments::Experiments;
use server::github::GitHubApi;
use server::http::Server;
use server::tokens::Tokens;

pub struct Data {
    pub bot_username: String,
    pub config: Config,
    pub github: GitHubApi,
    pub tokens: Tokens,
    pub agents: Agents,
    pub experiments: Experiments,
    pub db: db::Database,
}

pub fn run(config: Config) -> Result<()> {
    let db = db::Database::open()?;
    let tokens = tokens::Tokens::load()?;
    let github = GitHubApi::new(&tokens);
    let agents = Agents::new(db.clone(), &tokens)?;
    let bot_username = github.username()?;

    info!("bot username: {}", bot_username);

    let mut server = Server::new(Data {
        bot_username,
        config,
        github,
        tokens,
        agents,
        experiments: Experiments::new(db.clone()),
        db: db.clone(),
    })?;

    server.add_route(Method::Get, "/agent-api/config", auth_agent(agent::config));
    server.add_route(
        Method::Get,
        "/agent-api/next-experiment",
        auth_agent(agent::next_ex),
    );
    server.add_route(
        Method::Post,
        "/agent-api/complete-experiment",
        auth_agent(agent::complete_ex),
    );
    server.add_route(
        Method::Post,
        "/agent-api/record-result",
        auth_agent(agent::record_result),
    );
    server.add_route(
        Method::Post,
        "/agent-api/heartbeat",
        auth_agent(agent::heartbeat),
    );

    server.add_route(Method::Post, "/webhooks", webhooks::handle);

    info!("running server...");
    server.run()?;
    Ok(())
}
